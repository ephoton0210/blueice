// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Tiny scopes exercise joint admission before lexical state changes.

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

fn leave_room(vm: &mut Vm, bytes: usize) -> SharedSchedulingCharge {
    let used = vm.host_scheduling_usage().unwrap().payload_bytes;
    vm.host_scheduling
        .as_ref()
        .unwrap()
        .shared_charge(MAX_HOST_SCHEDULING_BYTES - used - bytes)
        .unwrap()
}

#[cfg_attr(test, test)]
fn scope_admission_refuses_before_scope_records_or_bindings_change() {
    let mut vm = bounded_vm();
    let code = compile("let first=1,second=2; first+second;");
    vm.prepare_root_execution(&code, false).unwrap();
    vm.bindings.fill(Some(Value::Number(17.0)));
    let before = vm.bindings.clone();
    let mut iterators = vm.iterator_records(Vec::new());
    let _peer = leave_room(&mut vm, 0);
    let result = vm.interpret(&code, &mut iterators, 0, None, None, None);
    assert!(matches!(result, Err(RuntimeError::SchedulingLimit)));
    assert_eq!(vm.bindings, before);
    assert_eq!(vm.active_scopes.capacity(), 0);
    assert_eq!(vm.active_scope_slots.capacity(), 0);
}

#[cfg_attr(test, test)]
fn scope_admission_joint_alias_refusal_publishes_no_cells() {
    let mut vm = bounded_vm();
    let code = compile("var first=1,second=2; first+second;");
    vm.prepare_root_execution(&code, false).unwrap();
    vm.prepare_global_declarations(&code).unwrap();
    vm.active_scopes.reserve(4);
    vm.active_scope_slots.reserve(4);
    assert!(vm.cells.is_empty());
    let mut iterators = vm.iterator_records(Vec::new());
    let _peer = leave_room(&mut vm, code.scopes[0].len() * std::mem::size_of::<u32>());
    let result = vm.interpret(&code, &mut iterators, 0, None, None, None);
    assert!(matches!(result, Err(RuntimeError::SchedulingLimit)));
    assert!(vm.cells.is_empty());
    assert_eq!(vm.cells.capacity(), 0);
    assert!(vm.active_scopes.is_empty() && vm.active_scope_slots.is_empty());
}

#[cfg_attr(test, test)]
fn scope_admission_joint_record_growth_refuses_before_either_vector_reserves() {
    let mut vm = bounded_vm();
    let code = compile("let first=1,second=2;");
    vm.prepare_root_execution(&code, false).unwrap();
    let room = 4 * std::mem::size_of::<u32>() + code.scopes[0].len() * std::mem::size_of::<u32>();
    let _peer = leave_room(&mut vm, room);
    assert_eq!(vm.enter_scope(&code, 0), Err(RuntimeError::SchedulingLimit));
    assert_eq!(vm.active_scopes.capacity(), 0);
    assert_eq!(vm.active_scope_slots.capacity(), 0);
}

#[cfg_attr(test, test)]
fn scope_admission_spare_record_slots_charge_only_the_new_slot_list() {
    for lexical in [false, true] {
        let mut vm = bounded_vm();
        let code = compile(if lexical {
            "let first=1,second=2;"
        } else {
            "var first=1,second=2;"
        });
        vm.prepare_root_execution(&code, false).unwrap();
        vm.active_scopes.reserve(4);
        vm.active_scope_slots.reserve(4);
        let before = vm.host_scheduling_usage().unwrap().payload_bytes;
        let capacities = (
            vm.active_scopes.capacity(),
            vm.active_scope_slots.capacity(),
        );
        let bytes = code.scopes[0].len() * std::mem::size_of::<u32>();
        let _peer = leave_room(&mut vm, bytes);
        vm.enter_scope(&code, 0).unwrap();
        assert_eq!(vm.active_scopes, vec![0]);
        assert_eq!(vm.active_scope_slots, vec![code.scopes[0].clone()]);
        assert_eq!(
            (
                vm.active_scopes.capacity(),
                vm.active_scope_slots.capacity()
            ),
            capacities
        );
        for &slot in &code.scopes[0] {
            assert_eq!(
                vm.bindings[slot as usize],
                if lexical {
                    None
                } else {
                    Some(Value::Undefined)
                }
            );
        }
        assert_eq!(
            vm.own_scheduling_usage().payload_bytes + vm.heap.collector_storage_bytes() - before,
            bytes
        );
        assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
    }
}

