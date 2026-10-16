// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Small generator parameter frames admit roots before creating their object.

use super::*;
use crate::vm::host_scheduling::SharedSchedulingCharge;

fn compile(source: &str) -> Bytecode {
    crate::compile(&crate::parse(source).unwrap()).unwrap()
}

fn bounded_vm() -> Vm {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm.global("globalThis").unwrap();
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

fn preparation_bytes(vm: &mut Vm, callee: &Value) -> usize {
    let code = vm
        .heap
        .closure_code(callee.object_id().unwrap())
        .unwrap()
        .unwrap();
    assert!(code.generator_initializes_parameters);
    let before = vm.displaced_execution_control_bytes.get();
    let bindings = vm
        .prepare_frame_bindings(&code, FrameBindingKind::Function)
        .unwrap();
    let maps = vm.prepare_frame_maps(0, 0, 0).unwrap();
    let bytes = vm.displaced_execution_control_bytes.get() - before;
    drop((bindings, maps));
    vm.reconcile_scheduling_ownership();
    bytes
}

#[cfg_attr(test, test)]
fn generator_entry_admission_root_refusal_precedes_generator_allocation() {
    for kind in ["function*", "async function*"] {
        let mut vm = bounded_vm();
        let callee = vm
            .execute_script(&compile(&format!("({kind}(a=0){{yield a;}});")))
            .unwrap();
        let bytes = preparation_bytes(&mut vm, &callee);
        vm.stack = Vec::with_capacity(4);
        vm.call_stack.reserve(16);
        vm.bindings = vec![Some(Value::Object(vm.object_prototype)); 48];
        vm.with_roots(|heap| {
            heap.collect_major();
            Ok(())
        })
        .unwrap();
        let before = vm.heap.stats().managed_bytes;
        let capacity = vm.stack.capacity();
        let _peer = leave_room(&mut vm, bytes + 128);
        assert_eq!(
            vm.call_native(callee, Value::Undefined, Vec::new(), false),
            Err(RuntimeError::SchedulingLimit)
        );
        assert_eq!(
            vm.heap.stats().managed_bytes,
            before,
            "{kind}: a generator was allocated before root admission"
        );
        assert_eq!(vm.stack.capacity(), capacity);
        assert!(vm.stack.is_empty());
        assert_eq!(vm.bindings.len(), 48);
        assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
    }
}

#[cfg_attr(test, test)]
fn generator_entry_admission_parameter_environment_refusal_precedes_allocation() {
    for kind in ["function*", "async function*"] {
        let mut vm = bounded_vm();
        let callee = vm
            .execute_script(&compile(&format!("({kind}(a=eval('0')){{yield a;}});")))
            .unwrap();
        let bytes = preparation_bytes(&mut vm, &callee);
        vm.stack.reserve(128);
        vm.call_stack.reserve(16);
        vm.with_roots(|heap| {
            heap.collect_major();
            Ok(())
        })
        .unwrap();
        let before = vm.heap.stats().managed_bytes;
        let _peer = leave_room(&mut vm, bytes);
        assert_eq!(
            vm.call_native(callee, Value::Undefined, Vec::new(), false),
            Err(RuntimeError::SchedulingLimit)
        );
        assert_eq!(
            vm.heap.stats().managed_bytes,
            before,
            "{kind}: an environment or generator was allocated before admission"
        );
        assert!(vm.stack.is_empty());
        assert!(vm.with_objects.is_empty());
        assert!(vm.parameter_eval_env.is_none());
        assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
    }
}

fn entry_charge(vm: &Vm, inherited: &Vec<Value>) -> CallEntryCharge {
    let mut entry = vm.call_entry_charge(&Value::Undefined, &Value::Undefined, &Vec::new());
    entry.refresh(
        &Value::Undefined,
        &Value::Undefined,
        &Vec::new(),
        host_scheduling::slots(inherited) + host_scheduling::values_bytes(inherited),
    );
    entry
}

#[cfg_attr(test, test)]
fn generator_entry_admission_joint_exact_budget_and_one_byte_refusal() {
    for spare in [false, true] {
        for parameter_env in [false, true] {
            for receiver in [false, true] {
                for accepts in [false, true] {
                    let mut vm = bounded_vm();
                    vm.stack = Vec::with_capacity(if spare { 16 } else { 1 });
                    vm.stack.push(Value::Number(7.0));
                    let mut inherited = Vec::with_capacity(if spare { 4 } else { 1 });
                    inherited.push(Value::Object(vm.object_prototype));
                    let mut entry = entry_charge(&vm, &inherited);
                    let initial = vm.displaced_execution_control_bytes.get();
                    // Four placeholders, one inherited edge, outer generator/
                    // optional receiver and the optional environment scratch root.
                    let required: usize =
                        1 + 4 + 1 + 1 + usize::from(receiver) + usize::from(parameter_env);
                    let bytes = required.saturating_sub(vm.stack.capacity())
                        * std::mem::size_of::<Value>()
                        + (1 + usize::from(parameter_env)).saturating_sub(inherited.capacity())
                            * std::mem::size_of::<Value>();
                    if bytes == 0 && !accepts {
                        continue;
                    }
                    let capacities = (vm.stack.capacity(), inherited.capacity());
                    let _peer = leave_room(&mut vm, bytes - usize::from(!accepts));
                    let result = vm.prepare_generator_entry(
                        &mut inherited,
                        receiver,
                        parameter_env,
                        &mut entry,
                    );
                    if accepts {
                        result.unwrap();
                        assert!(vm.stack.capacity() >= required);
                        assert!(inherited.capacity() > usize::from(parameter_env));
                        assert_eq!(
                            vm.displaced_execution_control_bytes.get(),
                            initial
                                + (inherited.capacity() - capacities.1)
                                    * std::mem::size_of::<Value>()
                        );
                    } else {
                        assert_eq!(result, Err(RuntimeError::SchedulingLimit));
                        assert_eq!((vm.stack.capacity(), inherited.capacity()), capacities);
                    }
                    assert_eq!(vm.stack, vec![Value::Number(7.0)]);
                    assert_eq!(inherited, vec![Value::Object(vm.object_prototype)]);
                    drop(entry);
                    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
                }
            }
        }
    }
}

#[cfg_attr(test, test)]
fn generator_entry_admission_spare_slots_need_no_growth_or_primitive_root_copies() {
    let mut vm = bounded_vm();
    vm.stack = Vec::with_capacity(32);
    vm.stack.push(Value::Number(7.0));
    vm.with_objects = vec![Value::String("caller".into())];
    let mut inherited = Vec::with_capacity(4);
    inherited.push(Value::String("callee".into()));
    let mut entry = entry_charge(&vm, &inherited);
    let pointer = vm.stack.as_ptr();
    let inherited_pointer = inherited.as_ptr();
    let _peer = leave_room(&mut vm, 0);
    vm.prepare_generator_entry(&mut inherited, false, true, &mut entry)
        .unwrap();
    vm.push_generator_entry_roots(&Value::String("receiver".into()), &inherited);
    assert_eq!(vm.stack.as_ptr(), pointer);
    assert_eq!(inherited.as_ptr(), inherited_pointer);
    assert_eq!(
        vm.stack,
        vec![
            Value::Number(7.0),
            Value::Undefined,
            Value::Undefined,
            Value::Undefined,
            Value::Undefined
        ]
    );
    vm.synchronize_scheduling_usage().unwrap();
}

#[cfg_attr(test, test)]
fn generator_entry_admission_forwarded_environment_keeps_one_native_owner() {
    let mut vm = bounded_vm();
    vm.stack.reserve(16);
    let mut inherited = vec![Value::Object(vm.object_prototype)];
    let mut entry = entry_charge(&vm, &inherited);
    vm.prepare_generator_entry(&mut inherited, true, true, &mut entry)
        .unwrap();
    let expected = host_scheduling::slots(&inherited);
    assert_eq!(vm.displaced_execution_control_bytes.get(), expected);
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    vm.with_objects = inherited;
    drop(entry);
    vm.synchronize_scheduling_usage().unwrap();
    assert_eq!(vm.host_scheduling_usage().unwrap().payload_bytes, before);
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
}

#[cfg_attr(test, test)]
fn generator_entry_admission_roots_keep_displaced_objects_until_caller_restoration() {
    let mut vm = bounded_vm();
    let mut objects = Vec::new();
    for _ in 0..21 {
        let id = vm.with_roots(|heap| heap.alloc_object(None)).unwrap();
        vm.stack.push(Value::Object(id));
        objects.push(id);
    }
    vm.bindings = vec![Some(Value::Object(objects[0]))];
    vm.cells.insert(0, objects[1]);
    vm.completion = Value::Object(objects[2]);
    vm.this = Value::Object(objects[3]);
    vm.callee = Value::Object(objects[4]);
    vm.arguments = vec![Value::Object(objects[5])];
    vm.with_objects = vec![Value::Object(objects[6])];
    let mut inherited = vec![Value::Object(objects[7])];
    vm.new_target = Value::Object(objects[8]);
    vm.home_object = Some(objects[9]);
    vm.parameter_eval_env = Some(objects[10]);
    vm.dynamic_eval_bindings.insert(
        "current".into(),
        DynamicEvalBinding {
            cell: objects[11],
            shadowed_cells: vec![objects[12]],
        },
    );
    vm.dynamic_eval_outer_bindings.push(HashMap::from([(
        "outer".into(),
        DynamicEvalBinding {
            cell: objects[13],
            shadowed_cells: vec![objects[14]],
        },
    )]));
    vm.pending_completions = vec![Completion::Return(Value::Object(objects[15]))];
    vm.completion_saves = vec![(Value::Object(objects[16]), false)];
    vm.disposables = vm.owned_records(vec![DisposableResource {
        receiver: Value::Object(objects[17]),
        argument: Some(Value::Object(objects[18])),
        method: Some(Value::Object(objects[19])),
        hint: crate::heap::DisposeHint::Sync,
        sync_fallback: false,
    }]);
    vm.stack.clear();
    let mut entry = entry_charge(&vm, &inherited);
    vm.prepare_generator_entry(&mut inherited, true, true, &mut entry)
        .unwrap();
    let pointer = vm.stack.as_ptr();
    vm.push_generator_entry_roots(&Value::Object(objects[20]), &inherited);
    assert_eq!(vm.stack.len(), objects.len());
    assert_eq!(vm.stack.as_ptr(), pointer);
    for id in &objects {
        assert!(vm.stack.contains(&Value::Object(*id)));
    }
    // The same fields that entry displaces now have only their stack edges.
    let displaced = (
        std::mem::take(&mut vm.bindings),
        std::mem::take(&mut vm.cells),
        std::mem::replace(&mut vm.completion, Value::Undefined),
        std::mem::replace(&mut vm.this, Value::Undefined),
        std::mem::replace(&mut vm.callee, Value::Undefined),
        std::mem::take(&mut vm.arguments),
        std::mem::take(&mut vm.with_objects),
        std::mem::replace(&mut vm.new_target, Value::Undefined),
        vm.home_object.take(),
        vm.parameter_eval_env.take(),
        std::mem::take(&mut vm.dynamic_eval_bindings),
        std::mem::take(&mut vm.dynamic_eval_outer_bindings),
        std::mem::take(&mut vm.pending_completions),
        std::mem::take(&mut vm.completion_saves),
        std::mem::take(&mut vm.disposables),
    );
    vm.with_roots(|heap| {
        heap.collect_major();
        Ok(())
    })
    .unwrap();
    for id in &objects {
        assert!(vm.heap.get_own(*id, "alive").is_ok());
    }
    vm.stack.clear();
    drop((displaced, inherited, entry));
    vm.with_roots(|heap| {
        heap.collect_major();
        Ok(())
    })
    .unwrap();
    for id in objects {
        assert_eq!(
            vm.heap.get_own(id, "alive"),
            Err(HeapError::InvalidObject(id))
        );
    }
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
}

#[cfg_attr(test, test)]
fn generator_entry_admission_real_captured_eval_parameters_survive_collection() {
    for nursery in [1, 16] {
        for kind in ["function*", "async function*"] {
            let mut config = VmConfig::default();
            config.heap.nursery_capacity = nursery;
            let mut vm = Vm::new(config).unwrap();
            vm.use_bounded_host_microtask_checkpoints().unwrap();
            let global = vm.global("globalThis").unwrap().object_id().unwrap();
            let prototype = vm.function_prototype().unwrap();
            // Install only the existing explicit collection hook; no agents.
            vm.install_native(
                global,
                prototype,
                "collect",
                0,
                NativeFunction::Test262("gc"),
            )
            .unwrap();
            let consume = if kind == "function*" {
                "answer=step.value;"
            } else {
                "step.then(r=>answer=r.value);"
            };
            let source = format!("var answer=0;var g;with({{outer:11}}){{g=({kind}(a=(collect(),eval('var local=31;local'))){{yield outer+a;}});}}var frame=g();var step=frame.next();{consume}0;");
            vm.execute_script(&compile(&source)).unwrap();
            for _ in 0..16 {
                if !vm.run_next_promise_job().unwrap() {
                    break;
                }
            }
            assert_eq!(
                vm.lookup_global_name("answer").unwrap(),
                Some(Value::Number(42.0)),
                "{kind}, nursery={nursery}"
            );
            assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
            assert_eq!(vm.displaced_execution_bytes.get(), 0);
        }
    }
}

#[cfg_attr(test, test)]
fn generator_entry_admission_parameter_throw_restores_caller_and_releases_leases() {
    for kind in ["function*", "async function*"] {
        let mut vm = bounded_vm();
        let callee = vm
            .execute_script(&compile(&format!("({kind}(a=eval('throw 42')){{}});")))
            .unwrap();
        vm.stack.push(Value::Number(7.0));
        vm.with_objects = vec![Value::Object(vm.object_prototype)];
        vm.inherited_with_depth = 1;
        vm.bindings = vec![Some(Value::Number(31.0))];
        assert_eq!(
            vm.call_native(callee, Value::Undefined, Vec::new(), false),
            Err(RuntimeError::Thrown(Value::Number(42.0)))
        );
        assert_eq!(vm.stack, vec![Value::Number(7.0)]);
        assert_eq!(vm.bindings, vec![Some(Value::Number(31.0))]);
        assert_eq!(vm.with_objects, vec![Value::Object(vm.object_prototype)]);
        assert_eq!(vm.inherited_with_depth, 1);
        assert!(vm.parameter_eval_env.is_none());
        assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
        assert_eq!(vm.displaced_execution_bytes.get(), 0);
    }
}

#[cfg_attr(test, test)]
fn generator_entry_admission_refusal_preserves_peer_usage_and_terminal_failure() {
    let mut vm = bounded_vm();
    vm.stack = Vec::new();
    let mut inherited = Vec::new();
    let mut entry = entry_charge(&vm, &inherited);
    let peer = leave_room(&mut vm, 0);
    let before = vm.host_scheduling_snapshot().payload_bytes;
    assert_eq!(
        vm.prepare_generator_entry(&mut inherited, true, true, &mut entry),
        Err(RuntimeError::SchedulingLimit)
    );
    drop(entry);
    vm.reconcile_scheduling_ownership();
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
    assert_eq!(vm.stack.capacity(), 0);
    assert_eq!(inherited.capacity(), 0);
    drop(peer);
    assert_eq!(
        vm.check_scheduling_failure(),
        Err(RuntimeError::SchedulingLimit)
    );
}

impl crate::Vm {
    /// Runs finite generator parameter-entry contracts in unit/coverage builds.
    #[doc(hidden)]
    pub fn verify_generator_entry_admission_contracts() {
        generator_entry_admission_root_refusal_precedes_generator_allocation();
        generator_entry_admission_parameter_environment_refusal_precedes_allocation();
        generator_entry_admission_joint_exact_budget_and_one_byte_refusal();
        generator_entry_admission_spare_slots_need_no_growth_or_primitive_root_copies();
        generator_entry_admission_forwarded_environment_keeps_one_native_owner();
        generator_entry_admission_roots_keep_displaced_objects_until_caller_restoration();
        generator_entry_admission_real_captured_eval_parameters_survive_collection();
        generator_entry_admission_parameter_throw_restores_caller_and_releases_leases();
        generator_entry_admission_refusal_preserves_peer_usage_and_terminal_failure();
    }
}
