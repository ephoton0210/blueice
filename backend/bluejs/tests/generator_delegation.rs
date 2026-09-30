// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `%GeneratorFunction.prototype%` / `%GeneratorPrototype%` wiring and the
//! synchronous `yield*` delegation protocol, through the real
//! parse/compile/execute path. Every script also runs under a one-object
//! nursery, where each allocation may collect.
use blueice_bluejs::{compile, parse, Value, Vm, VmConfig};

fn evaluate(source: &str, nursery_capacity: Option<usize>) -> Result<Value, String> {
    let mut config = VmConfig::default();
    if let Some(capacity) = nursery_capacity {
        config.heap.nursery_capacity = capacity;
    }
    Vm::new(config)
        .unwrap()
        .execute(&compile(&parse(source).unwrap()).unwrap())
        .map_err(|error| format!("{error:?}"))
}

fn truthy(source: &str) {
    assert_eq!(
        evaluate(source, None),
        Ok(Value::Bool(true)),
        "ordinary mode: {source}"
    );
    assert_eq!(
        evaluate(source, Some(1)),
        Ok(Value::Bool(true)),
        "GC-stress mode: {source}"
    );
}

#[test]
fn generator_function_prototype_and_generator_prototype_reference_each_other() {
    truthy(
        "function* g() {}\
         var GeneratorFunction = Object.getPrototypeOf(g);\
         var GeneratorPrototype = GeneratorFunction.prototype;\
         var link = Object.getOwnPropertyDescriptor(GeneratorFunction, 'prototype');\
         var back = Object.getOwnPropertyDescriptor(GeneratorPrototype, 'constructor');\
         GeneratorPrototype === Object.getPrototypeOf(g.prototype)\
           && back.value === GeneratorFunction\
           && !link.writable && !link.enumerable && link.configurable\
           && !back.writable && !back.enumerable && back.configurable",
    );
}

#[test]
fn generator_prototype_methods_are_reachable_through_the_generator_function_prototype() {
    truthy(
        "var GeneratorPrototype = Object.getPrototypeOf(function* () {}).prototype;\
         var next = Object.getOwnPropertyDescriptor(GeneratorPrototype, 'next');\
         next.value.name === 'next' && next.value.length === 1\
           && !next.enumerable && next.writable && next.configurable\
           && GeneratorPrototype.return.length === 1 && GeneratorPrototype.throw.length === 1",
    );
}

#[test]
fn a_running_generator_rejects_reentrant_next_and_is_then_completed() {
    truthy(
        "var iter, caught = [];\
         function* g() { try { iter.next(); } catch (e) { caught.push(e instanceof TypeError); throw e; } }\
         iter = g();\
         var threw = false;\
         try { iter.next(); } catch (e) { threw = e instanceof TypeError; }\
         var after = iter.next();\
         threw && caught[0] === true && after.done === true && after.value === undefined",
    );
}

#[test]
fn yield_star_yields_the_delegates_own_result_object() {
    truthy(
        "var results = [{value: 1}, {value: 8}, {value: 34, done: true}], index = 0;\
         var delegate = {[Symbol.iterator]() { return this; },\
                         next() { return results[index++]; }};\
         function* g() { var last = yield* delegate; return last; }\
         var it = g();\
         var first = it.next(), second = it.next(), final = it.next();\
         first === results[0] && second === results[1] && first.done === undefined\
           && final.value === 34 && final.done === true",
    );
}

#[test]
fn yield_star_does_not_read_the_value_of_an_unfinished_result() {
    truthy(
        "var reads = 0;\
         var result = Object.defineProperty({done: false}, 'value', {get() { reads++; }});\
         var delegate = {[Symbol.iterator]() { return this; }, next() { return result; }};\
         function* g() { yield* delegate; }\
         var it = g();\
         it.next(); it.next();\
         var beforeDone = reads;\
         result.done = true;\
         it.next();\
         beforeDone === 0 && reads === 1",
    );
}

