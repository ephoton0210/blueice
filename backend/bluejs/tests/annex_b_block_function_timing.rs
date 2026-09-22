// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Annex B.3.2: a block-level function is initialized in the block when the
//! block is entered, but the value is copied to the legacy outer var only when
//! the declaration itself is evaluated, into the nearest variable environment
//! (never a `with` object that shadows the name).
use blueice_bluejs::{compile, parse, Value, Vm};

fn evaluate(source: &str) -> Value {
    let program = parse(source).unwrap_or_else(|e| panic!("{source}: {e:?}"));
    Vm::default()
        .execute_script(&compile(&program).unwrap())
        .unwrap_or_else(|e| panic!("{source}: {e:?}"))
}

#[test]
fn the_outer_var_keeps_its_old_value_until_the_declaration_is_evaluated() {
    assert_eq!(
        evaluate(
            "var seen = [];
             { seen.push(typeof f, typeof Object.getOwnPropertyDescriptor(this, 'f').value);
               function f() {}
               seen.push(typeof Object.getOwnPropertyDescriptor(this, 'f').value); }
             seen.join()"
        ),
        Value::String("function,undefined,function".into())
    );
    assert_eq!(
        evaluate(
            "var seen = [];
             function fn() {
               var outerBefore = typeof g;
               { function g() {} }
               return outerBefore + ',' + typeof g;
             }
             fn()"
        ),
        Value::String("undefined,function".into())
    );
}

#[test]
fn a_reassignment_before_the_declaration_is_what_gets_copied() {
    assert_eq!(
        evaluate("{ f = 1; function f() {} } typeof f"),
        Value::String("number".into())
    );
}

#[test]
fn a_with_object_holding_the_name_is_left_alone() {
    assert_eq!(
        evaluate(
            "var o = { f: 'string-f' };
             with (o) {
               var desc = Object.getOwnPropertyDescriptor(this, 'f');
               var before = desc.value === undefined && desc.writable && desc.enumerable && !desc.configurable;
               function f() { return 'fun-f'; }
             }
             before && o.f === 'string-f' && f() === 'fun-f'"
        ),
        Value::Bool(true)
    );
}
