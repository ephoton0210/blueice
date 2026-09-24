// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The last error paths of the built-in runtime: exits that need a specific
//! throwing hook, a module namespace in its temporal dead zone, an uncatchable
//! error escaping a callback, or a limit closing in on one exact allocation.
mod cov_g2_support;
mod cov_g2_sweep;

use blueice_bluejs::{compile, parse, RuntimeError, Value, Vm, VmConfig};
use cov_g2_support::{expect_async_true, expect_true};
use cov_g2_sweep::{sweep_cold_by, sweep_native, sweep_ops_by, sweep_string_limit, Mode};
use std::collections::HashMap;

fn thrown(text: &str) -> Result<Value, RuntimeError> {
    Err(RuntimeError::Thrown(Value::String(text.into())))
}

#[test]
fn closing_a_flat_map_helper_closes_its_active_inner_iterator() {
    expect_true(
        "var closed = 0;
         var inner = { [Symbol.iterator]() { return {
             next() { return { value: 1, done: false }; },
             return() { closed++; return {}; } }; } };
         var it = [1].values().flatMap(() => inner);
         it.next();
         it.return();
         closed === 1",
    );
}

#[test]
fn a_thenable_check_that_throws_on_the_last_result_of_a_sync_iterator_rejects() {
    // The result is `done`, so the iterator is not closed; the error from
    // reading the value's `constructor` simply rejects the loop.
    expect_async_true(
        "var p = Promise.resolve(1);
         Object.defineProperty(p, 'constructor', { get() { throw 'ctor'; } });
         var source = { [Symbol.iterator]() { return { next() { return { done: true, value: p }; } }; } };
         (async () => { try { for await (var x of source) {} } catch (e) { return e; } })()
             .then(v => { globalThis.result = v === 'ctor'; });",
    );
}

#[test]
fn breaking_out_of_a_loop_reports_a_failing_return_lookup() {
    expect_true(
        "var r = [];
         var loop = (ret) => { try { for (var x of { [Symbol.iterator]() { return {
             next() { return { value: 1, done: false }; }, get return() { return ret(); } }; } }) break; }
             catch (e) { r.push(e instanceof TypeError ? 'type' : e); } };
         loop(() => { throw 'getter'; });
         loop(() => 5);
         r.join() === 'getter,type'",
    );
}

#[test]
fn the_iterator_prototype_setters_reject_a_namespace_whose_export_is_uninitialized() {
    let main = "import * as ns from './main.js';
        var outcome;
        try {
            Object.getOwnPropertyDescriptor(Iterator.prototype, 'constructor').set.call(ns, 1);
            outcome = 'no error';
        } catch (e) { outcome = e.name; }
        export let constructor = 1;
        outcome";
    let modules: HashMap<String, _> = [(
        "main.js".to_string(),
        blueice_bluejs::compile_module(&blueice_bluejs::parse_module(main).unwrap()).unwrap(),
    )]
    .into();
    assert_eq!(
        Vm::default().execute_module_graph("main.js", &modules),
        Ok(Value::String("ReferenceError".into()))
    );
}

#[test]
fn an_uninitialized_or_missing_result_length_rejects_array_from_async() {
    expect_async_true(
        "var Target = function () { return Object.defineProperty({}, 'length', { value: 0, writable: false }); };
         var empty = { [Symbol.asyncIterator]() { return { next() { return Promise.resolve({ done: true }); } }; } };
         Array.fromAsync.call(Target, empty).catch(e => { globalThis.result = e instanceof TypeError; });",
    );
}

#[test]
fn set_intersection_with_a_smaller_set_like_reads_its_keys() {
    expect_true(
        "var other = { size: 1, has(x) { return x === 1; }, keys() { return [1].values(); } };
         var r = new Set([1, 2, 3]).intersection(other);
         r.size === 1 && r.has(1)",
    );
}

#[test]
fn number_formatting_propagates_a_throwing_value() {
    expect_true(
        "var nf = new Intl.NumberFormat('en');
         var attempt = f => { try { f(); } catch (e) { return e; } };
         attempt(() => nf.formatToParts({ valueOf() { throw 'v'; } })) === 'v'
             && attempt(() => nf.format({ valueOf() { throw 'w'; } })) === 'w'",
    );
}

#[test]
fn a_numeric_conversion_propagates_a_throwing_primitive_hook() {
    expect_true(
        "var attempt = f => { try { f(); } catch (e) { return e; } };
         var bad = { valueOf() { throw 'v'; } };
         attempt(() => -bad) === 'v' && attempt(() => +bad) === 'v' && attempt(() => ~bad) === 'v'
             && attempt(() => { var x = bad; x++; }) === 'v' && attempt(() => { var x = bad; x--; }) === 'v'
             && attempt(() => 1 - bad) === 'v' && attempt(() => bad ** 2) === 'v'",
    );
}

/// Runs `script` and its promise jobs under an instruction budget that a
/// `for (;;)` in a callback exhausts, so the callback fails with the
/// uncatchable instruction-limit error.
fn exhausts_the_budget(script: &str) {
    let mut vm = Vm::new(VmConfig {
        instruction_budget: 400_000,
        ..VmConfig::default()
    })
    .unwrap();
    let code = compile(&parse(script).unwrap()).unwrap();
    let result = vm
        .execute_script(&code)
        .and_then(|_| vm.run_promise_jobs().map(|()| Value::Undefined));
    assert_eq!(result, Err(RuntimeError::InstructionLimit), "{script}");
}