#[test]
fn yield_star_passes_throw_and_return_results_through_unchanged() {
    truthy(
        "var thrown = {value: 'thrown'}, returned = {value: 'returned', done: false};\
         var delegate = {[Symbol.iterator]() { return this; },\
                         next() { return {value: 1, done: false}; },\
                         throw() { return thrown; },\
                         return() { return returned; }};\
         function* g() { yield* delegate; }\
         var it = g();\
         it.next();\
         it.throw('x') === thrown && it.return('y') === returned",
    );
}

#[test]
fn a_finished_return_result_completes_the_generator_with_its_value() {
    truthy(
        "var delegate = {[Symbol.iterator]() { return this; },\
                         next() { return {value: 1, done: false}; },\
                         return(v) { return {value: 'inner:' + v, done: true}; }};\
         var cleaned = false;\
         function* g() { try { yield* delegate; } finally { cleaned = true; } }\
         var it = g();\
         it.next();\
         var result = it.return('r');\
         result.value === 'inner:r' && result.done === true && cleaned",
    );
}

#[test]
fn a_delegate_without_a_throw_method_is_closed_before_a_type_error_is_thrown_inside() {
    truthy(
        "var log = [];\
         var delegate = {[Symbol.iterator]() { return this; },\
                         next() { return {value: 1, done: false}; },\
                         return() { log.push('return'); return {}; }};\
         var caught;\
         function* g() { try { yield* delegate; } catch (e) { caught = e; } return 'after'; }\
         var it = g();\
         it.next();\
         var result = it.throw('x');\
         log.join() === 'return' && caught instanceof TypeError\
           && result.value === 'after' && result.done === true",
    );
}

#[test]
fn a_failing_close_of_a_delegate_without_throw_replaces_the_type_error() {
    truthy(
        "var delegate = {[Symbol.iterator]() { return this; },\
                         next() { return {value: 1, done: false}; },\
                         return() { throw 87; }};\
         var caught;\
         function* g() { try { yield* delegate; } catch (e) { caught = e; } }\
         var it = g();\
         it.next();\
         it.throw('x');\
         caught === 87",
    );
}

#[test]
fn abrupt_delegate_protocol_steps_are_thrown_inside_the_generator() {
    truthy(
        "function delegateWith(members) {\
           var iterator = Object.defineProperties(\
             {next() { return {value: 1, done: false}; }}, members);\
           return {[Symbol.iterator]() { return iterator; }};\
         }\
         function run(members, request) {\
           var caught;\
           function* g() { try { yield* delegateWith(members); } catch (e) { caught = e; } return 'done'; }\
           var it = g();\
           it.next();\
           var result = request(it);\
           return [caught, result.value, result.done];\
         }\
         var boom = {};\
         var doReturn = function(it) { return it.return('v'); };\
         var doThrow = function(it) { return it.throw('v'); };\
         var a = run({return: {get() { throw boom; }}}, doReturn);\
         var b = run({return: {value() { throw boom; }}}, doReturn);\
         var c = run({return: {value() { return 5; }}}, doReturn);\
         var d = run({throw: {value() { throw boom; }}}, doThrow);\
         var e = run({throw: {value() { return 5; }}}, doThrow);\
         var f = run({throw: {get() { throw boom; }}}, doThrow);\
         a[0] === boom && a[1] === 'done' && a[2] === true\
           && b[0] === boom && c[0] instanceof TypeError\
           && d[0] === boom && e[0] instanceof TypeError && e[2] === true && f[0] === boom",
    );
}

#[test]
fn a_delegate_without_return_lets_a_generator_return_complete_normally() {
    truthy(
        "var reads = 0;\
         var delegate = {[Symbol.iterator]() { return this; },\
                         next() { return {value: 1, done: false}; },\
                         get return() { reads++; return undefined; }};\
         function* g() { yield* delegate; }\
         var it = g();\
         it.next();\
         var result = it.return('r');\
         result.value === 'r' && result.done === true && reads === 1",
    );
}
