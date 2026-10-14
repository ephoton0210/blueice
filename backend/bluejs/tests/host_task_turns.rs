// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! One bounded document task entry and its source-free wake-up result.

use blueice_bluejs::{
    parse, BlueJsPageOrigin, BlueJsPageRuntime, BlueJsPageRuntimeError, BlueJsProgramV1,
    BlueJsSourceIdentity, HostValue, RuntimeError,
};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

struct Page {
    runtime: BlueJsPageRuntime,
    log: Rc<RefCell<Vec<String>>>,
    serial: u32,
}

impl Page {
    fn new() -> Self {
        Self::with_clock(None)
    }

    fn with_clock(clock: Option<Rc<Cell<u64>>>) -> Self {
        let mut runtime = BlueJsPageRuntime::default();
        runtime.open_realm(1, origin()).unwrap();
        let log = Rc::new(RefCell::new(Vec::new()));
        let output = log.clone();
        runtime
            .configure_realm_bindings(1, |host| {
                host.install_web_file_api()?;
                if let Some(clock) = clock {
                    host.install_host_timers_with_clock(move || clock.get())?;
                } else {
                    host.install_host_timers()?;
                }
                host.install_host_animation_frames()?;
                host.install_global_function("record", 1, move |args: &[HostValue]| {
                    if let Some(HostValue::String(value)) = args.first() {
                        output.borrow_mut().push(value.to_utf8().unwrap());
                    }
                    Ok(HostValue::Undefined)
                })
            })
            .unwrap();
        Self {
            runtime,
            log,
            serial: 0,
        }
    }

    fn execute(&mut self, source: &str) {
        self.serial += 1;
        let handle = self
            .runtime
            .install_program(
                1,
                &origin(),
                BlueJsSourceIdentity::new(
                    format!("page:///task-{}.js", self.serial),
                    format!("sha256:task-{}", self.serial),
                )
                .unwrap(),
                &BlueJsProgramV1::Script(parse(source).unwrap()),
            )
            .unwrap();
        self.runtime.execute_program(1, handle).unwrap();
        self.runtime.run_microtask_checkpoint(1).unwrap();
    }

    fn expect(&self, expected: &[&str]) {
        assert_eq!(*self.log.borrow(), expected);
    }
}

fn origin() -> BlueJsPageOrigin {
    BlueJsPageOrigin::new("https://tasks.test").unwrap()
}

#[test]
fn one_host_turn_shares_one_budget_between_browser_and_timer_tasks() {
    let mut page = Page::new();
    page.execute("var registry=new FinalizationRegistry(()=>{record('cleanup');queueMicrotask(()=>record('cleanup-micro'));});registry.register({},0);setTimeout(()=>{record('timer');queueMicrotask(()=>record('timer-micro'));},0);");
    let first = page.runtime.run_task_turn(1, 0, 1).unwrap();
    assert_eq!(first.tasks_run, 1);
    assert_eq!(first.next_wake_in_ms, Some(0));
    page.expect(&["cleanup", "cleanup-micro"]);
    let second = page.runtime.run_task_turn(1, 0, 1).unwrap();
    assert_eq!(second.tasks_run, 1);
    assert_eq!(second.next_wake_in_ms, None);
    page.expect(&["cleanup", "cleanup-micro", "timer", "timer-micro"]);
}

#[test]
fn a_cleanup_backlog_yields_to_due_timers_between_callbacks() {
    let mut page = Page::new();
    page.execute("var registry=new FinalizationRegistry(()=>record('cleanup'));for(var i=0;i<100;i++)registry.register({},i);setTimeout(()=>record('timer'),0);");
    let turn = page.runtime.run_task_turn(1, 0, 4).unwrap();
    assert_eq!(turn.tasks_run, 4);
    assert_eq!(turn.next_wake_in_ms, Some(0));
    page.expect(&["cleanup", "timer", "cleanup", "cleanup"]);
}

