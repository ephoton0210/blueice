// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Finite eval declarations check admission before cell/alias publication.

use super::*;
use crate::vm::host_scheduling::SharedSchedulingCharge;

fn eval_code(source: &str) -> Bytecode {
    crate::compiler::compile_eval(
        &crate::parse(source).unwrap(),
        &[],
        &[],
        &[],
        crate::compiler::EvalContext::default(),
        crate::CompileLimits::default(),
    )
    .unwrap()
}

fn bounded_vm() -> Vm {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm.remaining_instructions = 100_000;
    vm
}

fn leave_room(vm: &mut Vm, bytes: usize) -> SharedSchedulingCharge {
    let used = vm.host_scheduling_usage().unwrap().payload_bytes;
    vm.host_scheduling
        .as_ref()
        .unwrap()
        .shared_charge(MAX_HOST_SCHEDULING_BYTES - used - bytes)
        .unwrap()
}

#[cfg_attr(test, test)]
fn dynamic_binding_admission_refuses_before_new_cells_or_aliases_publish() {
    let mut vm = bounded_vm();
    let code = eval_code("var first=1,second=2;");
    vm.prepare_root_execution(&code, false).unwrap();
    assert_eq!(code.dynamic_eval_slots.len(), 2);
    let before = vm.heap.stats();
    let _peer = leave_room(&mut vm, 0);
    assert_eq!(
        vm.prepare_eval_dynamic_var_declarations(&code),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(vm.dynamic_eval_bindings.is_empty() && vm.eval_dynamic_slots.is_empty());
    assert_eq!(vm.heap.stats(), before);
}

#[cfg_attr(test, test)]
fn dynamic_binding_admission_recreation_refuses_before_restoring_a_deleted_cell() {
    let mut vm = bounded_vm();
    let code = eval_code("var retained=1;");
    vm.prepare_root_execution(&code, false).unwrap();
    vm.prepare_eval_dynamic_var_declarations(&code).unwrap();
    vm.enter_scope(&code, 0).unwrap();
    let slot = code.dynamic_eval_slots[0] as usize;
    let cell = vm.cells[&slot];
    assert!(vm.delete_eval_var(slot).unwrap());
    assert_eq!(vm.heap.get_own(cell, "value").unwrap(), None);
    let _peer = leave_room(&mut vm, 0);
    assert_eq!(
        vm.assign_binding_slot(&code, slot, Value::Number(42.0), true),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(vm.heap.get_own(cell, "value").unwrap(), None);
    assert!(!vm.dynamic_eval_bindings.contains_key("retained"));
}

fn attach_env(vm: &mut Vm) -> ObjectId {
    let env = vm.new_parameter_eval_env().unwrap();
    vm.parameter_eval_env = Some(env);
    vm.with_objects.push(Value::Object(env));
    env
}

#[cfg_attr(test, test)]
fn dynamic_binding_admission_exact_budget_and_one_byte_refusal() {
    for spare in [false, true] {
        for environment in [false, true] {
            for accepts in [false, true] {
                let mut vm = bounded_vm();
                let code = eval_code("var first=1,second=2;");
                vm.prepare_root_execution(&code, false).unwrap();
                let env = environment.then(|| attach_env(&mut vm));
                if spare {
                    vm.dynamic_eval_bindings.reserve(7);
                    vm.eval_dynamic_slots.reserve(7);
                }
                vm.stack.reserve(16);
                let dynamic = SchedulingMapCapacity::for_additional(
                    &vm.dynamic_eval_bindings,
                    2,
                    vm.host_scheduling.as_ref(),
                )
                .unwrap();
                let aliases = SchedulingMapCapacity::for_additional(
                    &vm.eval_dynamic_slots,
                    2,
                    vm.host_scheduling.as_ref(),
                )
                .unwrap();
                let bytes = 2 * std::mem::size_of::<PreparedDeclaration>()
                    + 2 * ("first".len() + "second".len())
                    + dynamic.staging_bytes()
                    + aliases.staging_bytes();
                drop((dynamic, aliases));
                let before = vm.heap.stats();
                let _peer = leave_room(&mut vm, bytes - usize::from(!accepts));
                let result = vm.prepare_eval_dynamic_var_declarations(&code);
                if accepts {
                    result.unwrap();
                    assert_eq!(vm.dynamic_eval_bindings.len(), 2);
                    assert_eq!(vm.eval_dynamic_slots.len(), 2);
                    for name in ["first", "second"] {
                        let cell = vm.dynamic_eval_bindings[name].cell;
                        assert_eq!(
                            vm.heap.get_own(cell, "value").unwrap(),
                            Some(Value::Undefined)
                        );
                        if let Some(env) = env {
                            assert_eq!(
                                vm.heap.get_own(env, name).unwrap(),
                                Some(Value::Object(cell))
                            );
                        }
                    }
                } else {
                    assert_eq!(result, Err(RuntimeError::SchedulingLimit));
                    assert!(
                        vm.dynamic_eval_bindings.is_empty() && vm.eval_dynamic_slots.is_empty()
                    );
                    assert_eq!(vm.heap.stats(), before);
                }
                assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
                assert!(vm.stack.is_empty());
            }
        }
    }
}

fn captured_code(fresh: bool) -> Bytecode {
    let parent = crate::compile(&crate::parse("var captured;").unwrap()).unwrap();
    let slot = parent
        .bindings
        .iter()
        .position(|binding| binding.name == "captured")
        .unwrap();
    let existing = ["captured".to_owned()];
    crate::compiler::compile_eval(
        &crate::parse("var captured = 42;").unwrap(),
        &[(
            "captured".into(),
            parent.bindings[slot].clone(),
            slot as u32,
        )],
        if fresh { &[] } else { &existing },
        &[],
        crate::compiler::EvalContext::default(),
        crate::CompileLimits::default(),
    )
    .unwrap()
}

fn install_capture(vm: &mut Vm) -> ObjectId {
    let cell = vm.with_roots(|heap| heap.alloc_object(None)).unwrap();
    vm.cells.insert(0, cell);
    vm.with_roots(|heap| heap.set(cell, "value", Value::Number(7.0)))
        .unwrap();
    cell
}

#[cfg_attr(test, test)]
fn dynamic_binding_admission_captured_alias_is_prepared_before_publication() {
    for accepts in [false, true] {
        let code = captured_code(false);
        assert!(code.dynamic_eval_slots.is_empty());
        assert_eq!(code.captures.len(), 1);
        let mut vm = bounded_vm();
        vm.prepare_root_execution(&code, false).unwrap();
        let cell = install_capture(&mut vm);
        vm.dynamic_eval_bindings.insert(
            "captured".into(),
            DynamicEvalBinding {
                cell,
                shadowed_cells: Vec::new(),
            },
        );
        let _peer = leave_room(&mut vm, if accepts { 4096 } else { 0 });
        let result = vm.prepare_eval_dynamic_var_declarations(&code);
        if accepts {
            result.unwrap();
            assert_eq!(
                vm.eval_dynamic_slots.get(&0).map(String::as_str),
                Some("captured")
            );
        } else {
            assert_eq!(result, Err(RuntimeError::SchedulingLimit));
            assert!(vm.eval_dynamic_slots.is_empty());
        }
        assert_eq!(vm.dynamic_eval_bindings["captured"].cell, cell);
        assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
    }
}

#[cfg_attr(test, test)]
fn dynamic_binding_admission_existing_capture_alias_needs_no_copy() {
    let code = captured_code(false);
    let mut vm = bounded_vm();
    vm.prepare_root_execution(&code, false).unwrap();
    let cell = install_capture(&mut vm);
    vm.dynamic_eval_bindings.insert(
        "captured".into(),
        DynamicEvalBinding {
            cell,
            shadowed_cells: Vec::new(),
        },
    );
    vm.prepare_eval_dynamic_var_declarations(&code).unwrap();
    let pointer = vm.eval_dynamic_slots[&0].as_ptr();
    let _peer = leave_room(&mut vm, 0);
    vm.prepare_eval_dynamic_var_declarations(&code).unwrap();
    assert_eq!(vm.eval_dynamic_slots[&0].as_ptr(), pointer);
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
}

#[cfg_attr(test, test)]
fn dynamic_binding_admission_shadowed_cells_and_environment_edges_survive_gc() {
    for nursery in [1, 16] {
        for environment in [false, true] {
            let mut config = VmConfig::default();
            config.heap.nursery_capacity = nursery;
            let mut vm = Vm::new(config).unwrap();
            vm.use_bounded_host_microtask_checkpoints().unwrap();
            let code = captured_code(true);
            vm.prepare_root_execution(&code, false).unwrap();
            let original = install_capture(&mut vm);
            let env = environment.then(|| attach_env(&mut vm));
            vm.prepare_eval_dynamic_var_declarations(&code).unwrap();
            let binding = &vm.dynamic_eval_bindings["captured"];
            let cell = binding.cell;
            let expected = [original];
            assert_eq!(
                binding.shadowed_cells.as_slice(),
                if environment { &[][..] } else { &expected[..] }
            );
            vm.with_roots(|heap| {
                heap.collect_major();
                Ok(())
            })
            .unwrap();
            assert_eq!(
                vm.heap.get_own(original, "value").unwrap(),
                Some(Value::Number(7.0))
            );
            assert_eq!(
                vm.heap.get_own(cell, "value").unwrap(),
                Some(Value::Undefined)
            );
            if let Some(env) = env {
                assert_eq!(
                    vm.heap.get_own(env, "captured").unwrap(),
                    Some(Value::Object(cell))
                );
            }
            assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
        }
    }
}

#[cfg_attr(test, test)]
fn dynamic_binding_admission_heap_refusal_rolls_back_published_environment_rows() {
    // Measure just two ordinary cells and one short environment property.
    let mut probe = Heap::default();
    let cell = probe.alloc_object(None).unwrap();
    probe.set(cell, "value", Value::Undefined).unwrap();
    let cell_bytes = probe.stats().managed_bytes;
    let env = probe.alloc_object(None).unwrap();
    let before = probe.stats().managed_bytes;
    probe.set(env, "first", Value::Object(cell)).unwrap();
    let property_bytes = probe.stats().managed_bytes - before;
    let mut vm = bounded_vm();
    let code = eval_code("var first=1,second=2;");
    vm.prepare_root_execution(&code, false).unwrap();
    let env = attach_env(&mut vm);
    vm.with_roots(|heap| {
        heap.collect_major();
        Ok(())
    })
    .unwrap();
    let base = vm.stack.len();
    vm.heap.allow_only(2 * cell_bytes + property_bytes);
    assert!(matches!(
        vm.prepare_eval_dynamic_var_declarations(&code),
        Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { .. }))
    ));
    assert_eq!(vm.heap.get_own(env, "first").unwrap(), None);
    assert_eq!(vm.heap.get_own(env, "second").unwrap(), None);
    assert!(vm.dynamic_eval_bindings.is_empty() && vm.eval_dynamic_slots.is_empty());
    assert_eq!(vm.stack.len(), base);
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
}

