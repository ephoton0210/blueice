// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Small capture tables check entry effects and saved-state admission.

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

fn captures_source(body: &str) -> String {
    let declarations = (0..48)
        .map(|i| format!("let v{i}={i};"))
        .collect::<String>();
    let sum = (0..48)
        .map(|i| format!("v{i}"))
        .collect::<Vec<_>>()
        .join("+");
    format!(
        "(()=>{{{declarations} return {};}})();",
        body.replace("SUM", &sum)
    )
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
fn frame_map_admission_constructor_refuses_before_prototype_getter_effects() {
    let mut vm = bounded_vm();
    let seen = Rc::new(Cell::new(false));
    let output = seen.clone();
    vm.install_host_function("observe", 0, move |_: &[HostValue]| {
        output.set(true);
        Ok(HostValue::Undefined)
    })
    .unwrap();
    let target = vm.execute_script(&compile("function T(){}; new Proxy(T,{get(t,k,r){if(k==='prototype')observe(); return Reflect.get(t,k,r);}});")).unwrap();
    let target_root = vm.heap.root(target.object_id().unwrap()).unwrap();
    let callee = vm
        .execute_script(&compile(&captures_source(
            "class C {constructor(){return SUM;}}",
        )))
        .unwrap();
    let id = callee.object_id().unwrap();
    let code = vm.heap.closure_code(id).unwrap().unwrap();
    let (captures, with_objects, _) = vm.heap.closure_entry_storage(id).unwrap().unwrap();
    assert!(captures.len() >= 48);
    let incoming = std::mem::size_of_val(captures) + std::mem::size_of_val(with_objects);
    let prepared = vm
        .prepare_frame_bindings(&code, FrameBindingKind::Function)
        .unwrap();
    let bindings = vm.displaced_execution_control_bytes.get();
    drop(prepared);
    vm.reconcile_scheduling_ownership();
    vm.stack.reserve(256);
    vm.call_stack.reserve(16);
    let _peer = leave_room(&mut vm, incoming + bindings + 1024);
    assert_eq!(
        vm.call_with_target(callee, Value::Undefined, Vec::new(), true, target),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(
        !seen.get(),
        "prototype getter ran before captured-cell table admission"
    );
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
    vm.heap.unroot(target_root).unwrap();
}

#[cfg_attr(test, test)]
fn frame_map_admission_generator_refusal_preserves_start_state() {
    let mut vm = bounded_vm();
    let generator = vm
        .execute_script(&compile(&captures_source("(function*(){yield SUM;})()")))
        .unwrap();
    let id = generator.object_id().unwrap();
    let (code, start) = vm.heap.generator_execution_code(id).unwrap().unwrap();
    assert!(start);
    let prepared = vm
        .prepare_frame_bindings(&code, FrameBindingKind::Function)
        .unwrap();
    let bindings = vm.displaced_execution_control_bytes.get();
    drop(prepared);
    vm.reconcile_scheduling_ownership();
    let before = vm.heap.stats().managed_bytes;
    let _peer = leave_room(&mut vm, bindings);
    assert_eq!(
        vm.generator_next(&generator, None, None),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(
        vm.heap
            .generator_execution_code(id)
            .unwrap()
            .map(|(_, start)| start),
        Some(true)
    );
    assert_eq!(vm.heap.stats().managed_bytes, before);
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
}

fn table_bytes(cells: usize, dynamic: usize) -> usize {
    HashMap::<usize, ObjectId>::with_capacity(cells).capacity()
        * std::mem::size_of::<(usize, ObjectId)>()
        + HashMap::<String, DynamicEvalBinding>::with_capacity(dynamic).capacity()
            * std::mem::size_of::<(String, DynamicEvalBinding)>()
}

#[cfg_attr(test, test)]
fn frame_map_admission_tables_retain_actual_capacity_across_callbacks() {
    for (cells, dynamic) in [(0, 0), (2, 0), (0, 2), (2, 2)] {
        let mut vm = bounded_vm();
        let observed = Rc::new(Cell::new(usize::MAX));
        let output = observed.clone();
        let counter = vm.displaced_execution_control_bytes.clone();
        vm.install_host_function("observe", 0, move |_: &[HostValue]| {
            output.set(counter.get());
            Ok(HostValue::Undefined)
        })
        .unwrap();
        let callback = vm.lookup_global_name("observe").unwrap().unwrap();
        let mut prepared = vm.prepare_frame_maps(cells, dynamic, 0).unwrap();
        vm.call_native(callback, Value::Undefined, Vec::new(), false)
            .unwrap();
        assert_eq!(observed.get(), table_bytes(cells, dynamic));
        let parts = (prepared.take_cells(), prepared.take_dynamic());
        assert_eq!(
            vm.displaced_execution_control_bytes.get(),
            table_bytes(cells, dynamic)
        );
        drop(parts);
        drop(prepared);
        vm.reconcile_scheduling_ownership();
        assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
    }
}

#[cfg_attr(test, test)]
fn frame_map_admission_exact_capacity_and_one_byte_refusal() {
    for (cells, dynamic) in [(2, 0), (0, 2), (2, 2)] {
        for accepts in [false, true] {
            let mut vm = bounded_vm();
            let expected = table_bytes(cells, dynamic);
            let _peer = leave_room(&mut vm, expected - usize::from(!accepts));
            let result = vm.prepare_frame_maps(cells, dynamic, 0);
            if accepts {
                let prepared = result.unwrap();
                assert_eq!(vm.displaced_execution_control_bytes.get(), expected);
                drop(prepared);
            } else {
                assert!(matches!(result, Err(RuntimeError::SchedulingLimit)));
            }
            assert!(vm.cells.is_empty() && vm.dynamic_eval_bindings.is_empty());
            assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
        }
    }
}

#[cfg_attr(test, test)]
fn frame_map_admission_capture_adoption_reuses_the_prepared_table() {
    let mut vm = bounded_vm();
    let mut prepared = vm.prepare_frame_maps(3, 0, 0).unwrap();
    let expected = table_bytes(3, 0);
    prepared.adopt_captures(vec![vm.object_prototype; 3]);
    let cells = prepared.take_cells();
    assert_eq!(host_scheduling::map_slots(&cells), expected);
    for slot in 0..3 {
        assert_eq!(cells[&slot], vm.object_prototype);
    }
    assert_eq!(vm.displaced_execution_control_bytes.get(), expected);
    drop(cells);
    drop(prepared);
    vm.reconcile_scheduling_ownership();
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
}

fn dynamic_row(cell: ObjectId) -> (String, ObjectId, Vec<ObjectId>) {
    let mut name = String::with_capacity(64);
    name.push_str("retained");
    let mut shadowed = Vec::with_capacity(9);
    shadowed.push(cell);
    (name, cell, shadowed)
}

#[cfg_attr(test, test)]
fn frame_map_admission_dynamic_adoption_moves_spare_buffers_without_copying() {
    let mut vm = bounded_vm();
    let row = dynamic_row(vm.object_prototype);
    let name_pointer = row.0.as_ptr();
    let shadowed_pointer = row.2.as_ptr();
    let payload = row.0.capacity() + host_scheduling::slots(&row.2);
    let mut prepared = vm.prepare_frame_maps(1, 1, payload).unwrap();
    assert_eq!(
        vm.displaced_execution_control_bytes.get(),
        table_bytes(1, 1)
    );
    prepared.adopt_cells(vec![(4, vm.object_prototype)]);
    prepared.adopt_dynamic(vec![row]);
    let expected = table_bytes(1, 1) + payload;
    assert_eq!(vm.displaced_execution_control_bytes.get(), expected);
    let parts = (prepared.take_cells(), prepared.take_dynamic());
    let (name, binding) = parts.1.get_key_value("retained").unwrap();
    assert_eq!(name.as_ptr(), name_pointer);
    assert_eq!(binding.shadowed_cells.as_ptr(), shadowed_pointer);
    assert_eq!(name.capacity(), 64);
    assert_eq!(binding.shadowed_cells.capacity(), 9);
    assert_eq!(parts.0[&4], vm.object_prototype);
    assert_eq!(vm.displaced_execution_control_bytes.get(), expected);
    drop(parts);
    drop(prepared);
    vm.reconcile_scheduling_ownership();
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
}

#[cfg_attr(test, test)]
fn frame_map_admission_incoming_dynamic_payload_is_admitted_with_table_slots() {
    for accepts in [false, true] {
        let mut vm = bounded_vm();
        let row = dynamic_row(vm.object_prototype);
        let payload = row.0.capacity() + host_scheduling::slots(&row.2);
        let expected = table_bytes(0, 1) + payload;
        let _peer = leave_room(&mut vm, expected - usize::from(!accepts));
        let result = vm.prepare_frame_maps(0, 1, payload);
        if accepts {
            let mut prepared = result.unwrap();
            prepared.adopt_dynamic(vec![row]);
            vm.synchronize_scheduling_usage().unwrap();
            assert_eq!(vm.displaced_execution_control_bytes.get(), expected);
            drop(prepared);
            vm.reconcile_scheduling_ownership();
        } else {
            assert!(matches!(result, Err(RuntimeError::SchedulingLimit)));
        }
        assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
    }
}

fn dynamic_generator(vm: &mut Vm) -> (Value, usize) {
    let generator = vm
        .execute_script(&compile("(function*(){yield 1; return retained;})();"))
        .unwrap();
    vm.generator_next(&generator, None, None).unwrap();
    let cell = vm.heap.alloc_object(None).unwrap();
    let root = vm.heap.root(cell).unwrap();
    vm.heap.set(cell, "value", Value::Number(7.0)).unwrap();
    let row = dynamic_row(cell);
    let payload = row.0.capacity() + host_scheduling::slots(&row.2);
    let id = generator.object_id().unwrap();
    let mut state = vm.heap.take_generator_state(id).unwrap();
    let GeneratorState::Suspended {
        dynamic_bindings, ..
    } = &mut state
    else {
        panic!("the finite generator must be suspended")
    };
    dynamic_bindings.insert(
        row.0,
        DynamicEvalBinding {
            cell: row.1,
            shadowed_cells: row.2,
        },
    );
    vm.heap.set_generator_state(id, state).unwrap();
    vm.heap.unroot(root).unwrap();
    (generator, payload)
}

#[cfg_attr(test, test)]
fn frame_map_admission_dynamic_refusal_preserves_saved_state_and_peer_charge() {
    let mut vm = bounded_vm();
    let (generator, payload) = dynamic_generator(&mut vm);
    let id = generator.object_id().unwrap();
    assert_eq!(
        vm.heap.generator_map_layout(id).unwrap(),
        (0, 0, table_bytes(0, 1) + payload)
    );
    let peer = Vm {
        host_scheduling: Some(vm.host_scheduling.as_ref().unwrap().child()),
        ..Vm::default()
    };
    let (code, _) = vm.heap.generator_execution_code(id).unwrap().unwrap();
    let bindings = vm
        .prepare_frame_bindings(&code, FrameBindingKind::ResumedGenerator)
        .unwrap();
    let scopes = vm.prepare_generator_scope_slots(id).unwrap();
    let staging = vm.displaced_execution_control_bytes.get();
    drop(bindings);
    drop(scopes);
    vm.reconcile_scheduling_ownership();
    let before = peer.host_scheduling_snapshot().payload_bytes;
    let heap_before = vm.heap.stats().managed_bytes;
    let owner = leave_room(&mut vm, staging + table_bytes(0, 1) + payload - 1);
    assert_eq!(
        vm.generator_next(&generator, None, None),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(
        vm.heap
            .generator_execution_code(id)
            .unwrap()
            .map(|(_, start)| start),
        Some(false)
    );
    assert_eq!(
        vm.heap.generator_map_layout(id).unwrap(),
        (0, 0, table_bytes(0, 1) + payload)
    );
    assert_eq!(vm.heap.stats().managed_bytes, heap_before);
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
    drop(owner);
    assert_eq!(peer.host_scheduling_snapshot().payload_bytes, before);
}

#[cfg_attr(test, test)]
fn frame_map_admission_restored_dynamic_cells_survive_collection() {
    for nursery in [1, 16] {
        let mut config = VmConfig::default();
        config.heap.nursery_capacity = nursery;
        let mut vm = Vm::new(config).unwrap();
        vm.use_bounded_host_microtask_checkpoints().unwrap();
        let (generator, _) = dynamic_generator(&mut vm);
        vm.with_roots(|heap| {
            heap.collect_major();
            Ok(())
        })
        .unwrap();
        let result = vm.generator_next(&generator, None, None).unwrap();
        assert_eq!(
            vm.get_property(&result, &"value".into()),
            Ok(Value::Number(7.0))
        );
        assert_eq!(
            vm.get_property(&result, &"done".into()),
            Ok(Value::Bool(true))
        );
        assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
    }
}

#[cfg_attr(test, test)]
fn frame_map_admission_capture_refusal_precedes_all_parameter_entry_kinds() {
    for kind in ["function", "function*", "async function", "async function*"] {
        let mut vm = bounded_vm();
        let seen = Rc::new(Cell::new(false));
        let output = seen.clone();
        vm.install_host_function("observe", 0, move |_: &[HostValue]| {
            output.set(true);
            Ok(HostValue::Undefined)
        })
        .unwrap();
        let source = captures_source(&format!("({kind}(first=observe()){{return SUM;}})"));
        let callee = vm.execute_script(&compile(&source)).unwrap();
        let id = callee.object_id().unwrap();
        let code = vm.heap.closure_code(id).unwrap().unwrap();
        let capture_bytes =
            std::mem::size_of_val(vm.heap.closure_entry_storage(id).unwrap().unwrap().0);
        let prepared = vm
            .prepare_frame_bindings(&code, FrameBindingKind::Function)
            .unwrap();
        let bindings = vm.displaced_execution_control_bytes.get();
        drop(prepared);
        vm.reconcile_scheduling_ownership();
        vm.stack.reserve(256);
        vm.call_stack.reserve(16);
        let _peer = leave_room(&mut vm, capture_bytes + bindings);
        assert_eq!(
            vm.call_native(callee, Value::Undefined, Vec::new(), false),
            Err(RuntimeError::SchedulingLimit)
        );
        assert!(!seen.get(), "{kind}");
        assert!(
            vm.promises.is_empty(),
            "refused entry published a Promise capability"
        );
        assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
    }
}

impl crate::Vm {
    /// Runs finite frame table admission contracts in unit and coverage builds.
    #[doc(hidden)]
    pub fn verify_frame_map_admission_contracts() {
        frame_map_admission_constructor_refuses_before_prototype_getter_effects();
        frame_map_admission_generator_refusal_preserves_start_state();
        frame_map_admission_tables_retain_actual_capacity_across_callbacks();
        frame_map_admission_exact_capacity_and_one_byte_refusal();
        frame_map_admission_capture_adoption_reuses_the_prepared_table();
        frame_map_admission_dynamic_adoption_moves_spare_buffers_without_copying();
        frame_map_admission_incoming_dynamic_payload_is_admitted_with_table_slots();
        frame_map_admission_dynamic_refusal_preserves_saved_state_and_peer_charge();
        frame_map_admission_restored_dynamic_cells_survive_collection();
        frame_map_admission_capture_refusal_precedes_all_parameter_entry_kinds();
    }
}
