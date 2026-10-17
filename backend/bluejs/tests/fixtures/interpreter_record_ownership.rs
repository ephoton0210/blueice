// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Small interpreter records retain native ownership across callbacks/exits.

use super::*;

fn code(source: &str) -> Bytecode {
    crate::compile(&crate::parse(source).unwrap()).unwrap()
}

fn page_vm() -> Vm {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm
}

fn observed_records(source: &str) -> usize {
    let mut vm = page_vm();
    let observed = Rc::new(Cell::new(0));
    let output = observed.clone();
    let counter = vm.displaced_execution_control_bytes.clone();
    vm.install_host_function("observe", 0, move |_args: &[HostValue]| {
        output.set(counter.get());
        Ok(HostValue::Undefined)
    })
    .unwrap();
    vm.execute_script(&code(source)).unwrap();
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
    observed.get()
}

#[cfg_attr(test, test)]
fn interpreter_records_charge_handlers_during_native_callbacks() {
    assert!(
        observed_records("try {observe();} finally {} 0;") >= std::mem::size_of::<HandlerFrame>()
    );
}

#[cfg_attr(test, test)]
fn interpreter_records_charge_iterators_during_native_callbacks() {
    assert!(
        observed_records("for (const value of [1]) observe(); 0;") >= std::mem::size_of::<Value>()
    );
}

#[cfg_attr(test, test)]
fn interpreter_records_charge_both_owners_in_one_frame() {
    assert!(
        observed_records("for (const value of [1]) {try {observe();} finally {}} 0;")
            >= std::mem::size_of::<Value>() + std::mem::size_of::<HandlerFrame>()
    );
}

#[cfg_attr(test, test)]
fn interpreter_records_debugger_pause_preserves_and_resume_releases_charges() {
    let code = code("for (const value of [1]) {try {value;} finally {}} 0;");
    let push = code
        .instructions()
        .find(|instruction| instruction.opcode == Opcode::PushHandler)
        .unwrap();
    let target = push.offset + push.opcode.width();
    let mut vm = page_vm();
    assert!(matches!(
        vm.execute_script_until_debugger_pause(&code, target as u32)
            .unwrap(),
        VmDebuggerExecutionState::Paused { .. }
    ));
    assert!(
        vm.displaced_execution_control_bytes.get()
            >= std::mem::size_of::<Value>() + std::mem::size_of::<HandlerFrame>()
    );
    assert_eq!(
        vm.resume_debugger_execution().unwrap(),
        VmDebuggerExecutionState::Completed
    );
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
}

#[cfg_attr(test, test)]
fn interpreter_records_refuse_handler_growth_before_pause_publication() {
    let code = code("try {} finally {} 0;");
    let push = code
        .instructions()
        .find(|instruction| instruction.opcode == Opcode::PushHandler)
        .unwrap();
    let mut vm = page_vm();
    vm.remaining_instructions = 100;
    let used = vm.host_scheduling_usage().unwrap().payload_bytes;
    let _peer = vm
        .host_scheduling
        .as_ref()
        .unwrap()
        .shared_charge(MAX_HOST_SCHEDULING_BYTES - used)
        .unwrap();
    let mut iterators = vm.iterator_records(Vec::new());
    let result = vm.interpret(
        &code,
        &mut iterators,
        push.offset,
        None,
        Some(InterpreterSuspensionPoint::Offset(
            push.offset + push.opcode.width(),
        )),
        None,
    );
    assert!(matches!(result, Err(RuntimeError::SchedulingLimit)));
}

impl crate::Vm {
    /// Runs interpreter record ownership contracts in unit and coverage builds.
    #[doc(hidden)]
    pub fn verify_interpreter_record_ownership_contracts() {
        interpreter_records_charge_handlers_during_native_callbacks();
        interpreter_records_charge_iterators_during_native_callbacks();
        interpreter_records_charge_both_owners_in_one_frame();
        interpreter_records_debugger_pause_preserves_and_resume_releases_charges();
        interpreter_records_refuse_handler_growth_before_pause_publication();
        interpreter_records_refuse_iterator_growth_before_publication();
        interpreter_records_existing_slots_do_not_add_a_growth_forecast();
        interpreter_records_close_in_reverse_order_with_live_storage();
        interpreter_records_async_wait_transfers_one_charge_and_refunds_completion();
        interpreter_records_generators_transfer_native_records_to_managed_storage();
        interpreter_records_async_generator_crosses_both_storage_boundaries();
        interpreter_records_vm_drop_releases_pending_record_charges();
        interpreter_records_reconcile_direct_interpreter_exit();
        interpreter_records_root_metadata_capacity_is_charged_and_released();
    }
}

