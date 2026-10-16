// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_bluejs::{
    compile, parse, BlueJsPageOrigin, BlueJsPageRuntime, BlueJsPageRuntimeError, BlueJsProgramV1,
    BlueJsSourceIdentity, RuntimeError, Value, Vm,
};

fn execute(
    runtime: &mut BlueJsPageRuntime,
    tab: u64,
    source: &str,
) -> Result<Value, BlueJsPageRuntimeError> {
    let stats = runtime.realm_stats(tab).unwrap();
    let program = runtime
        .install_program(
            tab,
            &stats.origin,
            BlueJsSourceIdentity::new(format!("page:///frame-{}.js", stats.program_count), "frame")
                .unwrap(),
            &BlueJsProgramV1::Script(parse(source).unwrap()),
        )
        .unwrap();
    runtime.execute_program(tab, program)
}

fn exhaustion_is_document_owned(source: &str) {
    let mut runtime = BlueJsPageRuntime::default();
    let origin = BlueJsPageOrigin::new("https://active-frames.test").unwrap();
    runtime.open_realm(1, origin.clone()).unwrap();
    runtime.open_realm(2, origin.clone()).unwrap();
    let result = execute(&mut runtime, 1, source);
    assert!(
        matches!(
            result,
            Err(BlueJsPageRuntimeError::Runtime(
                RuntimeError::SchedulingLimit
            ))
        ),
        "live native frames bypassed the document budget: {result:?}; source={source}"
    );
    assert!(execute(&mut runtime, 1, "42;").is_err());
    assert!(runtime.run_microtask_checkpoint(1).is_err());
    assert_eq!(
        execute(&mut runtime, 2, "42;").unwrap(),
        Value::Number(42.0)
    );
    runtime.navigate(1, origin).unwrap();
    assert_eq!(
        execute(&mut runtime, 1, "42;").unwrap(),
        Value::Number(42.0)
    );
}

#[test]
fn active_local_strings_cannot_bypass_the_document_native_budget() {
    exhaustion_is_document_owned("function hold(n){var x='x'.repeat(400000);if(n===0)return x.length;return hold(n-1)+x.length;}hold(16);");
}

#[test]
fn active_argument_strings_cannot_bypass_the_document_native_budget() {
    exhaustion_is_document_owned("function hold(n,x){if(n===0)return x.length;return hold(n-1,'x'.repeat(400000))+x.length;}hold(16,'x'.repeat(400000));");
}

#[test]
fn eval_generator_and_async_entries_preserve_caller_payload_ownership() {
    for source in [
        "function hold(n){if(n===0)return 0;return 'x'.repeat(400000)+eval('hold(n-1)');}hold(16);",
        "function* item(n,value=hold(n-1)){yield value;}function hold(n){var x='x'.repeat(400000);if(n===0)return x.length;return item(n).next().value+x.length;}hold(16);",
        "function* hold(n){var x='x'.repeat(400000);if(n===0)yield x.length;else yield hold(n-1).next().value+x.length;}hold(16).next();",
        "async function hold(n){var x='x'.repeat(400000);if(n===0)return x.length;return (await hold(n-1))+x.length;}hold(16);",
    ] {
        exhaustion_is_document_owned(source);
    }
}

#[test]
fn operand_payload_growth_is_checked_between_instructions() {
    let expression = (0..16).fold("value".to_string(), |tail, _| format!("value+({tail})"));
    exhaustion_is_document_owned(&format!("var value='x'.repeat(400000);{expression};"));
}

#[test]
fn recursive_calls_share_the_cap_with_registry_promise_and_timer_owners() {
    let mut runtime = BlueJsPageRuntime::default();
    let origin = BlueJsPageOrigin::new("https://active-shared.test").unwrap();
    runtime.open_realm(1, origin).unwrap();
    runtime
        .configure_realm_bindings(1, |host| host.install_host_timers())
        .unwrap();
    execute(&mut runtime,1,"for(var i=0;i<4;i++)Symbol.for('r'.repeat(100000)+String(i));var held=[Promise.resolve('p'.repeat(200000)),Promise.resolve('q'.repeat(200000))];function noop(){}setTimeout(noop,100000,'t'.repeat(200000));setTimeout(noop,100000,'u'.repeat(200000));0;").unwrap();
    assert!(matches!(execute(&mut runtime,1,"function hold(n){var x='x'.repeat(400000);if(n===0)return x.length;return hold(n-1)+x.length;}hold(10);"),Err(BlueJsPageRuntimeError::Runtime(RuntimeError::SchedulingLimit))));
    assert!(runtime.run_task_turn(1, 100000, 64).is_err());
}