#[test]
fn newly_registered_timers_wait_for_the_next_common_turn() {
    let mut page = Page::new();
    page.execute("setTimeout(()=>{record('outer');queueMicrotask(()=>setTimeout(()=>record('inner'),0));},0);");
    let first = page.runtime.run_task_turn(1, 0, 64).unwrap();
    assert_eq!(first.tasks_run, 1);
    assert_eq!(first.next_wake_in_ms, Some(0));
    page.expect(&["outer"]);
    assert_eq!(page.runtime.run_task_turn(1, 0, 64).unwrap().tasks_run, 1);
    page.expect(&["outer", "inner"]);
}

#[test]
fn future_abort_tasks_do_not_consume_a_turn_or_block_ready_reads() {
    let mut page = Page::new();
    page.execute("var signal=AbortSignal.timeout(60000);signal.onabort=()=>record('late');var reader=new FileReader();reader.onloadstart=()=>record('read');reader.readAsText(new Blob(['x']));");
    let turn = page.runtime.run_task_turn(1, 0, 1).unwrap();
    assert_eq!(turn.tasks_run, 1);
    page.expect(&["read"]);
    assert_eq!(turn.next_wake_in_ms, Some(0));
}

#[test]
fn future_work_reports_a_positive_wake_without_running_a_task() {
    let mut page = Page::new();
    page.execute("var signal=AbortSignal.timeout(60000);signal.onabort=()=>record('late');");
    let turn = page.runtime.run_task_turn(1, 0, 64).unwrap();
    assert_eq!(turn.tasks_run, 0);
    assert!(turn
        .next_wake_in_ms
        .is_some_and(|delay| delay > 0 && delay <= 60000));
    page.expect(&[]);
}

#[test]
fn timers_and_frames_keep_separate_host_entries() {
    let mut page = Page::new();
    page.execute("requestAnimationFrame(()=>record('frame'));setTimeout(()=>record('timer'),0);");
    assert_eq!(page.runtime.run_task_turn(1, 0, 64).unwrap().tasks_run, 1);
    page.expect(&["timer"]);
    assert!(page.runtime.animation_frame_pending(1).unwrap());
    page.runtime.run_animation_frame(1, 1.0).unwrap();
    page.expect(&["timer", "frame"]);
}

#[test]
fn ordinary_timer_throws_complete_the_checkpoint_but_resource_failures_stop() {
    let mut page = Page::new();
    page.execute("setTimeout(()=>{record('first');queueMicrotask(()=>record('micro'));throw 7;},0);setTimeout(()=>record('second'),0);");
    assert_eq!(page.runtime.run_task_turn(1, 0, 2).unwrap().tasks_run, 2);
    page.expect(&["first", "micro", "second"]);
    page.execute("setTimeout(()=>{queueMicrotask(()=>record('must not run'));while(true){}},0);setTimeout(()=>record('must not run'),0);");
    assert_eq!(
        page.runtime.run_task_turn(1, 0, 2),
        Err(BlueJsPageRuntimeError::Runtime(
            RuntimeError::InstructionLimit
        ))
    );
    page.expect(&["first", "micro", "second"]);
}

#[test]
fn navigation_discards_tasks_and_their_wakes() {
    let mut page = Page::new();
    page.execute("var registry=new FinalizationRegistry(()=>record('old'));registry.register({},0);setTimeout(()=>record('old'),100);");
    page.runtime.navigate(1, origin()).unwrap();
    let turn = page.runtime.run_task_turn(1, 100, 64).unwrap();
    assert_eq!(turn.tasks_run, 0);
    assert_eq!(turn.next_wake_in_ms, None);
    page.expect(&[]);
}

#[test]
fn even_a_large_requested_turn_keeps_the_combined_64_task_bound() {
    let mut page = Page::new();
    page.execute("var registry=new FinalizationRegistry(()=>record('cleanup'));for(var i=0;i<100;i++){registry.register({},i);setTimeout(()=>record('timer'),0);}");
    let turn = page.runtime.run_task_turn(1, 0, usize::MAX).unwrap();
    assert_eq!(turn.tasks_run, 64);
    assert_eq!(turn.next_wake_in_ms, Some(0));
    let log = page.log.borrow();
    assert_eq!(log.len(), 64);
    assert!(log.chunks_exact(2).all(|pair| pair == ["cleanup", "timer"]));
}

