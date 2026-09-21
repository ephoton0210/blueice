// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Where an async generator awaits (§27.6.3): `yield value` and
//! `return value` await their operand inside the body, a `return(value)`
//! request awaits its operand when it reaches a generator suspended at a
//! yield, and a result is delivered to the consumer without a further await.
//! The observable difference is the order of promise jobs, so every script
//! logs against a chain of counter jobs (`tick 1`, `tick 2`, ...) queued
//! first. Each script also runs under a one-object nursery.

use blueice_bluejs::{compile, parse, Vm, VmConfig};

fn run(script: &str, nursery_capacity: Option<usize>) {
    let mut config = VmConfig::default();
    if let Some(capacity) = nursery_capacity {
        config.heap.nursery_capacity = capacity;
    }
    let mut vm = Vm::new(config).unwrap();
    vm.install_test262_done().unwrap();
    let source = format!(
        "var log = [];
         function ticks(n) {{ var p = Promise.resolve(0); for (var i = 1; i <= n; i++) {{ (function (i) {{ p = p.then(() => log.push('tick ' + i)); }})(i); }} return p; }}
         function expect(expected) {{ return Promise.resolve().then(() => 0).then(() => 0).then(() => 0).then(() => 0).then(() => 0).then(() => 0).then(() => 0).then(() => 0)
            .then(() => {{ var actual = log.join(' | '); if (actual === expected.join(' | ')) $DONE(); else $DONE(new Error('expected [' + expected.join(' | ') + '] but got [' + actual + ']')); }}); }}
         {script}"
    );
    vm.execute_script(&compile(&parse(&source).unwrap()).unwrap())
        .unwrap();
    vm.run_promise_jobs().unwrap();
    vm.take_test262_done()
        .expect("the script never reported through $DONE")
        .unwrap_or_else(|error| {
            let message = match &error {
                blueice_bluejs::Value::Object(object) => vm
                    .heap()
                    .get(*object, "message")
                    .ok()
                    .and_then(|value| match value {
                        blueice_bluejs::Value::String(text) => text.to_utf8().ok(),
                        _ => None,
                    })
                    .unwrap_or_default(),
                _ => String::new(),
            };
            panic!("{script}\n=> {message}")
        });
}

fn run_both(script: &str) {
    run(script, None);
    run(script, Some(1));
}

#[test]
fn a_return_awaits_only_when_it_has_an_operand() {
    run_both(
        "async function* g1() {}
         async function* g2() { return; }
         async function* g3() { return undefined; }
         async function* g4() { return void 0; }
         ticks(2);
         g1().next().then(() => log.push('g1 ret'));
         g2().next().then(() => log.push('g2 ret'));
         g3().next().then(() => log.push('g3 ret'));
         g4().next().then(() => log.push('g4 ret'));
         expect(['tick 1', 'g1 ret', 'g2 ret', 'tick 2', 'g3 ret', 'g4 ret']);",
    );
}

#[test]
fn a_plain_yield_awaits_its_operand_once_and_delivers_without_another_await() {
    run_both(
        "async function* g() { yield 1; }
         async function* h() { yield; }
         ticks(3);
         g().next().then(() => log.push('g'));
         h().next().then(() => log.push('h'));
         expect(['tick 1', 'tick 2', 'g', 'h', 'tick 3']);",
    );
}

#[test]
fn a_rejected_yield_operand_is_thrown_into_the_generator_body() {
    run_both(
        "async function* g() { try { yield Promise.reject('e'); } catch (x) { yield 'caught:' + x; } }
         g().next().then(v => log.push(v.value + ' ' + v.done), e => log.push('rejected ' + e));
         expect(['caught:e false']);",
    );
    run_both(
        "async function* g() { yield Promise.reject('e'); }
         g().next().then(v => log.push('fulfilled'), e => log.push('rejected ' + e));
         expect(['rejected e']);",
    );
}

#[test]
fn a_rejected_return_operand_is_thrown_into_the_generator_body() {
    run_both(
        "async function* g() { try { return Promise.reject('e'); } catch (x) { return 'caught:' + x; } }
         g().next().then(v => log.push(v.value + ' ' + v.done), e => log.push('rejected ' + e));
         expect(['caught:e true']);",
    );
    run_both(
        "async function* g() { try { return Promise.reject('e'); } finally { log.push('finally'); } }
         g().next().then(v => log.push('fulfilled'), e => log.push('rejected ' + e));
         expect(['finally', 'rejected e']);",
    );
}

