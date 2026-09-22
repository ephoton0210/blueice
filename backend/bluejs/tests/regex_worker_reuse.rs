// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! RegExp matching runs in a helper process. Starting a process costs
//! milliseconds (more on Windows, where every start is also scanned), so the
//! helper must serve many operations rather than be restarted for each one:
//! a script that matches a few hundred times must not start a few hundred
//! processes. Windows once retired the worker after every operation, which
//! made RegExp-heavy Test262 cases exceed their two-second deadline there
//! while running in a fraction of a second elsewhere.
use blueice_bluejs::regex_worker::{round_trips, workers_started};
use blueice_bluejs::{compile, parse, Value, Vm};
use std::sync::{Mutex, PoisonError};

/// The counters are process-wide, so the tests that read them must not overlap.
static COUNTERS: Mutex<()> = Mutex::new(());

fn run(vm: &mut Vm, source: &str) -> Value {
    vm.execute(&compile(&parse(source).unwrap()).unwrap())
        .unwrap()
}

const MANY_MATCHES: &str = "var r=/a+b/,n=0;for(var i=0;i<200;i++){if(r.exec('caab'+i))n++}n";

#[test]
fn one_worker_serves_many_operations_on_one_thread_and_on_several() {
    let _serial = COUNTERS.lock().unwrap_or_else(PoisonError::into_inner);
    let mut vm = Vm::default();
    run(&mut vm, "/a/.test('a')");
    let before = workers_started();
    assert_eq!(run(&mut vm, MANY_MATCHES), Value::Number(200.0));
    assert_eq!(
        workers_started() - before,
        0,
        "the worker was restarted instead of being reused"
    );

    // Independent threads may match at the same time, so each can hold a
    // worker of its own, but none may start one per operation.
    let before = workers_started();
    std::thread::scope(|scope| {
        for _ in 0..4 {
            scope.spawn(|| {
                let mut vm = Vm::default();
                assert_eq!(run(&mut vm, MANY_MATCHES), Value::Number(200.0));
            });
        }
    });
    let started = workers_started() - before;
    assert!(
        started <= 4,
        "four threads started {started} workers for 800 operations"
    );
}

#[test]
fn repeating_an_identical_match_is_answered_without_another_round_trip() {
    let _serial = COUNTERS.lock().unwrap_or_else(PoisonError::into_inner);
    let mut vm = Vm::default();
    // The first call may compile and start a worker; every repeat of the same
    // pattern, flags, subject and start index is then a memo hit.
    run(&mut vm, "/(?<word>x+)(y)?/.exec('__xxz')");
    let before = round_trips();
    let script = "var r=/(?<word>x+)(y)?/,out=[];\
        for(var i=0;i<200;i++){var m=r.exec('__xxz');out.push(m.index+':'+m[0]+':'+m[1]+':'+m[2]+':'+m.groups.word)}\
        out.every(function(s){return s==='2:xx:xx:undefined:xx'})";
    assert_eq!(run(&mut vm, script), Value::Bool(true));
    let sent = round_trips() - before;
    assert!(sent <= 2, "200 identical matches sent {sent} requests");

    // A different subject or start index is a different question and must
    // still reach the matcher (a hit must never answer for another subject).
    assert_eq!(
        run(
            &mut vm,
            "var s=/x+/g,a=s.exec('xx__x');var b=s.exec('xx__x');a.index+','+b.index"
        ),
        Value::String("0,4".into())
    );
    let before = round_trips();
    run(
        &mut vm,
        "var r=/q+r/;for(var i=0;i<200;i++){r.exec('unique-subject-'+i+'-qqr')}",
    );
    assert!(
        round_trips() - before >= 200,
        "distinct subjects were answered from the memo"
    );
}

#[test]
fn recompiling_an_accepted_literal_in_a_loop_does_not_reach_the_matcher_again() {
    let _serial = COUNTERS.lock().unwrap_or_else(PoisonError::into_inner);
    let mut vm = Vm::default();
    let script = "var n=0;for(var i=0;i<100;i++){if(/accepted-literal-\\d+/.test('accepted-literal-7'))n++}n";
    assert_eq!(run(&mut vm, script), Value::Number(100.0));
    let before = round_trips();
    assert_eq!(run(&mut vm, script), Value::Number(100.0));
    assert_eq!(round_trips() - before, 0);

    // A rejected pattern is still reported by the matcher every time.
    for _ in 0..2 {
        let before = round_trips();
        assert!(vm
            .execute(&compile(&parse("new RegExp('(')").unwrap()).unwrap())
            .is_err());
        assert!(round_trips() > before);
    }
}
