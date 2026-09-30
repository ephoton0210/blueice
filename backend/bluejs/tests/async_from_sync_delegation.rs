// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `yield*` in an async generator over a *synchronous* iterator: the outer
//! generator's `return()` and `throw()` are forwarded to the synchronous
//! iterator through AsyncFromSyncIteratorContinuation, so a poisoned result, a
//! throwing method or a rejecting value rejects the request and finishes the
//! generator (a later `next()` reports `done`).

use blueice_bluejs::{compile, parse, Value, Vm, VmConfig};

fn run(script: &str, gc_stress: bool) {
    let mut config = VmConfig::default();
    if gc_stress {
        config.heap.nursery_capacity = 1;
    }
    let mut vm = Vm::new(config).unwrap();
    vm.install_test262_done().unwrap();
    let source = format!(
        "var log = [];\
         function check(condition, message) {{ if (!condition) log.push(message); }}\
         function finish() {{ if (log.length === 0) $DONE(); else $DONE(log.join('; ')); }}\
         {script}"
    );
    vm.execute_script(&compile(&parse(&source).unwrap()).unwrap())
        .unwrap_or_else(|error| panic!("{script}\n=> {error:?}"));
    vm.run_promise_jobs().unwrap();
    match vm
        .take_test262_done()
        .expect("the script never reported through $DONE")
    {
        Ok(()) => {}
        Err(Value::String(problems)) => panic!(
            "{script}\n(gc_stress = {gc_stress}) => {}",
            problems.to_utf8().unwrap()
        ),
        Err(other) => panic!("{script}\n(gc_stress = {gc_stress}) => {other:?}"),
    }
}

fn check(script: &str) {
    run(script, false);
    run(script, true);
}

#[test]
fn a_failing_delegate_return_rejects_the_request_and_finishes_the_generator() {
    check(
        r#"
        var thrown = new Error("Catch me.");
        function delegate(returnMethod, extra) {
          var source = { [Symbol.iterator]() { return Object.assign({ next() { return { value: 1, done: false }; } }, returnMethod); } };
          async function* g() { yield* source; }
          return g();
        }
        var cases = {
          "return throws": { return() { throw thrown; } },
          "return getter throws": { get return() { throw thrown; } },
          "done getter throws": { return() { return { get done() { throw thrown; }, value: 1 }; } },
          "value getter throws": { return() { return { done: false, get value() { throw thrown; } }; } },
        };
        var pending = Object.keys(cases).map(function(name) {
          var it = delegate(cases[name]);
          return it.next().then(function() { return it.return("ignored"); }).then(
            function() { check(false, name + ": return() fulfilled"); },
            function(e) { check(e === thrown, name + ": rejected with " + e); }
          ).then(function() { return it.next(); }).then(function(r) {
            check(r.done === true && r.value === undefined, name + ": generator not finished (" + r.done + "," + r.value + ")");
          });
        });
        Promise.all(pending).then(finish);
        "#,
    );
}

#[test]
fn a_return_result_with_a_rejecting_value_does_not_close_the_iterator_but_throw_does() {
    check(
        r#"
        var closes = 0;
        function reject() {}
        function make(method) {
          var source = { [Symbol.iterator]() {
            return Object.assign({ next() { return { value: 1, done: false }; }, return() { closes++; return {}; } }, method);
          } };
          async function* g() { return yield* source; }
          return g();
        }
        var viaReturn = make({ return() { closes++; return { value: Promise.reject(new reject()), done: false }; } });
        var viaThrow = make({ throw() { return { value: Promise.reject(new reject()), done: false }; } });
        var events = [];
        viaReturn.next().then(function() { return viaReturn.return(); }).then(
          () => check(false, "return fulfilled"),
          function(e) { check(e instanceof reject, "return rejection"); events.push("return closes=" + closes); }
        ).then(function() {
          closes = 0;
          return viaThrow.next().then(function() { return viaThrow.throw(); });
        }).then(
          () => check(false, "throw fulfilled"),
          function(e) { check(e instanceof reject, "throw rejection"); check(closes === 1, "throw closes the iterator once, got " + closes); }
        ).then(function() {
          check(events.join() === "return closes=1", "return closes: " + events.join());
          finish();
        });
        "#,
    );
}