#[test]
fn an_uncatchable_error_in_an_array_from_async_callback_ends_the_run() {
    exhausts_the_budget("Array.fromAsync([1], x => { for (;;) {} });");
    exhausts_the_budget("Array.fromAsync({ length: 1, 0: 1 }, x => { for (;;) {} });");
    exhausts_the_budget(
        "var Target = function () { return new Proxy({}, { defineProperty() { for (;;) {} } }); };
         Array.fromAsync.call(Target, [1]);",
    );
    exhausts_the_budget(
        "var Target = function () { return new Proxy({}, { defineProperty() { for (;;) {} } }); };
         Array.fromAsync.call(Target, { length: 1, 0: 1 });",
    );
}

#[test]
fn a_new_target_whose_prototype_lookup_throws_stops_the_object_constructor() {
    expect_true(
        "var target = new Proxy(function () {}, { get(t, k) { if (k === 'prototype') throw 1; return t[k]; } });
         try { Reflect.construct(Object, [], target); false } catch (e) { e === 1 }",
    );
}

#[test]
fn test262_assertions_stop_at_a_failing_json_lookup() {
    for (setup, expected) in [
        (
            "Object.defineProperty(globalThis, 'JSON', { get() { throw 'getter'; }, configurable: true });",
            "getter",
        ),
        ("globalThis.JSON = { get stringify() { throw 'stringify'; } };", "stringify"),
    ] {
        let mut vm = Vm::default();
        vm.install_test262_harness().unwrap();
        let script = format!("{setup} assert.sameValue('ab', 'c');");
        let result = vm.execute(&compile(&parse(&script).unwrap()).unwrap());
        assert_eq!(result, thrown(expected), "{script}");
    }
}

#[test]
fn compare_array_stops_at_a_length_whose_conversion_throws() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    let script = "var length = { valueOf() { throw 'length'; } };
                  assert.compareArray({ length }, { length });";
    assert_eq!(
        vm.execute(&compile(&parse(script).unwrap()).unwrap()),
        thrown("length")
    );
}

#[test]
fn test262_messages_that_outgrow_the_string_limit_fail_at_every_concatenation() {
    for script in [
        "assert.throws(TypeError, function () { throw 1; }, 'mmm');",
        "assert.compareArray([1], [2], 'mm');",
        "assert.compareArray([1, 2, 3], [4, 5, 6], 'mm');",
        "assert.compareArray(1, [], 'mm');",
        "assert.compareArray([], 'a', 'mm');",
        "assert.sameValue('ab', 'c', 'mm');",
        "globalThis.JSON = undefined; assert.sameValue('ab', 'c', 'mm');",
        "assert.sameValue(1n, 2n, 'mm');",
    ] {
        sweep_string_limit(Mode::HARNESS, script, 1, 1, |result| {
            matches!(result, Err(RuntimeError::Test262(_)))
        });
    }
}

#[test]
fn a_test262_error_with_a_lone_surrogate_message_is_built_on_a_cold_heap() {
    sweep_cold_by(
        Mode::HARNESS,
        "assert.sameValue(1, 2, '\\ud800');",
        16,
        |result| result.is_err(),
    );
}

#[test]
fn case_mapping_stops_where_a_lone_surrogate_would_outgrow_the_string_limit() {
    for script in [
        // The pending run maps to more than the whole input.
        "'\\u00df\\u00df\\ud800'.toUpperCase()",
        // The run fits; the surrogate after it does not.
        "'\\u00df\\u0061\\ud800'.toUpperCase()",
        "'\\u00df\\u0061\\ud800'.toLocaleUpperCase('en')",
    ] {
        sweep_string_limit(Mode::PLAIN, script, 1, 1, |result| {
            matches!(result, Ok(Value::String(_)))
        });
    }
}

#[test]
fn installing_the_test262_done_hook_on_a_cold_heap() {
    sweep_native(8, |vm| vm.install_test262_done());
}

fn is_true(result: &Result<Value, RuntimeError>) -> bool {
    *result == Ok(Value::Bool(true))
}

#[test]
fn allocations_after_a_user_hook_in_generators_and_async_functions() {
    sweep_ops_by(
        Mode::JOBS,
        "(function* () {})(); (async function () { await 1; })(); (async function* () {})();",
        "b(); var g = (function* (a, b, c, d, e) { yield a; })(1, 2, 3, 4, 5);
         b(); var ag = (async function* (a, b, c, d, e) { yield a; })(1, 2, 3, 4, 5);
         b(); var p1 = (async function (a, b, c) { await a; return b; })(1, 2, 3);
         b(); var p2 = (async function () { await 1; await 2; })();
         b(); var p3 = (async function () { for await (var x of [1]) {} })();
         return true;",
        1,
        is_true,
    );
}

#[test]
fn allocations_while_assigning_to_primitives_globals_and_classes() {
    sweep_ops_by(
        Mode::PLAIN,
        "'x'.foo = 1; (1).foo = 1; true.foo = 1; globalThis.gv = 'a'; class Warm { x = 1; } var s = { ...'ab' };",
        "b(); 'str'.foo = 1; b(); (5).bar = 1; b(); true.baz = 1;
         b(); globalThis.gv = 'a longer value' + keep.length;
         b(); class C1 { x = 1; static y = 2; }
         b(); class C2 extends C1 {}
         b(); var o1 = { ...'abc' }; b(); var { a, ...rest } = 'xyz';
         return true;",
        1,
        is_true,
    );
}