#[test]
fn a_return_request_awaits_its_operand_before_the_body_sees_it() {
    // return(p) at a yield: the operand is awaited first, and a rejection is
    // thrown at the yield where the body can catch it.
    run_both(
        "async function* g() { try { yield 1; } catch (x) { log.push('caught ' + x); return 'recovered'; } }
         var it = g();
         it.next().then(() => it.return(Promise.reject('r'))).then(v => log.push(v.value + ' ' + v.done));
         expect(['caught r', 'recovered true']);",
    );
    run_both(
        "async function* g() { try { yield 1; } finally { log.push('finally'); } }
         var it = g();
         it.next().then(() => it.return(Promise.resolve('resolved'))).then(v => log.push(v.value + ' ' + v.done));
         expect(['finally', 'resolved true']);",
    );
}

#[test]
fn a_return_request_to_a_suspended_generator_takes_one_await_before_the_unwind() {
    run_both(
        "async function* g() { try { yield 1; } finally { log.push('finally'); } }
         var it = g();
         it.next().then(() => {
           ticks(2);
           it.return('x').then(v => log.push('returned ' + v.value));
         });
         expect(['tick 1', 'finally', 'tick 2', 'returned x']);",
    );
}

#[test]
fn a_return_request_to_a_generator_that_has_not_started_awaits_once() {
    run_both(
        "async function* g() { log.push('body'); }
         ticks(2);
         g().return('x').then(v => log.push('returned ' + v.value + ' ' + v.done));
         expect(['tick 1', 'tick 2', 'returned x true']);",
    );
    run_both(
        "async function* g() {}
         var it = g();
         it.next().then(() => it.return(Promise.reject('r'))).then(() => log.push('fulfilled'), e => log.push('rejected ' + e));
         expect(['rejected r']);",
    );
}

#[test]
fn yield_star_return_forwards_the_awaited_operand_and_awaits_again_without_a_return_method() {
    // The return operand is awaited when the request arrives (its `then` is
    // read), the delegate's `return` is looked up after that, and when there is
    // none the operand is awaited a second time.
    run_both(
        "var asyncIter = { [Symbol.asyncIterator]() { return this; },
           next() { return { done: false }; },
           get return() { log.push('get return'); } };
         async function* f() { log.push('start'); yield* asyncIter; log.push('never'); }
         ticks(3);
         var it = f();
         it.next();
         it.return({ get then() { log.push('get then'); } });
         expect(['start', 'tick 1', 'get then', 'tick 2', 'get return', 'get then', 'tick 3']);",
    );
}

#[test]
fn yield_star_delegates_receive_the_awaited_return_operand() {
    run_both(
        "var received;
         var inner = { [Symbol.asyncIterator]() { return this; },
           next() { return { value: 1, done: false }; },
           return(v) { received = v; return { value: 'done:' + v, done: true }; } };
         async function* g() { yield* inner; }
         var it = g();
         it.next().then(() => it.return(Promise.resolve('arg'))).then(r => log.push(received + ' ' + r.value + ' ' + r.done));
         expect(['arg done:arg true']);",
    );
}

#[test]
fn a_yield_star_return_with_a_rejecting_operand_is_forwarded_as_a_throw() {
    run_both(
        "var thrown;
         var inner = { [Symbol.asyncIterator]() { return this; },
           next() { return { value: 1, done: false }; },
           throw(e) { thrown = e; return { value: 'handled', done: true }; } };
         async function* g() { var r = yield* inner; return 'after ' + r; }
         var it = g();
         it.next().then(() => it.return(Promise.reject('boom'))).then(r => log.push(thrown + ' ' + r.value + ' ' + r.done));
         expect(['boom after handled true']);",
    );
}

#[test]
fn queued_requests_wait_while_a_return_operand_is_awaited() {
    run_both(
        "async function* g() { try { yield 1; } finally { log.push('finally'); } yield 'unreachable'; }
         var it = g();
         it.next().then(() => {
           it.return('r').then(v => log.push('return ' + v.value + ' ' + v.done));
           it.next().then(v => log.push('next ' + v.value + ' ' + v.done));
         });
         expect(['finally', 'return r true', 'next undefined true']);",
    );
}

#[test]
fn a_throw_request_needs_no_await() {
    run_both(
        "async function* g() { try { yield 1; } catch (x) { log.push('caught ' + x); yield 'after'; } }
         var it = g();
         it.next().then(() => it.throw('t')).then(v => log.push(v.value));
         expect(['caught t', 'after']);",
    );
}
