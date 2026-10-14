// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Promise semantics that need real PromiseCapability records: resolving
//! functions sharing one [[AlreadyResolved]] flag, thenable adoption on a later
//! job turn, species-aware `then`/`catch`/`finally`, the static combinators
//! run through user constructors, and `Promise.try`/`withResolvers` honouring
//! their receiver. Every script also runs under a one-object nursery (a
//! collection on nearly every allocation) to check that the closure state
//! held in heap records stays rooted.

use blueice_bluejs::{compile, parse, Value, Vm, VmConfig};

/// Runs `script` to completion (all promise jobs) and expects it to call
/// `finish(log)` with an empty log; otherwise the log is the failure message.
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
fn a_resolving_function_pair_shares_its_already_resolved_flag() {
    check(
        r#"
        var rejectedAfterThenable;
        var thenable = { then(resolve) { resolve("from thenable"); } };
        var p = new Promise(function(resolve, reject) {
          resolve(thenable);
          reject("ignored: the pair is already resolved");
          resolve("ignored too");
          throw "ignored: an executor throw after resolve";
        });
        var q = new Promise(function(resolve, reject) { resolve(1); reject(2); resolve(3); });
        var r = new Promise(function(resolve, reject) { throw new RangeError("boom"); });
        var s = new Promise(function(resolve, reject) { reject("first"); resolve("second"); });
        // The thenable's own callbacks form a fresh pair: a throw after resolve is swallowed.
        var t = new Promise(function(resolve) {
          resolve({ then(res, rej) { res("kept"); rej("dropped"); throw new Error("dropped too"); } });
        });
        Promise.all([p, q, r.catch(e => e instanceof RangeError), s.catch(e => e), t]).then(function(values) {
          check(values.join() === "from thenable,1,true,first,kept", "settlements: " + values.join());
        }).then(finish, function(e) { check(false, "unexpected rejection " + e); finish(); });
        "#,
    );
}

#[test]
fn resolving_with_a_thenable_or_promise_takes_extra_job_turns() {
    check(
        r#"
        var order = [];
        var inner = Promise.resolve("inner");
        var outer = new Promise(function(resolve) { resolve(inner); });
        outer.then(function() { order.push("outer"); });
        Promise.resolve().then(function() { order.push("a"); })
          .then(function() { order.push("b"); })
          .then(function() { order.push("c"); })
          .then(function() { order.push("d"); })
          .then(function() {
            check(order.join() === "a,b,outer,c,d", "order: " + order.join());
            finish();
          });
        // A then getter that throws rejects; a non-callable then fulfils with the object itself.
        var getterError = new Error("getter");
        var hostile = { get then() { throw getterError; } };
        Promise.resolve(hostile).then(() => check(false, "hostile fulfilled"), e => check(e === getterError, "getter error"));
        var plain = { then: 5 };
        Promise.resolve(plain).then(v => check(v === plain, "non-callable then keeps the object"));
        var self = new Promise(function(resolve) { setTimeoutLike(() => resolve(self)); });
        function setTimeoutLike(f) { Promise.resolve().then(f); }
        self.then(() => check(false, "self resolution fulfilled"), e => check(e instanceof TypeError, "self resolution rejects with TypeError"));
        "#,
    );
}

#[test]
fn then_creates_its_result_through_the_species_constructor() {
    check(
        r#"
        var constructed = 0;
        class Sub extends Promise {
          constructor(executor) { constructed++; super(executor); }
        }
        var derived = Sub.resolve(1).then(function(v) { return v + 1; });
        check(derived instanceof Sub && constructed === 2, "subclass result, constructions " + constructed);
        // Reading `constructor` and `@@species` happens once each per call.
        var reads = [];
        var p = Promise.resolve(1);
        Object.defineProperty(p, "constructor", { get() { reads.push("constructor"); return { get [Symbol.species]() { reads.push("species"); return Promise; } }; } });
        p.then();
        check(reads.join() === "constructor,species", "species reads: " + reads.join());
        // A species that is undefined or null falls back to %Promise%.
        var q = Promise.resolve(1);
        q.constructor = { [Symbol.species]: null };
        check(Object.getPrototypeOf(q.then()) === Promise.prototype, "null species");
        q.constructor = undefined;
        check(Object.getPrototypeOf(q.then()) === Promise.prototype, "undefined constructor");
        // Abrupt or invalid species.
        var bad = Promise.resolve(1);
        bad.constructor = 1;
        try { bad.then(); check(false, "primitive constructor"); } catch (e) { check(e instanceof TypeError, "primitive constructor error"); }
        bad.constructor = { [Symbol.species]: function() {} .bind ? {} : 0 };
        try { bad.then(); check(false, "non-constructor species"); } catch (e) { check(e instanceof TypeError, "non-constructor species error"); }
        // The species constructor's capability drives the result: its resolve/reject are called.
        var calls = [];
        function Custom(executor) {
          executor(function(v) { calls.push("resolve " + v); }, function(r) { calls.push("reject " + r); });
        }
        var r = Promise.resolve("A");
        r.constructor = { [Symbol.species]: Custom };
        var result = r.then(v => v + "!");
        check(result instanceof Custom, "custom result");
        var r2 = Promise.reject("B");
        r2.constructor = { [Symbol.species]: Custom };
        r2.then(undefined, e => { throw "T:" + e; });
        Promise.resolve().then(() => 0).then(() => 0).then(function() {
          check(calls.join() === "resolve A!,reject T:B", "custom capability calls: " + calls.join());
          finish();
        });
        "#,
    );
}

