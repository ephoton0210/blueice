// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `Array.fromAsync`, driven through the real parse/compile/execute path and
//! the Promise job queue.
use blueice_bluejs::{compile, parse, Vm, VmConfig};

const PRELUDE: &str = "
    var trace = [];
    function finish(ok) { if (ok) $DONE(); else $DONE(new Error(trace.join(' | '))); }
    function done(promise, check) {
        promise.then(v => finish(check(v)), e => (trace.push('rejected: ' + e), finish(false)));
    }
    function failed(promise, check) {
        promise.then(v => (trace.push('resolved: ' + v), finish(false)), e => finish(check(e)));
    }
";

fn run_with(config: VmConfig, script: &str) -> Result<(), String> {
    let mut vm = Vm::new(config).unwrap();
    vm.install_test262_done().unwrap();
    let source = format!("{PRELUDE}{script}");
    vm.execute_script(&compile(&parse(&source).unwrap()).unwrap())
        .unwrap();
    vm.run_promise_jobs().unwrap();
    vm.take_test262_done()
        .expect("the script never reported through $DONE")
        .map_err(|error| format!("{error:?}"))
}

fn run(script: &str) {
    if let Err(error) = run_with(VmConfig::default(), script) {
        panic!("{script}\n=> {error}");
    }
}

fn run_gc_stress(script: &str) {
    let mut config = VmConfig::default();
    config.heap.nursery_capacity = 1;
    if let Err(error) = run_with(config, script) {
        panic!("(GC stress) {script}\n=> {error}");
    }
}

#[test]
fn it_returns_a_promise_and_reports_bad_input_as_a_rejection() {
    run("var p = Array.fromAsync([1]); finish(p instanceof Promise && Object.getPrototypeOf(p) === Promise.prototype);");
    run(
        "var p; try { p = Array.fromAsync(null); } catch (e) { trace.push('threw'); }\
         failed(p, e => e instanceof TypeError);",
    );
    run("failed(Array.fromAsync(undefined), e => e instanceof TypeError);");
    run("failed(Array.fromAsync([], 5), e => e instanceof TypeError);");
    run("failed(Array.fromAsync([], {}), e => e instanceof TypeError);");
}

#[test]
fn it_collects_a_sync_iterable_awaiting_each_value() {
    run("done(Array.fromAsync([1, Promise.resolve(2), 3]), a =>\
         Array.isArray(a) && a.join() === '1,2,3' && a.length === 3);");
}

#[test]
fn it_collects_an_async_iterable() {
    run("var it = { [Symbol.asyncIterator]() { var n = 0; return {\
           next() { n++; return Promise.resolve(n <= 3 ? { done: false, value: n * 10 } : { done: true }); } }; } };\
         done(Array.fromAsync(it), a => a.join() === '10,20,30');");
}

#[test]
fn it_collects_an_array_like_awaiting_each_element() {
    run(
        "done(Array.fromAsync({ length: 3, 0: 'a', 1: Promise.resolve('b'), 2: 'c' }), a =>\
         a.join() === 'a,b,c');",
    );
    run("done(Array.fromAsync({ length: 2 }), a => a.length === 2 && a[0] === undefined);");
    run("done(Array.fromAsync('ab'), a => a.join() === 'a,b');");
    run("done(Array.fromAsync(5), a => Array.isArray(a) && a.length === 0);");
}

#[test]
fn it_applies_and_awaits_the_mapper() {
    run("done(Array.fromAsync([1, 2], x => x * 2), a => a.join() === '2,4');");
    run("done(Array.fromAsync([1, 2], async x => x + 1), a => a.join() === '2,3');");
    run("done(Array.fromAsync({ length: 2, 0: 5, 1: 6 }, (x, i) => Promise.resolve(x + i)), a => a.join() === '5,7');");
    run("var self = {}; done(Array.fromAsync([1], function () { return this === self; }, self), a => a[0] === true);");
}