#[test]
fn a_zero_task_budget_preserves_ready_work_and_advertises_it() {
    let mut page = Page::new();
    page.execute("setTimeout(()=>record('timer'),0);");
    let turn = page.runtime.run_task_turn(1, 0, 0).unwrap();
    assert_eq!(turn.tasks_run, 0);
    assert_eq!(turn.next_wake_in_ms, Some(0));
    page.expect(&[]);
    assert_eq!(page.runtime.run_task_turn(1, 0, 1).unwrap().tasks_run, 1);
    page.expect(&["timer"]);
}

#[test]
fn callback_microtasks_can_cancel_a_timer_without_spending_another_task() {
    let mut page = Page::new();
    page.execute("var timer=setTimeout(()=>record('must not run'),0);var registry=new FinalizationRegistry(()=>{record('cleanup');queueMicrotask(()=>clearTimeout(timer));});registry.register({},0);");
    let turn = page.runtime.run_task_turn(1, 0, 64).unwrap();
    assert_eq!(turn.tasks_run, 1);
    assert_eq!(turn.next_wake_in_ms, None);
    page.expect(&["cleanup"]);
}

#[test]
fn late_intervals_do_not_replay_missed_periods_in_a_common_turn() {
    let mut page = Page::new();
    page.execute("setInterval(()=>record('interval'),10);");
    let turn = page.runtime.run_task_turn(1, 1000, 64).unwrap();
    assert_eq!(turn.tasks_run, 1);
    assert_eq!(turn.next_wake_in_ms, Some(10));
    assert_eq!(
        page.runtime.run_task_turn(1, 1000, 64).unwrap().tasks_run,
        0
    );
    assert_eq!(
        page.runtime.run_task_turn(1, 1010, 64).unwrap().tasks_run,
        1
    );
    page.expect(&["interval", "interval"]);
}

#[test]
fn the_earliest_wake_includes_timers_and_future_browser_deadlines() {
    let mut page = Page::new();
    page.execute("var signal=AbortSignal.timeout(60000);signal.onabort=()=>{};setTimeout(()=>record('timer'),10);");
    assert_eq!(page.runtime.next_task_wake_in_ms(1, 0).unwrap(), Some(10));
    assert_eq!(page.runtime.next_task_wake_in_ms(1, 9).unwrap(), Some(1));
    assert_eq!(page.runtime.next_task_wake_in_ms(1, 10).unwrap(), Some(0));
    let turn = page.runtime.run_task_turn(1, 10, 64).unwrap();
    assert_eq!(turn.tasks_run, 1);
    assert!(turn.next_wake_in_ms.is_some_and(|wake| wake > 0));
    page.expect(&["timer"]);
}

#[test]
fn unreachable_future_abort_entries_are_reaped_without_waiting_for_their_deadline() {
    let mut page = Page::new();
    page.execute("AbortSignal.timeout(60000);true;");
    let turn = page.runtime.run_task_turn(1, 0, 64).unwrap();
    assert_eq!(turn.tasks_run, 1);
    assert_eq!(turn.next_wake_in_ms, None);
    assert_eq!(
        page.runtime
            .host_scheduling_usage(1)
            .unwrap()
            .pending_browser_tasks,
        0
    );
}

#[test]
fn document_turns_drive_same_document_shadow_cleanup_and_root_timers() {
    let mut page = Page::new();
    page.execute("var read=new ShadowRealm().evaluate('globalThis.count=0;globalThis.registry=new FinalizationRegistry(()=>count++);registry.register({},0);()=>count;');setTimeout(()=>record('timer'),0);");
    let first = page.runtime.run_task_turn(1, 0, 1).unwrap();
    assert_eq!(first.tasks_run, 1);
    assert_eq!(first.next_wake_in_ms, Some(0));
    page.execute("record(String(read()));");
    page.expect(&["1"]);
    assert_eq!(page.runtime.run_task_turn(1, 0, 1).unwrap().tasks_run, 1);
    page.expect(&["1", "timer"]);
}

#[test]
fn browser_task_fifo_is_preserved_while_timer_tasks_interleave() {
    let mut page = Page::new();
    page.execute("var first=new FileReader(),second=new FileReader();first.onloadstart=()=>record('first');second.onloadstart=()=>record('second');first.readAsText(new Blob(['a']));second.readAsText(new Blob(['b']));setTimeout(()=>record('timer'),0);");
    let turn = page.runtime.run_task_turn(1, 0, 3).unwrap();
    assert_eq!(turn.tasks_run, 3);
    page.expect(&["first", "timer", "second"]);
}

