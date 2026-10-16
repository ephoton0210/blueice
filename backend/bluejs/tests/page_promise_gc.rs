// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_bluejs::{
    compile, parse, BlueJsPageOrigin, BlueJsPageRuntime, BlueJsPageRuntimeConfig, BlueJsProgramV1,
    BlueJsSourceIdentity, RuntimeError, Value, Vm, VmConfig,
};

fn page_script(runtime: &mut BlueJsPageRuntime, source: &str) -> Value {
    let stats = runtime.realm_stats(1).unwrap();
    let identity = format!("page:///gc-control-{}.js", stats.program_count);
    let handle = runtime
        .install_program(
            1,
            &stats.origin,
            BlueJsSourceIdentity::new(identity, "sha256:gc-control").unwrap(),
            &BlueJsProgramV1::Script(parse(source).unwrap()),
        )
        .unwrap();
    runtime.execute_program(1, handle).unwrap()
}

#[test]
fn iterator_close_awaits_once_without_inserting_an_extra_reaction_turn() {
    let mut vm = Vm::default();
    let source = "var order = []; var cursor = {[Symbol.asyncIterator](){return this;}, next(){return Promise.resolve({value:1,done:false});}, return(){order.push('return');return Promise.resolve({done:true});}}; (async()=>{for await(const value of cursor){break;}order.push('left');})(); Promise.resolve().then(()=>order.push('peer'));";
    vm.execute_script(&compile(&parse(source).unwrap()).unwrap())
        .unwrap();
    let read = compile(&parse("order.join('|');").unwrap()).unwrap();
    assert_eq!(
        vm.run_promise_jobs_bounded(1),
        Err(RuntimeError::InstructionLimit)
    );
    assert_eq!(
        vm.execute_script(&read).unwrap(),
        Value::String("return".into())
    );
    assert_eq!(
        vm.run_promise_jobs_bounded(1),
        Err(RuntimeError::InstructionLimit)
    );
    assert_eq!(
        vm.execute_script(&read).unwrap(),
        Value::String("return|peer".into())
    );
    vm.run_promise_jobs_bounded(1).unwrap();
    assert_eq!(
        vm.execute_script(&read).unwrap(),
        Value::String("return|peer|left".into())
    );
}

#[test]
fn page_checkpoints_collect_unreachable_native_string_payload_before_the_byte_cap() {
    let mut runtime = BlueJsPageRuntime::default();
    let origin = BlueJsPageOrigin::new("https://promise-payload.test").unwrap();
    runtime.open_realm(1, origin.clone()).unwrap();
    runtime
        .configure_realm_bindings(1, |host| host.install_host_timers())
        .unwrap();
    let script = parse("function noop() {} function poll() { Promise.resolve('x'.repeat(400000)).then(noop); } setInterval(poll, 10);").unwrap();
    let program = runtime
        .install_program(
            1,
            &origin,
            BlueJsSourceIdentity::new("page:///payload.js", "sha256:payload").unwrap(),
            &BlueJsProgramV1::Script(script),
        )
        .unwrap();
    runtime.execute_program(1, program).unwrap();
    for tick in 1..=64 {
        assert_eq!(
            runtime
                .run_due_timers(1, tick * 10)
                .unwrap_or_else(|error| panic!("payload poll {tick}: {error:?}"))
                .tasks_run,
            1
        );
        assert!(
            runtime.realm_stats(1).unwrap().scheduling.payload_bytes
                < blueice_bluejs::MAX_HOST_SCHEDULING_BYTES / 2
        );
    }
}

#[test]
fn a_timer_can_discard_large_promise_results_before_its_checkpoint() {
    let mut config = BlueJsPageRuntimeConfig::default();
    config.vm.heap.max_heap_bytes = 1024 * 1024;
    config.vm.heap.major_threshold_bytes = 512 * 1024;
    config.vm.heap.nursery_capacity = 4096;
    let mut runtime = BlueJsPageRuntime::new(config).unwrap();
    let origin = BlueJsPageOrigin::new("https://promise-burst.test").unwrap();
    runtime.open_realm(1, origin.clone()).unwrap();
    runtime
        .configure_realm_bindings(1, |host| host.install_host_timers())
        .unwrap();
    let source = "var ticks=0;function poll(){for(var i=0;i<32;i++){Promise.resolve('x'.repeat(400000));}ticks++;}setInterval(poll,10);";
    let program = runtime
        .install_program(
            1,
            &origin,
            BlueJsSourceIdentity::new("page:///burst.js", "sha256:burst").unwrap(),
            &BlueJsProgramV1::Script(parse(source).unwrap()),
        )
        .unwrap();
    runtime.execute_program(1, program).unwrap();
    let before = runtime.realm_stats(1).unwrap().heap.major_collections;
    for tick in 1..=16 {
        assert_eq!(
            runtime
                .run_due_timers(1, tick * 10)
                .unwrap_or_else(|error| panic!("burst {tick}: {error:?}"))
                .tasks_run,
            1
        );
        let usage = runtime.host_scheduling_usage(1).unwrap();
        assert_eq!(usage.pending_microtasks, 0);
        assert_eq!(usage.pending_reactions, 0);
        assert!(usage.payload_bytes < blueice_bluejs::MAX_HOST_SCHEDULING_BYTES / 2);
    }
    assert!(runtime.realm_stats(1).unwrap().heap.major_collections > before);
}

