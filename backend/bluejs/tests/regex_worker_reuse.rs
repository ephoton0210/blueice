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
use blueice_bluejs::regex_worker::workers_started;
use blueice_bluejs::{compile, parse, Value, Vm};

fn run(vm: &mut Vm, source: &str) -> Value {
    vm.execute(&compile(&parse(source).unwrap()).unwrap())
        .unwrap()
}

const MANY_MATCHES: &str = "var r=/a+b/,n=0;for(var i=0;i<200;i++){if(r.exec('caab'+i))n++}n";

#[test]
fn one_worker_serves_many_operations_on_one_thread_and_on_several() {
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
