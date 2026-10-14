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

#[test]
fn a_call_in_a_try_block_is_not_a_tail_call() {
    // §15.10.2: a TryBlock is not a tail position (its catch/finally must see
    // the call's outcome), nor is the Block of a catch that has a finally.
    for (source, expected) in [
        (
            "(function self(n){ 'use strict'; if (n<0) throw 'neg'; try { if (n===0) return self(-1); return 1; } catch (e) { return 'c'; } })(0)",
            "c",
        ),
        (
            "(function self(n){ 'use strict'; if (n<0) throw 'neg'; var log = ''; try { try { if (n===0) return self(-1); } finally { log += 'f'; } } catch (e) { return 'c' + log; } })(0)",
            "cf",
        ),
        (
            "var log = ''; (function self(n){ 'use strict'; if (n<0) throw 'neg'; try { throw 0; } catch (e) { return self(-1); } finally { log += 'f'; } })(0)",
            "",
        ),
    ] {
        let result = std::panic::catch_unwind(|| {
            let mut vm = Vm::default();
            vm.execute(&compile(&parse(source).unwrap()).unwrap())
        });
        match (expected, result.unwrap()) {
            ("", Err(_)) => {}
            (text, Ok(Value::String(value))) => {
                assert_eq!(value.to_utf8().unwrap(), text, "{source}")
            }
            (text, other) => panic!("{source}: expected {text:?}, got {other:?}"),
        }
    }
}

fn run(source: &str) -> Value {
    evaluate(source)
}

#[test]
fn a_call_to_any_callee_in_tail_position_reuses_the_frame() {
    for (name, source) in [
        (
            "computed callee",
            "var calls = 0; (function f(n) { 'use strict'; if (n === 0) { calls += 1; return; } function getF() { return f; } return getF()(n - 1); }(5000)); calls",
        ),
        (
            "function declaration",
            "'use strict'; var calls = 0; function f(n) { if (n === 0) { calls += 1; return; } return f(n - 1); } f(5000); calls",
        ),
        (
            "method call",
            "'use strict'; var calls = 0; var o = { f(n) { if (n === 0) { calls += 1; return; } return this.f(n - 1); } }; o.f(5000); calls",
        ),
        (
            "mutual recursion",
            "'use strict'; var calls = 0; function even(n) { return n === 0 ? (calls += 1, true) : odd(n - 1); } function odd(n) { return n === 0 ? false : even(n - 1); } even(5000); calls",
        ),
        (
            "arrow function",
            "'use strict'; var calls = 0; var f = n => n === 0 ? (calls += 1, 0) : f(n - 1); f(5000); calls",
        ),
        (
            "tagged template",
            "'use strict'; var calls = 0; function f(_, n) { if (n === 0) { calls += 1; return; } return f`${n - 1}`; } f(null, 5000); calls",
        ),
        (
            "tagged template through a call",
            "'use strict'; var calls = 0; function getF() { return f; } function f(_, n) { if (n === 0) { calls += 1; return; } return getF()`${n - 1}`; } f(null, 5000); calls",
        ),
        (
            "an identifier named eval that is not the intrinsic",
            "var calls = 0; (function() { function f(n) { 'use strict'; if (n === 0) { calls += 1; return; } return eval(n - 1); } var eval = f; f(5000); }()); calls",
        ),
        (
            "an eval property found through with",
            "var calls = 0; var f, scope = {}; with (scope) { f = function (n) { 'use strict'; if (n === 0) { calls += 1; return; } return eval(n - 1); }; } scope.eval = f; f(5000); calls",
        ),
    ] {
        assert_eq!(run(source), Value::Number(1.0), "{name}");
    }
}

