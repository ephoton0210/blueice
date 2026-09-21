// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! An async generator's `yield*` forwards `return`/`throw` to its delegate
//! exactly once: a delegate that completes in response is finished and must
//! not be closed a second time while the generator unwinds (§15.5.5,
//! YieldExpression evaluation with generatorKind async).

use blueice_bluejs::{compile, parse, Vm, VmConfig};

fn run(script: &str, nursery_capacity: Option<usize>) {
    let mut config = VmConfig::default();
    if let Some(capacity) = nursery_capacity {
        config.heap.nursery_capacity = capacity;
    }
    let mut vm = Vm::new(config).unwrap();
    vm.install_test262_done().unwrap();
    let source = format!(
        "function finish(ok) {{ if (ok) $DONE(); else $DONE(new Error('check failed')); }}{script}"
    );
    vm.execute_script(&compile(&parse(&source).unwrap()).unwrap())
        .unwrap();
    vm.run_promise_jobs().unwrap();
    vm.take_test262_done()
        .expect("the script never reported through $DONE")
        .unwrap_or_else(|error| panic!("{script}\n=> {error:?}"));
}

fn run_both(script: &str) {
    run(script, None);
    run(script, Some(1));
}

#[test]
fn a_finished_delegate_is_not_returned_to_twice() {
    run_both(
        "var returns = 0;
         var inner = { [Symbol.asyncIterator]() { return this; },
           next() { return { value: 1, done: false }; },
           return(v) { returns++; return { value: 'r:' + v, done: true }; } };
         async function* g() { yield* inner; }
         var it = g();
         it.next().then(() => it.return('a')).then(r => {
           finish(r.value === 'r:a' && r.done === true && returns === 1);
         }, () => finish(false));",
    );
}

#[test]
fn a_delegate_that_finishes_after_throw_is_not_closed_when_the_generator_returns() {
    run_both(
        "var returns = 0, throws = 0;
         var inner = { [Symbol.asyncIterator]() { return this; },
           next() { return { value: 1, done: false }; },
           throw(e) { throws++; return { value: 'caught:' + e, done: true }; },
           return(v) { returns++; return { value: v, done: true }; } };
         async function* g() { var x = yield* inner; return x; }
         var it = g();
         it.next().then(() => it.throw('e')).then(r => {
           finish(r.value === 'caught:e' && r.done === true && throws === 1 && returns === 0);
         }, () => finish(false));",
    );
}

#[test]
fn an_unfinished_delegate_keeps_being_delegated_to() {
    run_both(
        "var returns = 0;
         var inner = { [Symbol.asyncIterator]() { return this; },
           next() { return { value: 1, done: false }; },
           return(v) { returns++; return { value: 'r' + returns, done: returns >= 2 }; } };
         async function* g() { yield* inner; }
         var it = g();
         it.next().then(() => it.return('a')).then(r1 => it.return('b').then(r2 => {
           finish(r1.value === 'r1' && r1.done === false && r2.value === 'r2' && r2.done === true && returns === 2);
         }), () => finish(false));",
    );
}