#[cfg_attr(test, test)]
fn dynamic_binding_admission_recreated_object_value_survives_gc() {
    let mut config = VmConfig::default();
    config.heap.nursery_capacity = 1;
    let mut vm = Vm::new(config).unwrap();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    let code = eval_code("var retained=1;");
    vm.prepare_root_execution(&code, false).unwrap();
    let env = attach_env(&mut vm);
    vm.prepare_eval_dynamic_var_declarations(&code).unwrap();
    vm.enter_scope(&code, 0).unwrap();
    let slot = code.dynamic_eval_slots[0] as usize;
    let cell = vm.cells[&slot];
    assert!(vm.delete_eval_var(slot).unwrap());
    let object = vm.with_roots(|heap| heap.alloc_object(None)).unwrap();
    vm.assign_binding_slot(&code, slot, Value::Object(object), true)
        .unwrap();
    vm.with_roots(|heap| {
        heap.collect_major();
        Ok(())
    })
    .unwrap();
    assert_eq!(
        vm.heap.get_own(cell, "value").unwrap(),
        Some(Value::Object(object))
    );
    assert_eq!(
        vm.heap.get_own(env, "retained").unwrap(),
        Some(Value::Object(cell))
    );
    assert!(vm.heap.get_own(object, "anything").is_ok());
    assert_eq!(vm.displaced_execution_bytes.get(), 0);
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
}

