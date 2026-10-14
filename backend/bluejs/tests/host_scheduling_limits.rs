// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_bluejs::{
    compile, parse, BlueJsPageOrigin, BlueJsPageRuntime, BlueJsPageRuntimeError, BlueJsProgramV1,
    BlueJsSourceIdentity, RuntimeError, Value, Vm,
};

fn page() -> BlueJsPageRuntime {
    let mut runtime = BlueJsPageRuntime::default();
    runtime
        .open_realm(1, BlueJsPageOrigin::new("https://queue.test").unwrap())
        .unwrap();
    runtime
        .configure_realm_bindings(1, |host| host.install_host_timers())
        .unwrap();
    runtime
}

fn execute(
    runtime: &mut BlueJsPageRuntime,
    source: &str,
) -> Result<blueice_bluejs::Value, blueice_bluejs::BlueJsPageRuntimeError> {
    let serial = runtime.realm_stats(1).unwrap().program_count;
    let handle = runtime
        .install_program(
            1,
            &BlueJsPageOrigin::new("https://queue.test").unwrap(),
            BlueJsSourceIdentity::new(
                format!("page:///queue-{serial}.js"),
                format!("sha256:queue-{serial}"),
            )
            .unwrap(),
            &BlueJsProgramV1::Script(parse(source).unwrap()),
        )
        .unwrap();
    runtime.execute_program(1, handle)
}

#[test]
fn all_page_job_producers_have_a_pending_work_bound_before_the_checkpoint() {
    let mut unbounded = Vec::new();
    for (name, source) in [
        ("queueMicrotask", "function noop() {} for (var i = 0; i < 1025; i += 1) { queueMicrotask(noop); }"),
        ("fulfilled reactions", "function noop() {} var promise = Promise.resolve(); for (var i = 0; i < 1025; i += 1) { promise.then(noop); }"),
        ("pending reactions", "function noop() {} var promise = new Promise(noop); for (var i = 0; i < 1025; i += 1) { promise.then(noop); }"),
        ("thenable jobs", "function then(resolve) {} var value = {then: then}; for (var i = 0; i < 1025; i += 1) { Promise.resolve(value); }"),
        ("async await jobs", "async function wait() { await 0; } for (var i = 0; i < 1025; i += 1) { wait(); }"),
        ("async generator jobs", "async function* items() { yield 0; } for (var i = 0; i < 1025; i += 1) { items().next(); }"),
        ("async generator requests", "function noop() {} var pending = new Promise(noop); async function* items() { await pending; yield 0; } var iterator = items(); for (var i = 0; i < 1025; i += 1) { iterator.next(); }"),
        ("pending async continuations", "function noop() {} var pending = new Promise(noop); async function wait() { await pending; } for (var i = 0; i < 1025; i += 1) { wait(); }"),
    ] {
        if !matches!(execute(&mut page(), source), Err(BlueJsPageRuntimeError::Runtime(RuntimeError::SchedulingLimit))) { unbounded.push(name); }
    }
    assert!(
        unbounded.is_empty(),
        "producers accepted excess pending work before a manual checkpoint: {unbounded:?}"
    );
}

#[test]
fn timer_and_reaction_owned_strings_cannot_escape_the_page_scheduling_byte_budget() {
    for source in [
        "function noop() {} var payload = 'x'.repeat(400000); for (var i = 0; i < 16; i += 1) { setTimeout(noop, 1000, payload); }",
        "function noop() {} var payload = 'x'.repeat(400000); for (var i = 0; i < 16; i += 1) { Promise.resolve(payload).then(noop); }",
    ] {
        assert!(matches!(execute(&mut page(), source), Err(BlueJsPageRuntimeError::Runtime(RuntimeError::SchedulingLimit))), "owned scheduling strings must be bounded independently of managed heap objects");
    }
}

#[test]
fn page_scheduling_counts_are_visible_and_checkpoint_releases_pending_jobs() {
    let mut runtime = page();
    execute(
        &mut runtime,
        "function noop() {} for (var i = 0; i < 128; i += 1) { queueMicrotask(noop); }",
    )
    .unwrap();
    let before = runtime.host_scheduling_usage(1).unwrap();
    assert_eq!(runtime.realm_stats(1).unwrap().scheduling, before);
    assert_eq!(before.pending_microtasks, 128);
    assert!(before.payload_bytes > 0);
    runtime.run_microtask_checkpoint(1).unwrap();
    let after = runtime.host_scheduling_usage(1).unwrap();
    assert_eq!(runtime.realm_stats(1).unwrap().scheduling, after);
    assert_eq!(after.pending_microtasks, 0);
    assert!(after.payload_bytes < before.payload_bytes);
}

#[test]
fn the_exact_pending_job_and_reaction_limits_are_admitted() {
    let mut runtime = page();
    execute(
        &mut runtime,
        "function noop() {} for (var i = 0; i < 1024; i += 1) { queueMicrotask(noop); }",
    )
    .unwrap();
    assert_eq!(
        runtime.host_scheduling_usage(1).unwrap().pending_microtasks,
        1024
    );
    runtime.run_microtask_checkpoint(1).unwrap();
    assert_eq!(
        runtime.host_scheduling_usage(1).unwrap().pending_microtasks,
        0
    );
    execute(&mut runtime, "var pending = new Promise(noop); for (var i = 0; i < 1024; i += 1) { pending.then(noop); }").unwrap();
    assert_eq!(
        runtime.host_scheduling_usage(1).unwrap().pending_reactions,
        1024
    );
}

