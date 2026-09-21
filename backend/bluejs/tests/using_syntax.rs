// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Grammar-level behavior of `using`/`await using` declarations and the
//! contextual keywords (`let`, `await`) their Test262 syntax fixtures probe.

use blueice_bluejs::{compile, parse, Value, Vm};

fn evaluate(source: &str) -> Value {
    let mut vm = Vm::default();
    vm.execute(&compile(&parse(source).unwrap()).unwrap())
        .unwrap()
}

/// A rejection the adapter may count against a negative test: the parser
/// must know the source is invalid, not merely fail to support it.
fn assert_known_syntax_error(source: &str) {
    let error = parse(source).expect_err(source);
    assert!(error.known_syntax, "{source}: {error:?}");
}

#[test]
fn using_does_not_take_a_binding_pattern() {
    for source in [
        "{ using [] = null; }",
        "{ using {} = null; }",
        "{ using {a} = null; }",
        "async function f() { await using [] = null; }",
        "async function f() { await using {} = null; }",
    ] {
        assert_known_syntax_error(source);
    }
}

#[test]
fn using_followed_by_a_bracket_stays_a_member_expression() {
    assert_eq!(
        evaluate("var using = [7]; var a = 0; using [a] = 3; using[0]"),
        Value::Number(3.0)
    );
}

#[test]
fn expressions_cannot_start_with_a_closing_token() {
    for source in [
        "x = ];", "x = );", "x = };", "x = , 1;", "x = ;", "x = : 1;", "x =", "let =",
    ] {
        assert_known_syntax_error(source);
    }
}

#[test]
fn let_is_an_ordinary_identifier_in_sloppy_code() {
    assert_eq!(
        evaluate("var let; let = 5; let"),
        Value::Number(5.0),
        "var let, then assignment to the identifier"
    );
    assert_eq!(
        evaluate("var using, let; { using\nlet = 'x'; typeof let }"),
        Value::String("string".into())
    );
    assert_eq!(evaluate("var let = 1; let + 1"), Value::Number(2.0));
}

#[test]
fn let_stays_reserved_where_the_grammar_reserves_it() {
    for source in [
        "'use strict'; var let;",
        "let let = 1;",
        "const let = 1;",
        "'use strict'; let = 1;",
        "for (let let of []);",
    ] {
        assert_known_syntax_error(source);
    }
}

#[test]
fn function_declaration_names_follow_the_enclosing_await_context() {
    // At script top level `await` is an ordinary identifier, so an async
    // function declaration may be named `await`.
    assert_eq!(
        evaluate("async function await() { return 1 } await instanceof Function"),
        Value::Bool(true)
    );
    assert_eq!(
        evaluate("function await() { return 2 } await()"),
        Value::Number(2.0)
    );
    // An async function *expression* binds its own name with `[+Await]`.
    assert_known_syntax_error("(async function await() {})");
    // Inside an async function, module or class static block the enclosing
    // context reserves `await`, for both flavors of declaration.
    for source in [
        "async function foo() { function await() {} }",
        "async function foo() { async function await() {} }",
        "class C { static { function await() {} } }",
        "class C { static { async function await() {} } }",
    ] {
        assert_known_syntax_error(source);
    }
    // A nested non-async function may still bind `await` in its own scope.
    assert!(parse("async function foo() { (function await() {}); }").is_ok());
    assert!(parse("function outer() { async function await() {} }").is_ok());
}

#[test]
fn await_cannot_be_bound_inside_a_class_static_block() {
    for source in [
        "class C { static { using await = null; } }",
        "class C { static { let await; } }",
        "class C { static { const await = 1; } }",
        "class C { static { var await; } }",
    ] {
        assert_known_syntax_error(source);
    }
    // A nested ordinary function is a boundary the restriction does not cross.
    assert!(parse("class C { static { function f() { let await; } } }").is_ok());
}
