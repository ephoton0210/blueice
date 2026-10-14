// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Page-realm timers and `queueMicrotask`, driven only through the public
//! page-runtime interface: the host supplies the clock, the VM owns every
//! callback.

use blueice_bluejs::{
    compile_module, parse, parse_module, BlueJsPageOrigin, BlueJsPageRuntime,
    BlueJsPageRuntimeError, BlueJsProgramV1, BlueJsSourceIdentity, BlueJsTimerRun, RuntimeError,
    Value, Vm, MAX_HOST_TIMERS_PER_REALM,
};

const TAB: u64 = 1;

fn origin() -> BlueJsPageOrigin {
    BlueJsPageOrigin::new("https://example.test").unwrap()
}

struct Page {
    runtime: BlueJsPageRuntime,
    scripts: u32,
}

impl Page {
    fn new() -> Self {
        let mut runtime = BlueJsPageRuntime::default();
        runtime.open_realm(TAB, origin()).unwrap();
        runtime
            .configure_realm_bindings(TAB, |registrar| registrar.install_host_timers())
            .unwrap();
        let mut page = Self {
            runtime,
            scripts: 0,
        };
        page.run("var log = [];").unwrap();
        page
    }

    fn run(&mut self, source: &str) -> Result<Value, BlueJsPageRuntimeError> {
        let result = self.execute(source);
        // HTML script cleanup also checkpoints a script that threw.
        let checkpoint = self.runtime.run_click_microtask_checkpoint(TAB);
        let value = result?;
        checkpoint?;
        Ok(value)
    }

    fn execute(&mut self, source: &str) -> Result<Value, BlueJsPageRuntimeError> {
        self.scripts += 1;
        let identity = BlueJsSourceIdentity::new(
            format!("page:///script-{}.js", self.scripts),
            format!("sha256:script-{}", self.scripts),
        )
        .unwrap();
        let program = BlueJsProgramV1::Script(parse(source).unwrap());
        let handle = self
            .runtime
            .install_program(TAB, &origin(), identity, &program)?;
        self.runtime.execute_program(TAB, handle)
    }

    fn log(&mut self) -> String {
        match self.run("log.join(',')").unwrap() {
            Value::String(text) => text.to_utf8().unwrap(),
            other => panic!("expected the joined log, got {other:?}"),
        }
    }

    fn tick(&mut self, now_ms: u64) -> BlueJsTimerRun {
        self.runtime.run_due_timers(TAB, now_ms).unwrap()
    }
}

#[test]
fn a_timer_runs_once_when_due_and_receives_its_extra_arguments() {
    let mut page = Page::new();
    page.run("setTimeout(function (a, b) { log.push('t' + a + b); }, 10, 1, 2);")
        .unwrap();
    assert_eq!(page.log(), "");
    assert_eq!(page.tick(9).tasks_run, 0);
    assert_eq!(page.log(), "");
    assert_eq!(page.tick(10).tasks_run, 1);
    assert_eq!(page.log(), "t12");
    assert_eq!(page.tick(1_000).tasks_run, 0);
    assert_eq!(page.log(), "t12");
}

#[test]
fn due_timers_run_in_due_time_then_creation_order() {
    let mut page = Page::new();
    page.run(
        "setTimeout(function () { log.push('late'); }, 20);
         setTimeout(function () { log.push('a'); }, 5);
         setTimeout(function () { log.push('b'); }, 5);
         setTimeout(function () { log.push('first'); }, 0);",
    )
    .unwrap();
    assert_eq!(page.tick(100).tasks_run, 4);
    assert_eq!(page.log(), "first,a,b,late");
}

