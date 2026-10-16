// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Scoped AST allocation, capacity growth and converter handoffs with tiny data.

use super::*;
use crate::vm::module_values::ModuleValueCopy;

fn bounded_vm() -> Vm {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm.remaining_instructions = 100;
    vm.stack.reserve(4);
    vm
}

fn leave_room(vm: &mut Vm, room: usize) -> SharedSchedulingCharge {
    let used = vm.host_scheduling_usage().unwrap().payload_bytes;
    vm.host_scheduling
        .as_ref()
        .unwrap()
        .shared_charge(MAX_HOST_SCHEDULING_BYTES - used - room)
        .unwrap()
}

fn buffers(node: &JsonNode) -> usize {
    match node {
        JsonNode::Primitive { value, source } => {
            assert!(
                source.is_empty(),
                "module AST must omit reviver source copies"
            );
            if let Value::String(text) = value {
                text.owned_heap_payload_bytes().unwrap()
            } else {
                0
            }
        }
        JsonNode::Array(values) => {
            values.capacity() * std::mem::size_of::<JsonNode>()
                + values.iter().map(buffers).sum::<usize>()
        }
        JsonNode::Object(values) => {
            values.capacity() * std::mem::size_of::<(JsString, JsonNode)>()
                + values
                    .iter()
                    .map(|(key, value)| key.owned_heap_payload_bytes().unwrap() + buffers(value))
                    .sum::<usize>()
        }
    }
}

#[cfg_attr(test, test)]
fn json_module_ownership_live_ast_matches_its_actual_buffers_and_refunds_final_drop() {
    let mut vm = bounded_vm();
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    let parsed = JsonSourceParser::for_module("{\"x\":[\"a😀\",0]}", vm.host_scheduling.as_ref())
        .unwrap()
        .parse()
        .unwrap();
    let bytes = buffers(&parsed);
    assert!(bytes > 0);
    assert_eq!(parsed.depth, 2);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before + bytes);
    vm.host_scheduling.as_ref().unwrap().fail();
    drop(parsed);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
}

#[cfg_attr(test, test)]
fn json_module_ownership_vector_growth_admits_old_and_new_allocations_together() {
    for object in [false, true] {
        for accepts in [false, true] {
            let mut vm = bounded_vm();
            let (source, peak) = if object {
                (
                    "{\"a\":0,\"b\":1,\"c\":2,\"d\":3,\"e\":4}",
                    12 * std::mem::size_of::<(JsString, JsonNode)>() + 10,
                )
            } else {
                ("[0,1,2,3,4]", 12 * std::mem::size_of::<JsonNode>())
            };
            let _peer = leave_room(&mut vm, peak - usize::from(!accepts));
            let before = vm.host_scheduling_snapshot().payload_bytes;
            let result = JsonSourceParser::for_module(source, vm.host_scheduling.as_ref())
                .unwrap()
                .parse();
            if accepts {
                let parsed = result.unwrap();
                assert_eq!(
                    vm.host_scheduling_snapshot().payload_bytes,
                    before + buffers(&parsed)
                );
                drop(parsed);
            } else {
                assert!(matches!(result, Err(RuntimeError::SchedulingLimit)));
            }
            assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
        }
    }
}

#[cfg_attr(test, test)]
fn json_module_ownership_partial_parse_errors_refund_every_native_buffer() {
    for source in ["[\"a\",", "{\"x\":\"a\",}", "{\"x\":[1]}x", "\"\\u12G4\""] {
        let mut vm = bounded_vm();
        let before = vm.host_scheduling_usage().unwrap().payload_bytes;
        assert!(matches!(
            JsonSourceParser::for_module(source, vm.host_scheduling.as_ref())
                .unwrap()
                .parse(),
            Err(RuntimeError::SyntaxError(_))
        ));
        assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
        assert_eq!(vm.check_scheduling_failure(), Ok(()));
    }
}

#[cfg_attr(test, test)]
fn json_module_ownership_nesting_refusal_is_bounded_with_a_tiny_lowered_limit() {
    for (source, accepts) in [("[[0]]", true), ("[[[0]]]", false)] {
        let mut vm = bounded_vm();
        let before = vm.host_scheduling_usage().unwrap().payload_bytes;
        let mut parser = JsonSourceParser::for_module(source, vm.host_scheduling.as_ref()).unwrap();
        parser.depth_limit = Some(2);
        let result = parser.parse();
        if accepts {
            drop(result.unwrap());
        } else {
            assert!(matches!(result, Err(RuntimeError::InstructionLimit)));
        }
        assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
    }
}

