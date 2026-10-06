// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Deterministic host queue, worker shutdown, and real ingress boundaries.

use super::*;

fn script(source: &str) -> Bytecode {
    compile(&parse(source).unwrap()).unwrap()
}

fn host_vm() -> Vm {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    vm
}

#[cfg_attr(test, test)]
fn host_shutdown_collects_failures_reported_by_a_joining_worker() {
    let host = Arc::new(Test262AgentHost::new());
    let worker_host = Arc::clone(&host);
    host.add_handle(thread::spawn(move || {
        while !worker_host.is_shutting_down() {
            thread::yield_now();
        }
        worker_host.report_error(RuntimeError::TypeError("late worker failure".into()));
    }));
    assert!(
        matches!(host.shutdown(), Err(RuntimeError::Test262(message))
        if message.contains("late worker failure"))
    );
    assert_eq!(host.shutdown(), Ok(()));
}

#[cfg_attr(test, test)]
fn idle_and_already_queued_completions_never_wait_for_another_wakeup() {
    let mut vm = host_vm();
    vm.test262_async_waits.wait_for_event();
    assert!(!vm.test262_async_waits.has_pending());
    let promise = vm.new_promise().unwrap();
    vm.test262_async_waits.register_pending();
    vm.test262_async_waits.push(AsyncHostEvent::Wait {
        promise,
        result: crate::heap::SharedWaitResult::TimedOut,
    });
    vm.test262_async_waits.wait_for_event();
    assert_eq!(vm.process_test262_async_wait_events(), Ok(true));
    assert!(!vm.test262_async_waits.has_pending());
    assert_eq!(vm.run_test262_async_until_done(), Ok(None));
    assert_eq!(vm.process_test262_async_wait_events(), Ok(false));
}

#[cfg_attr(test, test)]
fn agent_ingress_preserves_string_number_callback_and_buffer_errors() {
    let mut vm = host_vm();
    for name in ["agentStart", "agentReport", "agentSleep"] {
        let value = vm
            .execute_script(&script("({[Symbol.toPrimitive]() {throw 7;}})"))
            .unwrap();
        assert_eq!(
            vm.test262_agent_call(name, &[value]).unwrap(),
            Err(RuntimeError::Thrown(Value::Number(7.0)))
        );
    }
    assert_eq!(
        vm.test262_agent_start(&Value::String(JsString::from_code_units(vec![0xd800]))),
        Err(RuntimeError::TypeError("agent source is not UTF-8".into()))
    );
    assert!(matches!(
        vm.test262_agent_broadcast(&Value::Undefined),
        Err(RuntimeError::TypeError(_))
    ));
    let ordinary = vm.heap.alloc_object(None).unwrap();
    let ordinary_root = vm.heap.root(ordinary).unwrap();
    assert!(matches!(
        vm.test262_agent_broadcast(&Value::Object(ordinary)),
        Err(RuntimeError::TypeError(_))
    ));
    assert!(matches!(
        vm.test262_agent_receive(&Value::Number(7.0)),
        Err(RuntimeError::TypeError(_))
    ));
    let callback = vm.execute_script(&script("(() => 42)")).unwrap();
    assert_eq!(
        vm.test262_agent_receive(&callback),
        Err(RuntimeError::TypeError(
            "receiveBroadcast requires an agent".into()
        ))
    );
    let mut owner = Vm::default();
    let foreign = owner.execute_script(&script("(() => 42)")).unwrap();
    let foreign_root = owner.heap.root(foreign.object_id().unwrap()).unwrap();
    assert_eq!(
        vm.test262_agent_receive(&foreign),
        Err(RuntimeError::Heap(HeapError::InvalidObject(
            foreign.object_id().unwrap()
        )))
    );
    vm.test262_agent_control = Some(vm.agent_host().register());
    vm.shutdown_test262_agents().unwrap();
    assert_eq!(vm.test262_agent_receive(&callback), Ok(Value::Undefined));
    assert!(vm.stack.is_empty());
    assert_eq!(
        vm.execute_script(&script("21 + 21")),
        Ok(Value::Number(42.0))
    );
    vm.heap.unroot(ordinary_root).unwrap();
    owner.heap.unroot(foreign_root).unwrap();
}

#[cfg_attr(test, test)]
fn agent_receive_preserves_cold_constructor_and_wrapper_refusals() {
    for warm in [false, true] {
        let mut vm = host_vm();
        let callback = vm.execute_script(&script("(() => 42)")).unwrap();
        let root = vm.heap.root(callback.object_id().unwrap()).unwrap();
        vm.test262_agent_control = Some(vm.agent_host().register());
        if warm {
            let constructor = vm.global("SharedArrayBuffer").unwrap();
            assert!(vm.is_callable(&callback).unwrap());
            assert!(vm
                .get_property(&constructor, &"prototype".into())
                .unwrap()
                .object_id()
                .is_some());
        }
        let backing = Arc::new(SharedBuffer::new(8));
        vm.agent_host().broadcast(Broadcast {
            backing,
            max_byte_length: None,
        });
        vm.with_roots(|heap| {
            heap.collect_major();
            Ok(())
        })
        .unwrap();
        let limit = vm.heap.allow_only(0);
        assert_eq!(
            vm.test262_agent_receive(&callback),
            Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { limit }))
        );
        assert!(vm.stack.is_empty());
        vm.heap.allow_only(16 * 1024 * 1024);
        assert_eq!(
            vm.execute_script(&script("21 + 21")),
            Ok(Value::Number(42.0))
        );
        vm.shutdown_test262_agents().unwrap();
        vm.heap.unroot(root).unwrap();
    }
}

