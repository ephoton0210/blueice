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

/// Installs a program (the registry retains and charges it) without running it.
fn install(
    runtime: &mut BlueJsPageRuntime,
    tab: u64,
    source: &str,
) -> blueice_bluejs::BlueJsProgramHandle {
    let stats = runtime.realm_stats(tab).unwrap();
    runtime
        .install_program(
            tab,
            &stats.origin,
            BlueJsSourceIdentity::new(format!("page:///frame-{}.js", stats.program_count), "frame")
                .unwrap(),
            &BlueJsProgramV1::Script(parse(source).unwrap()),
        )
        .unwrap()
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
    // One installed program run repeatedly: after a warm-up run the document
    // owes exactly what it owed, so no frame payload outlives its run.
    let program = install(&mut runtime,1,"var sum=0;for(var i=0;i<32;i++){sum+=work();try{fail();}catch(error){if(error.length!==200000)throw 'lost value';}}sum;");
    assert_eq!(
        runtime.execute_program(1, program).unwrap(),
        Value::Number(6400000.0)
    );
    let before = runtime.realm_stats(1).unwrap().scheduling.payload_bytes;
    for _ in 0..16 {
        assert_eq!(
            runtime.execute_program(1, program).unwrap(),
            Value::Number(6400000.0)
        );
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

/// What a resumed pause leaves charged beyond the installed program, for an
/// operand of `size` code units (the digits are kept the same length so the
/// source text is too).
fn resumed_pause_residual(size: &str) -> usize {
    let mut runtime = BlueJsPageRuntime::default();
    let origin = BlueJsPageOrigin::new("https://paused-residual.test").unwrap();
    runtime.open_realm(1, origin.clone()).unwrap();
    let program = install(&mut runtime, 1, &format!("'x'.repeat({size})+'y';"));
    let installed = runtime.realm_stats(1).unwrap().scheduling.payload_bytes;
    let offset = add_offset(&runtime, program);
    runtime
        .execute_program_until_debugger_pause_at_root_offset(1, program, offset)
        .unwrap();
    runtime.resume_debugger_execution(1).unwrap();
    runtime.realm_stats(1).unwrap().scheduling.payload_bytes - installed
}

fn add_offset(runtime: &BlueJsPageRuntime, program: blueice_bluejs::BlueJsProgramHandle) -> u32 {
    runtime
        .program_registry()
        .get(program)
        .unwrap()
        .bytecode()
        .instructions()
        .find(|instruction| instruction.opcode == blueice_bluejs::Opcode::Add)
        .unwrap()
        .offset as u32
}

#[test]
fn paused_operand_storage_is_visible_and_released_on_resume_or_navigation() {
    let mut runtime = BlueJsPageRuntime::default();
    let origin = BlueJsPageOrigin::new("https://paused-payload.test").unwrap();
    runtime.open_realm(1, origin.clone()).unwrap();
    // A fresh realm already owes its own root-table storage.
    let fresh = runtime.realm_stats(1).unwrap().scheduling.payload_bytes;
    let program = install(&mut runtime, 1, "'x'.repeat(400000)+'y';");
    let offset = add_offset(&runtime, program);
    runtime
        .execute_program_until_debugger_pause_at_root_offset(1, program, offset)
        .unwrap();
    assert!(runtime.realm_stats(1).unwrap().scheduling.payload_bytes >= 800000);
    runtime.navigate(1, origin).unwrap();
    assert_eq!(
        runtime.realm_stats(1).unwrap().scheduling.payload_bytes,
        fresh
    );
    // Resuming leaves only the program's own code/source charge, which does
    // not depend on how large the released operand was.
    assert_eq!(
        resumed_pause_residual("400000"),
        resumed_pause_residual("300000")
    );
}

/// What a refused deep child-frame run leaves charged beyond its installed
/// program, for frames holding `size` code units each.
fn failed_frame_residual(wrapped: bool, size: &str) -> usize {
    let mut runtime = BlueJsPageRuntime::default();
    let origin = BlueJsPageOrigin::new("https://failed-frame.test").unwrap();
    runtime.open_realm(1, origin.clone()).unwrap();
    let fresh = runtime.realm_stats(1).unwrap().scheduling.payload_bytes;
    execute(&mut runtime, 1, "Symbol.for('r'.repeat(100000));0;").unwrap();
    let body = format!("function hold(n){{var x='x'.repeat({size});if(n===0)return x.length;return hold(n-1)+x.length;}}");
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
    let program = install(&mut runtime, 1, &source);
    let before = runtime.realm_stats(1).unwrap().scheduling.payload_bytes;
    assert!(matches!(
        runtime.execute_program(1, program),
        Err(BlueJsPageRuntimeError::Runtime(
            RuntimeError::SchedulingLimit
        ))
    ));
    let after = runtime.realm_stats(1).unwrap().scheduling.payload_bytes;
    assert!(execute(&mut runtime, 1, "42;").is_err());
    runtime.navigate(1, origin).unwrap();
    assert_eq!(
        runtime.realm_stats(1).unwrap().scheduling.payload_bytes,
        fresh
    );
    after - before
}

#[test]
fn failed_child_frames_refund_payload_without_recovering_the_document() {
    for wrapped in [false, true] {
        // Both sizes overrun the 8 MiB budget at the same frame depth; what stays charged is the
        // program's own code and must not depend on the released frames.
        assert_eq!(
            failed_frame_residual(wrapped, "400000"),
            failed_frame_residual(wrapped, "390000"),
            "released child frames remain charged; wrapped={wrapped}"
        );
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
    let source_for = |size: &str| format!("var value='x'.repeat({size});{expression};");
    let source = source_for("400000");
    let prepare_with = |source: &str| {
        let mut runtime = BlueJsPageRuntime::default();
        let origin = BlueJsPageOrigin::new("https://pause-budget.test").unwrap();
        runtime.open_realm(1, origin.clone()).unwrap();
        let program = runtime
            .install_program(
                1,
                &origin,
                BlueJsSourceIdentity::new("page:///pause-budget.js", "pause-budget").unwrap(),
                &BlueJsProgramV1::Script(parse(source).unwrap()),
            )
            .unwrap();
        (runtime, program)
    };
    let prepare = || prepare_with(&source);
    let twin_source = source_for("390000");
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
    let mut compared = 0;
    for offset in offsets {
        let (mut runtime, program) = prepare();
        let installed = runtime.realm_stats(1).unwrap().scheduling.payload_bytes;
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
                // Only the program's bounded code/source charge stays after
                // the refusal; the operand payload is released.
                let after = runtime.realm_stats(1).unwrap().scheduling.payload_bytes;
                // The same program with a slightly smaller operand, refused
                // at the same offset, must leave exactly the same residue.
                let (mut twin, twin_program) = prepare_with(&twin_source);
                let twin_installed = twin.realm_stats(1).unwrap().scheduling.payload_bytes;
                if matches!(
                    twin.execute_program_until_debugger_pause_at_root_offset(
                        1,
                        twin_program,
                        offset
                    ),
                    Err(BlueJsPageRuntimeError::Runtime(
                        RuntimeError::SchedulingLimit
                    ))
                ) {
                    compared += 1;
                    assert_eq!(
                        after - installed,
                        twin.realm_stats(1).unwrap().scheduling.payload_bytes - twin_installed,
                        "pause at {offset} left operand payload charged"
                    );
                }
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
    assert!(compared > 0);
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