#[cfg_attr(test, test)]
fn json_module_ownership_converter_copy_refusal_cleans_ast_roots_and_orphans() {
    for (source, extra, property, expected) in [
        ("[1]", 2, "0", Value::Number(1.0)),
        ("{\"x\":\"a\"}", 4, "x", Value::String("a".into())),
    ] {
        for accepts in [false, true] {
            let mut vm = bounded_vm();
            vm.stack.push(Value::Number(7.0));
            // Keep the converter-copy budget separate from reusable GC slots.
            vm.stack.push(Value::Object(vm.object_prototype));
            vm.with_roots(|heap| heap.try_collect_major()).unwrap();
            vm.stack.pop();
            let ast = JsonSourceParser::for_module(source, vm.host_scheduling.as_ref())
                .unwrap()
                .parse()
                .unwrap();
            let bytes = buffers(&ast);
            drop(ast);
            let _peer = leave_room(&mut vm, bytes + extra - usize::from(!accepts));
            let before = vm.host_scheduling_snapshot().payload_bytes;
            let managed = vm.heap.stats().managed_bytes;
            let result = vm.json_module_value(source);
            if accepts {
                let value = result.unwrap();
                let object = value.value().object_id().unwrap();
                assert_eq!(vm.heap.get(object, property), Ok(expected.clone()));
                drop(value);
            } else {
                assert!(matches!(result, Err(RuntimeError::SchedulingLimit)));
            }
            assert_eq!(vm.stack, vec![Value::Number(7.0)]);
            assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
            vm.with_roots(|heap| {
                heap.collect_major();
                Ok(())
            })
            .unwrap();
            assert_eq!(vm.heap.stats().managed_bytes, managed);
        }
    }
}

#[cfg_attr(test, test)]
fn json_module_ownership_export_handoff_merges_same_document_ownership_and_rejects_foreign_ownership(
) {
    for terminal in [false, true] {
        let mut vm = bounded_vm();
        let bytes = SyntheticModuleValue::header_bytes() + 2;
        let _peer = leave_room(&mut vm, bytes);
        let before = vm.host_scheduling_snapshot().payload_bytes;
        let owner = vm.prepare_synthetic_value(0).unwrap();
        let copy =
            ModuleValueCopy::copy(&Value::String("a".into()), vm.host_scheduling.as_ref()).unwrap();
        if terminal {
            vm.host_scheduling.as_ref().unwrap().fail();
        }
        let result = owner.adopt_copy(copy);
        if terminal {
            assert!(matches!(result, Err(RuntimeError::SchedulingLimit)));
        } else {
            let value = result.unwrap();
            assert_eq!(value.retained_bytes(), bytes);
            assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before + bytes);
            drop(value);
        }
        assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
    }
    let mut target = bounded_vm();
    let mut other = bounded_vm();
    let before = target.host_scheduling_usage().unwrap().payload_bytes;
    let other_before = other.host_scheduling_usage().unwrap().payload_bytes;
    let owner = target.prepare_synthetic_value(0).unwrap();
    let copy =
        ModuleValueCopy::copy(&Value::String("a".into()), other.host_scheduling.as_ref()).unwrap();
    assert!(matches!(
        owner.adopt_copy(copy),
        Err(RuntimeError::SchedulingLimit)
    ));
    assert_eq!(target.host_scheduling_snapshot().payload_bytes, before);
    assert_eq!(other.host_scheduling_snapshot().payload_bytes, other_before);
    assert_eq!(
        target.check_scheduling_failure(),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(other.check_scheduling_failure(), Ok(()));
}

impl crate::Vm {
    /// Runs finite AST/handoff contracts in unit/coverage builds.
    #[doc(hidden)]
    pub fn verify_json_parser_ownership_contracts() {
        json_module_ownership_live_ast_matches_its_actual_buffers_and_refunds_final_drop();
        json_module_ownership_vector_growth_admits_old_and_new_allocations_together();
        json_module_ownership_partial_parse_errors_refund_every_native_buffer();
        json_module_ownership_nesting_refusal_is_bounded_with_a_tiny_lowered_limit();
        json_module_ownership_converter_copy_refusal_cleans_ast_roots_and_orphans();
        json_module_ownership_export_handoff_merges_same_document_ownership_and_rejects_foreign_ownership();
    }
}