#[test]
fn a_throwing_mapper_rejects_and_closes_an_async_iterator_once() {
    run(
        "var closed = 0; var it = { [Symbol.asyncIterator]() { return {\
           next() { return { done: false, value: 1 }; },\
           return() { closed++; return Promise.resolve({}); } }; } };\
         var boom = new Error('boom');\
         failed(Array.fromAsync(it, () => { throw boom; }), e => e === boom && closed === 1);",
    );
    run(
        "var closed = 0; var it = { [Symbol.iterator]() { return {\
           next() { return { done: false, value: 1 }; },\
           return() { closed++; return {}; } }; } };\
         var boom = new Error('boom');\
         failed(Array.fromAsync(it, () => Promise.reject(boom)), e => e === boom && closed === 1);",
    );
}

#[test]
fn an_iterator_failure_rejects_without_closing() {
    run(
        "var closed = 0; var it = { [Symbol.asyncIterator]() { return {\
           next() { return Promise.reject(new RangeError('n')); },\
           return() { closed++; return {}; } }; } };\
         failed(Array.fromAsync(it), e => e instanceof RangeError && closed === 0);",
    );
    run(
        "var it = { [Symbol.asyncIterator]() { return { next() { return 1; } }; } };\
         failed(Array.fromAsync(it), e => e instanceof TypeError);",
    );
}

#[test]
fn a_custom_constructor_builds_the_result() {
    run("function C() { this.made = true; }\
         done(Array.fromAsync.call(C, [7, 8]), a => a instanceof C && a.made && a.length === 2 && a[1] === 8);");
    run("var seen; function C(n) { seen = n; }\
         done(Array.fromAsync.call(C, { length: 2, 0: 1, 1: 2 }), a => seen === 2 && a instanceof C && a.length === 2);");
    run("done(Array.fromAsync.call({}, [1]), a => Array.isArray(a));");
    run("function C() { Object.freeze(this); }\
         failed(Array.fromAsync.call(C, [1]), e => e instanceof TypeError);");
}

#[test]
fn it_reads_the_iterator_methods_in_specification_order() {
    run("var log = []; var it = { get [Symbol.asyncIterator]() { log.push('async'); return undefined; },\
           get [Symbol.iterator]() { log.push('sync'); return function () { log.push('call');\
             return { next() { return { done: true }; } }; }; } };\
         function C() { log.push('construct'); }\
         done(Array.fromAsync.call(C, it), () => log.join() === 'async,sync,call,construct');");
}

#[test]
fn it_exposes_standard_function_metadata() {
    run("var d = Object.getOwnPropertyDescriptor(Array, 'fromAsync');\
         var threw = false; try { new Array.fromAsync([]); } catch (e) { threw = e instanceof TypeError; }\
         finish(d.writable && !d.enumerable && d.configurable && d.value.name === 'fromAsync'\
           && d.value.length === 1 && !('prototype' in d.value) && threw);");
}

#[test]
fn results_survive_collection_on_every_allocation() {
    run_gc_stress(
        "var it = { [Symbol.asyncIterator]() { var n = 0; return {\
           next() { n++; return Promise.resolve(n <= 40 ? { done: false, value: { n } } : { done: true }); } }; } };\
         done(Array.fromAsync(it, o => ({ m: o.n })), a => a.length === 40 && a[0].m === 1 && a[39].m === 40);",
    );
    run_gc_stress(
        "done(Array.fromAsync({ length: 30, 0: 1 }, (x, i) => ({ i })), a => a.length === 30 && a[29].i === 29);",
    );
}

#[test]
fn from_async_over_sync_iterables_survives_collection_on_every_allocation() {
    run_gc_stress("done(Array.fromAsync([Promise.resolve(1), 2, 3]), a => a.join() === '1,2,3');");
    run_gc_stress(
        "var it = { [Symbol.iterator]() { return { next() { return { done: false, value: 1 }; } }; } };\
         failed(Array.fromAsync(it, () => { throw new RangeError('x'); }), e => e instanceof RangeError);",
    );
    run_gc_stress(
        "function* g() { throw new RangeError('gen'); }\
         failed(Array.fromAsync(g()), e => e instanceof RangeError && e.message === 'gen');",
    );
}