#[cfg_attr(test, test)]
fn dynamic_binding_admission_recreation_heap_refusal_keeps_the_cell_deleted() {
    let mut probe = Heap::default();
    let cell = probe.alloc_object(None).unwrap();
    let before = probe.stats().managed_bytes;
    probe.set(cell, "value", Value::Number(42.0)).unwrap();
    let property_bytes = probe.stats().managed_bytes - before;
    let mut vm = bounded_vm();
    let code = eval_code("var retained=1;");
    vm.prepare_root_execution(&code, false).unwrap();
    let env = attach_env(&mut vm);
    vm.prepare_eval_dynamic_var_declarations(&code).unwrap();
    vm.enter_scope(&code, 0).unwrap();
    let slot = code.dynamic_eval_slots[0] as usize;
    let cell = vm.cells[&slot];
    assert!(vm.delete_eval_var(slot).unwrap());
    vm.with_roots(|heap| {
        heap.collect_major();
        Ok(())
    })
    .unwrap();
    vm.heap.allow_only(property_bytes);
    assert!(matches!(
        vm.assign_binding_slot(&code, slot, Value::Number(42.0), true),
        Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { .. }))
    ));
    assert_eq!(vm.heap.get_own(cell, "value").unwrap(), None);
    assert_eq!(vm.heap.get_own(env, "retained").unwrap(), None);
    assert!(!vm.dynamic_eval_bindings.contains_key("retained"));
    assert!(vm.eval_dynamic_slots.contains_key(&slot));
    assert!(vm.stack.is_empty());
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
}

