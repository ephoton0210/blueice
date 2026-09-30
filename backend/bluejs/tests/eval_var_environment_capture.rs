// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A `var` declared by a sloppy direct eval lives in the variable environment
//! of the function that called eval. Every closure created there captures that
//! environment, whenever it runs and whichever way it was created, and a
//! function defined outside it never sees the variable, even when called from
//! inside.
use blueice_bluejs::{compile, parse, Value, Vm};

fn evaluate(source: &str) -> Value {
    let program = parse(source).unwrap_or_else(|e| panic!("{source}: {e:?}"));
    let code = compile(&program).unwrap_or_else(|e| panic!("{source}: {e:?}"));
    Vm::default()
        .execute_script(&code)
        .unwrap_or_else(|e| panic!("{source}: {e:?}"))
}

fn text(value: &str) -> Value {
    Value::String(value.into())
}

#[test]
fn a_returned_closure_sees_the_eval_var_of_its_creator_not_the_global() {
    assert_eq!(
        evaluate(
            "function f(s) { eval(s); return function (a) { return b; }; }
             var b = 1;
             var g1 = f('');
             var g2 = f('var b = 2;');
             [g1(0), g2(0), g1(0), g2(0)].join()"
        ),
        text("1,2,1,2")
    );
    assert_eq!(
        evaluate(
            "function f(s) { eval(s); return function (a) { with ({}) {} eval(a); return b; }; }
             var b = 1;
             var g1 = f('');
             var g2 = f('var b = 2;');
             [g1(''), g2(''), g1('var b = 3'), g2('')].join()"
        ),
        text("1,2,3,2")
    );
    assert_eq!(
        evaluate(
            "function f(s) {
               eval(s);
               return function (a) { var d; { let c = 3; d = function () { a; }; with ({}) {} return b; } };
             }
             var b = 1;
             var g1 = f('');
             var g2 = f('var b = 2;');
             [g1(0), g2(0)].join()"
        ),
        text("1,2")
    );
}

#[test]
fn a_closure_created_before_the_eval_sees_its_var() {
    assert_eq!(
        evaluate(
            "var y = 42;
             function f() { var g = function () { return y; }; var before = g(); eval('var y = 5'); return [before, g(), y].join(); }
             f()"
        ),
        text("42,5,5")
    );
}

#[test]
fn a_function_defined_outside_never_sees_the_eval_var() {
    assert_eq!(
        evaluate(
            "var y = 42;
             function outer() { return y; }
             function test() { eval('var y = 5'); return [y, outer()].join(); }
             test() + '|' + y"
        ),
        text("5,42|42")
    );
}

#[test]
fn eval_vars_of_generators_and_arrows_are_captured_too() {
    assert_eq!(
        evaluate(
            "function* g() { eval('var q = 1'); var h = function () { return q; }; yield q; yield h(); q = 3; yield h(); }
             var it = g(); [it.next().value, it.next().value, it.next().value].join()"
        ),
        text("1,1,3")
    );
    assert_eq!(
        evaluate(
            "var w = 'global';
             function f() { var a = () => { eval('var w = 1'); return () => w; }; return [a()(), w].join(); }
             f()"
        ),
        text("1,global")
    );
}

#[test]
fn a_function_deletes_its_own_eval_var() {
    assert_eq!(
        evaluate(
            "var z = 'outer';
             function f() { eval('var z = 1'); var log = [z]; log.push(delete z); log.push(z); return log.join(); }
             f()"
        ),
        text("1,true,outer")
    );
}

#[test]
fn simple_cases_keep_working() {
    assert_eq!(
        evaluate(
            "function f(a) { eval('var a = 3; var b = 4'); return [a, b, arguments[0], typeof c].join(); }
             f(1)"
        ),
        text("3,4,3,undefined")
    );
    assert_eq!(
        evaluate(
            "function f() { eval('function inner() { return 7; }'); return inner(); }
             f() + '|' + typeof inner"
        ),
        text("7|undefined")
    );
}