#[test]
fn pending_reactions_transfer_to_jobs_when_their_promise_settles() {
    let mut runtime = page();
    execute(&mut runtime, "var resolve; function executor(done) { resolve = done; } function noop() {} var promise = new Promise(executor); for (var i = 0; i < 128; i += 1) { promise.then(noop); }").unwrap();
    assert_eq!(
        runtime.host_scheduling_usage(1).unwrap().pending_reactions,
        128
    );
    execute(&mut runtime, "resolve(42);").unwrap();
    let queued = runtime.host_scheduling_usage(1).unwrap();
    assert_eq!(queued.pending_reactions, 0);
    assert_eq!(queued.pending_microtasks, 128);
    runtime.run_microtask_checkpoint(1).unwrap();
    assert_eq!(
        runtime.host_scheduling_usage(1).unwrap().pending_microtasks,
        0
    );
}

#[test]
fn shadow_realm_work_is_charged_to_the_initiating_document() {
    let mut runtime = page();
    execute(&mut runtime, "var child = new ShadowRealm(); child.evaluate('function noop() {} var promise = new Promise(noop); for (var i = 0; i < 600; i += 1) { promise.then(noop); } 0;');").unwrap();
    assert_eq!(
        runtime.host_scheduling_usage(1).unwrap().pending_reactions,
        600
    );
    assert_eq!(
        runtime.realm_stats(1).unwrap().scheduling.pending_reactions,
        600
    );
    assert!(matches!(execute(&mut runtime, "function noop() {} var promise = new Promise(noop); for (var i = 0; i < 600; i += 1) { promise.then(noop); }"), Err(BlueJsPageRuntimeError::Runtime(RuntimeError::SchedulingLimit))));
}

#[test]
fn scheduling_exhaustion_is_uncatchable_and_navigation_admits_a_fresh_document() {
    let mut runtime = page();
    assert!(matches!(execute(&mut runtime, "function noop() {} try { for (var i = 0; i < 1025; i += 1) { queueMicrotask(noop); } } catch (error) {} 42;"), Err(BlueJsPageRuntimeError::Runtime(RuntimeError::SchedulingLimit))));
    assert!(runtime.run_due_timers(1, 1).is_err());
    assert!(execute(&mut runtime, "42;").is_err());
    runtime
        .navigate(1, BlueJsPageOrigin::new("https://queue.test").unwrap())
        .unwrap();
    assert_eq!(
        runtime.host_scheduling_usage(1).unwrap().pending_microtasks,
        0
    );
    assert_eq!(execute(&mut runtime, "42;").unwrap(), Value::Number(42.0));
}

#[test]
fn standalone_language_vms_keep_their_unbounded_queue_policy() {
    let mut vm = Vm::default();
    vm.execute_script(&compile(&parse("function noop() {} var promise = Promise.resolve(); for (var i = 0; i < 1100; i += 1) { promise.then(noop); }").unwrap()).unwrap()).unwrap();
    vm.run_promise_jobs().unwrap();
}

#[test]
fn native_combinators_report_fanout_exhaustion_without_panicking_or_masking_it() {
    for method in ["all", "allSettled", "race", "any"] {
        let source = format!("function noop() {{}} var pending = new Promise(noop); Promise.{method}(Array(1025).fill(pending));");
        assert!(
            matches!(
                execute(&mut page(), &source),
                Err(BlueJsPageRuntimeError::Runtime(
                    RuntimeError::SchedulingLimit
                ))
            ),
            "Promise.{method}"
        );
    }
}

#[test]
fn suspended_async_arguments_are_charged_before_their_frames_are_published() {
    assert!(matches!(execute(&mut page(), "function noop() {} var pending = new Promise(noop); async function wait(payload) { await pending; return payload; } var payload = 'x'.repeat(400000); for (var i = 0; i < 16; i += 1) { wait(payload); }"), Err(BlueJsPageRuntimeError::Runtime(RuntimeError::SchedulingLimit))));
}

#[test]
fn firing_and_clearing_timers_release_owned_payload_for_later_admission() {
    let mut runtime = page();
    execute(&mut runtime, "function noop() {} var payload = 'x'.repeat(400000); var timer = setTimeout(noop, 1, payload); var cancelled = setTimeout(noop, 100000, payload);").unwrap();
    let before = runtime.host_scheduling_usage(1).unwrap().payload_bytes;
    execute(&mut runtime, "clearTimeout(cancelled);").unwrap();
    let cleared = runtime.host_scheduling_usage(1).unwrap().payload_bytes;
    assert!(cleared + 700000 < before);
    assert_eq!(runtime.run_due_timers(1, 1).unwrap().tasks_run, 1);
    assert_eq!(runtime.host_scheduling_usage(1).unwrap().payload_bytes, 0);
    execute(
        &mut runtime,
        "for (var i = 0; i < 10; i += 1) { setTimeout(noop, 100000, payload); }",
    )
    .unwrap();
}

#[test]
fn interval_argument_copies_and_running_oneshot_arguments_remain_in_the_budget() {
    for first in [
        "setInterval(noop, 1, payload);",
        "setTimeout(rearm, 1, payload);",
    ] {
        let mut runtime = page();
        execute(&mut runtime, &format!("function noop() {{}} function rearm(value) {{ setTimeout(noop, 100000, value); }} var payload = 'x'.repeat(400000); {first} for (var i = 0; i < 9; i += 1) {{ setTimeout(noop, 100000, payload); }}")).unwrap();
        assert!(
            matches!(
                runtime.run_due_timers(1, 1),
                Err(BlueJsPageRuntimeError::Runtime(
                    RuntimeError::SchedulingLimit
                ))
            ),
            "{first}"
        );
    }
}
