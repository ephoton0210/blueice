// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_bluejs::{
    compile, parse, BlueJsAnimationFrameRun, BlueJsPageOrigin, BlueJsPageRuntime,
    BlueJsPageRuntimeError, BlueJsProgramV1, BlueJsSourceIdentity, RuntimeError, Value, Vm,
    MAX_HOST_ANIMATION_FRAME_CALLBACKS,
};

const TAB: u64 = 1;
fn origin() -> BlueJsPageOrigin {
    BlueJsPageOrigin::new("https://frame.test").unwrap()
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
            .configure_realm_bindings(TAB, |host| {
                host.install_host_timers()?;
                host.install_host_animation_frames()
            })
            .unwrap();
        let mut page = Self {
            runtime,
            scripts: 0,
        };
        page.run("var log = [];").unwrap();
        page
    }
    fn run(&mut self, source: &str) -> Result<Value, BlueJsPageRuntimeError> {
        let outcome = self.execute(source);
        let checkpoint = self.runtime.run_microtask_checkpoint(TAB);
        let value = outcome?;
        checkpoint?;
        Ok(value)
    }
    fn execute(&mut self, source: &str) -> Result<Value, BlueJsPageRuntimeError> {
        self.scripts += 1;
        let handle = self.runtime.install_program(
            TAB,
            &origin(),
            BlueJsSourceIdentity::new(
                format!("page:///{}.js", self.scripts),
                format!("sha256:{}", self.scripts),
            )
            .unwrap(),
            &BlueJsProgramV1::Script(parse(source).unwrap()),
        )?;
        self.runtime.execute_program(TAB, handle)
    }
    fn frame(&mut self, timestamp: f64) -> BlueJsAnimationFrameRun {
        self.runtime.run_animation_frame(TAB, timestamp).unwrap()
    }
    fn log(&mut self) -> String {
        let Value::String(log) = self.run("log.join(',')").unwrap() else {
            panic!("string log");
        };
        log.to_utf8().unwrap()
    }
    fn pending(&self) -> bool {
        self.runtime.animation_frame_pending(TAB).unwrap()
    }
}

#[test]
fn frames_are_absent_from_unconfigured_vms_and_installed_functions_are_not_constructors() {
    let mut vm = Vm::default();
    assert_eq!(vm.execute_script(&compile(&parse("typeof requestAnimationFrame === 'undefined' && typeof cancelAnimationFrame === 'undefined'").unwrap()).unwrap()).unwrap(), Value::Bool(true));
    let mut page = Page::new();
    assert_eq!(
        page.run("requestAnimationFrame.length === 1 && cancelAnimationFrame.length === 1")
            .unwrap(),
        Value::Bool(true)
    );
    for source in [
        "new requestAnimationFrame(function () {})",
        "new cancelAnimationFrame(1)",
        "requestAnimationFrame(7)",
        "requestAnimationFrame()",
        "cancelAnimationFrame()",
    ] {
        assert!(
            matches!(
                page.run(source),
                Err(BlueJsPageRuntimeError::Runtime(RuntimeError::TypeError(_)))
            ),
            "{source}"
        );
    }
    assert!(!page.pending());
}

#[test]
fn one_render_opportunity_runs_a_snapshot_in_order_with_one_shared_fractional_timestamp() {
    let mut page = Page::new();
    page.run("requestAnimationFrame(function (time) { 'use strict'; log.push('a:' + time + ':' + (this === undefined)); });
        requestAnimationFrame(function (time) { log.push('b:' + time); });").unwrap();
    assert!(page.pending());
    assert_eq!(page.frame(12.25).callbacks_run, 2);
    assert_eq!(page.log(), "a:12.25:true,b:12.25");
    assert!(!page.pending());
    assert_eq!(page.frame(25.5).callbacks_run, 0);
}

#[test]
fn browser_and_timer_turns_do_not_run_frame_callbacks() {
    let mut page = Page::new();
    page.run("requestAnimationFrame(function () { log.push('frame'); }); setTimeout(function () { log.push('timer'); }, 0);").unwrap();
    page.runtime.flush_drag_tasks(TAB).unwrap();
    page.runtime.run_due_timers(TAB, 100).unwrap();
    assert_eq!(page.log(), "timer");
    assert!(page.pending());
    page.frame(101.0);
    assert_eq!(page.log(), "timer,frame");
}

#[test]
fn callbacks_registered_from_callbacks_and_microtasks_wait_for_the_next_frame() {
    let mut page = Page::new();
    page.run(
        "requestAnimationFrame(function () {
        log.push('first'); requestAnimationFrame(function () { log.push('nested'); });
        queueMicrotask(function () { requestAnimationFrame(function () { log.push('job'); }); });
    });",
    )
    .unwrap();
    assert_eq!(page.frame(1.0).callbacks_run, 1);
    assert_eq!(page.log(), "first");
    assert!(page.pending());
    assert_eq!(page.frame(2.0).callbacks_run, 2);
    assert_eq!(page.log(), "first,nested,job");
}

#[test]
fn callback_microtasks_can_cancel_later_callbacks_in_the_same_snapshot() {
    let mut page = Page::new();
    page.run("var later; requestAnimationFrame(function () {
        log.push('first'); queueMicrotask(function () { log.push('job'); cancelAnimationFrame(later); });
    }); later = requestAnimationFrame(function () { log.push('cancelled'); });").unwrap();
    assert_eq!(page.frame(1.0).callbacks_run, 1);
    assert_eq!(page.log(), "first,job");
    assert!(!page.pending());
}