#[test]
fn native_pressure_preserves_timer_reaction_await_and_weakref_owners() {
    let mut config = BlueJsPageRuntimeConfig::default();
    config.vm.heap.nursery_capacity = 4096;
    config.vm.heap.major_threshold_bytes = 512 * 1024;
    let mut runtime = BlueJsPageRuntime::new(config).unwrap();
    runtime
        .open_realm(
            1,
            BlueJsPageOrigin::new("https://live-pressure.test").unwrap(),
        )
        .unwrap();
    runtime
        .configure_realm_bindings(1, |host| host.install_host_timers())
        .unwrap();
    page_script(&mut runtime, "var ticks=0,order=[],release;function churn(){for(var i=0;i<32;i++){Promise.resolve('x'.repeat(400000));}}var pending=new Promise(function(done){release=done;});pending.then(function(value){churn();order.push(value.tag);});async function wait(){var value={tag:'await'};await pending;churn();order.push(value.tag);}var invocation=wait();setTimeout(function(value){churn();order.push(value.tag);},100,{tag:'timer'});var timer=setInterval(function(){var value={tag:'weak'},reference=new WeakRef(value);value=null;churn();if(reference.deref().tag!=='weak'){throw 'lost current-job target';}ticks++;if(ticks===4){clearInterval(timer);release({tag:'reaction'});}},10);");
    let before = runtime.realm_stats(1).unwrap().heap.major_collections;
    for tick in 1..=4 {
        assert_eq!(runtime.run_due_timers(1, tick * 10).unwrap().tasks_run, 1);
    }
    assert_eq!(
        page_script(&mut runtime, "order.join('|');"),
        Value::String("reaction|await".into())
    );
    assert_eq!(runtime.run_due_timers(1, 100).unwrap().tasks_run, 1);
    assert_eq!(
        page_script(&mut runtime, "order.join('|');"),
        Value::String("reaction|await|timer".into())
    );
    assert!(runtime.realm_stats(1).unwrap().heap.major_collections > before);
    let usage = runtime.host_scheduling_usage(1).unwrap();
    assert_eq!(usage.pending_microtasks, 0);
    assert_eq!(usage.pending_reactions, 0);
}

#[test]
fn large_native_results_collect_previous_garbage_before_promise_settlement() {
    for finish in [
        "Promise.resolve('y'.repeat(3700000));",
        "async function answer(){return 'y'.repeat(3700000);}answer();",
    ] {
        let mut config = BlueJsPageRuntimeConfig::default();
        config.vm.max_string_bytes = blueice_bluejs::MAX_HOST_SCHEDULING_BYTES;
        config.vm.heap.nursery_capacity = 4096;
        config.vm.heap.major_threshold_bytes = 512 * 1024;
        let mut runtime = BlueJsPageRuntime::new(config).unwrap();
        runtime
            .open_realm(
                1,
                BlueJsPageOrigin::new("https://large-result.test").unwrap(),
            )
            .unwrap();
        runtime
            .configure_realm_bindings(1, |host| host.install_host_timers())
            .unwrap();
        page_script(&mut runtime, &format!("function poll(){{void Promise.resolve('x'.repeat(700000));{finish}}}setTimeout(poll,10);"));
        let result = runtime.run_due_timers(1, 10);
        assert!(result.is_ok(), "{finish}: {result:?}");
        assert!(
            runtime.host_scheduling_usage(1).unwrap().payload_bytes
                < blueice_bluejs::MAX_HOST_SCHEDULING_BYTES
        );
    }
}

#[test]
fn unchanged_live_promise_payload_does_not_force_gc_at_every_call() {
    let mut config = BlueJsPageRuntimeConfig::default();
    config.vm.max_string_bytes = blueice_bluejs::MAX_HOST_SCHEDULING_BYTES;
    config.vm.heap.nursery_capacity = 4096;
    config.vm.heap.major_threshold_bytes = 512 * 1024;
    let mut runtime = BlueJsPageRuntime::new(config).unwrap();
    let origin = BlueJsPageOrigin::new("https://live-watermark.test").unwrap();
    runtime.open_realm(1, origin.clone()).unwrap();
    runtime
        .configure_realm_bindings(1, |host| host.install_host_timers())
        .unwrap();
    page_script(
        &mut runtime,
        "var held=Promise.resolve('x'.repeat(2000000));function read(){return 42;}",
    );
    let retained = runtime.host_scheduling_usage(1).unwrap().payload_bytes;
    assert!(retained >= 4000000);
    let before = runtime.realm_stats(1).unwrap().heap.major_collections;
    page_script(&mut runtime, "read();42;");
    let after_control = runtime.realm_stats(1).unwrap().heap.major_collections;
    let entry_collections = after_control - before;
    assert_eq!(
        page_script(&mut runtime, "for(var i=0;i<1000;i++){read();}42;"),
        Value::Number(42.0)
    );
    assert!(
        runtime.realm_stats(1).unwrap().heap.major_collections - after_control <= entry_collections,
        "one thousand calls must not collect more than the one-call control"
    );
    page_script(
        &mut runtime,
        "held=null;for(var i=0;i<32;i++){Promise.resolve('x'.repeat(400000));}0;",
    );
    assert!(runtime.realm_stats(1).unwrap().heap.major_collections > before);
    assert!(runtime.host_scheduling_usage(1).unwrap().payload_bytes < retained);
    runtime.navigate(1, origin).unwrap();
    assert_eq!(runtime.host_scheduling_usage(1).unwrap().payload_bytes, 0);
}