#[test]
fn timer_ids_are_distinct_positive_integers_and_clearing_cancels() {
    let mut page = Page::new();
    let value = page
        .run(
            "var one = setTimeout(function () { log.push('one'); }, 1);
             var two = setTimeout(function () { log.push('two'); }, 1);
             clearTimeout(one);
             clearTimeout(99999);
             clearTimeout(undefined);
             clearTimeout('not a timer');
             one > 0 && two > 0 && one !== two && Number.isInteger(one);",
        )
        .unwrap();
    assert_eq!(value, Value::Bool(true));
    assert_eq!(page.tick(5).tasks_run, 1);
    assert_eq!(page.log(), "two");
}

#[test]
fn set_timeout_and_set_interval_share_one_id_space_for_clearing() {
    let mut page = Page::new();
    page.run(
        "var id = setInterval(function () { log.push('i'); }, 1);
         clearTimeout(id);
         var other = setTimeout(function () { log.push('t'); }, 1);
         clearInterval(other);",
    )
    .unwrap();
    assert_eq!(page.tick(10).tasks_run, 0);
    assert_eq!(page.log(), "");
}

#[test]
fn an_interval_repeats_each_tick_until_cleared_from_its_own_callback() {
    let mut page = Page::new();
    page.run(
        "var count = 0;
         var id = setInterval(function () {
             count += 1;
             log.push('i' + count);
             if (count === 3) { clearInterval(id); }
         }, 10);",
    )
    .unwrap();
    assert_eq!(page.tick(10).tasks_run, 1);
    assert_eq!(page.tick(15).tasks_run, 0);
    assert_eq!(page.tick(20).tasks_run, 1);
    assert_eq!(page.tick(30).tasks_run, 1);
    assert_eq!(page.tick(40).tasks_run, 0);
    assert_eq!(page.log(), "i1,i2,i3");
    assert_eq!(page.runtime.next_timer_due(TAB).unwrap(), None);
}

#[test]
fn a_late_tick_runs_an_interval_once_not_once_per_missed_period() {
    let mut page = Page::new();
    page.run("setInterval(function () { log.push('i'); }, 10);")
        .unwrap();
    assert_eq!(page.tick(1_000).tasks_run, 1);
    assert_eq!(page.log(), "i");
    assert_eq!(page.runtime.next_timer_due(TAB).unwrap(), Some(1_010));
}

#[test]
fn a_timer_scheduled_by_a_timer_waits_for_a_later_tick() {
    let mut page = Page::new();
    page.run(
        "setTimeout(function () {
             log.push('outer');
             setTimeout(function () { log.push('inner'); }, 0);
         }, 0);",
    )
    .unwrap();
    assert_eq!(page.tick(0).tasks_run, 1);
    assert_eq!(page.log(), "outer");
    assert_eq!(page.tick(0).tasks_run, 1);
    assert_eq!(page.log(), "outer,inner");
}

#[test]
fn a_timer_scheduled_during_a_tick_is_relative_to_that_tick() {
    let mut page = Page::new();
    page.run("setTimeout(function () { setTimeout(function () { log.push('x'); }, 50); }, 100);")
        .unwrap();
    page.tick(100);
    assert_eq!(page.runtime.next_timer_due(TAB).unwrap(), Some(150));
}

#[test]
fn microtasks_drain_after_the_script_and_after_each_timer_task() {
    let mut page = Page::new();
    page.run(
        "Promise.resolve().then(function () { log.push('promise'); });
         queueMicrotask(function () { log.push('micro'); });
         log.push('script');",
    )
    .unwrap();
    assert_eq!(page.log(), "script,promise,micro");

    page.run(
        "setTimeout(function () {
             log.push('t1');
             Promise.resolve().then(function () { log.push('t1-then'); });
             queueMicrotask(function () { log.push('t1-micro'); });
         }, 1);
         setTimeout(function () { log.push('t2'); }, 1);",
    )
    .unwrap();
    assert_eq!(page.tick(1).tasks_run, 2);
    assert_eq!(page.log(), "script,promise,micro,t1,t1-then,t1-micro,t2");
}

#[test]
fn queue_microtask_calls_its_callback_without_arguments() {
    let mut page = Page::new();
    page.run("queueMicrotask(function () { log.push(arguments.length); });")
        .unwrap();
    assert_eq!(page.log(), "0");
}