#[test]
fn debugger_pauses_preserve_tasks_without_busy_wakes() {
    let mut page = Page::new();
    page.execute("var registry=new FinalizationRegistry(()=>record('cleanup'));registry.register({},0);setTimeout(()=>record('timer'),0);");
    let program = page
        .runtime
        .install_program(
            1,
            &origin(),
            BlueJsSourceIdentity::new("page:///pause.js", "sha256:pause").unwrap(),
            &BlueJsProgramV1::Script(parse("debugger;").unwrap()),
        )
        .unwrap();
    page.runtime
        .execute_program_until_debugger_pause_at_root_offset(1, program, 0)
        .unwrap();
    let turn = page.runtime.run_task_turn(1, 100, 64).unwrap();
    assert_eq!(turn.tasks_run, 0);
    assert_eq!(turn.next_wake_in_ms, None);
    assert_eq!(page.runtime.next_task_wake_in_ms(1, 100).unwrap(), None);
    page.expect(&[]);
    page.runtime.resume_debugger_execution(1).unwrap();
    assert_eq!(page.runtime.run_task_turn(1, 100, 2).unwrap().tasks_run, 2);
    page.expect(&["cleanup", "timer"]);
}

#[test]
fn large_abort_deadlines_remain_pending_instead_of_becoming_ready() {
    let mut page = Page::new();
    page.execute("var signal=AbortSignal.timeout(9007199254740991);signal.onabort=()=>record('must not run');");
    let turn = page.runtime.run_task_turn(1, 0, 64).unwrap();
    assert_eq!(turn.tasks_run, 0);
    assert!(turn
        .next_wake_in_ms
        .is_some_and(|delay| delay > i32::MAX as u64));
    page.expect(&[]);
}

#[test]
fn timeout_callbacks_and_abort_timeouts_share_timer_source_order() {
    let mut page = Page::new();
    page.execute("setTimeout(()=>{record('timer');queueMicrotask(()=>record('timer-micro'));},0);var signal=AbortSignal.timeout(0);signal.onabort=()=>record('abort');");
    assert_eq!(page.runtime.run_task_turn(1, 0, 2).unwrap().tasks_run, 2);
    page.expect(&["timer", "timer-micro", "abort"]);
}

#[test]
fn abort_timeouts_follow_the_page_hosts_monotonic_clock() {
    let mut page = Page::new();
    page.execute("var signal=AbortSignal.timeout(10);signal.onabort=()=>record('abort');");
    assert_eq!(page.runtime.run_task_turn(1, 9, 64).unwrap().tasks_run, 0);
    assert_eq!(page.runtime.next_task_wake_in_ms(1, 9).unwrap(), Some(1));
    assert_eq!(page.runtime.run_task_turn(1, 10, 64).unwrap().tasks_run, 1);
    page.expect(&["abort"]);
}

#[test]
fn timer_source_orders_overdue_work_by_deadline_before_registration() {
    let mut page = Page::new();
    page.execute("var signal=AbortSignal.timeout(10);signal.onabort=()=>record('abort');setTimeout(()=>record('timer'),5);");
    assert_eq!(page.runtime.run_task_turn(1, 20, 2).unwrap().tasks_run, 2);
    page.expect(&["timer", "abort"]);
}

#[test]
fn equal_deadline_abort_timeouts_keep_registration_order_across_polls() {
    let mut page = Page::new();
    page.execute("var first=AbortSignal.timeout(0);first.onabort=()=>{record('abort');queueMicrotask(()=>record('micro'));};setTimeout(()=>record('timer'),0);var last=AbortSignal.timeout(0);last.onabort=()=>record('last');");
    for _ in 0..3 {
        assert_eq!(page.runtime.run_task_turn(1, 0, 1).unwrap().tasks_run, 1);
    }
    page.expect(&["abort", "micro", "timer", "last"]);
}