#[test]
fn parent_and_child_active_frames_cannot_split_the_document_cap() {
    exhaustion_is_document_owned("function hold(n){var x='x'.repeat(400000);if(n===0){var realm=new ShadowRealm();return realm.evaluate(\"function child(n){var y='y'.repeat(400000);if(n===0)return y.length;return child(n-1)+y.length;}child(5);\")+x.length;}return hold(n-1)+x.length;}hold(5);");
}

#[test]
fn completed_thrown_and_eval_frames_refund_their_payload() {
    let mut runtime = BlueJsPageRuntime::default();
    runtime
        .open_realm(
            1,
            BlueJsPageOrigin::new("https://active-release.test").unwrap(),
        )
        .unwrap();
    execute(&mut runtime,1,"Symbol.for('r'.repeat(100000));var held=Promise.resolve('p'.repeat(100000));function work(){var x='x'.repeat(200000);return eval('x.length');}function fail(){var x='y'.repeat(200000);throw x;}0;").unwrap();
    let before = runtime.realm_stats(1).unwrap().scheduling.payload_bytes;
    for _ in 0..16 {
        assert_eq!(execute(&mut runtime,1,"var sum=0;for(var i=0;i<32;i++){sum+=work();try{fail();}catch(error){if(error.length!==200000)throw 'lost value';}}sum;").unwrap(),Value::Number(6400000.0));
        assert_eq!(
            runtime.realm_stats(1).unwrap().scheduling.payload_bytes,
            before
        );
    }
}

#[test]
fn host_neutral_execution_keeps_its_existing_policy() {
    let mut vm = Vm::default();
    let code=compile(&parse("function hold(n){var x='x'.repeat(400000);if(n===0)return x.length;return hold(n-1)+x.length;}hold(16);").unwrap()).unwrap();
    assert_eq!(vm.execute_script(&code).unwrap(), Value::Number(6800000.0));
}

#[test]
fn paused_operand_storage_is_visible_and_released_on_resume_or_navigation() {
    let mut runtime = BlueJsPageRuntime::default();
    let origin = BlueJsPageOrigin::new("https://paused-payload.test").unwrap();
    runtime.open_realm(1, origin.clone()).unwrap();
    for navigate in [false, true] {
        let program = runtime
            .install_program(
                1,
                &origin,
                BlueJsSourceIdentity::new("page:///paused.js", "paused").unwrap(),
                &BlueJsProgramV1::Script(parse("'x'.repeat(400000)+'y';").unwrap()),
            )
            .unwrap();
        let offset = runtime
            .program_registry()
            .get(program)
            .unwrap()
            .bytecode()
            .instructions()
            .find(|instruction| instruction.opcode == blueice_bluejs::Opcode::Add)
            .unwrap()
            .offset as u32;
        runtime
            .execute_program_until_debugger_pause_at_root_offset(1, program, offset)
            .unwrap();
        assert!(runtime.realm_stats(1).unwrap().scheduling.payload_bytes >= 800000);
        if navigate {
            runtime.navigate(1, origin.clone()).unwrap();
        } else {
            runtime.resume_debugger_execution(1).unwrap();
        }
        assert_eq!(runtime.realm_stats(1).unwrap().scheduling.payload_bytes, 0);
    }
}

#[test]
fn failed_child_frames_refund_payload_without_recovering_the_document() {
    for wrapped in [false, true] {
        let mut runtime = BlueJsPageRuntime::default();
        let origin = BlueJsPageOrigin::new("https://failed-frame.test").unwrap();
        runtime.open_realm(1, origin.clone()).unwrap();
        execute(&mut runtime, 1, "Symbol.for('r'.repeat(100000));0;").unwrap();
        let before = runtime.realm_stats(1).unwrap().scheduling.payload_bytes;
        let body = "function hold(n){var x='x'.repeat(400000);if(n===0)return x.length;return hold(n-1)+x.length;}";
        let source = if wrapped {
            format!(
                "var realm=new ShadowRealm();var hold=realm.evaluate({:?});hold(16);",
                format!("{body}hold;")
            )
        } else {
            format!(
                "var realm=new ShadowRealm();realm.evaluate({:?});",
                format!("{body}hold(16);")
            )
        };
        assert!(matches!(
            execute(&mut runtime, 1, &source),
            Err(BlueJsPageRuntimeError::Runtime(
                RuntimeError::SchedulingLimit
            ))
        ));
        assert_eq!(
            runtime.realm_stats(1).unwrap().scheduling.payload_bytes,
            before,
            "released child frames remain charged; wrapped={wrapped}"
        );
        assert!(execute(&mut runtime, 1, "42;").is_err());
        runtime.navigate(1, origin).unwrap();
        assert_eq!(runtime.realm_stats(1).unwrap().scheduling.payload_bytes, 0);
    }
}