#[test]
fn a_throwing_microtask_does_not_skip_later_jobs_at_the_host_checkpoint() {
    let mut page = Page::new();
    page.run(
        "queueMicrotask(function () { throw new Error('job'); });
         Promise.resolve().then(function () { log.push('promise'); });
         queueMicrotask(function () { log.push('after'); });",
    )
    .unwrap();
    assert_eq!(page.log(), "promise,after");
}

#[test]
fn a_throwing_script_still_reaches_its_host_checkpoint() {
    let mut page = Page::new();
    assert!(matches!(
        page.run("queueMicrotask(function () { log.push('cleanup'); }); throw 42;"),
        Err(BlueJsPageRuntimeError::Runtime(RuntimeError::Thrown(_)))
    ));
    assert_eq!(
        page.execute("log.join(',')").unwrap(),
        Value::String("cleanup".into())
    );
}

#[test]
fn self_rearming_microtasks_cannot_hold_a_timer_pass_forever() {
    let mut page = Page::new();
    page.run(
        "setTimeout(function () {
             queueMicrotask(function again() { queueMicrotask(again); });
         }, 1);",
    )
    .unwrap();
    assert_eq!(
        page.runtime.run_due_timers(TAB, 1),
        Err(BlueJsPageRuntimeError::Runtime(
            RuntimeError::InstructionLimit
        ))
    );
    // Navigation discards the runaway queue and the new realm remains usable.
    page.runtime.navigate(TAB, origin()).unwrap();
    assert_eq!(page.run("1 + 2").unwrap(), Value::Number(3.0));
}

#[test]
fn the_script_checkpoint_also_bounds_self_rearming_jobs() {
    let mut page = Page::new();
    assert_eq!(
        page.run("queueMicrotask(function again() { queueMicrotask(again); });"),
        Err(BlueJsPageRuntimeError::Runtime(
            RuntimeError::InstructionLimit
        ))
    );
}

#[test]
fn a_throwing_timer_microtask_is_counted_before_the_next_timer() {
    let mut page = Page::new();
    page.run(
        "setTimeout(function () {
             queueMicrotask(function () { throw 42; });
             queueMicrotask(function () { log.push('cleanup'); });
         }, 1);
         setTimeout(function () { log.push('next'); }, 1);",
    )
    .unwrap();
    assert_eq!(
        page.tick(1),
        BlueJsTimerRun {
            tasks_run: 2,
            uncaught_errors: 1
        }
    );
    assert_eq!(page.log(), "cleanup,next");
}

#[test]
fn the_browser_turn_checkpoints_past_a_throwing_microtask() {
    let mut page = Page::new();
    page.execute(
        "queueMicrotask(function () { throw 42; });
         queueMicrotask(function () { log.push('after'); });",
    )
    .unwrap();
    page.runtime.flush_drag_tasks(TAB).unwrap();
    assert_eq!(
        page.execute("log.join(',')").unwrap(),
        Value::String("after".into())
    );
}

#[test]
fn the_public_script_checkpoint_reports_job_exceptions_and_preserves_raw_execution() {
    let mut page = Page::new();
    page.execute(
        "queueMicrotask(function () { throw 42; });
         queueMicrotask(function () { log.push('after'); });",
    )
    .unwrap();
    // Raw execution deliberately leaves script cleanup to its host.
    assert_eq!(page.execute("log.length").unwrap(), Value::Number(0.0));
    assert_eq!(page.runtime.run_microtask_checkpoint(TAB).unwrap(), 1);
    assert_eq!(
        page.execute("log.join(',')").unwrap(),
        Value::String("after".into())
    );
    assert_eq!(page.runtime.run_microtask_checkpoint(TAB).unwrap(), 0);
    assert_eq!(
        page.runtime.run_microtask_checkpoint(9),
        Err(BlueJsPageRuntimeError::UnknownRealm(9))
    );
}