#[test]
fn an_abort_timeout_created_by_a_timer_waits_for_another_turn() {
    let mut page = Page::new();
    page.execute("setTimeout(()=>{record('timer');queueMicrotask(()=>{globalThis.signal=AbortSignal.timeout(0);signal.onabort=()=>record('abort');});},0);");
    let first = page.runtime.run_task_turn(1, 0, 64).unwrap();
    assert_eq!(first.tasks_run, 1);
    assert_eq!(first.next_wake_in_ms, Some(0));
    page.expect(&["timer"]);
    assert_eq!(page.runtime.run_task_turn(1, 0, 64).unwrap().tasks_run, 1);
    page.expect(&["timer", "abort"]);
}

#[test]
fn timeout_registration_during_a_task_uses_the_current_document_time() {
    let mut page = Page::new();
    page.execute("setTimeout(()=>{globalThis.signal=AbortSignal.timeout(10);signal.onabort=()=>record('abort');},100);");
    let first = page.runtime.run_task_turn(1, 100, 64).unwrap();
    assert_eq!(first.tasks_run, 1);
    assert_eq!(first.next_wake_in_ms, Some(10));
    assert_eq!(page.runtime.run_task_turn(1, 109, 64).unwrap().tasks_run, 0);
    assert_eq!(page.runtime.run_task_turn(1, 110, 64).unwrap().tasks_run, 1);
    page.expect(&["abort"]);
}

#[test]
fn abort_timeouts_sample_the_same_injected_clock_as_callback_timers() {
    let clock = Rc::new(Cell::new(50));
    let mut page = Page::with_clock(Some(clock.clone()));
    page.execute("setTimeout(()=>record('timer'),5);var signal=AbortSignal.timeout(5);signal.onabort=()=>record('abort');");
    assert_eq!(page.runtime.next_task_wake_in_ms(1, 0).unwrap(), Some(5));
    clock.set(54);
    assert_eq!(page.runtime.run_task_turn(1, 0, 64).unwrap().tasks_run, 0);
    assert_eq!(page.runtime.next_task_wake_in_ms(1, 0).unwrap(), Some(1));
    clock.set(55);
    assert_eq!(page.runtime.run_task_turn(1, 0, 64).unwrap().tasks_run, 2);
    page.expect(&["timer", "abort"]);
}

#[test]
fn file_reading_and_dom_manipulation_sources_get_separate_turns() {
    let mut page = Page::new();
    page.execute("var transfer=new DataTransfer();var item=transfer.items.add('drag','text/plain');for(var i=0;i<20;i++)item.getAsString(record);var reader=new FileReader();reader.onloadstart=()=>record('read');reader.readAsText(new Blob(['x']));setTimeout(()=>record('timer'),0);");
    assert_eq!(page.runtime.run_task_turn(1, 0, 3).unwrap().tasks_run, 3);
    page.expect(&["read", "timer", "drag"]);
}

#[test]
fn file_reading_fifo_follows_stage_enqueue_order() {
    let mut page = Page::new();
    page.execute("var first=new FileReader(), second=new FileReader();first.onloadstart=()=>{record('first-start');second.readAsText(new Blob(['b']));};second.onloadstart=()=>record('second-start');first.onprogress=()=>record('first-progress');second.onprogress=()=>record('second-progress');first.readAsText(new Blob(['a']));");
    assert_eq!(page.runtime.run_task_turn(1, 0, 4).unwrap().tasks_run, 4);
    page.expect(&[
        "first-start",
        "second-start",
        "first-progress",
        "second-progress",
    ]);
}

#[test]
fn a_rearmed_interval_joins_the_timer_source_after_existing_ready_work() {
    let mut page = Page::new();
    page.execute("var interval=setInterval(()=>record('interval'),0);var signal=AbortSignal.timeout(0);signal.onabort=()=>{record('abort');clearInterval(interval);};");
    let first = page.runtime.run_task_turn(1, 0, 1).unwrap();
    assert_eq!(first.tasks_run, 1);
    page.expect(&["interval"]);
    let second = page.runtime.run_task_turn(1, 0, 64).unwrap();
    assert_eq!(second.tasks_run, 1);
    assert_eq!(second.next_wake_in_ms, None);
    page.expect(&["interval", "abort"]);
}