#[cfg_attr(test, test)]
fn scope_admission_alias_table_rounding_is_checked_before_publication() {
    for accepts in [false, true] {
        let mut vm = bounded_vm();
        let code = compile("var first=1,second=2;");
        vm.prepare_root_execution(&code, false).unwrap();
        vm.prepare_global_declarations(&code).unwrap();
        vm.active_scopes.reserve(4);
        vm.active_scope_slots.reserve(4);
        let slots = code.scopes[0].len() * std::mem::size_of::<u32>();
        let capacity = HashMap::<usize, ObjectId>::with_capacity(2).capacity();
        let entries = if accepts { capacity } else { 2 };
        let _peer = leave_room(
            &mut vm,
            slots + entries * std::mem::size_of::<(usize, ObjectId)>(),
        );
        let result = vm.enter_scope(&code, 0);
        if accepts {
            result.unwrap();
            assert_eq!(vm.cells.len(), 2);
            for (slot, name) in &vm.script_global_slots {
                assert_eq!(vm.cells[slot], vm.global_bindings[name].cell);
            }
        } else {
            assert_eq!(result, Err(RuntimeError::SchedulingLimit));
            assert_eq!(vm.cells.capacity(), 0);
            assert!(vm.active_scopes.is_empty() && vm.active_scope_slots.is_empty());
        }
    }
}

#[cfg_attr(test, test)]
fn scope_admission_reuses_cells_released_by_the_same_scope() {
    let mut vm = bounded_vm();
    let code = compile("var first=1,second=2,third=3,fourth=4;");
    vm.prepare_root_execution(&code, false).unwrap();
    vm.prepare_global_declarations(&code).unwrap();
    let aliases = vm.script_global_slots.clone();
    let new_slot = *aliases
        .iter()
        .find(|(_, name)| name.as_str() == "fourth")
        .unwrap()
        .0;
    for (slot, name) in &aliases {
        if *slot != new_slot {
            vm.cells.insert(*slot, vm.global_bindings[name].cell);
        }
    }
    vm.script_global_slots.retain(|slot, _| *slot == new_slot);
    assert_eq!(vm.cells.len(), vm.cells.capacity());
    let capacity = vm.cells.capacity();
    vm.active_scopes.reserve(4);
    vm.active_scope_slots.reserve(4);
    let _peer = leave_room(&mut vm, code.scopes[0].len() * std::mem::size_of::<u32>());
    vm.enter_scope(&code, 0).unwrap();
    assert_eq!(vm.cells.len(), 1);
    assert_eq!(vm.cells[&new_slot], vm.global_bindings["fourth"].cell);
    assert!(vm.cells.capacity() <= capacity);
}

#[cfg_attr(test, test)]
fn scope_admission_eval_aliases_take_precedence_over_script_aliases() {
    let mut vm = bounded_vm();
    let code = compile("var first=1,second=2;");
    vm.prepare_root_execution(&code, false).unwrap();
    vm.prepare_global_declarations(&code).unwrap();
    for (slot, name) in vm.script_global_slots.clone() {
        let cell = vm.heap.alloc_object(None).unwrap();
        vm.dynamic_eval_bindings.insert(
            name.clone(),
            DynamicEvalBinding {
                cell,
                shadowed_cells: Vec::new(),
            },
        );
        vm.eval_dynamic_slots.insert(slot, name);
    }
    vm.enter_scope(&code, 0).unwrap();
    for (slot, name) in &vm.eval_dynamic_slots {
        assert_eq!(vm.cells[slot], vm.dynamic_eval_bindings[name].cell);
        assert_ne!(vm.cells[slot], vm.global_bindings[name].cell);
    }
}

#[cfg_attr(test, test)]
fn scope_admission_module_outer_scope_retains_seeded_cells() {
    let mut vm = bounded_vm();
    let code = crate::compile_module(&crate::parse_module("export var first=1,second=2;").unwrap())
        .unwrap();
    let mut cells = HashMap::new();
    for &slot in &code.scopes[0] {
        cells.insert(slot as usize, vm.heap.alloc_object(None).unwrap());
    }
    let expected = cells.clone();
    vm.enter_module_record(&code, &mut cells).unwrap();
    vm.active_scopes.reserve(4);
    vm.active_scope_slots.reserve(4);
    let _peer = leave_room(&mut vm, code.scopes[0].len() * std::mem::size_of::<u32>());
    vm.enter_scope(&code, 0).unwrap();
    assert_eq!(vm.cells, expected);
    assert!(vm.bindings.iter().all(Option::is_none));
    vm.leave_scope(&code, 0);
    assert_eq!(vm.cells, expected);
}