#[test]
fn a_capability_executor_may_only_be_given_its_functions_once() {
    check(
        r#"
        var first = true;
        function Twice(executor) {
          executor(undefined, undefined);   // both still undefined: allowed
          executor(function() {}, function() {});
          try { executor(function() {}, function() {}); check(false, "third call accepted"); }
          catch (e) { check(e instanceof TypeError, "third call error"); }
        }
        Twice.resolve = Promise.resolve;
        Promise.all.call(Twice, []);      // NewPromiseCapability(Twice) must succeed
        function NotCallable(executor) { executor(undefined, function() {}); }
        try { Promise.resolve.call(NotCallable, 1); check(false, "missing resolve accepted"); }
        catch (e) { check(e instanceof TypeError, "missing resolve error"); }
        function Silent(executor) {}
        try { Promise.reject.call(Silent, 1); check(false, "silent constructor accepted"); }
        catch (e) { check(e instanceof TypeError, "silent constructor error"); }
        try { Promise.resolve.call(undefined, 1); check(false, "undefined this"); } catch (e) { check(e instanceof TypeError, "undefined this error"); }
        try { Promise.reject.call(eval, 1); check(false, "non-constructor"); } catch (e) { check(e instanceof TypeError, "non-constructor error"); }
        finish();
        "#,
    );
}

#[test]
fn catch_and_finally_use_an_observable_then() {
    check(
        r#"
        var seen = [];
        var thenable = { then(a, b) { seen.push("then " + typeof a + " " + typeof b); return "result"; } };
        var caught = Promise.prototype.catch.call(thenable, function() {});
        check(caught === "result" && seen[0] === "then undefined function", "catch: " + seen[0]);
        seen.length = 0;
        var finalized = Promise.prototype.finally.call(thenable, function() {});
        check(finalized === "result" && seen[0] === "then function function", "finally: " + seen[0]);
        seen.length = 0;
        Promise.prototype.finally.call(thenable, 5);   // non-callable: passed through as both arguments
        check(seen[0] === "then number number", "finally non-callable: " + seen[0]);
        for (var bad of [undefined, null, 1, "s"]) {
          try { Promise.prototype.finally.call(bad); check(false, "finally accepted " + bad); }
          catch (e) { check(e instanceof TypeError, "finally receiver error"); }
          try { Promise.prototype.catch.call(bad); check(false, "catch accepted " + bad); }
          catch (e) { check(e instanceof TypeError, "catch receiver error"); }
        }
        var events = [];
        Promise.resolve("v").finally(function() { events.push("f1"); return "ignored"; })
          .then(function(v) { events.push("v=" + v); return Promise.reject("r"); })
          .finally(function() { events.push("f2"); })
          .catch(function(e) { events.push("caught " + e); })
          .finally(function() { throw "from finally"; })
          .catch(function(e) { events.push("caught " + e); })
          .finally(function() { return Promise.reject("rejected finally"); })
          .catch(function(e) { events.push("caught " + e); })
          .then(function() {
            check(events.join() === "f1,v=v,f2,caught r,caught from finally,caught rejected finally", "events: " + events.join());
            finish();
          });
        "#,
    );
}