#[test]
fn callback_and_job_exceptions_are_counted_without_skipping_later_callbacks() {
    let mut page = Page::new();
    page.run(
        "requestAnimationFrame(function () {
        log.push('throw'); queueMicrotask(function () { throw 'job'; }); throw 'callback';
    }); requestAnimationFrame(function () { log.push('after'); });",
    )
    .unwrap();
    assert_eq!(
        page.frame(1.0),
        BlueJsAnimationFrameRun {
            callbacks_run: 2,
            uncaught_errors: 2
        }
    );
    assert_eq!(page.log(), "throw,after");
}

#[test]
fn cancellation_uses_unsigned_long_conversion_and_does_not_cancel_timers() {
    let mut page = Page::new();
    assert_eq!(
        page.run(
            "var frame = requestAnimationFrame(function () { log.push('cancelled'); });
        var timer = setTimeout(function () { log.push('timer'); }, 0);
        cancelAnimationFrame(frame + 4294967296.9);
        Number.isInteger(frame) && frame > 0;"
        )
        .unwrap(),
        Value::Bool(true)
    );
    assert_eq!(page.frame(1.0).callbacks_run, 0);
    assert_eq!(page.runtime.run_due_timers(TAB, 1).unwrap().tasks_run, 1);
    assert_eq!(page.log(), "timer");
}

#[test]
fn frame_callback_capacity_is_bounded_and_cancellation_releases_a_slot() {
    let mut page = Page::new();
    page.run(&format!("for (var i = 0; i < {MAX_HOST_ANIMATION_FRAME_CALLBACKS}; i += 1) {{ requestAnimationFrame(function () {{}}); }}")).unwrap();
    assert!(matches!(
        page.run("requestAnimationFrame(function () {});"),
        Err(BlueJsPageRuntimeError::Runtime(RuntimeError::RangeError(_)))
    ));
    page.run("cancelAnimationFrame(1); requestAnimationFrame(function () {});")
        .unwrap();
    assert_eq!(
        page.frame(1.0).callbacks_run as usize,
        MAX_HOST_ANIMATION_FRAME_CALLBACKS
    );
    assert!(!page.pending());
}

#[test]
fn frame_callback_closures_survive_collection_while_waiting_to_render() {
    let mut page = Page::new();
    let before = page.runtime.realm_stats(TAB).unwrap().heap;
    page.run(
        "requestAnimationFrame((function () { var held = { value: 'kept' };
        return function () { log.push(held.value); }; })());
        for (var i = 0; i < 2000; i += 1) { ({ garbage: [i, i + 1] }); }",
    )
    .unwrap();
    let after = page.runtime.realm_stats(TAB).unwrap().heap;
    assert!(
        after.minor_collections > before.minor_collections
            || after.major_collections > before.major_collections,
        "the waiting callback must survive an actual collection"
    );
    page.frame(1.0);
    assert_eq!(page.log(), "kept");
}

#[test]
fn navigation_and_close_discard_pending_frame_callbacks() {
    let mut page = Page::new();
    page.run("requestAnimationFrame(function () { log.push('stale'); });")
        .unwrap();
    page.runtime.navigate(TAB, origin()).unwrap();
    assert!(!page.pending());
    assert_eq!(page.frame(1.0).callbacks_run, 0);
    page.runtime.close_realm(TAB);
    assert!(matches!(
        page.runtime.run_animation_frame(TAB, 2.0),
        Err(BlueJsPageRuntimeError::UnknownRealm(TAB))
    ));
}

#[test]
fn invalid_frame_timestamps_do_not_consume_callbacks_and_time_cannot_regress() {
    let mut page = Page::new();
    page.run("requestAnimationFrame(function (time) { log.push(time); });")
        .unwrap();
    for timestamp in [-1.0, f64::INFINITY, f64::NAN] {
        assert!(page.runtime.run_animation_frame(TAB, timestamp).is_err());
        assert!(page.pending());
    }
    page.frame(5.5);
    page.run("requestAnimationFrame(function (time) { log.push(time); });")
        .unwrap();
    page.frame(3.0);
    assert_eq!(page.log(), "5.5,5.5");
}

#[test]
fn a_render_boundary_checkpoints_jobs_even_without_a_frame_callback() {
    let mut page = Page::new();
    page.execute("queueMicrotask(function () { log.push('job'); });")
        .unwrap();
    assert_eq!(page.frame(1.0).callbacks_run, 0);
    assert_eq!(page.log(), "job");
}

#[test]
fn debugger_pause_retains_callbacks_and_suppresses_runnable_frame_work() {
    let mut vm = Vm::default();
    vm.install_host_animation_frame_functions().unwrap();
    vm.execute_script(
        &compile(
            &parse("var count = 0; requestAnimationFrame(function () { count += 1; });").unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    vm.execute_script_until_debugger_pause(&compile(&parse("count += 10;").unwrap()).unwrap(), 0)
        .unwrap();
    assert!(!vm.host_animation_frame_pending());
    assert_eq!(vm.run_host_animation_frame(1.0).unwrap().callbacks_run, 0);
    vm.resume_debugger_execution().unwrap();
    assert!(vm.host_animation_frame_pending());
    vm.run_host_animation_frame(2.0).unwrap();
    assert_eq!(
        vm.execute_script(&compile(&parse("count").unwrap()).unwrap())
            .unwrap(),
        Value::Number(11.0)
    );
}

#[test]
fn runaway_frame_callbacks_and_checkpoints_return_resource_failures() {
    for body in [
        "while (true) {}",
        "function again() { queueMicrotask(again); } queueMicrotask(again);",
    ] {
        let mut page = Page::new();
        page.run(&format!("requestAnimationFrame(function () {{ {body} }});"))
            .unwrap();
        assert!(matches!(
            page.runtime.run_animation_frame(TAB, 1.0),
            Err(BlueJsPageRuntimeError::Runtime(
                RuntimeError::InstructionLimit
            ))
        ));
    }
}