fn suspended_generator(vm: &mut Vm) -> (Value, usize) {
    let generator = vm
        .execute_script(&compile(
            "(function*(){let first=1; {let second=2;yield first+second;} yield 4;})();",
        ))
        .unwrap();
    vm.generator_next(&generator, None, None).unwrap();
    let (code, scopes) = vm
        .heap
        .generator_scope_layout(generator.object_id().unwrap())
        .unwrap()
        .unwrap();
    assert!(scopes.len() >= 2);
    let bytes = scopes.len() * std::mem::size_of::<Vec<u32>>()
        + scopes
            .iter()
            .map(|scope| code.scopes[*scope as usize].len() * std::mem::size_of::<u32>())
            .sum::<usize>();
    (generator, bytes)
}

#[cfg_attr(test, test)]
fn scope_admission_generator_scope_staging_keeps_its_native_charge() {
    let mut vm = bounded_vm();
    let (generator, bytes) = suspended_generator(&mut vm);
    let id = generator.object_id().unwrap();
    let _peer = leave_room(&mut vm, bytes);
    let mut prepared = vm.prepare_generator_scope_slots(id).unwrap();
    assert_eq!(vm.displaced_execution_control_bytes.get(), bytes);
    let slots = prepared.take();
    assert_eq!(vm.displaced_execution_control_bytes.get(), bytes);
    let (code, scopes) = vm.heap.generator_scope_layout(id).unwrap().unwrap();
    assert_eq!(
        slots,
        scopes
            .iter()
            .map(|scope| code.scopes[*scope as usize].clone())
            .collect::<Vec<_>>()
    );
    drop(slots);
    drop(prepared);
    vm.reconcile_scheduling_ownership();
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
}

#[cfg_attr(test, test)]
fn scope_admission_generator_refusal_preserves_state_and_refunds_staging() {
    let mut vm = bounded_vm();
    let (generator, scope_bytes) = suspended_generator(&mut vm);
    let id = generator.object_id().unwrap();
    let (code, _) = vm.heap.generator_execution_code(id).unwrap().unwrap();
    let prepared = vm
        .prepare_frame_bindings(&code, FrameBindingKind::ResumedGenerator)
        .unwrap();
    let frame_bytes = vm.displaced_execution_control_bytes.get();
    drop(prepared);
    vm.reconcile_scheduling_ownership();
    let peer = Vm {
        host_scheduling: Some(vm.host_scheduling.as_ref().unwrap().child()),
        ..Vm::default()
    };
    let before = peer.host_scheduling_snapshot().payload_bytes;
    let saved = vm
        .heap
        .generator_scope_layout(id)
        .unwrap()
        .unwrap()
        .1
        .to_vec();
    let heap_before = vm.heap.stats().managed_bytes;
    let owner = leave_room(&mut vm, frame_bytes + scope_bytes - 1);
    assert_eq!(
        vm.generator_next(&generator, None, None),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(
        vm.heap.generator_scope_layout(id).unwrap().unwrap().1,
        saved
    );
    assert_eq!(vm.heap.stats().managed_bytes, heap_before);
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
    drop(owner);
    assert_eq!(peer.host_scheduling_snapshot().payload_bytes, before);
}

impl crate::Vm {
    /// Runs finite scope admission and restoration contracts in unit/coverage builds.
    #[doc(hidden)]
    pub fn verify_scope_admission_contracts() {
        scope_admission_refuses_before_scope_records_or_bindings_change();
        scope_admission_joint_alias_refusal_publishes_no_cells();
        scope_admission_joint_record_growth_refuses_before_either_vector_reserves();
        scope_admission_spare_record_slots_charge_only_the_new_slot_list();
        scope_admission_alias_table_rounding_is_checked_before_publication();
        scope_admission_reuses_cells_released_by_the_same_scope();
        scope_admission_eval_aliases_take_precedence_over_script_aliases();
        scope_admission_module_outer_scope_retains_seeded_cells();
        scope_admission_generator_scope_staging_keeps_its_native_charge();
        scope_admission_generator_refusal_preserves_state_and_refunds_staging();
    }
}