#[test]
fn combinators_run_through_a_user_constructor() {
    check(
        r#"
        var settled = [];
        function Custom(executor) {
          executor(function(v) { settled.push(["resolve", v]); }, function(r) { settled.push(["reject", r]); });
        }
        var resolveGets = 0;
        Object.defineProperty(Custom, "resolve", { get() { resolveGets++; return function(v) { return v; }; } });
        var funcs = [];
        var thenable = { then(f, r) { funcs.push(f); } };
        var p = Promise.all.call(Custom, [thenable, thenable]);
        check(p instanceof Custom && resolveGets === 1, "all constructs its result with C, reading resolve once");
        // Element functions: length 1, empty name, length before name; each runs only once.
        var f = funcs[0];
        var names = Object.getOwnPropertyNames(f);
        check(names.join() === "length,name" && f.length === 1 && f.name === "", "element function shape: " + names.join());
        check(!("prototype" in f), "element function has no prototype");
        try { new f(); check(false, "element function is a constructor"); } catch (e) { check(e instanceof TypeError, "constructor error"); }
        f("one");
        f("ignored");
        check(settled.length === 0, "not yet complete");
        funcs[1]("two");
        check(settled.length === 1 && settled[0][0] === "resolve" && settled[0][1].join() === "one,two", "all result: " + JSON.stringify(settled));
        // allSettled records, any with an AggregateError.
        var results = [];
        Promise.allSettled([1, Promise.reject("x"), { then(f) { f("t"); } }]).then(function(v) {
          results.push(v.map(r => r.status + ":" + (r.value !== undefined ? r.value : r.reason)).join());
          return Promise.any([Promise.reject(1), Promise.reject(2)]);
        }).then(() => check(false, "any fulfilled"), function(e) {
          check(e instanceof AggregateError && e.errors.join() === "1,2", "any error");
          check(!Object.prototype.hasOwnProperty.call(e, "message"), "any error has no own message");
          var d = Object.getOwnPropertyDescriptor(e, "errors");
          check(!d.enumerable && d.writable && d.configurable, "errors descriptor");
          check(results[0] === "fulfilled:1,rejected:x,fulfilled:t", "allSettled: " + results[0]);
          return Promise.any([Promise.reject(1), Promise.resolve("ok"), Promise.reject(2)]);
        }).then(function(v) { check(v === "ok", "any value"); finish(); });
        "#,
    );
}

#[test]
fn combinators_consume_iterables_and_close_them_on_abrupt_completion() {
    check(
        r#"
        var closed = 0;
        function iterable(values, thrower) {
          return { [Symbol.iterator]() {
            var i = 0;
            return { next() { if (thrower && i === values.length) throw new EvalError("next"); return i < values.length ? { done: false, value: values[i++] } : { done: true }; },
                     return() { closed++; return {}; } };
          } };
        }
        var pending = [];
        // A non-array iterable works; a non-iterable rejects (never throws synchronously).
        Promise.all(new Set([1, 2])).then(v => check(v.join() === "1,2", "set input"));
        Promise.race(iterable([Promise.resolve("first"), new Promise(() => {})])).then(v => check(v === "first", "race"));
        pending.push(Promise.all(undefined).then(() => check(false, "undefined accepted"), e => check(e instanceof TypeError, "undefined error")));
        pending.push(Promise.allSettled(5).then(() => check(false, "number accepted"), e => check(e instanceof TypeError, "number error")));
        // `resolve` throwing closes the iterator and rejects.
        var resolveError = new RangeError("resolve");
        function Failing(executor) { executor(() => {}, () => {}); }
        var seenReject;
        function Rejecting(executor) { executor(() => {}, function(r) { seenReject = r; }); }
        Rejecting.resolve = function() { throw resolveError; };
        Promise.all.call(Rejecting, iterable([1, 2]));
        check(seenReject === resolveError && closed === 1, "resolve throw closes: " + closed);
        closed = 0;
        // A throwing `next` rejects without closing.
        var nextRejection;
        Promise.all(iterable([1], true)).catch(e => { nextRejection = e; });
        // A missing `resolve` rejects with a TypeError, before the iterator is touched.
        function NoResolve(executor) { executor(() => {}, function(r) { seenReject = r; }); }
        var touched = false;
        Promise.all.call(NoResolve, { [Symbol.iterator]() { touched = true; return [][Symbol.iterator](); } });
        check(seenReject instanceof TypeError && !touched, "missing resolve");
        Promise.resolve().then(() => 0).then(function() {
          check(nextRejection instanceof EvalError && closed === 0, "throwing next: " + closed);
          finish();
        });
        "#,
    );
}

