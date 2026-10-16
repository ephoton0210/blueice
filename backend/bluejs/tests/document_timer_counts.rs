// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_bluejs::{
    parse, BlueJsPageOrigin, BlueJsPageRuntime, BlueJsProgramV1, BlueJsSourceIdentity, Value,
};

fn execute(runtime: &mut BlueJsPageRuntime, tab: u64, source: &str) -> Value {
    let stats = runtime.realm_stats(tab).unwrap();
    let handle = runtime
        .install_program(
            tab,
            &stats.origin,
            BlueJsSourceIdentity::new(
                format!("page:///timer-count-{}.js", stats.program_count),
                "timer-count",
            )
            .unwrap(),
            &BlueJsProgramV1::Script(parse(source).unwrap()),
        )
        .unwrap();
    runtime.execute_program(tab, handle).unwrap()
}

fn open(runtime: &mut BlueJsPageRuntime, tab: u64) {
    runtime
        .open_realm(
            tab,
            BlueJsPageOrigin::new("https://timer-count.test").unwrap(),
        )
        .unwrap();
    runtime
        .configure_realm_bindings(tab, |host| host.install_host_timers())
        .unwrap();
}

#[test]
fn document_timer_counts_are_independent_and_navigation_replaces_the_account() {
    let mut runtime = BlueJsPageRuntime::default();
    open(&mut runtime, 1);
    open(&mut runtime, 2);
    execute(
        &mut runtime,
        1,
        "setTimeout(() => {}, 100); setInterval(() => {}, 100); 0;",
    );
    execute(&mut runtime, 2, "setTimeout(() => {}, 100); 0;");
    assert_eq!(runtime.realm_stats(1).unwrap().scheduling.pending_timers, 2);
    assert_eq!(runtime.host_scheduling_usage(2).unwrap().pending_timers, 1);
    runtime
        .navigate(
            1,
            BlueJsPageOrigin::new("https://timer-count.test").unwrap(),
        )
        .unwrap();
    assert_eq!(runtime.realm_stats(1).unwrap().scheduling.pending_timers, 0);
    assert_eq!(runtime.realm_stats(2).unwrap().scheduling.pending_timers, 1);
    assert!(runtime.close_realm(1));
    assert_eq!(runtime.realm_stats(2).unwrap().scheduling.pending_timers, 1);
    assert!(runtime.close_realm(2));
}

#[test]
fn document_timer_counts_follow_registration_delivery_and_clearing() {
    let mut runtime = BlueJsPageRuntime::default();
    open(&mut runtime, 1);
    execute(&mut runtime, 1, "var ticks = 0; setTimeout(() => ticks++, 1); var interval = setInterval(() => ticks++, 1); 0;");
    assert_eq!(runtime.host_scheduling_usage(1).unwrap().pending_timers, 2);
    assert_eq!(runtime.run_due_timers(1, 0).unwrap().tasks_run, 0);
    assert_eq!(runtime.run_due_timers(1, 1).unwrap().tasks_run, 2);
    assert_eq!(runtime.realm_stats(1).unwrap().scheduling.pending_timers, 1);
    execute(&mut runtime, 1, "clearInterval(interval); 0;");
    assert_eq!(runtime.host_scheduling_usage(1).unwrap().pending_timers, 0);
    assert_eq!(runtime.run_due_timers(1, 2).unwrap().tasks_run, 0);
    assert_eq!(execute(&mut runtime, 1, "ticks;"), Value::Number(2.0));
}
