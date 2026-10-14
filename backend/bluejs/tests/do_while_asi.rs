// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! ECMA-262 §12.10.1: a semicolon is inserted after the `)` that closes a
//! `do`-`while` statement even when no line terminator follows it.

use blueice_bluejs::{compile, parse, Value, Vm};

fn evaluate(source: &str) -> Value {
    Vm::default()
        .execute(&compile(&parse(source).unwrap()).unwrap())
        .unwrap()
}

#[test]
fn do_while_needs_no_terminator_before_the_next_statement_on_the_same_line() {
    assert_eq!(
        evaluate("var x; do break ; while (0) x = 42; x"),
        Value::Number(42.0)
    );
    assert_eq!(
        evaluate("var x = 0; do do do ; while (x) while (x) while (x) x = 39; x"),
        Value::Number(39.0)
    );
    assert_eq!(
        evaluate("var n = 0; do n++; while (n < 3); n"),
        Value::Number(3.0)
    );
}