#[test]
fn tail_calls_keep_the_calls_semantics() {
    for (name, source, expected) in [
        (
            "the result of the callee is the result",
            "'use strict'; function g() { return 7; } function f() { return g(); } f()",
            Value::Number(7.0),
        ),
        (
            "this is the receiver of the tail call",
            "'use strict'; var o = { v: 3, m() { return this.v; }, f() { return this.m(); } }; o.f()",
            Value::Number(3.0),
        ),
        (
            "arguments are evaluated before the call",
            "'use strict'; var log = []; function g(a, b) { return log.join() + a + b; } function f() { return g(log.push('a'), log.push('b')); } f()",
            Value::String("a,b12".into()),
        ),
        (
            "a native callee",
            "'use strict'; function f(a, b) { return Math.max(a, b); } f(2, 9)",
            Value::Number(9.0),
        ),
        (
            "a bound function callee",
            "'use strict'; function g(a, b) { return this.x + a + b; } function f() { return g.bind({ x: 1 }, 2)(3); } f()",
            Value::Number(6.0),
        ),
        (
            "a direct eval is not a tail call and sees the callers scope",
            "function f() { 'use strict'; var a = 5; return eval('a + 1'); } f()",
            Value::Number(6.0),
        ),
        (
            "a direct eval in an arrow returns its value",
            "var g = () => { 'use strict'; return eval('3 * 4'); }; g()",
            Value::Number(12.0),
        ),
        (
            "a constructor still returns the instance for a primitive result",
            "'use strict'; function h() { return 1; } function C() { this.v = 4; return h(); } new C().v",
            Value::Number(4.0),
        ),
        (
            "a constructor returns an object result",
            "'use strict'; function h() { return { w: 5 }; } function C() { return h(); } new C().w",
            Value::Number(5.0),
        ),
        (
            "new.target in the callee is undefined for a tail call from a constructor",
            "'use strict'; var seen = 'unset'; function h() { seen = new.target; return {}; } function C() { return h(); } new C(); String(seen)",
            Value::String("undefined".into()),
        ),
        (
            "an arrow keeps its own new.target",
            "'use strict'; function C() { this.a = () => new.target; } var c = new C(); c.a() === C",
            Value::Bool(true),
        ),
        (
            "an uncallable value throws in the callers frame",
            "'use strict'; function f() { return undefined(); } var r; try { f(); } catch (e) { r = e instanceof TypeError; } r",
            Value::Bool(true),
        ),
        (
            "a class constructor callee throws",
            "'use strict'; class K {} function f() { return K(); } var r; try { f(); } catch (e) { r = e instanceof TypeError; } r",
            Value::Bool(true),
        ),
        (
            "an exception from the callee propagates",
            "'use strict'; function g() { throw 'boom'; } function f() { return g(); } var r; try { f(); } catch (e) { r = e; } r",
            Value::String("boom".into()),
        ),
        (
            "the callee sees its own frame, not the callers",
            "'use strict'; function g() { return typeof local; } function f() { var local = 1; return g(); } f()",
            Value::String("undefined".into()),
        ),
        (
            "closures created before the tail call keep their captures",
            "'use strict'; var keep; function g() { return 0; } function f() { var x = 41; keep = () => x + 1; return g(); } f(); keep()",
            Value::Number(42.0),
        ),
        (
            "a call in a sloppy function is not a tail call",
            "function g() { return 1; } function f() { return g(); } f()",
            Value::Number(1.0),
        ),
    ] {
        assert_eq!(run(source), expected, "{name}");
    }
}

#[test]
fn tail_calls_from_finally_and_catch_blocks_reuse_the_frame() {
    for tail in [
        "(function() { try { throw 0; } catch (e) { return f(n - 1); } })()",
        "(function() { try { } finally { return f(n - 1); } })()",
    ] {
        assert_eq!(recursion(tail), Value::Number(1.0), "{tail}");
    }
}

#[test]
fn a_tail_call_inside_a_generator_or_async_function_is_an_ordinary_call() {
    assert_eq!(
        run("'use strict'; function g() { return 5; } function* gen() { return g(); } gen().next().value"),
        Value::Number(5.0)
    );
    assert_eq!(
        run("'use strict'; function g() { return 5; } var r; (async function () { return g(); })().then(v => { r = v; }); r === undefined ? 'pending' : r"),
        Value::String("pending".into())
    );
}

#[test]
fn a_call_in_an_optional_chain_is_an_ordinary_call() {
    assert_eq!(
        run("'use strict'; function g() { return 4; } var o = { g }; function f(o) { return o?.g(); } f(o) + (f(null) === undefined ? 10 : 0)"),
        Value::Number(14.0)
    );
    assert_eq!(
        run("'use strict'; var o = { m() { return 6; } }; function f(o) { return (o?.m)(); } f(o)"),
        Value::Number(6.0)
    );
}