#[test]
fn keyed_combinators_collect_enumerable_own_properties() {
    check(
        r#"
        var sym = Symbol("s");
        var input = { a: Promise.resolve(1), b: 2, [sym]: new Promise(r => r(3)) };
        Object.defineProperty(input, "hidden", { value: Promise.reject("never read"), enumerable: false });
        var proto = { inherited: 1 };
        Object.setPrototypeOf(input, proto);
        Promise.allKeyed(input).then(function(result) {
          check(Object.getPrototypeOf(result) === null, "null prototype");
          var keys = Reflect.ownKeys(result);
          check(keys.length === 3 && keys[0] === "a" && keys[1] === "b" && keys[2] === sym, "keys: " + keys.map(String).join());
          check(result.a === 1 && result.b === 2 && result[sym] === 3, "values");
          var d = Object.getOwnPropertyDescriptor(result, "a");
          check(d.writable && d.enumerable && d.configurable, "data property");
          return Promise.allSettledKeyed({ ok: 1, bad: Promise.reject("no") });
        }).then(function(result) {
          check(result.ok.status === "fulfilled" && result.ok.value === 1 && result.bad.status === "rejected" && result.bad.reason === "no", "allSettledKeyed");
          return Promise.allKeyed({});
        }).then(function(result) {
          check(Reflect.ownKeys(result).length === 0 && Object.getPrototypeOf(result) === null, "empty");
          return Promise.allKeyed({ x: Promise.resolve(1), y: Promise.reject("boom") }).catch(e => e);
        }).then(function(reason) {
          check(reason === "boom", "rejection");
          return Promise.allKeyed(1).catch(e => e);
        }).then(function(reason) {
          check(reason instanceof TypeError, "non-object argument rejects with TypeError");
          finish();
        });
        "#,
    );
}

#[test]
fn try_and_with_resolvers_honour_their_receiver_and_try_does_not_wrap_promises() {
    check(
        r#"
        var constructed = 0;
        class Sub extends Promise { constructor(e) { constructed++; super(e); } }
        var s = Sub.try(function(a, b) { return a + b; }, 1, 2);
        check(s instanceof Sub && constructed === 1, "try builds a receiver instance");
        var sentinel = Sub.resolve();
        check(Sub.try(function() { return sentinel; }) === sentinel, "a returned promise of the same constructor is not wrapped");
        var plain = Promise.resolve();
        check(Promise.try(function() { return plain; }) === plain, "not wrapped for %Promise% either");
        check(Sub.try(function() { return plain; }) !== plain, "a promise of another constructor is wrapped");
        var thrown = Promise.try(function() { throw new RangeError("t"); });
        try { Promise.try.call(undefined, function() {}); check(false, "undefined receiver"); } catch (e) { check(e instanceof TypeError, "receiver error"); }
        try { Promise.try.call(function() { throw new EvalError("ctor"); }, function() {}); check(false, "constructor throw"); } catch (e) { check(e instanceof EvalError, "constructor error propagates"); }
        var resolvers = Promise.withResolvers.call(Sub);
        check(resolvers.promise instanceof Sub, "withResolvers builds a receiver instance");
        check(Object.keys(resolvers).join() === "promise,resolve,reject", "withResolvers keys");
        try { Promise.withResolvers.call(undefined); check(false, "withResolvers undefined"); } catch (e) { check(e instanceof TypeError, "withResolvers receiver error"); }
        resolvers.resolve("done");
        Promise.all([thrown.catch(e => e instanceof RangeError), resolvers.promise, s]).then(function(v) {
          check(v[0] === true && v[1] === "done" && v[2] === 3, "values " + v.join());
          finish();
        });
        "#,
    );
}

#[test]
fn promise_functions_have_their_specified_shape() {
    check(
        r#"
        var captured = {};
        var thenable = { then(resolve, reject) { captured.resolve = resolve; captured.reject = reject; } };
        Promise.resolve(thenable);
        // The resolving functions only exist after the thenable job has run.
        var executorFunction;
        new Promise(function(resolve, reject) {
          executorFunction = arguments.callee || null;
          captured.first = resolve; captured.second = reject;
          for (var f of [resolve, reject]) {
            check(Object.getOwnPropertyNames(f).join() === "length,name", "resolving function keys: " + Object.getOwnPropertyNames(f).join());
            check(f.length === 1 && f.name === "" && !("prototype" in f), "resolving function shape");
            try { new f(); check(false, "constructor"); } catch (e) { check(e instanceof TypeError, "constructor error"); }
          }
        });
        var executor;
        function Capture(fn) { executor = fn; fn(function() {}, function() {}); }
        Promise.resolve.call(Capture, 1);
        check(Object.getOwnPropertyNames(executor).join() === "length,name" && executor.length === 2 && executor.name === "", "executor shape");
        for (var name of ["all", "allSettled", "any", "race", "resolve", "reject", "withResolvers", "try", "allKeyed", "allSettledKeyed"]) {
          var d = Object.getOwnPropertyDescriptor(Promise, name);
          check(d && d.writable && !d.enumerable && d.configurable && d.value.name === name, name + " descriptor");
          try { new d.value(); check(false, name + " is a constructor"); } catch (e) { check(e instanceof TypeError, name + " constructor error"); }
        }
        check(Promise.withResolvers.length === 0 && Promise.try.length === 1 && Promise.allKeyed.length === 1, "lengths");
        finish();
        "#,
    );
}
