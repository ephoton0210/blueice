// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Tiny frames verify admission before frame allocation and entry effects.

use super::*;
use crate::vm::host_scheduling::SharedSchedulingCharge;

fn bounded_vm() -> Vm {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm.remaining_instructions = 100_000;
    vm
}

fn compile(source: &str) -> Bytecode {
    crate::compile(&crate::parse(source).unwrap()).unwrap()
}

fn leave_room(vm: &mut Vm, room: usize) -> SharedSchedulingCharge {
    let used = vm.host_scheduling_usage().unwrap().payload_bytes;
    vm.host_scheduling
        .as_ref()
        .unwrap()
        .shared_charge(MAX_HOST_SCHEDULING_BYTES - used - room)
        .unwrap()
}

#[cfg_attr(test, test)]
fn frame_binding_admission_root_refuses_before_allocating_bindings() {
    let mut vm = bounded_vm();
    let code = compile("let a=1,b=2,c=3,d=4; a+b+c+d;");
    let before = (vm.bindings.capacity(), vm.binding_metadata.capacity());
    let _peer = leave_room(&mut vm, 0);
    assert_eq!(
        vm.prepare_root_execution(&code, false),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(
        (vm.bindings.capacity(), vm.binding_metadata.capacity()),
        before
    );
}

#[cfg_attr(test, test)]
fn frame_binding_admission_constructor_refuses_before_field_initializers() {
    let mut vm = bounded_vm();
    let seen = Rc::new(Cell::new(false));
    let output = seen.clone();
    vm.install_host_function("observe", 0, move |_: &[HostValue]| {
        output.set(true);
        Ok(HostValue::Undefined)
    })
    .unwrap();
    let parameters = (0..48)
        .map(|i| format!("p{i}"))
        .collect::<Vec<_>>()
        .join(",");
    let callee = vm
        .execute_script(&compile(&format!(
            "class C {{field=observe(); constructor({parameters}){{}}}} C;"
        )))
        .unwrap();
    vm.stack.reserve(64);
    vm.call_stack.reserve(16);
    let _peer = leave_room(&mut vm, 2048);
    assert_eq!(
        vm.call_native(callee, Value::Undefined, Vec::new(), true),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(!seen.get(), "field initializer ran before frame admission");
    assert_eq!(vm.call_depth, 0);
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
}

fn binding_bytes(code: &Bytecode, kind: &FrameBindingKind) -> usize {
    let mut bytes = code.bindings.len() * std::mem::size_of::<Binding>()
        + code
            .bindings
            .iter()
            .map(|binding| binding.name.len())
            .sum::<usize>();
    if !matches!(kind, FrameBindingKind::ResumedGenerator) {
        bytes += code.bindings.len() * std::mem::size_of::<Option<Value>>();
    }
    if !matches!(kind, FrameBindingKind::Script) {
        for slot in &code.scopes[code.variable_scope as usize] {
            let binding = &code.bindings[*slot as usize];
            if binding.lexical {
                bytes += std::mem::size_of::<String>() + binding.name.len();
            }
        }
    }
    bytes
}

#[cfg_attr(test, test)]
fn frame_binding_admission_staging_retains_one_charge_across_callbacks() {
    for kind in [
        FrameBindingKind::Script,
        FrameBindingKind::Function,
        FrameBindingKind::ResumedGenerator,
    ] {
        let mut vm = bounded_vm();
        let code = compile("let first=1,second=2; first+second;");
        let expected = binding_bytes(&code, &kind);
        let observed = Rc::new(Cell::new(0));
        let output = observed.clone();
        let counter = vm.displaced_execution_control_bytes.clone();
        vm.install_host_function("observe", 0, move |_: &[HostValue]| {
            output.set(counter.get());
            Ok(HostValue::Undefined)
        })
        .unwrap();
        let callback = vm.lookup_global_name("observe").unwrap().unwrap();
        let mut prepared = vm.prepare_frame_bindings(&code, kind).unwrap();
        vm.call_native(callback, Value::Undefined, Vec::new(), false)
            .unwrap();
        assert_eq!(observed.get(), expected);
        let parts = prepared.take_parts();
        assert_eq!(vm.displaced_execution_control_bytes.get(), expected);
        drop(parts);
        drop(prepared);
        vm.reconcile_scheduling_ownership();
        assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
    }
}

#[cfg_attr(test, test)]
fn frame_binding_admission_exact_budget_and_one_byte_refusal() {
    for room_delta in [0, 1] {
        for kind in [
            FrameBindingKind::Script,
            FrameBindingKind::Function,
            FrameBindingKind::ResumedGenerator,
        ] {
            let mut vm = bounded_vm();
            let code = compile("let first=1,second=2; first+second;");
            let expected = binding_bytes(&code, &kind);
            let _peer = leave_room(&mut vm, expected - room_delta);
            let result = vm.prepare_frame_bindings(&code, kind);
            if room_delta == 0 {
                let prepared = result.unwrap();
                assert_eq!(vm.displaced_execution_control_bytes.get(), expected);
                drop(prepared);
            } else {
                assert!(matches!(result, Err(RuntimeError::SchedulingLimit)));
            }
            assert!(vm.bindings.is_empty() && vm.binding_metadata.is_empty());
            assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
        }
    }
}

#[cfg_attr(test, test)]
fn frame_binding_admission_eval_and_nested_refusal_preserve_the_caller() {
    for nested in [false, true] {
        let mut vm = bounded_vm();
        vm.global("globalThis").unwrap();
        vm.stack.reserve(32);
        vm.stack.push(Value::Number(7.0));
        vm.bindings = vec![Some(Value::Number(42.0))];
        vm.binding_metadata = compile("let original;").bindings;
        let original_metadata = vm.binding_metadata.clone();
        let code = compile("let a=1,b=2,c=3; a+b+c;");
        let _peer = leave_room(&mut vm, 0);
        let result = if nested {
            vm.execute_nested_script(&code)
        } else {
            vm.execute_eval(&code, Vec::new(), false)
        };
        assert_eq!(result, Err(RuntimeError::SchedulingLimit));
        assert_eq!(vm.bindings, vec![Some(Value::Number(42.0))]);
        assert!(vm.binding_metadata == original_metadata);
        assert_eq!(vm.stack, vec![Value::Number(7.0)]);
        assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
    }
}

#[cfg_attr(test, test)]
fn frame_binding_admission_module_refusal_preserves_input_cells_and_context() {
    let mut vm = bounded_vm();
    let code = crate::compile_module(&crate::parse_module("export let a=1,b=2;").unwrap()).unwrap();
    let cell = vm.heap.alloc_object(None).unwrap();
    let mut cells: crate::slot_map::CellMap = [(0, cell)].into_iter().collect();
    vm.stack.push(Value::Number(7.0));
    let _peer = leave_room(&mut vm, 0);
    assert_eq!(
        vm.enter_module_record(&code, &mut cells),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(
        cells,
        [(0, cell)]
            .into_iter()
            .collect::<crate::slot_map::CellMap>()
    );
    assert_eq!(vm.stack, vec![Value::Number(7.0)]);
    assert!(vm.bindings.is_empty() && vm.binding_metadata.is_empty());
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
}

#[cfg_attr(test, test)]
fn frame_binding_admission_generator_refusal_preserves_saved_state() {
    for suspended in [false, true] {
        let mut vm = bounded_vm();
        let generator = vm
            .execute_script(&compile(
                "(function*(){let a=1,b=2; yield a+b; yield 4;})();",
            ))
            .unwrap();
        if suspended {
            vm.generator_next(&generator, None, None).unwrap();
        }
        let id = generator.object_id().unwrap();
        assert_eq!(
            vm.heap.generator_execution_code(id).unwrap().unwrap().1,
            !suspended
        );
        let before = vm.heap.stats().managed_bytes;
        let _peer = leave_room(&mut vm, 0);
        assert_eq!(
            vm.generator_next(&generator, None, None),
            Err(RuntimeError::SchedulingLimit)
        );
        assert_eq!(
            vm.heap.generator_execution_code(id).unwrap().unwrap().1,
            !suspended
        );
        assert_eq!(vm.heap.stats().managed_bytes, before);
        assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
    }
}

#[cfg_attr(test, test)]
fn frame_binding_admission_parameter_refusal_precedes_initializers() {
    for kind in ["function", "function*", "async function", "async function*"] {
        let mut vm = bounded_vm();
        let seen = Rc::new(Cell::new(false));
        let output = seen.clone();
        vm.install_host_function("observe", 0, move |_: &[HostValue]| {
            output.set(true);
            Ok(HostValue::Undefined)
        })
        .unwrap();
        let parameters = (1..48)
            .map(|i| format!("p{i}"))
            .collect::<Vec<_>>()
            .join(",");
        let source = format!("({kind}(first=observe(),{parameters}){{}});");
        let callee = vm.execute_script(&compile(&source)).unwrap();
        vm.stack.reserve(64);
        vm.call_stack.reserve(16);
        let _peer = leave_room(&mut vm, 2048);
        assert_eq!(
            vm.call_native(callee, Value::Undefined, Vec::new(), false),
            Err(RuntimeError::SchedulingLimit)
        );
        assert!(!seen.get(), "{kind}");
        assert!(
            vm.promises.is_empty(),
            "refused entry published a Promise capability"
        );
        assert_eq!(vm.call_depth, 0);
        assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
    }
}

impl crate::Vm {
    /// Runs finite frame-binding admission contracts in unit and coverage builds.
    #[doc(hidden)]
    pub fn verify_frame_binding_admission_contracts() {
        frame_binding_admission_root_refuses_before_allocating_bindings();
        frame_binding_admission_constructor_refuses_before_field_initializers();
        frame_binding_admission_staging_retains_one_charge_across_callbacks();
        frame_binding_admission_exact_budget_and_one_byte_refusal();
        frame_binding_admission_eval_and_nested_refusal_preserve_the_caller();
        frame_binding_admission_module_refusal_preserves_input_cells_and_context();
        frame_binding_admission_generator_refusal_preserves_saved_state();
        frame_binding_admission_parameter_refusal_precedes_initializers();
    }
}