#[test]
fn awaiting_stream_cancellation_preserves_its_return_promise_across_major_collection() {
    let mut config = VmConfig::default();
    config.heap.nursery_capacity = 1;
    config.heap.major_threshold_bytes = 1;
    let mut vm = Vm::new(config).unwrap();
    vm.install_web_file_api().unwrap();
    let source = "var called=0, finish, left=false, failure; var stream = new ReadableStream({start(c){c.enqueue(7);}, cancel(){called++;return new Promise(resolve=>finish=resolve);}}); (async function(){for await(var value of stream){break;} left=true;})().catch(error=>failure=error);";
    vm.execute_script(&compile(&parse(source).unwrap()).unwrap())
        .unwrap();
    vm.run_browser_tasks_bounded(256).unwrap();
    let state = vm.execute_script(&compile(&parse("[called,stream.locked,left,failure && failure.name,failure && failure.message].join('|');").unwrap()).unwrap()).unwrap();
    assert_eq!(state, Value::String("1|false|false||".into()));
    vm.execute_script(&compile(&parse("finish();true;").unwrap()).unwrap())
        .unwrap();
    vm.run_browser_tasks_bounded(256).unwrap();
    assert_eq!(
        vm.execute_script(&compile(&parse("left && failure === undefined;").unwrap()).unwrap())
            .unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn a_long_lived_page_timer_can_poll_promises_with_a_bounded_heap_and_scheduling_ledger() {
    let mut config = BlueJsPageRuntimeConfig::default();
    config.vm.heap.max_heap_bytes = 1024 * 1024;
    config.vm.heap.major_threshold_bytes = 16 * 1024;
    config.vm.heap.nursery_capacity = 1;
    let mut runtime = BlueJsPageRuntime::new(config).unwrap();
    let origin = BlueJsPageOrigin::new("https://promise-gc.test").unwrap();
    runtime.open_realm(1, origin.clone()).unwrap();
    runtime
        .configure_realm_bindings(1, |host| host.install_host_timers())
        .unwrap();
    let script = parse("var ticks = 0, observed = 0; function publish(value) { observed = value.tick; } function poll() { ticks += 1; Promise.resolve({tick: ticks}).then(publish); } setInterval(poll, 10);").unwrap();
    let program = runtime
        .install_program(
            1,
            &origin,
            BlueJsSourceIdentity::new("page:///poll.js", "sha256:poll").unwrap(),
            &BlueJsProgramV1::Script(script),
        )
        .unwrap();
    runtime.execute_program(1, program).unwrap();
    runtime.run_microtask_checkpoint(1).unwrap();
    let observe = runtime
        .install_program(
            1,
            &origin,
            BlueJsSourceIdentity::new("page:///observe.js", "sha256:observe").unwrap(),
            &BlueJsProgramV1::Script(parse("observed === ticks;").unwrap()),
        )
        .unwrap();
    let mut first_heap = 0;
    let mut first_scheduling = 0;
    for tick in 1..=4000 {
        assert_eq!(
            runtime
                .run_due_timers(1, tick * 10)
                .unwrap_or_else(|error| panic!("poll {tick}: {error:?}"))
                .tasks_run,
            1
        );
        if tick == 200 {
            assert_eq!(
                runtime.execute_program(1, observe).unwrap(),
                Value::Bool(true)
            );
            let stats = runtime.realm_stats(1).unwrap();
            first_heap = stats.heap.managed_bytes;
            first_scheduling = stats.scheduling.payload_bytes;
        }
    }
    assert_eq!(
        runtime.execute_program(1, observe).unwrap(),
        Value::Bool(true)
    );
    let stats = runtime.realm_stats(1).unwrap();
    assert!(stats.heap.major_collections > 10);
    assert!(stats.heap.managed_bytes <= first_heap + 32 * 1024);
    assert!(stats.scheduling.payload_bytes <= first_scheduling + 16 * 1024);
    assert_eq!(stats.scheduling.pending_microtasks, 0);
    assert_eq!(stats.scheduling.pending_reactions, 0);
    let verify = runtime
        .install_program(
            1,
            &origin,
            BlueJsSourceIdentity::new("page:///verify.js", "sha256:verify").unwrap(),
            &BlueJsProgramV1::Script(parse("ticks === 4000 && observed === ticks;").unwrap()),
        )
        .unwrap();
    assert_eq!(
        runtime.execute_program(1, verify).unwrap(),
        Value::Bool(true)
    );
}
