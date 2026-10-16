// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Tiny dynamic environments check entry admission before observable work.

use super::*;
use crate::vm::host_scheduling::SharedSchedulingCharge;

fn compile(source: &str) -> Bytecode {
    crate::compile(&crate::parse(source).unwrap()).unwrap()
}

fn bounded_vm() -> Vm {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm.remaining_instructions = 100_000;
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

fn binding_bytes(vm: &mut Vm, callee: &Value) -> usize {
    let code = vm
        .heap
        .closure_code(callee.object_id().unwrap())
        .unwrap()
        .unwrap();
    let prepared = vm
        .prepare_frame_bindings(&code, FrameBindingKind::Function)
        .unwrap();
    let bytes = vm.displaced_execution_control_bytes.get();
    drop(prepared);
    vm.reconcile_scheduling_ownership();
    bytes
}

#[cfg_attr(test, test)]
fn environment_admission_refuses_scope_growth_before_constructor_getters() {
    let mut vm = bounded_vm();
    let seen = Rc::new(Cell::new(false));
    let output = seen.clone();
    vm.install_host_function("observe", 0, move |_: &[HostValue]| {
        output.set(true);
        Ok(HostValue::Undefined)
    })
    .unwrap();
    let target = vm.execute_script(&compile("function T(){};var B=T.bind(null);Object.defineProperty(B,'prototype',{get:observe});B;")).unwrap();
    let root = vm.heap.root(target.object_id().unwrap()).unwrap();
    let callee = vm
        .execute_script(&compile("class C{constructor(){}} C;"))
        .unwrap();
    let bindings = binding_bytes(&mut vm, &callee);
    vm.dynamic_eval_outer_bindings = Vec::with_capacity(4);
    for _ in 0..4 {
        vm.dynamic_eval_outer_bindings.push(HashMap::new());
    }
    vm.stack.reserve(64);
    vm.call_stack.reserve(16);
    assert!(4 * std::mem::size_of::<HashMap<String, DynamicEvalBinding>>() > 128);
    let _peer = leave_room(&mut vm, bindings + 128);
    assert_eq!(
        vm.call_with_target(callee, Value::Undefined, Vec::new(), true, target),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(
        !seen.get(),
        "constructor getter ran before environment-stack growth was admitted"
    );
    assert_eq!(vm.dynamic_eval_outer_bindings.len(), 4);
    vm.heap.unroot(root).unwrap();
}

#[cfg_attr(test, test)]
fn environment_admission_refuses_recovery_before_publishing_async_capability() {
    let mut vm = bounded_vm();
    let callee = vm
        .execute_script(&compile("(async function(){await 0;return 1;});"))
        .unwrap();
    let bindings = binding_bytes(&mut vm, &callee);
    let warm = vm.new_promise().unwrap();
    let root = vm.heap.root(warm).unwrap();
    assert!(vm.promises.len() < vm.promises.capacity());
    let promise_count = vm.promises.len();
    vm.dynamic_eval_bindings.insert(
        "retained".into(),
        DynamicEvalBinding {
            cell: vm.object_prototype,
            shadowed_cells: vec![vm.array_prototype],
        },
    );
    vm.dynamic_eval_bindings.reserve(31);
    let capacity = vm.dynamic_eval_bindings.capacity();
    vm.dynamic_eval_outer_bindings.reserve(4);
    vm.stack.reserve(64);
    vm.call_stack.reserve(16);
    let _peer = leave_room(&mut vm, bindings);
    assert_eq!(
        vm.call_native(callee, Value::Undefined, Vec::new(), false),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(
        vm.promises.len(),
        promise_count,
        "recovery refusal published an async result capability"
    );
    assert_eq!(vm.dynamic_eval_bindings.capacity(), capacity);
    assert!(vm.dynamic_eval_bindings.contains_key("retained"));
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
    vm.heap.unroot(root).unwrap();
}

fn populate_environment(vm: &mut Vm, spare: bool) {
    let mut name = String::with_capacity(64);
    name.push_str("retained");
    let mut shadowed_cells = Vec::with_capacity(9);
    shadowed_cells.push(vm.array_prototype);
    vm.dynamic_eval_bindings.insert(
        name,
        DynamicEvalBinding {
            cell: vm.object_prototype,
            shadowed_cells,
        },
    );
    vm.dynamic_eval_bindings.reserve(5);
    vm.dynamic_eval_outer_bindings = Vec::with_capacity(if spare { 4 } else { 1 });
    vm.dynamic_eval_outer_bindings.push(HashMap::from([(
        "outer".into(),
        DynamicEvalBinding {
            cell: vm.array_prototype,
            shadowed_cells: Vec::new(),
        },
    )]));
    let mut module = String::with_capacity(64);
    module.push_str("memory:///owner.js");
    vm.active_module_name = Some(module);
}

fn recovery_bytes(vm: &Vm) -> usize {
    let mut copies = Vec::with_capacity(vm.dynamic_eval_outer_bindings.len() + 1);
    copies.extend(vm.dynamic_eval_outer_bindings.iter().cloned());
    copies.push(vm.dynamic_eval_bindings.clone());
    let module = vm.active_module_name.clone();
    outer_binding_bytes(&copies) + module.as_ref().map_or(0, String::capacity)
}

#[cfg_attr(test, test)]
fn environment_admission_exact_joint_budget_and_one_byte_refusal() {
    for can_suspend in [false, true] {
        for spare in [false, true] {
            for empty in [false, true] {
                for accepts in [false, true] {
                    let mut vm = bounded_vm();
                    populate_environment(&mut vm, spare);
                    if empty {
                        vm.dynamic_eval_bindings.clear();
                    }
                    let growth = HostSchedulingUsage::default()
                        .with_growth(SchedulingCapacity::for_vec_push(
                            &vm.dynamic_eval_outer_bindings,
                        ))
                        .payload_bytes;
                    let copied = if can_suspend { recovery_bytes(&vm) } else { 0 };
                    let bytes = growth + copied;
                    if bytes == 0 && !accepts {
                        continue;
                    }
                    let capacity = vm.dynamic_eval_outer_bindings.capacity();
                    let _peer = leave_room(&mut vm, bytes - usize::from(!accepts));
                    let result = vm.prepare_call_environment(can_suspend);
                    if accepts {
                        let mut prepared = result.unwrap();
                        assert_eq!(vm.displaced_execution_control_bytes.get(), copied);
                        let parts = prepared.take_parts();
                        assert_eq!(parts.0.len(), if can_suspend { 2 } else { 0 });
                        assert_eq!(vm.displaced_execution_control_bytes.get(), copied);
                        drop(parts);
                        drop(prepared);
                        vm.reconcile_scheduling_ownership();
                    } else {
                        assert!(matches!(result, Err(RuntimeError::SchedulingLimit)));
                        assert_eq!(vm.dynamic_eval_outer_bindings.capacity(), capacity);
                    }
                    assert_eq!(vm.dynamic_eval_outer_bindings.len(), 1);
                    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
                }
            }
        }
    }
}

#[cfg_attr(test, test)]
fn environment_admission_sync_spare_slots_create_no_recovery_copy() {
    let mut vm = bounded_vm();
    populate_environment(&mut vm, true);
    let pointer = vm
        .dynamic_eval_bindings
        .get_key_value("retained")
        .unwrap()
        .0
        .as_ptr();
    let module_pointer = vm.active_module_name.as_ref().unwrap().as_ptr();
    let _peer = leave_room(&mut vm, 0);
    let mut prepared = vm.prepare_call_environment(false).unwrap();
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
    let parts = prepared.take_parts();
    assert!(parts.0.is_empty() && parts.1.is_none());
    assert_eq!(
        vm.dynamic_eval_bindings
            .get_key_value("retained")
            .unwrap()
            .0
            .as_ptr(),
        pointer
    );
    assert_eq!(
        vm.active_module_name.as_ref().unwrap().as_ptr(),
        module_pointer
    );
}

#[cfg_attr(test, test)]
fn environment_admission_recovery_owns_detached_buffers_during_callbacks() {
    let mut vm = bounded_vm();
    populate_environment(&mut vm, true);
    let expected = recovery_bytes(&vm);
    let observed = Rc::new(Cell::new(0));
    let output = observed.clone();
    let counter = vm.displaced_execution_control_bytes.clone();
    vm.install_host_function("observe", 0, move |_: &[HostValue]| {
        output.set(counter.get());
        Ok(HostValue::Undefined)
    })
    .unwrap();
    let callback = vm.lookup_global_name("observe").unwrap().unwrap();
    let mut prepared = vm.prepare_call_environment(true).unwrap();
    let parts = prepared.take_parts();
    let (name, binding) = parts.0.last().unwrap().get_key_value("retained").unwrap();
    let original = vm.dynamic_eval_bindings.get_key_value("retained").unwrap();
    assert_ne!(name.as_ptr(), original.0.as_ptr());
    assert_eq!(name.capacity(), name.len());
    assert_eq!(binding.shadowed_cells.capacity(), 1);
    assert_eq!(original.0.capacity(), 64);
    assert_eq!(original.1.shadowed_cells.capacity(), 9);
    assert_eq!(parts.1.as_deref(), Some("memory:///owner.js"));
    vm.call_native(callback, Value::Undefined, Vec::new(), false)
        .unwrap();
    assert_eq!(observed.get(), expected);
    drop(parts);
    drop(prepared);
    vm.reconcile_scheduling_ownership();
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
}

#[cfg_attr(test, test)]
fn environment_admission_refusal_preserves_caller_and_peer_contribution() {
    let mut vm = bounded_vm();
    populate_environment(&mut vm, true);
    vm.stack.push(Value::Number(7.0));
    let peer = Vm {
        host_scheduling: Some(vm.host_scheduling.as_ref().unwrap().child()),
        ..Vm::default()
    };
    let used = vm.host_scheduling_usage().unwrap().payload_bytes;
    let capacity = vm.dynamic_eval_outer_bindings.capacity();
    let bytes = recovery_bytes(&vm);
    let owner = leave_room(&mut vm, bytes - 1);
    assert!(matches!(
        vm.prepare_call_environment(true),
        Err(RuntimeError::SchedulingLimit)
    ));
    assert_eq!(vm.stack, vec![Value::Number(7.0)]);
    assert_eq!(vm.dynamic_eval_outer_bindings.len(), 1);
    assert_eq!(vm.dynamic_eval_outer_bindings.capacity(), capacity);
    assert!(vm.dynamic_eval_bindings.contains_key("retained"));
    assert_eq!(vm.active_module_name.as_deref(), Some("memory:///owner.js"));
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
    drop(owner);
    assert_eq!(peer.host_scheduling_snapshot().payload_bytes, used);
}

#[cfg_attr(test, test)]
fn environment_admission_module_copy_is_part_of_joint_admission() {
    let mut vm = bounded_vm();
    vm.dynamic_eval_outer_bindings.reserve(4);
    vm.active_module_name = Some("memory:///owner.js".into());
    let bytes = std::mem::size_of::<HashMap<String, DynamicEvalBinding>>();
    let _peer = leave_room(&mut vm, bytes);
    assert!(matches!(
        vm.prepare_call_environment(true),
        Err(RuntimeError::SchedulingLimit)
    ));
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
    assert_eq!(vm.active_module_name.as_deref(), Some("memory:///owner.js"));
}

#[cfg_attr(test, test)]
fn environment_admission_await_restores_the_caller_environment_and_module() {
    for nursery in [1, 16] {
        let mut config = VmConfig::default();
        config.heap.nursery_capacity = nursery;
        let mut vm = Vm::new(config).unwrap();
        vm.use_bounded_host_microtask_checkpoints().unwrap();
        let callee = vm.execute_script(&compile("var gate=Promise.withResolvers(); (async function(){await gate.promise;return 2;});")).unwrap();
        populate_environment(&mut vm, true);
        let promise = vm
            .call_native(callee, Value::Undefined, Vec::new(), false)
            .unwrap();
        let root = vm.heap.root(promise.object_id().unwrap()).unwrap();
        assert_eq!(vm.dynamic_eval_outer_bindings.len(), 1);
        assert!(vm.dynamic_eval_bindings.contains_key("retained"));
        assert_eq!(vm.active_module_name.as_deref(), Some("memory:///owner.js"));
        assert_eq!(vm.async_continuations.len(), 1);
        let gate = vm.lookup_global_name("gate").unwrap().unwrap();
        let resolve = vm.get_property(&gate, &"resolve".into()).unwrap();
        vm.call_native(resolve, Value::Undefined, Vec::new(), false)
            .unwrap();
        for _ in 0..32 {
            if !vm.run_next_promise_job().unwrap() {
                break;
            }
        }
        assert!(vm.async_continuations.is_empty());
        assert!(vm.dynamic_eval_bindings.contains_key("retained"));
        assert_eq!(vm.dynamic_eval_outer_bindings.len(), 1);
        assert_eq!(vm.active_module_name.as_deref(), Some("memory:///owner.js"));
        assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
        vm.heap.unroot(root).unwrap();
    }
}

impl crate::Vm {
    /// Runs finite environment-entry admission contracts in unit/coverage builds.
    #[doc(hidden)]
    pub fn verify_environment_admission_contracts() {
        environment_admission_refuses_scope_growth_before_constructor_getters();
        environment_admission_refuses_recovery_before_publishing_async_capability();
        environment_admission_exact_joint_budget_and_one_byte_refusal();
        environment_admission_sync_spare_slots_create_no_recovery_copy();
        environment_admission_recovery_owns_detached_buffers_during_callbacks();
        environment_admission_refusal_preserves_caller_and_peer_contribution();
        environment_admission_module_copy_is_part_of_joint_admission();
        environment_admission_await_restores_the_caller_environment_and_module();
    }
}