#[test]
fn discarded_async_return_bursts_collect_before_settlement() {
    for small_heap in [false, true] {
        for unified in [false, true] {
            let mut config = blueice_bluejs::BlueJsPageRuntimeConfig::default();
            if small_heap {
                config.vm.heap.max_heap_bytes = 1024 * 1024;
                config.vm.heap.major_threshold_bytes = 512 * 1024;
                config.vm.heap.nursery_capacity = 4096;
            }
            let mut runtime = BlueJsPageRuntime::new(config).unwrap();
            runtime
                .open_realm(
                    1,
                    BlueJsPageOrigin::new("https://async-return.test").unwrap(),
                )
                .unwrap();
            runtime
                .configure_realm_bindings(1, |host| host.install_host_timers())
                .unwrap();
            let source=format!("var ticks=0;async function garbage(){{return 'x'.repeat(400000);}}function tick(){{{}ticks++;}}setInterval(tick,10);","garbage();".repeat(32));
            execute(&mut runtime, 1, &source).unwrap();
            let before = runtime.realm_stats(1).unwrap().heap.major_collections;
            for tick in 1..=16 {
                let result = if unified {
                    runtime
                        .run_task_turn(1, tick * 10, 64)
                        .map(|run| run.tasks_run)
                } else {
                    runtime
                        .run_due_timers(1, tick * 10)
                        .map(|run| run.tasks_run)
                };
                assert_eq!(
                    result.unwrap_or_else(|error| panic!(
                        "small_heap={small_heap} unified={unified} tick={tick}: {error:?}"
                    )),
                    1
                );
                assert!(runtime.realm_stats(1).unwrap().scheduling.payload_bytes < 4 * 1024 * 1024);
            }
            assert!(runtime.realm_stats(1).unwrap().heap.major_collections > before);
        }
    }
}

#[test]
fn caller_finally_completions_survive_async_and_generator_collection() {
    for entry in ["churn();", "items().next();", "parameterized();"] {
        let mut runtime = BlueJsPageRuntime::default();
        runtime
            .open_realm(
                1,
                BlueJsPageOrigin::new("https://pending-roots.test").unwrap(),
            )
            .unwrap();
        let source="async function garbage(){return 'x'.repeat(400000);}function churn(){__BURST__}function* items(){churn();yield 0;}function* parameterized(value=churn()){yield value;}function returning(){try{return {tag:42};}finally{__ENTRY__}}returning().tag;".replace("__BURST__",&"garbage();".repeat(32)).replace("__ENTRY__",entry);
        assert_eq!(
            execute(&mut runtime, 1, &source).unwrap(),
            Value::Number(42.0)
        );
        assert!(runtime.realm_stats(1).unwrap().heap.major_collections > 1);
    }
}

#[test]
fn child_await_does_not_capture_the_callers_pending_return() {
    let mut runtime = BlueJsPageRuntime::default();
    runtime
        .open_realm(
            1,
            BlueJsPageOrigin::new("https://pending-return.test").unwrap(),
        )
        .unwrap();
    assert_eq!(execute(&mut runtime,1,"var release,held,result=0;var pending=new Promise(function(done){release=done;});async function wait(){return await pending;}function returning(){try{return 'x'.repeat(400000);}finally{held=wait();}}returning().length;").unwrap(),Value::Number(400000.0));
    assert!(
        runtime.realm_stats(1).unwrap().scheduling.payload_bytes < 64 * 1024,
        "child continuation retained its caller's returned string"
    );
    execute(
        &mut runtime,
        1,
        "held.then(function(value){result=value;});release(42);0;",
    )
    .unwrap();
    runtime.run_microtask_checkpoint(1).unwrap();
    assert_eq!(
        execute(&mut runtime, 1, "result;").unwrap(),
        Value::Number(42.0)
    );
}

