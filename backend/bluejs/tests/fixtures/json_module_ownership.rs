// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Tiny JSON modules own parser storage without redundant text conversions.

use super::*;
use crate::vm::host_scheduling::SharedSchedulingCharge;

fn bounded_vm(source: &str) -> Vm {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm.remaining_instructions = 100;
    vm.stack.reserve(16);
    vm.set_json_module_sources(HashMap::from([("json".into(), source.into())]));
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

fn synthesize(
    vm: &mut Vm,
    roots: &mut Vec<RootId>,
) -> Result<HashMap<String, Bytecode>, RuntimeError> {
    let mut modules = HashMap::new();
    vm.ensure_synthetic_module("json\0json", SyntheticModuleType::Json, &mut modules, roots)?;
    Ok(modules)
}

#[cfg_attr(test, test)]
fn json_module_ownership_number_needs_only_the_export_owner() {
    let mut vm = bounded_vm("42");
    let _peer = leave_room(&mut vm, SyntheticModuleValue::header_bytes());
    let modules =
        synthesize(&mut vm, &mut Vec::new()).expect("a number has no native parse buffer");
    assert_eq!(
        modules["json\0json"]
            .synthetic_default_export
            .as_ref()
            .unwrap()
            .value(),
        &Value::Number(42.0)
    );
}

#[cfg_attr(test, test)]
fn json_module_ownership_string_adopts_its_copy_without_another_payload_reservation() {
    let mut vm = bounded_vm("\"a\"");
    let _peer = leave_room(&mut vm, SyntheticModuleValue::header_bytes() + 4);
    let modules =
        synthesize(&mut vm, &mut Vec::new()).expect("AST and returned string each own two bytes");
    assert_eq!(
        modules["json\0json"]
            .synthetic_default_export
            .as_ref()
            .unwrap()
            .value(),
        &Value::String("a".into())
    );
}

#[cfg_attr(test, test)]
fn json_module_ownership_container_storage_refuses_before_managed_allocation() {
    for source in ["[0]", "{\"k\":0}"] {
        let mut vm = bounded_vm(source);
        let room = SyntheticModuleValue::header_bytes() + source.encode_utf16().count() * 4;
        let _peer = leave_room(&mut vm, room);
        let before = vm.heap.stats();
        assert!(matches!(
            synthesize(&mut vm, &mut Vec::new()),
            Err(RuntimeError::SchedulingLimit)
        ));
        assert_eq!(vm.heap.stats().managed_bytes, before.managed_bytes);
    }
}

fn value(modules: &HashMap<String, Bytecode>) -> &Value {
    modules["json\0json"]
        .synthetic_default_export
        .as_ref()
        .unwrap()
        .value()
}

#[cfg_attr(test, test)]
fn json_module_ownership_values_preserve_grammar_order_unicode_and_numeric_semantics() {
    for (source, expected) in [
        ("null", "null"),
        ("true", "true"),
        ("false", "false"),
        (" -0 ", "0"),
        ("-12.5e+2", "-1250"),
        ("1e400", "null"),
        ("\"é😀\"", "\"é😀\""),
        ("\"\\ud800\"", "\"\\ud800\""),
        ("[]", "[]"),
        ("{}", "{}"),
        ("{\"b\":1,\"a\":2,\"b\":3}", "{\"b\":3,\"a\":2}"),
        ("[0,1,2,3,4,5,6,7,8,9,10]", "[0,1,2,3,4,5,6,7,8,9,10]"),
    ] {
        let mut vm = bounded_vm(source);
        let mut roots = Vec::new();
        let modules = synthesize(&mut vm, &mut roots).unwrap();
        assert_eq!(
            vm.json_stringify(&[value(&modules).clone()]),
            Ok(Value::String(expected.into())),
            "{source}"
        );
        if source == " -0 " {
            assert!(matches!(value(&modules), Value::Number(number) if number.is_sign_negative()));
        }
        for root in roots {
            vm.heap.unroot(root).unwrap();
        }
    }
}

#[cfg_attr(test, test)]
fn json_module_ownership_complete_syntax_validation_precedes_all_managed_construction() {
    for source in [
        "[{},]",
        "{\"x\":{}}x",
        "[1,]",
        "01",
        "1e+-2",
        "\"\\x\"",
        "{bad}",
    ] {
        let mut vm = bounded_vm(source);
        let before = vm.host_scheduling_usage().unwrap().payload_bytes;
        let managed = vm.heap.stats().managed_bytes;
        assert!(
            matches!(
                synthesize(&mut vm, &mut Vec::new()),
                Err(RuntimeError::ModuleResolution(_))
            ),
            "{source}"
        );
        assert_eq!(vm.heap.stats().managed_bytes, managed);
        assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
    }
}

#[cfg_attr(test, test)]
fn json_module_ownership_string_peak_exact_budget_and_one_byte_refusal() {
    for source in ["\"\"", "\"a\"", "\"😀\"", "\"\\ud800\""] {
        for accepts in [false, true] {
            let units = match source {
                "\"\"" => 0,
                "\"😀\"" => 2,
                _ => 1,
            };
            let mut vm = bounded_vm(source);
            let bytes = SyntheticModuleValue::header_bytes() + units * 4;
            let _peer = leave_room(&mut vm, bytes - usize::from(!accepts));
            let before = vm.host_scheduling_snapshot().payload_bytes;
            let result = synthesize(&mut vm, &mut Vec::new());
            if accepts {
                let modules = result.unwrap();
                assert_eq!(
                    vm.host_scheduling_snapshot().payload_bytes,
                    before + SyntheticModuleValue::header_bytes() + units * 2
                );
                drop(modules);
            } else {
                assert!(matches!(result, Err(RuntimeError::SchedulingLimit)));
            }
            assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
        }
    }
}

#[cfg_attr(test, test)]
fn json_module_ownership_root_storage_is_admitted_before_heap_effects_and_preserves_the_prefix() {
    for accepts in [false, true] {
        let mut vm = bounded_vm("{}");
        vm.stack = Vec::with_capacity(1);
        vm.stack.push(Value::Number(7.0));
        let growth = std::mem::size_of::<Value>();
        let header = SyntheticModuleValue::header_bytes();
        let _peer = leave_room(&mut vm, header + growth - usize::from(!accepts));
        let before = vm.host_scheduling_snapshot().payload_bytes;
        let managed = vm.heap.stats().managed_bytes;
        let mut roots = Vec::new();
        let result = synthesize(&mut vm, &mut roots);
        if accepts {
            let modules = result.unwrap();
            assert_eq!(
                vm.host_scheduling_snapshot().payload_bytes,
                before + header + growth
            );
            drop(modules);
        } else {
            assert!(matches!(result, Err(RuntimeError::SchedulingLimit)));
            assert_eq!(vm.heap.stats().managed_bytes, managed);
        }
        assert_eq!(vm.stack, vec![Value::Number(7.0)]);
        assert_eq!(
            vm.host_scheduling_snapshot().payload_bytes,
            before + if accepts { growth } else { 0 }
        );
        for root in roots {
            vm.heap.unroot(root).unwrap();
        }
    }
}

#[cfg_attr(test, test)]
fn json_module_ownership_resource_refusals_refund_ast_and_preserve_original_error_types() {
    for kind in 0..3 {
        let source = match kind {
            0 => "[1]",
            1 => "\"ab\"",
            _ => "{}",
        };
        let mut vm = bounded_vm(source);
        vm.stack.push(Value::Number(7.0));
        if kind == 0 {
            vm.remaining_instructions = 0;
        }
        if kind == 1 {
            vm.config.max_string_bytes = 2;
        }
        if kind == 2 {
            vm.heap.allow_root_registrations(0);
        }
        let before = vm.host_scheduling_usage().unwrap().payload_bytes;
        let result = synthesize(&mut vm, &mut Vec::new());
        assert!(matches!(
            (kind, result),
            (0, Err(RuntimeError::InstructionLimit))
                | (1, Err(RuntimeError::StringLimit { limit: 2 }))
                | (2, Err(RuntimeError::Heap(HeapError::IdExhausted)))
        ));
        assert_eq!(vm.stack, vec![Value::Number(7.0)]);
        assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
    }
}

#[cfg_attr(test, test)]
fn json_module_ownership_nested_objects_remain_rooted_with_a_one_object_nursery() {
    let mut vm = Vm::new(VmConfig {
        heap: HeapConfig {
            nursery_capacity: 1,
            major_threshold_bytes: 64 * 1024,
            max_heap_bytes: 512 * 1024,
        },
        ..VmConfig::default()
    })
    .unwrap();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm.remaining_instructions = 100;
    vm.stack.push(Value::Number(7.0));
    vm.set_json_module_sources(HashMap::from([(
        "json".into(),
        "{\"parent\":[{\"value\":\"a\"},{\"value\":\"b\"}]}".into(),
    )]));
    let mut roots = Vec::new();
    let modules = synthesize(&mut vm, &mut roots).unwrap();
    vm.with_roots(|heap| {
        heap.collect_major();
        Ok(())
    })
    .unwrap();
    let object = value(&modules).object_id().unwrap();
    let array = vm.heap.get(object, "parent").unwrap().object_id().unwrap();
    for (index, expected) in [("0", "a"), ("1", "b")] {
        let child = vm.heap.get(array, index).unwrap().object_id().unwrap();
        assert_eq!(
            vm.heap.get(child, "value"),
            Ok(Value::String(expected.into()))
        );
    }
    assert!(vm.heap.stats().minor_collections > 0);
    assert_eq!(vm.stack, vec![Value::Number(7.0)]);
    for root in roots {
        vm.heap.unroot(root).unwrap();
    }
}

#[cfg_attr(test, test)]
fn json_module_ownership_real_imports_preserve_identity_surrogates_and_data_properties() {
    let mut vm = bounded_vm("0");
    vm.set_module_loader_context("page/main.js", HashMap::new());
    vm.set_json_module_sources(HashMap::from([(
        "page/data".into(),
        "{\"a\":[1],\"a\":[2,{\"x\":\"\\ud800\"}],\"__proto__\":{\"safe\":true}}".into(),
    )]));
    let code = crate::compile(&crate::parse("var seen=false;Promise.all([import('./data',{with:{type:'json'}}),import('./data',{with:{type:'json'}})]).then(([a,b])=>seen=a.default===b.default&&a.default.a[0]===2&&a.default.a[1].x.charCodeAt(0)===0xd800&&Object.getPrototypeOf(a.default)===Object.prototype&&a.default.__proto__.safe);0;").unwrap()).unwrap();
    vm.execute_script(&code).unwrap();
    for _ in 0..32 {
        if !vm.run_next_promise_job().unwrap() {
            break;
        }
    }
    assert!(vm.promise_jobs.is_empty());
    vm.with_roots(|heap| {
        heap.collect_major();
        Ok(())
    })
    .unwrap();
    assert_eq!(
        vm.lookup_global_name("seen").unwrap(),
        Some(Value::Bool(true))
    );
    assert_eq!(vm.check_scheduling_failure(), Ok(()));
}

impl crate::Vm {
    /// Runs finite JSON-module contracts in unit/coverage builds.
    #[doc(hidden)]
    pub fn verify_json_module_ownership_contracts() {
        json_module_ownership_number_needs_only_the_export_owner();
        json_module_ownership_string_adopts_its_copy_without_another_payload_reservation();
        json_module_ownership_container_storage_refuses_before_managed_allocation();
        json_module_ownership_values_preserve_grammar_order_unicode_and_numeric_semantics();
        json_module_ownership_complete_syntax_validation_precedes_all_managed_construction();
        json_module_ownership_string_peak_exact_budget_and_one_byte_refusal();
        json_module_ownership_root_storage_is_admitted_before_heap_effects_and_preserves_the_prefix(
        );
        json_module_ownership_resource_refusals_refund_ast_and_preserve_original_error_types();
        json_module_ownership_nested_objects_remain_rooted_with_a_one_object_nursery();
        json_module_ownership_real_imports_preserve_identity_surrogates_and_data_properties();
    }
}
