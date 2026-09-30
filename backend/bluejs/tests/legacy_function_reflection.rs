// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Legacy function reflection (`f.caller`, `f.arguments`) on sloppy ordinary
//! functions, restricted as in the es-legacy-function-reflection proposal: a
//! caller that is strict, a generator or async yields `null`, and eval frames
//! are transparent.
use blueice_bluejs::{compile, parse, Value, Vm};

fn check(sources: &[&str]) {
    for source in sources {
        let value = Vm::default()
            .execute(&compile(&parse(source).unwrap()).unwrap())
            .unwrap_or_else(|error| panic!("{source}: {error:?}"));
        assert_eq!(value, Value::Bool(true), "{source}");
    }
}

#[test]
fn caller_is_the_calling_sloppy_function_or_null() {
    check(&[
        // Called from the top level: no calling function.
        "function f() { return f.caller; } f() === null",
        "function f() { return f.caller; } function g() { return f(); } g() === g",
        // Only while the function is running.
        "function f() {} function g() { f(); } g(); f.caller === null",
        "function f() { return arguments.callee.caller; } function g() { return f(); } g() === g",
        // A recursive call is its own caller.
        "function f(n) { return n ? f(n - 1) : f.caller; } f(2) === f",
        // Reflection through a method-like property.
        "var o = { f: function () { return o.g(); }, g: function () { return arguments.callee.caller; } }; \
         o.f() === o.f",
    ]);
}

#[test]
fn a_strict_generator_or_async_caller_is_censored() {
    check(&[
        "function f() { return f.caller; } function strict() { 'use strict'; return f(); } strict() === null",
        "function f() { return f.caller; } function* g() { yield f(); } g().next().value === null",
        "var seen; function f() { seen = f.caller; } \
         (async function () { f(); })(); seen === null",
    ]);
}

#[test]
fn eval_frames_are_transparent() {
    check(&[
        "function inner() { return arguments.callee.caller; } \
         function nest() { return eval('inner();'); } \
         function nest2() { return nest(); } nest2() === nest",
        "function inner() { return eval('arguments.callee.caller'); } \
         function nest() { return eval('eval(\"inner();\")'); } nest() === nest",
    ]);
}

#[test]
fn caller_and_arguments_cannot_be_assigned() {
    check(&[
        "function f() {} f.caller = 1; f.arguments = 2; f.caller === null && f.arguments === null",
        "function f() {} try { (function () { 'use strict'; f.caller = 1; })(); false } \
         catch (e) { e instanceof TypeError }",
    ]);
}

#[test]
fn arguments_is_the_running_functions_own_arguments_or_null() {
    check(&[
        "function foo() { return foo.arguments; } foo.arguments === null \
           && foo(5, undefined).length === 2 && foo(5, undefined)[0] === 5 \
           && foo(5, undefined)[1] === undefined && foo.arguments === null",
        "function foo() { return foo.arguments.length; } foo() === 0 && foo(1, 2, 3) === 3",
        "function foo(a) { a = 9; return foo.arguments[0]; } typeof foo(1) === 'number'",
    ]);
}

#[test]
fn strict_functions_keep_their_restricted_properties() {
    check(&[
        "function f() { 'use strict'; } \
         [function () { return f.caller; }, function () { return f.arguments; }].every(function (read) { \
           try { read(); return false; } catch (e) { return e instanceof TypeError; } })",
        "var d = Object.getOwnPropertyDescriptor(function () {}, 'caller'); \
         d.enumerable === false && d.configurable === false",
    ]);
}