#[test]
fn top_level_await_cannot_bypass_the_page_job_budget() {
    let mut page = Page::new();
    // A finite queue makes the regression fail promptly even on the old,
    // unbounded module drain. An endlessly rearming job would hang that VM.
    let source = "for (let i = 0; i < 1100; i++) { Promise.resolve().then(() => {}); } await 0;";
    let module = parse_module(source).unwrap();
    let handle = page
        .runtime
        .install_program(
            TAB,
            &origin(),
            BlueJsSourceIdentity::new("page:///await.js", "sha256:await").unwrap(),
            &BlueJsProgramV1::Module(module.clone()),
        )
        .unwrap();
    assert_eq!(
        page.runtime.execute_program(TAB, handle),
        Err(BlueJsPageRuntimeError::Runtime(
            RuntimeError::InstructionLimit
        ))
    );

    // The standalone ECMAScript/Test262 VM retains its explicit job-drain API.
    let mut vm = Vm::default();
    vm.execute_module_graph(
        "entry",
        &std::collections::HashMap::from([("entry".to_string(), compile_module(&module).unwrap())]),
    )
    .unwrap();
    vm.run_promise_jobs().unwrap();
}

#[test]
fn a_single_page_module_resumes_top_level_await_before_host_cleanup() {
    let mut page = Page::new();
    let handle = page
        .runtime
        .install_program(
            TAB,
            &origin(),
            BlueJsSourceIdentity::new("page:///answer.js", "sha256:answer").unwrap(),
            &BlueJsProgramV1::Module(
                parse_module("globalThis.answer = await Promise.resolve(42);").unwrap(),
            ),
        )
        .unwrap();
    page.runtime.execute_program(TAB, handle).unwrap();
    page.runtime.run_microtask_checkpoint(TAB).unwrap();
    assert_eq!(page.execute("answer").unwrap(), Value::Number(42.0));
}

#[test]
fn an_async_module_graph_cannot_bypass_the_page_job_budget() {
    let mut page = Page::new();
    let handle = page
        .runtime
        .install_program(
            TAB,
            &origin(),
            BlueJsSourceIdentity::new("page:///graph.js", "sha256:graph").unwrap(),
            &BlueJsProgramV1::Module(
                parse_module(
                    "for (let i = 0; i < 1100; i++) { Promise.resolve().then(() => {}); } await 0;",
                )
                .unwrap(),
            ),
        )
        .unwrap();
    assert_eq!(
        page.runtime.execute_module_graph(TAB, handle, [handle]),
        Err(BlueJsPageRuntimeError::Runtime(
            RuntimeError::InstructionLimit
        ))
    );
}

#[test]
fn a_page_shadow_realm_inherits_bounded_job_draining() {
    let mut page = Page::new();
    assert_eq!(
        page.run(
            "var limited = false;
             try {
                 new ShadowRealm().evaluate('for (var i = 0; i < 1100; i++) { Promise.resolve().then(() => {}); } 0;');
             } catch (error) { limited = error instanceof TypeError; }
             limited;",
        ).unwrap(),
        // ShadowRealm intentionally masks the child's exception as TypeError.
        Value::Bool(true)
    );
}

#[test]
fn a_throwing_timer_is_counted_and_does_not_stop_later_timers() {
    let mut page = Page::new();
    page.run(
        "setTimeout(function () { throw new Error('boom'); }, 1);
         setTimeout(function () { log.push('after'); }, 1);
         setTimeout(function () { null.x; }, 1);",
    )
    .unwrap();
    let run = page.tick(1);
    assert_eq!(run.tasks_run, 3);
    assert_eq!(run.uncaught_errors, 2);
    assert_eq!(page.log(), "after");
}

