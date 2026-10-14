// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_bluejs::{
    compile, parse, BlueJsPageOrigin, BlueJsPageRuntime, BlueJsPageRuntimeConfig, BlueJsProgramV1,
    BlueJsSourceIdentity, RuntimeError, Value, Vm, VmConfig,
};

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