#[cfg_attr(test, test)]
fn dynamic_binding_admission_refusal_refunds_staging_to_a_peer() {
    let mut vm = bounded_vm();
    let code = eval_code("var first=1,second=2;");
    vm.prepare_root_execution(&code, false).unwrap();
    vm.stack.reserve(16);
    let peer = Vm {
        host_scheduling: Some(vm.host_scheduling.as_ref().unwrap().child()),
        ..Vm::default()
    };
    let used = vm.host_scheduling_usage().unwrap().payload_bytes;
    let dynamic = SchedulingMapCapacity::for_additional(
        &vm.dynamic_eval_bindings,
        2,
        vm.host_scheduling.as_ref(),
    )
    .unwrap();
    let aliases = SchedulingMapCapacity::for_additional(
        &vm.eval_dynamic_slots,
        2,
        vm.host_scheduling.as_ref(),
    )
    .unwrap();
    let bytes = 2 * std::mem::size_of::<PreparedDeclaration>()
        + 2 * ("first".len() + "second".len())
        + dynamic.staging_bytes()
        + aliases.staging_bytes();
    drop((dynamic, aliases));
    let owner = leave_room(&mut vm, bytes - 1);
    assert_eq!(
        vm.prepare_eval_dynamic_var_declarations(&code),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
    drop(owner);
    assert_eq!(peer.host_scheduling_snapshot().payload_bytes, used);
    assert_eq!(
        vm.check_scheduling_failure(),
        Err(RuntimeError::SchedulingLimit)
    );
}

#[cfg_attr(test, test)]
fn dynamic_binding_admission_environment_rollback_survives_a_latched_heap_limit() {
    let mut heap = Heap::default();
    let old = heap.alloc_object(None).unwrap();
    let old_root = heap.root(old).unwrap();
    let new = heap.alloc_object(None).unwrap();
    let env = heap.alloc_object(None).unwrap();
    heap.set(env, "retained", Value::Object(old)).unwrap();
    heap.set(env, "retained", Value::Object(new)).unwrap();
    heap.allow_only(0);
    heap.begin_document_heap_budget();
    assert!(matches!(
        heap.set(env, "refused", Value::Object(old)),
        Err(HeapError::HeapLimitExceeded { .. })
    ));
    heap.restore_eval_environment_cell(env, "retained", Some(old));
    assert_eq!(
        heap.get_own(env, "retained").unwrap(),
        Some(Value::Object(old))
    );
    assert!(heap.check_document_heap_budget().is_err());
    heap.unroot(old_root).unwrap();
}

impl crate::Vm {
    /// Runs finite function-eval publication contracts in unit/coverage builds.
    #[doc(hidden)]
    pub fn verify_dynamic_binding_admission_contracts() {
        dynamic_binding_admission_refuses_before_new_cells_or_aliases_publish();
        dynamic_binding_admission_recreation_refuses_before_restoring_a_deleted_cell();
        dynamic_binding_admission_exact_budget_and_one_byte_refusal();
        dynamic_binding_admission_captured_alias_is_prepared_before_publication();
        dynamic_binding_admission_existing_capture_alias_needs_no_copy();
        dynamic_binding_admission_shadowed_cells_and_environment_edges_survive_gc();
        dynamic_binding_admission_heap_refusal_rolls_back_published_environment_rows();
        dynamic_binding_admission_recreated_object_value_survives_gc();
        dynamic_binding_admission_recreation_heap_refusal_keeps_the_cell_deleted();
        dynamic_binding_admission_refusal_refunds_staging_to_a_peer();
        dynamic_binding_admission_environment_rollback_survives_a_latched_heap_limit();
    }
}
