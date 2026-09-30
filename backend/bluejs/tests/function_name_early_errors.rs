// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The BindingIdentifier of a function declaration or expression obeys the
//! same early errors as any other binding: ReservedWords are never valid, the
//! strict-mode reserved words are invalid in strict code (a Use Strict
//! Directive in the function's own body makes its name strict too), and the
//! `yield`/`await` contexts follow the declaration/expression parameters.
//! `let` is an ordinary identifier in sloppy code, including as a label.

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
        .execute(&compile(&program).unwrap())
        .unwrap_or_else(|e| panic!("{source}: {e:?}"))
}

const RESERVED: &[&str] = &["class", "enum", "export", "extends", "import", "super"];
const STRICT_RESERVED: &[&str] = &[
    "implements",
    "interface",
    "let",
    "package",
    "private",
    "protected",
    "public",
    "static",
    "yield",
];

#[test]
fn a_reserved_word_is_never_a_function_name() {
    for word in RESERVED {
        for source in [
            format!("function {word}() {{}}"),
            format!("(function {word}() {{}});"),
            format!("(function* {word}() {{}});"),
            format!("async function {word}() {{}}"),
            format!("(async function {word}() {{}});"),
            format!("'use strict'; function {word}() {{}}"),
        ] {
            assert!(is_rejected(&source), "{source}");
        }
    }
}

#[test]
fn a_strict_reserved_word_names_a_sloppy_function_but_not_a_strict_one() {
    for word in STRICT_RESERVED {
        for source in [
            format!("function {word}() {{}}"),
            format!("(function {word}() {{}});"),
        ] {
            assert!(!is_rejected(&source), "{source}");
        }
        for source in [
            // Strict through the enclosing code.
            format!("'use strict'; function {word}() {{}}"),
            format!("'use strict'; (function {word}() {{}});"),
            // Strict through the function's own Use Strict Directive: the
            // BindingIdentifier is part of the strict function code.
            format!("function {word}() {{ 'use strict'; }}"),
            format!("(function {word}() {{ 'use strict'; }});"),
        ] {
            assert!(is_rejected(&source), "{source}");
        }
    }
}

#[test]
fn let_is_an_ordinary_identifier_in_sloppy_code() {
    assert_eq!(
        evaluate(
            "var log = [];
             let: log.push('label');
             var o = { let: 1 };
             function let() { return 'function'; }
             log.push(let());
             log.push((function let() { return typeof let; })());
             var obj = {}; ({ let: obj.a } = { let: 7 });
             log.push(obj.a);
             log.join()"
        ),
        Value::String("label,function,function,7".into())
    );
    // A shorthand `{ let }` assignment pattern reads the property `let`.
    assert_eq!(
        evaluate("var let; ({ let } = { let: 5 }); let"),
        Value::Number(5.0)
    );
    for source in [
        "'use strict'; let: 1;",
        "'use strict'; ({ let } = {});",
        "function f() { 'use strict'; let: 1; }",
    ] {
        assert!(is_rejected(source), "{source}");
    }
}

#[test]
fn yield_and_await_function_names_follow_declaration_and_expression_contexts() {
    for source in [
        // A declaration's name is parsed with the enclosing [Yield]/[Await].
        "function* g() { function yield() {} }",
        "function* g() { function* yield() {} }",
        "async function f() { function await() {} }",
        // An expression's name uses its own function kind.
        "(function* yield() {});",
        "function* g() { (function* yield() {}); }",
        "(async function await() {});",
    ] {
        assert!(is_rejected(source), "{source}");
    }
    for source in [
        "function yield() {}",
        "function* g() { (function yield() {}); }",
        "function f() { (function* g() { yield 1; }); }",
        "async function f() { (function await() {}); }",
    ] {
        assert!(!is_rejected(source), "{source}");
    }
}
