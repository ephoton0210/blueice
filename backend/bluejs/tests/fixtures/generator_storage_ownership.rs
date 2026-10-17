// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Small saved frames check allocation ownership across generator turns.

use super::*;

fn compile(source: &str) -> Bytecode {
    crate::compile(&crate::parse(source).unwrap()).unwrap()
}

fn bounded_vm() -> Vm {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm.remaining_instructions = 100_000;
    vm
}

fn check_reserved_table_survives_resume(dynamic: bool) {
    let mut vm = bounded_vm();
    let generator = vm
        .execute_script(&compile("(function*(){yield 1;yield 2;})();"))
        .unwrap();
    vm.generator_next(&generator, None, None).unwrap();
    let id = generator.object_id().unwrap();
    let mut state = vm.heap.take_generator_state(id).unwrap();
    let GeneratorState::Suspended {
        cells,
        dynamic_bindings,
        ..
    } = &mut state
    else {
        panic!("the finite generator must suspend")
    };
    let capacity = if dynamic {
        assert!(dynamic_bindings.is_empty());
        dynamic_bindings.reserve(31);
        dynamic_bindings.capacity()
    } else {
        assert!(cells.is_empty());
        cells.reserve(31);
        cells.capacity()
    };
    vm.heap.set_generator_state(id, state).unwrap();
    vm.generator_next(&generator, Some(Value::Undefined), None)
        .unwrap();
    let state = vm.heap.take_generator_state(id).unwrap();
    let GeneratorState::Suspended {
        cells,
        dynamic_bindings,
        ..
    } = &state
    else {
        panic!("the second yield must retain the same frame")
    };
    assert_eq!(
        if dynamic {
            dynamic_bindings.capacity()
        } else {
            cells.capacity()
        },
        capacity
    );
    vm.heap.set_generator_state(id, state).unwrap();
}

#[cfg_attr(test, test)]
fn generator_storage_ownership_preserves_reserved_cell_table() {
    check_reserved_table_survives_resume(false);
}

#[cfg_attr(test, test)]
fn generator_storage_ownership_preserves_reserved_dynamic_table() {
    check_reserved_table_survives_resume(true);
}

fn start_state(vm: &Vm, capacity: usize) -> GeneratorState {
    let mut args = Vec::with_capacity(capacity);
    args.push(Value::String("tiny".into()));
    GeneratorState::Start {
        code: SharedBytecode::new(Bytecode::empty()),
        captures: vec![vm.object_prototype],
        callee: Value::Undefined,
        receiver: Value::Undefined,
        args,
        home: None,
        with_objects: Vec::new(),
    }
}

#[cfg_attr(test, test)]
fn generator_storage_ownership_native_lease_survives_a_callback_and_drop() {
    let mut vm = bounded_vm();
    let observed = Rc::new(Cell::new(0));
    let output = observed.clone();
    let counter = vm.displaced_execution_control_bytes.clone();
    vm.install_host_function("observe", 0, move |_: &[HostValue]| {
        output.set(counter.get());
        Ok(HostValue::Undefined)
    })
    .unwrap();
    let callback = vm.lookup_global_name("observe").unwrap().unwrap();
    let state = start_state(&vm, 9);
    let expected = state.owned_buffer_bytes();
    let owned = vm.own_generator_state(state);
    vm.call_native(callback, Value::Undefined, Vec::new(), false)
        .unwrap();
    assert_eq!(observed.get(), expected);
    drop(owned);
    vm.reconcile_scheduling_ownership();
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
}

#[cfg_attr(test, test)]
fn generator_storage_ownership_store_transfers_native_bytes_to_the_heap() {
    let mut vm = bounded_vm();
    let generator = vm
        .heap
        .alloc_generator(GeneratorState::Done, vm.object_prototype)
        .unwrap();
    let before = vm.heap.stats().managed_bytes;
    let state = start_state(&vm, 9);
    let bytes = state.owned_buffer_bytes();
    vm.own_generator_state(state)
        .store(&mut vm, generator)
        .unwrap();
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
    assert_eq!(vm.heap.stats().managed_bytes, before + bytes);
    let state = vm.heap.take_generator_state(generator).unwrap();
    let GeneratorState::Start { args, .. } = &state else {
        panic!("stored start state")
    };
    assert_eq!(args.capacity(), 9);
    vm.heap.set_generator_state(generator, state).unwrap();
}