#[test]
fn every_root_pause_handoff_obeys_the_native_budget_without_spending_fuel() {
    let expression = (0..16).fold("value".to_string(), |tail, _| format!("value+({tail})"));
    let source = format!("var value='x'.repeat(400000);{expression};");
    let prepare = || {
        let mut runtime = BlueJsPageRuntime::default();
        let origin = BlueJsPageOrigin::new("https://pause-budget.test").unwrap();
        runtime.open_realm(1, origin.clone()).unwrap();
        let program = runtime
            .install_program(
                1,
                &origin,
                BlueJsSourceIdentity::new("page:///pause-budget.js", "pause-budget").unwrap(),
                &BlueJsProgramV1::Script(parse(&source).unwrap()),
            )
            .unwrap();
        (runtime, program)
    };
    let (template, program) = prepare();
    let offsets = template
        .program_registry()
        .get(program)
        .unwrap()
        .bytecode()
        .instructions()
        .map(|instruction| instruction.offset as u32)
        .collect::<Vec<_>>();
    let mut largest_pause = 0;
    let mut refused = 0;
    for offset in offsets {
        let (mut runtime, program) = prepare();
        match runtime.execute_program_until_debugger_pause_at_root_offset(1, program, offset) {
            Ok(blueice_bluejs::BlueJsPageDebuggerExecutionState::Paused { .. }) => {
                let bytes = runtime.realm_stats(1).unwrap().scheduling.payload_bytes;
                assert!(
                    bytes <= blueice_bluejs::MAX_HOST_SCHEDULING_BYTES,
                    "pause at {offset} retained {bytes} bytes"
                );
                largest_pause = largest_pause.max(bytes);
            }
            Err(BlueJsPageRuntimeError::Runtime(RuntimeError::SchedulingLimit)) => {
                refused += 1;
                assert_eq!(runtime.realm_stats(1).unwrap().scheduling.payload_bytes, 0);
                assert!(runtime.resume_debugger_execution(1).is_err());
                assert!(matches!(
                    execute(&mut runtime, 1, "42;"),
                    Err(BlueJsPageRuntimeError::Runtime(
                        RuntimeError::SchedulingLimit
                    ))
                ));
            }
            result => panic!("unexpected boundary result at {offset}: {result:?}"),
        }
    }
    assert!(largest_pause >= 8000000);
    assert!(refused > 0);
    let mut config = blueice_bluejs::BlueJsPageRuntimeConfig::default();
    config.vm.instruction_budget = 1;
    let mut runtime = BlueJsPageRuntime::new(config).unwrap();
    let origin = BlueJsPageOrigin::new("https://pause-fuel.test").unwrap();
    runtime.open_realm(1, origin.clone()).unwrap();
    let program = runtime
        .install_program(
            1,
            &origin,
            BlueJsSourceIdentity::new("page:///fuel.js", "fuel").unwrap(),
            &BlueJsProgramV1::Script(parse("42;").unwrap()),
        )
        .unwrap();
    assert!(matches!(
        runtime
            .execute_program_until_debugger_pause_at_root_offset(1, program, 0)
            .unwrap(),
        blueice_bluejs::BlueJsPageDebuggerExecutionState::Paused { .. }
    ));
    assert!(matches!(
        runtime.step_debugger_root_instruction(1).unwrap(),
        blueice_bluejs::BlueJsPageDebuggerExecutionState::Paused { .. }
    ));
    assert!(matches!(
        runtime.resume_debugger_execution(1),
        Err(BlueJsPageRuntimeError::Runtime(
            RuntimeError::InstructionLimit
        ))
    ));
}

#[test]
fn eval_promoted_values_use_the_managed_account_without_a_second_native_charge() {
    let mut runtime = BlueJsPageRuntime::default();
    runtime
        .open_realm(1, BlueJsPageOrigin::new("https://eval-owned.test").unwrap())
        .unwrap();
    assert_eq!(execute(&mut runtime,1,"function hold(n){var x='x'.repeat(400000);if(n===0)return x.length;return eval('hold(n-1)')+x.length;}hold(16);").unwrap(),Value::Number(6800000.0));
}
