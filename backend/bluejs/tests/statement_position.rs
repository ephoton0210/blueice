// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Statement versus declaration position: the body of an `if`, loop, `with`
//! or label is a `Statement`, so it cannot be a class, function, generator or
//! async declaration, and `let` followed by a line break there is an
//! identifier. Also the `for` head grammar around `in` and stray commas.
use blueice_bluejs::{compile, parse, Value, Vm};

/// The source is rejected before running: a parse or a compile error.
fn is_rejected(source: &str) -> bool {
    match parse(source) {
        Err(_) => true,
        Ok(program) => compile(&program).is_err(),
    }
}

fn evaluate_throws(source: &str) -> bool {
    Vm::default()
        .execute(&compile(&parse(source).unwrap()).unwrap())
        .is_err()
}

fn evaluate(source: &str) -> Value {
    Vm::default()
        .execute(&compile(&parse(source).unwrap()).unwrap())
        .unwrap()
}

#[test]
fn a_class_declaration_is_not_a_statement_body() {
    for statement in [
        "for (var x of []) class C {}",
        "for (var x in {}) class C {}",
        "for (;;) class C {}",
        "while (false) class C {}",
        "do class C {} while (false)",
        "if (true) class C {}",
        "if (false) ; else class C {}",
        "with ({}) class C {}",
        "label: class C {}",
    ] {
        assert!(is_rejected(statement), "{statement}");
    }
}

#[test]
fn function_declarations_are_not_statement_bodies_except_sloppy_if() {
    for statement in [
        "for (var x of []) function f() {}",
        "for (var x of []) async function f() {}",
        "for (var x of []) function* f() {}",
        "while (false) function f() {}",
        "do function f() {} while (false)",
        "with ({}) function f() {}",
        "if (true) async function f() {}",
        "if (true) function* f() {}",
        "'use strict'; if (true) function f() {}",
        "for (var x of []) label: function f() {}",
        "for (const x of []) label1: label2: function f() {}",
        "if (true) label: function f() {}",
    ] {
        assert!(is_rejected(statement), "{statement}");
    }
    assert!(!is_rejected("if (true) function f() {}"));
    assert!(!is_rejected("label: function f() {}"));
}

#[test]
fn let_followed_by_a_line_break_in_a_statement_body_is_an_identifier() {
    for statement in [
        "for (var x of []) let\n{}",
        "for (var x in {}) let\n{}",
        "if (true) let\n{}",
        "while (false) let\n{}",
        "do let\nwhile (false)",
    ] {
        assert!(!is_rejected(statement), "{statement}");
    }
    assert!(is_rejected("for (var x of []) let\n[a] = [1]"));
    assert!(is_rejected("for (var x of []) let y = 1"));
    // The body is just the identifier `let`; the block after it is a separate
    // statement that runs once, after the loop.
    assert_eq!(
        evaluate("globalThis.let = 3; var n = 0; for (var x of [1, 2]) let\n{ n++; } n"),
        Value::Number(1.0)
    );
    assert!(evaluate_throws("for (var x of [1]) let\n{}"));
}

#[test]
fn a_for_of_head_takes_one_assignment_expression() {
    for head in ["let x of [], []", "x of [], []", "var x of [], []"] {
        let source = format!("for ({head}) {{}}");
        let error = parse(&source).expect_err(&source);
        assert!(error.known_syntax, "{source}: {error:?}");
    }
    let error = parse("for (let of []) ;").expect_err("`let of` head");
    assert!(error.known_syntax, "{error:?}");
}

#[test]
fn in_is_allowed_inside_a_destructuring_assignment_default_of_a_for_head() {
    assert_eq!(
        evaluate("var x, n = 0; for ([x = 'k' in {}] of [[]]) { n += x === false ? 1 : 0; } n"),
        Value::Number(1.0)
    );
    assert_eq!(
        evaluate("var x, n = 0; for ({x = 'k' in {}} of [{}]) { n += x === false ? 1 : 0; } n"),
        Value::Number(1.0)
    );
    assert_eq!(
        evaluate("var x, n = 0; for ({y: x = 'k' in {}} of [{}]) { n += x === false ? 1 : 0; } n"),
        Value::Number(1.0)
    );
}

#[test]
fn a_labelled_function_declaration_binds_its_name_as_a_var() {
    assert_eq!(
        evaluate("label: function f() { return 7; } f()"),
        Value::Number(7.0)
    );
    assert_eq!(
        evaluate("function outer() { a: b: function g() { return 9; } return g(); } outer()"),
        Value::Number(9.0)
    );
}

#[test]
fn a_lexical_for_head_name_cannot_be_redeclared_with_var_in_the_body() {
    for statement in [
        "for (const x of []) { var x; }",
        "for (let x of []) { var x; }",
        "for (let x in {}) { var x; }",
        "for (let x = 0;;) { var x; }",
        "for (let [x] of []) { { var x; } }",
    ] {
        assert!(is_rejected(statement), "{statement}");
    }
    for statement in [
        "for (var x of []) { var x; }",
        "for (let x of []) { let y; { let x; } }",
        "for (let x of []) { function f() { var x; } }",
    ] {
        assert!(!is_rejected(statement), "{statement}");
    }
}

#[test]
fn the_of_keyword_cannot_be_escaped_and_a_for_of_head_cannot_start_with_async_of() {
    let error = parse("for (var x o\\u0066 []) ;").expect_err("escaped of");
    assert!(error.known_syntax, "{error:?}");
    let error = parse("for (async of []) ;").expect_err("async of");
    assert!(error.known_syntax, "{error:?}");
    assert!(parse("for (async of => {}; false;) ;").is_ok());
    assert!(parse("async function f() { for await (async of []) ; }").is_ok());
    assert!(parse("for (var async of []) ;").is_ok());
    // An escaped `async` is an ordinary identifier, so the lookahead does not apply.
    assert!(parse("for (\\u0061sync of []) ;").is_ok());
}
