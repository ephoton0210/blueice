// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Tail positions (§15.10.2): in strict code a call is a tail call when it is
//! the value of a `return` directly, or through the branches of `?:`, the
//! right operand of `&&`, `||` and `??`, the last operand of a comma
//! expression, or parentheses. A recursion of thousands of calls (far beyond the call-depth limit) through any of
//! these must not exhaust the call stack, and the returned values must be
//! unchanged.

use blueice_bluejs::{compile, parse, Value, Vm, VmConfig};

fn evaluate(source: &str) -> Value {
    let mut vm = Vm::new(VmConfig {
        instruction_budget: 20_000_000,
        ..VmConfig::default()
    })
    .unwrap();
    vm.execute(&compile(&parse(source).unwrap()).unwrap())
        .unwrap()
}

fn recursion(tail: &str) -> Value {
    evaluate(&format!(
        "var calls = 0;
         (function f(n) {{
           'use strict';
           if (n === 0) {{ calls += 1; return; }}
           return {tail};
         }}(5000));
         calls"
    ))
}

#[test]
fn a_tail_call_through_an_operator_position_reuses_the_frame() {
    for tail in [
        "f(n - 1)",
        "(f(n - 1))",
        "true ? f(n - 1) : 0",
        "false ? 0 : f(n - 1)",
        "true && f(n - 1)",
        "false || f(n - 1)",
        "null ?? f(n - 1)",
        "undefined ?? f(n - 1)",
        "0, f(n - 1)",
        "(0, 1, f(n - 1))",
        "true ? (false ? 0 : f(n - 1)) : 0",
        "true && (false || f(n - 1))",
    ] {
        assert_eq!(recursion(tail), Value::Number(1.0), "{tail}");
    }
}

#[test]
fn values_returned_through_those_positions_are_unchanged() {
    for (source, expected) in [
        ("(function() { 'use strict'; return true ? 1 : 2; })()", 1.0),
        ("(function() { 'use strict'; return false ? 1 : 2; })()", 2.0),
        ("(function() { 'use strict'; return 0 || 5; })()", 5.0),
        ("(function() { 'use strict'; return 3 && 4; })()", 4.0),
        ("(function() { 'use strict'; return null ?? 6; })()", 6.0),
        ("(function() { 'use strict'; return 7 ?? 6; })()", 7.0),
        ("(function() { 'use strict'; return 0, 1, 8; })()", 8.0),
        (
            "(function f(n) { 'use strict'; return n === 0 ? 10 : f(n - 1) + 1; })(5)",
            15.0,
        ),
        (
            "(function f(n, acc) { 'use strict'; return n === 0 ? acc : f(n - 1, acc + n); })(1000, 0)",
            500500.0,
        ),
    ] {
        assert_eq!(evaluate(source), Value::Number(expected), "{source}");
    }
}

#[test]
fn a_short_circuited_left_operand_is_returned_as_is() {
    assert_eq!(
        evaluate("(function f(n) { 'use strict'; return n === 0 ? 10 : n > 100 && f(n - 1); })(5)"),
        Value::Bool(false)
    );
    assert_eq!(
        evaluate("(function f(n) { 'use strict'; return n && f(n - 1); })(3)"),
        Value::Number(0.0)
    );
    assert_eq!(
        evaluate("(function f(n) { 'use strict'; return n || f(n + 1); })(0)"),
        Value::Number(1.0)
    );
    assert_eq!(
        evaluate("(function f(n) { 'use strict'; return n ?? f(1); })(null)"),
        Value::Number(1.0)
    );
}

#[test]
fn a_call_that_is_not_in_tail_position_still_nests() {
    // `f(n - 1) + 1` and `!f(n - 1)` are not tail calls; a deep one exceeds
    // the call-depth limit rather than being silently rewritten.
    let mut vm = Vm::default();
    let result = vm.execute(
        &compile(
            &parse("(function f(n) { 'use strict'; return n === 0 ? 0 : f(n - 1) + 1; })(5000)")
                .unwrap(),
        )
        .unwrap(),
    );
    assert!(result.is_err());
}
