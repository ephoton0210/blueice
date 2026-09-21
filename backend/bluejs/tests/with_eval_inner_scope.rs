// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A direct eval inside a function that was created within a `with`
//! statement sees the function's own bindings before the with object: the
//! function's scope is nested inside the object environment.
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
fn eval_in_a_function_created_in_with_prefers_the_functions_locals() {
    check(&[
        "globalThis.a = 9; \
         function directWith(obj, s) { \
           var f; with (obj) { f = function () { var a = 1; return eval(s); }; } return f(); } \
         directWith(this, 'a+1') === 2 && directWith({a: -1000}, 'a+1') === 2",
        // A parameter of the function shadows the with object too.
        "function make(obj) { var f; with (obj) { f = function (p) { return eval('p'); }; } return f; } \
         make({p: 'object'})('param') === 'param'",
    ]);
}

#[test]
fn eval_still_reads_the_with_object_for_names_the_function_does_not_declare() {
    check(&[
        "function make(obj) { var f; with (obj) { f = function () { return eval('q'); }; } return f; } \
         make({q: 'object'})() === 'object'",
        // A with statement entered inside the function still shadows the
        // bindings declared before it.
        "function f() { var v = 'local'; with ({v: 'object'}) { return eval('v'); } } f() === 'object'",
    ]);
}
