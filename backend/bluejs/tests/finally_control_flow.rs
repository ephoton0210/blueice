// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `break`/`continue` whose target is inside the `finally` block that is
//! running for a pending abrupt completion must not leave that block.
use blueice_bluejs::{compile, parse, Value, Vm};

fn evaluate(source: &str) -> Value {
    Vm::default()
        .execute(&compile(&parse(source).unwrap()).unwrap())
        .unwrap()
}

#[test]
fn loop_inside_finally_keeps_the_pending_return() {
    assert_eq!(
        evaluate("function f(){ try { return 1 } finally { while (true) { break; } } } f()"),
        Value::Number(1.0)
    );
    assert_eq!(
        evaluate(
            "function g(){ try { return 2 } finally { \
             for (var i = 0; i < 3; i++) { if (i == 1) continue; } } } g()"
        ),
        Value::Number(2.0)
    );
}

#[test]
fn loop_inside_finally_keeps_the_pending_throw() {
    assert_eq!(
        evaluate(
            "function f(){ try { try { throw 7 } finally { do { break; } while (0); } } \
             catch (e) { return e } } f()"
        ),
        Value::Number(7.0)
    );
}

#[test]
fn nested_finally_may_break_out_of_a_loop_inside_the_outer_finally() {
    // The inner finalizer's `break` completes normally: the outer pending
    // return survives and the inner pending return is discarded.
    assert_eq!(
        evaluate(
            "function f(){ try { return 42 } finally { \
             do try { return 43 } finally { break } while (0) } } f()"
        ),
        Value::Number(42.0)
    );
    assert_eq!(
        evaluate(
            "function f(){ try { return 42 } finally { \
             do try { return 43 } finally { continue } while (0) } } f()"
        ),
        Value::Number(42.0)
    );
}

#[test]
fn nested_finally_may_break_a_label_inside_the_outer_finally() {
    assert_eq!(
        evaluate(
            "function f(){ try { return 42 } finally { \
             L: try { return 43 } finally { break L } } } f()"
        ),
        Value::Number(42.0)
    );
    assert_eq!(
        evaluate(
            "function f(){ try { return 41 } finally { try { return 42 } finally { \
             do try { return 43 } finally { break } while (0) } } } f()"
        ),
        Value::Number(42.0)
    );
}

#[test]
fn break_out_of_finally_still_replaces_the_pending_completion() {
    assert_eq!(
        evaluate(
            "function f(){ do try { return 42 } finally { break } while (false); return 43 } f()"
        ),
        Value::Number(43.0)
    );
    assert_eq!(
        evaluate("function f(){ L: try { return 42 } finally { break L } return 43 } f()"),
        Value::Number(43.0)
    );
}