#[cfg_attr(test, test)]
fn agent_workers_report_parse_compile_and_initialization_failures() {
    for source in ["var =;", "let duplicate; let duplicate;"] {
        let mut vm = host_vm();
        vm.test262_agent_start(&Value::String(source.into()))
            .unwrap();
        assert!(
            matches!(vm.shutdown_test262_agents(), Err(RuntimeError::Test262(message))
            if message.contains("SyntaxError"))
        );
        assert_eq!(
            vm.execute_script(&script("21 + 21")),
            Ok(Value::Number(42.0))
        );
    }
    let config = VmConfig {
        heap: HeapConfig {
            max_heap_bytes: 4096,
            major_threshold_bytes: 4096,
            ..HeapConfig::default()
        },
        ..VmConfig::default()
    };
    let mut vm = Vm::new(config).unwrap();
    vm.test262_agent_host = Some(Arc::new(Test262AgentHost::new()));
    vm.test262_agent_start(&Value::String("0".into())).unwrap();
    assert!(
        matches!(vm.shutdown_test262_agents(), Err(RuntimeError::Test262(message))
        if message.contains("heap"))
    );

    let mut vm = host_vm();
    // Lower the parent execution quota after initialization, as other
    // resource fixtures do. A new worker must validate the copied budget.
    let mut probe = Heap::new(HeapConfig::default()).unwrap();
    probe.alloc_object(None).unwrap();
    vm.config.heap.max_heap_bytes = probe.stats().managed_bytes;
    vm.config.heap.major_threshold_bytes = vm.config.heap.max_heap_bytes;
    vm.test262_agent_start(&Value::String("".into())).unwrap();
    assert!(matches!(
        vm.shutdown_test262_agents(),
        Err(RuntimeError::Test262(_))
    ));
    vm.config.heap = VmConfig::default().heap;
    assert_eq!(
        vm.execute_script(&script("21 + 21")),
        Ok(Value::Number(42.0))
    );

    let mut vm = host_vm();
    vm.test262_agent_start(&Value::String("setTimeout(() => {throw 7;}, 1);".into()))
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while vm.agent_host().state.lock().unwrap().errors.is_empty() {
        assert!(Instant::now() < deadline);
        thread::yield_now();
    }
    assert!(
        matches!(vm.shutdown_test262_agents(), Err(RuntimeError::Test262(message))
        if message.contains('7'))
    );
}

#[cfg_attr(test, test)]
fn timers_preserve_root_refusal_and_callback_errors_after_releasing_ownership() {
    let mut vm = host_vm();
    let callback = vm
        .execute_script(&script("(() => {throw 7;})"))
        .unwrap()
        .object_id()
        .unwrap();
    let root = vm.heap.root(callback).unwrap();
    vm.schedule_test262_timer(callback, Duration::ZERO).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while vm.test262_async_waits.events.lock().unwrap().is_empty() {
        assert!(Instant::now() < deadline);
        thread::yield_now();
    }
    assert_eq!(
        vm.run_test262_async_until_done(),
        Err(RuntimeError::Thrown(Value::Number(7.0)))
    );
    assert!(!vm.test262_async_waits.has_pending());
    assert!(vm.stack.is_empty());
    vm.heap.allow_root_registrations(0);
    assert_eq!(
        vm.schedule_test262_timer(callback, Duration::ZERO),
        Err(RuntimeError::Heap(HeapError::IdExhausted))
    );
    vm.heap.unroot(root).unwrap();
}

impl Vm {
    /// Runs agent host contracts in unit and coverage builds.
    #[doc(hidden)]
    pub fn verify_agent_host_boundary_contracts() {
        finalization_job_errors_are_reported_by_the_idle_agent_loop();
        host_shutdown_collects_failures_reported_by_a_joining_worker();
        idle_and_already_queued_completions_never_wait_for_another_wakeup();
        agent_ingress_preserves_string_number_callback_and_buffer_errors();
        agent_receive_preserves_cold_constructor_and_wrapper_refusals();
        agent_workers_report_parse_compile_and_initialization_failures();
        timers_preserve_root_refusal_and_callback_errors_after_releasing_ownership();
    }
}

#[cfg_attr(test, test)]
fn finalization_job_errors_are_reported_by_the_idle_agent_loop() {
    let mut vm = host_vm();
    vm.test262_agent_start(&Value::String(
        "var registry = new FinalizationRegistry(() => {throw 7;}); (function() {registry.register({}, 'held');})(); $262.gc();".into()
    )).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while vm.agent_host().state.lock().unwrap().errors.is_empty() {
        assert!(
            Instant::now() < deadline,
            "the idle agent did not report its cleanup job error"
        );
        thread::yield_now();
    }
    assert!(
        matches!(vm.shutdown_test262_agents(), Err(RuntimeError::Test262(message)) if message.contains('7'))
    );
    assert_eq!(
        vm.execute_script(&script("21 + 21")),
        Ok(Value::Number(42.0))
    );
}