#[cfg_attr(test, test)]
fn generator_storage_ownership_native_refusal_closes_and_refunds_the_state() {
    let mut vm = bounded_vm();
    let generator = vm
        .heap
        .alloc_generator(GeneratorState::Running, vm.object_prototype)
        .unwrap();
    let peer = Vm {
        host_scheduling: Some(vm.host_scheduling.as_ref().unwrap().child()),
        ..Vm::default()
    };
    let used = vm.host_scheduling_usage().unwrap().payload_bytes;
    let before = peer.host_scheduling_snapshot().payload_bytes;
    let owner = vm
        .host_scheduling
        .as_ref()
        .unwrap()
        .shared_charge(MAX_HOST_SCHEDULING_BYTES - used)
        .unwrap();
    let state = start_state(&vm, 9);
    assert_eq!(
        vm.own_generator_state(state).store(&mut vm, generator),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(vm.heap.generator_state_is_done(generator).unwrap());
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
    drop(owner);
    assert_eq!(peer.host_scheduling_snapshot().payload_bytes, before);
}

#[cfg_attr(test, test)]
fn generator_storage_ownership_heap_refusal_releases_the_native_lease() {
    let mut config = VmConfig::default();
    config.heap.max_heap_bytes = 4096;
    config.heap.major_threshold_bytes = 1024;
    let mut vm = Vm::new(config).unwrap();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    let generator = vm
        .heap
        .alloc_generator(GeneratorState::Done, vm.object_prototype)
        .unwrap();
    let state = start_state(&vm, 128);
    assert!(state.owned_buffer_bytes() > 4096);
    assert_eq!(
        vm.own_generator_state(state).store(&mut vm, generator),
        Err(RuntimeError::Heap(HeapError::HeapLimitExceeded {
            limit: 4096
        }))
    );
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
    assert!(vm.heap.generator_state_is_done(generator).unwrap());
}

#[cfg_attr(test, test)]
fn generator_storage_ownership_preserves_nonempty_table_allocations() {
    let mut vm = bounded_vm();
    let generator = vm
        .execute_script(&compile(
            "(()=>{let retained={}; return (function*(){yield retained;yield retained;})();})();",
        ))
        .unwrap();
    vm.generator_next(&generator, None, None).unwrap();
    let id = generator.object_id().unwrap();
    let mut state = vm.heap.take_generator_state(id).unwrap();
    let GeneratorState::Suspended {
        cells,
        dynamic_bindings,
        ..
    } = &mut state
    else {
        panic!("suspended")
    };
    cells.reserve(17);
    let (&slot, &cell) = cells.iter().next().unwrap();
    let mut name = String::with_capacity(64);
    name.push_str("retained_alias");
    let mut shadowed_cells = Vec::with_capacity(9);
    shadowed_cells.push(cell);
    dynamic_bindings.insert(
        name,
        DynamicEvalBinding {
            cell,
            shadowed_cells,
        },
    );
    dynamic_bindings.reserve(17);
    let cell_pointer = cells.get(&slot).unwrap() as *const ObjectId;
    let binding_pointer =
        dynamic_bindings.get("retained_alias").unwrap() as *const DynamicEvalBinding;
    let capacities = (cells.capacity(), dynamic_bindings.capacity());
    vm.heap.set_generator_state(id, state).unwrap();
    vm.with_roots(|heap| {
        heap.collect_major();
        Ok(())
    })
    .unwrap();
    vm.generator_next(&generator, Some(Value::Undefined), None)
        .unwrap();
    let state = vm.heap.take_generator_state(id).unwrap();
    let GeneratorState::Suspended {
        cells,
        dynamic_bindings,
        ..
    } = &state
    else {
        panic!("second yield")
    };
    assert_eq!(cells.get(&slot).unwrap() as *const ObjectId, cell_pointer);
    assert_eq!(
        dynamic_bindings.get("retained_alias").unwrap() as *const DynamicEvalBinding,
        binding_pointer
    );
    assert_eq!((cells.capacity(), dynamic_bindings.capacity()), capacities);
    vm.heap.set_generator_state(id, state).unwrap();
}

#[cfg_attr(test, test)]
fn generator_storage_ownership_shared_binding_edges_survive_collection() {
    for nursery in [1, 16] {
        let mut config = VmConfig::default();
        config.heap.nursery_capacity = nursery;
        let mut vm = Vm::new(config).unwrap();
        vm.use_bounded_host_microtask_checkpoints().unwrap();
        let generator = vm
            .execute_script(&compile("(function*(){yield 1;return live;})();"))
            .unwrap();
        vm.generator_next(&generator, None, None).unwrap();
        let value = vm.heap.alloc_object(None).unwrap();
        let value_root = vm.heap.root(value).unwrap();
        let cell = vm.heap.alloc_object(None).unwrap();
        let cell_root = vm.heap.root(cell).unwrap();
        vm.heap.set(cell, "value", Value::Object(value)).unwrap();
        let shadowed = vm.heap.alloc_object(None).unwrap();
        let shadowed_root = vm.heap.root(shadowed).unwrap();
        let id = generator.object_id().unwrap();
        let mut state = vm.heap.take_generator_state(id).unwrap();
        let GeneratorState::Suspended {
            dynamic_bindings, ..
        } = &mut state
        else {
            panic!("suspended")
        };
        dynamic_bindings.insert(
            "live".into(),
            DynamicEvalBinding {
                cell,
                shadowed_cells: vec![shadowed],
            },
        );
        vm.heap.set_generator_state(id, state).unwrap();
        for root in [value_root, cell_root, shadowed_root] {
            vm.heap.unroot(root).unwrap();
        }
        vm.with_roots(|heap| {
            heap.collect_major();
            Ok(())
        })
        .unwrap();
        assert!(vm.heap.contains(cell) && vm.heap.contains(value) && vm.heap.contains(shadowed));
        let result = vm
            .generator_next(&generator, Some(Value::Undefined), None)
            .unwrap();
        assert_eq!(
            vm.get_property(&result, &"value".into()),
            Ok(Value::Object(value))
        );
        assert_eq!(
            vm.get_property(&result, &"done".into()),
            Ok(Value::Bool(true))
        );
        assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
    }
}

#[cfg_attr(test, test)]
fn generator_storage_ownership_async_await_keeps_the_same_tables() {
    let mut vm = bounded_vm();
    vm.execute_script(&compile("var gate=Promise.withResolvers(); async function* g(){yield 1;await gate.promise;yield 2;} var iterator=g();var first=iterator.next();0;")).unwrap();
    for _ in 0..32 {
        if !vm.run_next_promise_job().unwrap() {
            break;
        }
    }
    let id = vm
        .lookup_global_name("iterator")
        .unwrap()
        .unwrap()
        .object_id()
        .unwrap();
    let mut state = vm.heap.take_generator_state(id).unwrap();
    let GeneratorState::Suspended {
        cells,
        dynamic_bindings,
        ..
    } = &mut state
    else {
        panic!("first async yield")
    };
    cells.reserve(17);
    dynamic_bindings.reserve(17);
    let expected = (cells.capacity(), dynamic_bindings.capacity());
    vm.heap.set_generator_state(id, state).unwrap();
    vm.execute_script(&compile("var second=iterator.next();gate.resolve();0;"))
        .unwrap();
    for _ in 0..32 {
        if !vm.run_next_promise_job().unwrap() {
            break;
        }
    }
    let state = vm.heap.take_generator_state(id).unwrap();
    let GeneratorState::Suspended {
        cells,
        dynamic_bindings,
        ..
    } = &state
    else {
        panic!("yield after await")
    };
    assert_eq!((cells.capacity(), dynamic_bindings.capacity()), expected);
    vm.heap.set_generator_state(id, state).unwrap();
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
}

#[cfg_attr(test, test)]
fn generator_storage_ownership_parameter_entry_finishes_its_native_handoff() {
    for kind in ["function*", "async function*"] {
        let mut vm = bounded_vm();
        let generator = vm
            .execute_script(&compile(&format!(
                "({kind}(first={{}}){{yield first;}})();"
            )))
            .unwrap();
        assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
        let id = generator.object_id().unwrap();
        assert_eq!(
            vm.heap
                .generator_execution_code(id)
                .unwrap()
                .map(|(_, start)| start),
            Some(false)
        );
        let state = vm.heap.take_generator_state(id).unwrap();
        assert!(state.owned_buffer_bytes() > 0);
        vm.heap.set_generator_state(id, state).unwrap();
    }
}

impl crate::Vm {
    /// Runs finite generator storage ownership contracts in unit/coverage builds.
    #[doc(hidden)]
    pub fn verify_generator_storage_ownership_contracts() {
        generator_storage_ownership_preserves_reserved_cell_table();
        generator_storage_ownership_preserves_reserved_dynamic_table();
        generator_storage_ownership_native_lease_survives_a_callback_and_drop();
        generator_storage_ownership_store_transfers_native_bytes_to_the_heap();
        generator_storage_ownership_native_refusal_closes_and_refunds_the_state();
        generator_storage_ownership_heap_refusal_releases_the_native_lease();
        generator_storage_ownership_preserves_nonempty_table_allocations();
        generator_storage_ownership_shared_binding_edges_survive_collection();
        generator_storage_ownership_async_await_keeps_the_same_tables();
        generator_storage_ownership_parameter_entry_finishes_its_native_handoff();
    }
}