#[cfg_attr(test, test)]
fn interpreter_records_refuse_iterator_growth_before_publication() {
    let mut vm = page_vm();
    let array = vm.execute_script(&code("[1];")).unwrap();
    let code = code("for (const value of [1]) {} 0;");
    let get = code
        .instructions()
        .find(|instruction| instruction.opcode == Opcode::GetIterator)
        .unwrap();
    vm.stack = Vec::with_capacity(32);
    vm.stack.push(array);
    vm.remaining_instructions = 100;
    let mut iterators = vm.iterator_records(Vec::new());
    let used = vm.host_scheduling_usage().unwrap().payload_bytes;
    let _peer = vm
        .host_scheduling
        .as_ref()
        .unwrap()
        .shared_charge(MAX_HOST_SCHEDULING_BYTES - used)
        .unwrap();
    let result = vm.interpret(
        &code,
        &mut iterators,
        get.offset,
        None,
        Some(InterpreterSuspensionPoint::Offset(
            get.offset + get.opcode.width(),
        )),
        None,
    );
    assert!(matches!(result, Err(RuntimeError::SchedulingLimit)));
    assert!(iterators.is_empty());
    assert_eq!(iterators.capacity(), 0);
}

#[cfg_attr(test, test)]
fn interpreter_records_existing_slots_do_not_add_a_growth_forecast() {
    let mut vm = page_vm();
    let mut iterators = vm.iterator_records(Vec::with_capacity(4));
    let mut handlers = vm.handler_records(Vec::with_capacity(4));
    let used = vm.host_scheduling_usage().unwrap().payload_bytes;
    let _peer = vm
        .host_scheduling
        .as_ref()
        .unwrap()
        .shared_charge(MAX_HOST_SCHEDULING_BYTES - used)
        .unwrap();
    let record = Value::Object(vm.object_prototype);
    iterators.try_push(&mut vm, record).unwrap();
    handlers
        .try_push(
            &mut vm,
            HandlerFrame {
                metadata: 0,
                stack_depth: 0,
                scope_depth: 0,
                iterator_depth: 0,
                with_depth: 0,
                state: HandlerState::Try,
                pending: None,
                pending_origin: Default::default(),
            },
        )
        .unwrap();
    assert_eq!(iterators.capacity(), 4);
    assert_eq!(handlers.capacity(), 4);
    assert_eq!(
        vm.host_scheduling_usage().unwrap().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES
    );
}

#[cfg_attr(test, test)]
fn interpreter_records_close_in_reverse_order_with_live_storage() {
    for nursery in [1, VmConfig::default().heap.nursery_capacity] {
        let mut config = VmConfig::default();
        config.heap.nursery_capacity = nursery;
        let mut vm = Vm::new(config).unwrap();
        vm.use_bounded_host_microtask_checkpoints().unwrap();
        let source = "var closed=[]; var sources=[0,1,2].map(i=>({[Symbol.iterator](){return this;},next(){return {value:i,done:false};},return(){closed.push(i); var a=[{},{},{},{}]; if(i===2)throw 7; return {done:true};}})); function f(){for(const a of sources[0]){for(const b of sources[1]){for(const c of sources[2]){return 1;}}}} var caught; try{f();}catch(e){caught=e;} closed.join(',')+':'+caught;";
        assert_eq!(
            vm.execute_script(&code(source)),
            Ok(Value::String("2,1,0:7".into()))
        );
        assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
        assert_eq!(vm.async_frame_roots.capacity(), 0);
    }
}

#[cfg_attr(test, test)]
fn interpreter_records_async_wait_transfers_one_charge_and_refunds_completion() {
    let mut vm = page_vm();
    vm.execute_script(&code("var gate=Promise.withResolvers(); var result=(async()=>{for(const value of [1]){try{await gate.promise;}finally{}}})(); 0;")).unwrap();
    assert_eq!(vm.async_continuations.len(), 1);
    let frame = vm.async_continuations.values().next().unwrap();
    assert_eq!(frame.iterators.len(), 1);
    assert_eq!(frame.handlers.len(), 1);
    assert_eq!(frame.iterators.unleased_bytes(), 0);
    assert_eq!(frame.handlers.unleased_bytes(), 0);
    let expected = frame.execution.control_bytes()
        + frame.iterators.capacity() * std::mem::size_of::<Value>()
        + frame.handlers.capacity() * std::mem::size_of::<HandlerFrame>();
    assert_eq!(vm.displaced_execution_control_bytes.get(), expected);
    vm.execute_script(&code("gate.resolve(); 0;")).unwrap();
    for _ in 0..16 {
        if !vm.run_next_promise_job().unwrap() {
            break;
        }
    }
    assert!(vm.async_continuations.is_empty());
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
}