#[test]
fn invalid_arguments_throw_type_errors_and_schedule_nothing() {
    let mut page = Page::new();
    for call in [
        "setTimeout()",
        "setTimeout('log.push(1)', 0)",
        "setInterval({}, 0)",
        "queueMicrotask(1)",
        "queueMicrotask()",
    ] {
        assert!(
            matches!(
                page.run(call),
                Err(BlueJsPageRuntimeError::Runtime(RuntimeError::TypeError(_)))
            ),
            "{call} must throw a TypeError"
        );
    }
    assert_eq!(page.runtime.next_timer_due(TAB).unwrap(), None);
}

#[test]
fn delays_are_coerced_like_the_html_standard() {
    let mut page = Page::new();
    page.run(
        "setTimeout(function () { log.push('neg'); }, -5);
         setTimeout(function () { log.push('nan'); }, NaN);
         setTimeout(function () { log.push('str'); }, '7');
         setTimeout(function () { log.push('frac'); }, 2.9);",
    )
    .unwrap();
    assert_eq!(page.tick(0).tasks_run, 2);
    assert_eq!(page.log(), "neg,nan");
    assert_eq!(page.tick(2).tasks_run, 1);
    assert_eq!(page.tick(7).tasks_run, 1);
    assert_eq!(page.log(), "neg,nan,frac,str");
}

#[test]
fn the_pending_timer_count_is_bounded_per_realm() {
    let mut page = Page::new();
    let source = format!(
        "for (var i = 0; i < {}; i += 1) {{ setTimeout(function () {{}}, 1000); }}",
        MAX_HOST_TIMERS_PER_REALM
    );
    page.run(&source).unwrap();
    assert!(matches!(
        page.run("setTimeout(function () {}, 1000);"),
        Err(BlueJsPageRuntimeError::Runtime(RuntimeError::RangeError(_)))
    ));
    // Clearing frees capacity.
    page.run("clearTimeout(1);").unwrap();
    page.run("setTimeout(function () {}, 1000);").unwrap();
}

#[test]
fn timer_callbacks_stay_alive_across_collections() {
    let mut page = Page::new();
    page.run(
        "setTimeout((function () {
             var held = { value: 'kept' };
             return function () { log.push(held.value); };
         })(), 1);
         for (var i = 0; i < 2000; i += 1) { ({ garbage: [i, i + 1] }); }",
    )
    .unwrap();
    assert_eq!(page.tick(1).tasks_run, 1);
    assert_eq!(page.log(), "kept");
}

#[test]
fn navigation_drops_every_pending_timer() {
    let mut page = Page::new();
    page.run("setTimeout(function () { log.push('stale'); }, 1);")
        .unwrap();
    assert_eq!(page.runtime.next_timer_due(TAB).unwrap(), Some(1));
    page.runtime.navigate(TAB, origin()).unwrap();
    assert_eq!(page.runtime.next_timer_due(TAB).unwrap(), None);
    assert_eq!(
        page.runtime.run_due_timers(TAB, 1_000).unwrap().tasks_run,
        0
    );
}

#[test]
fn an_unknown_realm_is_rejected() {
    let mut runtime = BlueJsPageRuntime::default();
    assert_eq!(
        runtime.run_due_timers(9, 0),
        Err(BlueJsPageRuntimeError::UnknownRealm(9))
    );
    assert_eq!(
        runtime.next_timer_due(9),
        Err(BlueJsPageRuntimeError::UnknownRealm(9))
    );
}

#[test]
fn timers_are_absent_until_the_host_installs_them() {
    let mut runtime = BlueJsPageRuntime::default();
    runtime.open_realm(TAB, origin()).unwrap();
    let identity = BlueJsSourceIdentity::new("page:///bare.js", "sha256:bare").unwrap();
    let program =
        BlueJsProgramV1::Script(parse("typeof setTimeout + typeof queueMicrotask").unwrap());
    let handle = runtime
        .install_program(TAB, &origin(), identity, &program)
        .unwrap();
    assert_eq!(
        runtime.execute_program(TAB, handle).unwrap(),
        Value::String("undefinedundefined".into())
    );
}
