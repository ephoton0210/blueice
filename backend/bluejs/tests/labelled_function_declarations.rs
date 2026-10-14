// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Annex B.3.2: a label on a function declaration in a statement list changes
//! nothing about the declaration. It is hoisted and scoped exactly like the
//! unlabelled form (block-scoped in a block or switch case, with the legacy
//! outer var), and it is an early error in strict code.
use blueice_bluejs::{compile, parse, Value, Vm};

fn is_rejected(source: &str) -> bool {
    match parse(source) {
        Err(error) => {
            assert!(error.known_syntax, "{source:?}: {error:?}");
            true
        }
        Ok(program) => compile(&program).is_err(),
    }
}

fn evaluate(source: &str) -> Value {
    let program = parse(source).unwrap_or_else(|e| panic!("{source}: {e:?}"));
    Vm::default()
        .execute_script(&compile(&program).unwrap())
        .unwrap_or_else(|e| panic!("{source}: {e:?}"))
}

#[test]
fn a_labelled_function_in_a_block_is_hoisted_like_an_unlabelled_one() {
    for source in [
        "{ var early = f(); l: function f() { return 42; } } early === 42 && f() === 42",
        "{ var early = f(); l0: l: function f() { return 42; } } early === 42 && f() === 42",
        "switch (1) { case 1: var early = f(); case 2: l: function f() { return 42; } } early === 42 && f() === 42",
        "eval('{ var early = fe(); l: function fe() { return 42; } }'); fe() === 42",
    ] {
        assert_eq!(evaluate(source), Value::Bool(true), "{source}");
    }
}

#[test]
fn the_legacy_outer_var_is_assigned_where_the_labelled_declaration_stands() {
    assert_eq!(
        evaluate(
            "var seen = [];
             { seen.push(typeof globalThis.g); l: function g() {} seen.push(typeof globalThis.g); }
             seen.join()"
        ),
        Value::String("undefined,function".into())
    );
}

#[test]
fn a_labelled_function_may_repeat_the_name_of_a_block_function_in_sloppy_code() {
    assert_eq!(
        evaluate(
            "{ function f() { return 3; } l: function f() { return 4; } var inside = f(); }
             inside === 4 && f() === 4"
        ),
        Value::Bool(true)
    );
    assert!(is_rejected("{ let x = 1; l: function x() {} }"));
}

#[test]
fn a_labelled_function_at_the_top_level_is_still_a_var() {
    assert_eq!(
        evaluate("label: function f() { return 7; } f()"),
        Value::Number(7.0)
    );
    assert_eq!(
        evaluate("(function () { early: function g() { return 1; } return g(); })()"),
        Value::Number(1.0)
    );
}

#[test]
fn a_labelled_function_is_rejected_in_strict_code_and_in_statement_position() {
    for source in [
        "'use strict'; l: function f() {}",
        "'use strict'; { l: function f() {} }",
        "function g() { 'use strict'; l: function f() {} }",
        "if (1) l: function f() {}",
        "do l: function f() {} while (0)",
        "for (;;) l: function f() {}",
        "l: function* f() {}",
        "{ l: async function f() {} }",
    ] {
        assert!(is_rejected(source), "{source}");
    }
}