#[cfg_attr(test, test)]
fn interpreter_records_generators_transfer_native_records_to_managed_storage() {
    let mut vm = page_vm();
    vm.execute_script(&code("function* f(){for(const value of [1]){try{yield value;}finally{}}} var g=f(); var first=g.next(); 0;")).unwrap();
    let generator = vm
        .lookup_global_name("g")
        .unwrap()
        .unwrap()
        .object_id()
        .unwrap();
    let state = vm.heap.take_generator_state(generator).unwrap();
    let GeneratorState::Suspended {
        iterators,
        handlers,
        ..
    } = &state
    else {
        panic!("generator must retain its records")
    };
    assert_eq!(iterators.len(), 1);
    assert_eq!(handlers.len(), 1);
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
    vm.heap.set_generator_state(generator, state).unwrap();
    assert_eq!(
        vm.execute_script(&code("g.next().done;")),
        Ok(Value::Bool(true))
    );
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
}

#[cfg_attr(test, test)]
fn interpreter_records_async_generator_crosses_both_storage_boundaries() {
    let mut vm = page_vm();
    vm.execute_script(&code("var gate=Promise.withResolvers(); async function* f(){for(const value of [1]){try{await gate.promise; yield value;}finally{}}} var g=f(); var first=g.next(); 0;")).unwrap();
    assert_eq!(vm.async_continuations.len(), 1);
    assert!(vm.displaced_execution_control_bytes.get() > 0);
    vm.execute_script(&code("gate.resolve(); 0;")).unwrap();
    for _ in 0..32 {
        if !vm.run_next_promise_job().unwrap() {
            break;
        }
    }
    assert!(vm.async_continuations.is_empty());
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
    vm.execute_script(&code("var last=g.next(); 0;")).unwrap();
    for _ in 0..16 {
        if !vm.run_next_promise_job().unwrap() {
            break;
        }
    }
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
}

#[cfg_attr(test, test)]
fn interpreter_records_vm_drop_releases_pending_record_charges() {
    let mut vm = page_vm();
    let counter = vm.displaced_execution_control_bytes.clone();
    vm.execute_script(&code("var gate=Promise.withResolvers(); var result=(async()=>{for(const value of [1]){try{await gate.promise;}finally{}}})(); 0;")).unwrap();
    assert!(counter.get() > 0);
    drop(vm);
    assert_eq!(counter.get(), 0);
}

#[cfg_attr(test, test)]
fn interpreter_records_reconcile_direct_interpreter_exit() {
    let mut vm = page_vm();
    // A second realm reads the stored shared contribution, rather than the
    // current realm's snapshot that recomputes its own live allocation.
    let peer = Vm {
        host_scheduling: Some(vm.host_scheduling.as_ref().unwrap().child()),
        ..Vm::default()
    };
    let code = code("try {0;} finally {} 0;");
    vm.prepare_root_execution(&code, false).unwrap();
    let mut iterators = vm.iterator_records(Vec::new());
    assert!(matches!(
        vm.interpret(&code, &mut iterators, 0, None, None, None),
        Ok(InterpreterExit::Return(_))
    ));
    assert_eq!(vm.displaced_execution_control_bytes.get(), 0);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        vm.own_scheduling_usage().payload_bytes
            + vm.heap.collector_storage_bytes()
            + vm.heap.root_storage_bytes()
    );
    assert_eq!(
        peer.host_scheduling_snapshot().payload_bytes,
        vm.host_scheduling_snapshot().payload_bytes
    );
}

#[cfg_attr(test, test)]
fn interpreter_records_root_metadata_capacity_is_charged_and_released() {
    let mut vm = page_vm();
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    vm.async_frame_roots.reserve(7);
    let delta = vm.async_frame_roots.capacity() * std::mem::size_of::<ObjectId>();
    assert_eq!(
        vm.host_scheduling_usage().unwrap().payload_bytes - before,
        delta
    );
    vm.execute_script(&code("for(const value of [1]) {value;} 0;"))
        .unwrap();
    assert_eq!(vm.async_frame_roots.capacity(), 0);
}
