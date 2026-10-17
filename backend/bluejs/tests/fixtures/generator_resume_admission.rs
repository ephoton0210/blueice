// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Finite saved frames must survive a refused resume before state extraction.

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

fn existing_preparation_bytes(vm: &mut Vm, id: ObjectId) -> usize {
    let (code, start) = vm.heap.generator_execution_code(id).unwrap().unwrap();
    assert!(!start);
    let before = vm.displaced_execution_control_bytes.get();
    let bindings = vm
        .prepare_frame_bindings(&code, FrameBindingKind::ResumedGenerator)
        .unwrap();
    let scopes = vm.prepare_generator_scope_slots(id).unwrap();
    let maps = vm.prepare_generator_frame_maps(id).unwrap();
    let bytes = vm.displaced_execution_control_bytes.get() - before;
    drop((bindings, scopes, maps));
    vm.reconcile_scheduling_ownership();
    bytes
}

#[cfg_attr(test, test)]
fn generator_resume_admission_preserves_saved_state_before_stack_growth_refusal() {
    let mut vm = bounded_vm();
    let generator = vm
        .execute_script(&compile("(function*(){yield 0;yield 1;})();"))
        .unwrap();
    vm.generator_next(&generator, None, None).unwrap();
    let id = generator.object_id().unwrap();
    vm.stack = Vec::with_capacity(4);
    vm.stack.resize(4, Value::Undefined);
    let bytes = existing_preparation_bytes(&mut vm, id);
    let _peer = leave_room(&mut vm, bytes);
    assert_eq!(
        vm.generator_next(&generator, Some(Value::Undefined), None),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(
        matches!(
            vm.heap.generator_execution_code(id).unwrap(),
            Some((_, false))
        ),
        "resume consumed the saved frame before admitting storage"
    );
    assert_eq!(vm.stack, vec![Value::Undefined; 4]);
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
}

#[cfg_attr(test, test)]
fn generator_resume_admission_preserves_pending_return_before_conversion_refusal() {
    let mut vm = bounded_vm();
    let generator = vm
        .execute_script(&compile(
            "(function*(){try{return 7;}finally{yield 0;yield 1;}})();",
        ))
        .unwrap();
    vm.generator_next(&generator, None, None).unwrap();
    let id = generator.object_id().unwrap();
    vm.stack.reserve(16);
    let bytes = existing_preparation_bytes(&mut vm, id);
    let _peer = leave_room(&mut vm, bytes);
    assert_eq!(
        vm.generator_next(&generator, Some(Value::Undefined), None),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(
        matches!(
            vm.heap.generator_execution_code(id).unwrap(),
            Some((_, false))
        ),
        "resume consumed the pending return before admitting its conversion"
    );
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
}

fn saved_generator(vm: &mut Vm, source: &str) -> (Value, ObjectId) {
    let generator = vm.execute_script(&compile(source)).unwrap();
    vm.generator_next(&generator, None, None).unwrap();
    let id = generator.object_id().unwrap();
    (generator, id)
}

#[cfg_attr(test, test)]
fn generator_resume_admission_exact_joint_budget_and_one_byte_refusal() {
    for spare in [false, true] {
        for referrer in [false, true] {
            for accepts in [false, true] {
                let mut vm = bounded_vm();
                let (_, id) = saved_generator(
                    &mut vm,
                    "(function*(){try{return 'tiny';}finally{yield 0;yield 1;}})();",
                );
                vm.stack = Vec::with_capacity(if spare { 16 } else { 0 });
                let layout = vm.heap.generator_resume_layout(id).unwrap().unwrap();
                let mut name = String::with_capacity(64);
                name.push_str("owner");
                vm.active_module_name = Some(name);
                if referrer {
                    vm.module_closure_referrers.reserve(1);
                    vm.module_closure_referrers
                        .insert(layout.callee.unwrap(), "child".into());
                }
                let growth = (3 + layout.stack + 1).saturating_sub(vm.stack.capacity())
                    * std::mem::size_of::<Value>();
                let bytes = layout.bytes
                    + layout.completions * std::mem::size_of::<Completion>()
                    + 5
                    + growth;
                let pointer = vm.active_module_name.as_ref().unwrap().as_ptr();
                let _peer = leave_room(&mut vm, bytes - usize::from(!accepts));
                let prepared = vm.prepare_generator_resume(id);
                if accepts {
                    let prepared = prepared.unwrap();
                    assert_eq!(
                        vm.displaced_execution_control_bytes.get(),
                        layout.completions * std::mem::size_of::<Completion>() + 5
                    );
                    assert!(vm.stack.capacity() > 3 + layout.stack);
                    drop(prepared);
                    vm.reconcile_scheduling_ownership();
                } else {
                    assert!(matches!(prepared, Err(RuntimeError::SchedulingLimit)));
                }
                assert!(vm.stack.is_empty());
                assert!(vm.heap.generator_execution_code(id).unwrap().is_some());
                assert_eq!(vm.active_module_name.as_ref().unwrap().as_ptr(), pointer);
                assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
            }
        }
    }
}

#[cfg_attr(test, test)]
fn generator_resume_admission_module_copy_refuses_before_state_extraction() {
    let mut vm = bounded_vm();
    let (generator, id) = saved_generator(&mut vm, "(function*(){yield 0;yield 1;})();");
    vm.stack.reserve(16);
    vm.active_module_name = Some("owner-module".into());
    let pointer = vm.active_module_name.as_ref().unwrap().as_ptr();
    let layout = vm.heap.generator_resume_layout(id).unwrap().unwrap();
    let bytes = existing_preparation_bytes(&mut vm, id)
        + layout.bytes
        + layout.completions * std::mem::size_of::<Completion>()
        + "owner-module".len();
    let _peer = leave_room(&mut vm, bytes - 1);
    assert_eq!(
        vm.generator_next(&generator, Some(Value::Undefined), None),
        Err(RuntimeError::SchedulingLimit)
    );
    assert!(vm.heap.generator_execution_code(id).unwrap().is_some());
    assert_eq!(vm.active_module_name.as_ref().unwrap().as_ptr(), pointer);
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
}

#[cfg_attr(test, test)]
fn generator_resume_admission_reserves_every_caller_root_source() {
    let mut vm = bounded_vm();
    let (_, id) = saved_generator(&mut vm, "(function*(){yield 0;yield 1;})();");
    let method = vm.global("eval").unwrap();
    let cell = vm.with_roots(|heap| heap.alloc_object(None)).unwrap();
    vm.cells.insert(0, cell);
    vm.with_roots(|heap| heap.set(cell, "value", Value::Number(7.0)))
        .unwrap();
    vm.bindings = vec![
        Some(Value::Object(vm.object_prototype)),
        Some(Value::Number(0.0)),
    ];
    vm.arguments = vec![Value::Object(vm.array_prototype), Value::Undefined];
    vm.with_objects = vec![Value::Object(vm.object_prototype)];
    vm.disposables = vm.owned_records(vec![DisposableResource {
        receiver: Value::Object(vm.array_prototype),
        argument: Some(Value::Object(vm.object_prototype)),
        method: Some(method),
        hint: DisposeHint::Sync,
        sync_fallback: false,
    }]);
    vm.pending_completions = vec![Completion::TailCall(vec![
        Value::Object(vm.object_prototype),
        Value::Number(0.0),
        Value::Object(vm.array_prototype),
    ])];
    vm.completion_saves = vec![(Value::Object(vm.object_prototype), false)];
    vm.stack = vec![Value::Undefined];
    let layout = vm.heap.generator_resume_layout(id).unwrap().unwrap();
    // Binding + cell + three placeholders + argument + with + three resource
    // values + two tail operands + saved completion = thirteen root slots.
    let required = 1 + 13 + layout.stack + 1;
    let growth = (required - vm.stack.capacity()) * std::mem::size_of::<Value>();
    let _peer = leave_room(&mut vm, layout.bytes + growth);
    let prepared = vm.prepare_generator_resume(id).unwrap();
    assert!(vm.stack.capacity() >= required);
    assert_eq!(vm.stack.len(), 1);
    drop(prepared);
    vm.reconcile_scheduling_ownership();
}

#[cfg_attr(test, test)]
fn generator_resume_admission_detached_state_and_completion_buffers_keep_their_lease() {
    let mut vm = bounded_vm();
    let (generator, id) = saved_generator(
        &mut vm,
        "(function*(){try{return 'tiny';}finally{yield 0;yield 1;}})();",
    );
    let layout = vm.heap.generator_resume_layout(id).unwrap().unwrap();
    let mut prepared = vm.prepare_generator_resume(id).unwrap();
    let target_bytes = vm.displaced_execution_control_bytes.get();
    let mut state = vm.heap.take_generator_state(id).unwrap();
    prepared.retain_state(&state);
    assert_eq!(
        vm.displaced_execution_control_bytes.get(),
        target_bytes + layout.bytes
    );
    let GeneratorState::Suspended {
        pending_completions,
        ..
    } = &mut state
    else {
        panic!("saved finally")
    };
    let converted = prepared.take_completions(std::mem::take(pending_completions));
    assert_eq!(converted.len(), layout.completions);
    assert!(
        !converted.is_empty(),
        "the finally must retain a completion record"
    );
    assert_eq!(
        vm.displaced_execution_control_bytes.get(),
        target_bytes + layout.bytes
    );
    *pending_completions = converted
        .into_iter()
        .map(|completion| completion.into_generator_pending().unwrap())
        .collect();
    vm.heap.set_generator_state(id, state).unwrap();
    drop(prepared);
    vm.reconcile_scheduling_ownership();
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
    vm.generator_next(&generator, Some(Value::Undefined), None)
        .unwrap();
    let result = vm
        .generator_next(&generator, Some(Value::Undefined), None)
        .unwrap()
        .object_id()
        .unwrap();
    assert_eq!(
        vm.heap.get_own(result, "done").unwrap(),
        Some(Value::Bool(true))
    );
    assert_eq!(
        vm.heap.get_own(result, "value").unwrap(),
        Some(Value::String("tiny".into()))
    );
}

#[cfg_attr(test, test)]
fn generator_resume_admission_pending_object_survives_resume_collection_and_final_return() {
    for nursery in [1, 16] {
        let mut config = VmConfig::default();
        config.heap.nursery_capacity = nursery;
        let mut vm = Vm::new(config).unwrap();
        vm.use_bounded_host_microtask_checkpoints().unwrap();
        let (generator, _) = saved_generator(
            &mut vm,
            "(function*(){try{return {answer:42};}finally{yield 0;({});yield 1;}})();",
        );
        vm.with_roots(|heap| {
            heap.collect_major();
            Ok(())
        })
        .unwrap();
        vm.generator_next(&generator, Some(Value::Undefined), None)
            .unwrap();
        vm.with_roots(|heap| {
            heap.collect_major();
            Ok(())
        })
        .unwrap();
        let result = vm
            .generator_next(&generator, Some(Value::Undefined), None)
            .unwrap()
            .object_id()
            .unwrap();
        assert_eq!(
            vm.heap.get_own(result, "done").unwrap(),
            Some(Value::Bool(true))
        );
        let returned = vm
            .heap
            .get_own(result, "value")
            .unwrap()
            .unwrap()
            .object_id()
            .unwrap();
        assert_eq!(
            vm.heap.get_own(returned, "answer").unwrap(),
            Some(Value::Number(42.0))
        );
        assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
    }
}

#[cfg_attr(test, test)]
fn generator_resume_admission_async_finally_round_trip_survives_collection() {
    let mut config = VmConfig::default();
    config.heap.nursery_capacity = 1;
    let mut vm = Vm::new(config).unwrap();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm.execute_script(&compile("globalThis.answer=0;(async()=>{let it=(async function*(){try{return {answer:42};}finally{yield 0;({});yield 1;}})();await it.next();await it.next();let last=await it.next();answer=last.value.answer;})();")).unwrap();
    let mut drained = false;
    for _ in 0..64 {
        if !vm.run_next_promise_job().unwrap() {
            drained = true;
            break;
        }
        vm.with_roots(|heap| {
            heap.collect_major();
            Ok(())
        })
        .unwrap();
    }
    assert!(drained);
    assert_eq!(
        vm.execute_script(&compile("answer;")),
        Ok(Value::Number(42.0))
    );
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
    assert_eq!(vm.displaced_execution_bytes.get(), 0);
}

#[cfg_attr(test, test)]
fn generator_resume_admission_refusal_refunds_prepared_owners_to_a_peer() {
    let mut vm = bounded_vm();
    let (generator, id) = saved_generator(
        &mut vm,
        "(function*(){let retained='tiny';yield retained;yield retained;})();",
    );
    vm.stack.reserve(16);
    let bytes = existing_preparation_bytes(&mut vm, id);
    let peer = Vm {
        host_scheduling: Some(vm.host_scheduling.as_ref().unwrap().child()),
        ..Vm::default()
    };
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    let owner = leave_room(&mut vm, bytes);
    assert_eq!(
        vm.generator_next(&generator, Some(Value::Undefined), None),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
    drop(owner);
    assert_eq!(peer.host_scheduling_snapshot().payload_bytes, before);
    assert!(vm.heap.generator_execution_code(id).unwrap().is_some());
    assert_eq!(
        vm.check_scheduling_failure(),
        Err(RuntimeError::SchedulingLimit)
    );
}

#[cfg_attr(test, test)]
fn generator_resume_admission_inactive_states_need_no_resume_allocation() {
    for state in [GeneratorState::Done, GeneratorState::Running] {
        let mut vm = bounded_vm();
        let (generator, id) = saved_generator(&mut vm, "(function*(){yield 0;})();");
        vm.heap.set_generator_state(id, state).unwrap();
        vm.stack = Vec::new();
        let _peer = leave_room(&mut vm, 0);
        let prepared = vm.prepare_generator_resume(id).unwrap();
        assert!(prepared.module.is_none());
        assert_eq!(vm.stack.capacity(), 0);
        assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
        drop((prepared, generator));
    }
}

impl crate::Vm {
    /// Runs finite generator-resume admission contracts in unit/coverage builds.
    #[doc(hidden)]
    pub fn verify_generator_resume_admission_contracts() {
        generator_resume_admission_preserves_saved_state_before_stack_growth_refusal();
        generator_resume_admission_preserves_pending_return_before_conversion_refusal();
        generator_resume_admission_exact_joint_budget_and_one_byte_refusal();
        generator_resume_admission_module_copy_refuses_before_state_extraction();
        generator_resume_admission_reserves_every_caller_root_source();
        generator_resume_admission_detached_state_and_completion_buffers_keep_their_lease();
        generator_resume_admission_pending_object_survives_resume_collection_and_final_return();
        generator_resume_admission_async_finally_round_trip_survives_collection();
        generator_resume_admission_refusal_refunds_prepared_owners_to_a_peer();
        generator_resume_admission_inactive_states_need_no_resume_allocation();
    }
}
