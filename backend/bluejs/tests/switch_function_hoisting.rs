// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! BlockDeclarationInstantiation of a CaseBlock: a function declared in any
//! case clause is initialized when the switch is entered, so an earlier (or
//! the same) clause can call it whichever case is selected.
use blueice_bluejs::{compile, parse, Value, Vm};

fn evaluate(source: &str) -> Value {
    let program = parse(source).unwrap_or_else(|e| panic!("{source}: {e:?}"));
    Vm::default()
        .execute_script(&compile(&program).unwrap())
        .unwrap_or_else(|e| panic!("{source}: {e:?}"))
}

#[test]
fn a_function_of_a_later_case_exists_in_an_earlier_one() {
    assert_eq!(
        evaluate(
            "var r = [];
             switch (1) { case 1: r.push(typeof f); case 2: function f() { return 42; } }
             r.push(typeof f);
             r.join()"
        ),
        Value::String("function,function".into())
    );
}

#[test]
fn a_function_of_an_unselected_case_and_the_default_clause_exist_too() {
    assert_eq!(
        evaluate(
            "function t(x) {
               var r = [];
               switch (x) {
                 case 1: function one() { return 'one'; }
                 default: r.push(typeof one, typeof two);
                 case 3: function two() { return 'two'; }
               }
               return r.join();
             }
             t(5) + '|' + t(1)"
        ),
        Value::String("function,function|function,function".into())
    );
}

#[test]
fn switch_functions_stay_scoped_to_the_case_block() {
    assert_eq!(
        evaluate(
            "'use strict';
             function t() { switch (1) { case 1: function inner() { return 1; } } return typeof inner; }
             t()"
        ),
        Value::String("undefined".into())
    );
}
