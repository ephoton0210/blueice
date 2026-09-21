// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Grammar-level early errors: arrow-function line-terminator restrictions,
//! `yield` outside an AssignmentExpression position, contextual keywords
//! spelled with escapes, unique method parameters and untagged template
//! escapes.

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

fn assert_all_rejected(sources: &[&str]) {
    for source in sources {
        assert!(is_rejected(source), "{source}");
    }
}

fn assert_all_accepted(sources: &[&str]) {
    for source in sources {
        assert!(!is_rejected(source), "{source}");
    }
}

fn evaluate(source: &str) -> Value {
    Vm::default()
        .execute(&compile(&parse(source).unwrap()).unwrap())
        .unwrap()
}

#[test]
fn no_line_terminator_may_precede_an_arrow() {
    assert_all_rejected(&[
        "var af = ()\n=> {};",
        "var af = x\n=> {};",
        "var af = x\n=> x;",
        "var af = (x, y)\n=> x;",
        "var af = async ()\n=> {};",
        "var af = async x\n=> x;",
    ]);
    assert_all_accepted(&[
        "var af = () =>\n{};",
        "var af = x =>\nx;",
        "var af = (\nx\n) => x;",
        "var af = async (\n) => {};",
    ]);
}

#[test]
fn yield_is_not_an_operand_of_a_binary_or_unary_operator() {
    assert_all_rejected(&[
        "function* g() { yield 3 + yield 4; }",
        "function* g() { yield 3 * yield 4; }",
        "function* g() { 1 + yield; }",
        "function* g() { !yield 1; }",
        "function* g() { typeof yield; }",
        "function* g() { (yield) + yield; }",
        "var g = function*() { yield 3 + yield 4; };",
        "({ *g() { yield 3 + yield 4; } });",
        "class A { *g() { yield 3 + yield 4; } }",
    ]);
    assert_all_accepted(&[
        "function* g() { yield yield 1; }",
        "function* g() { (yield) + (yield); }",
        "function* g() { var x = yield; x = yield 1; }",
        "function* g() { f(yield 1, yield 2); }",
        "function* g() { [yield, yield]; }",
        "function* g() { `${yield}`; }",
        "function* g() { yield\n1; }",
        "function* g() { a ? yield : yield; }",
    ]);
}

#[test]
fn contextual_keywords_cannot_be_spelled_with_escapes_in_method_definitions() {
    assert_all_rejected(&[
        "({ \\u0061sync m(){} });",
        "({ \\u0061sync *m(){} });",
        "({ g\\u0065t m() {} });",
        "({ s\\u0065t m(v) {} });",
        "class C { \\u0061sync m(){} }",
        "class C { \\u0061sync *m(){} }",
    ]);
    assert_all_accepted(&[
        "({ \\u0061sync(){} });",
        "({ \\u0061sync: 1 });",
        "({ g\\u0065t: 1 });",
        "({ g\\u0065t(){} });",
        "({ \\u0061sync });",
        "class C { \\u0061sync(){} }",
    ]);
}

#[test]
fn method_parameters_must_be_unique_even_in_sloppy_code() {
    assert_all_rejected(&[
        "({ foo(a, a) {} });",
        "({ async foo(a, a) {} });",
        "({ *foo(a, a) {} });",
        "({ async *foo(a, a) {} });",
        "({ foo([a], a) {} });",
        "({ foo(a, {a}) {} });",
        "({ foo(a, ...a) {} });",
        "class C { foo(a, a) {} }",
    ]);
    assert_all_accepted(&[
        "function f(a, a) {}",
        "(function(a, a) {});",
        "({ foo: function(a, a) {} });",
        "async function f(a, a) {}",
        "({ foo(a, b) {} });",
    ]);
}

#[test]
fn untagged_templates_reject_legacy_octal_escapes() {
    assert_all_rejected(&[
        "`\\8`;",
        "`\\9`;",
        "`\\00`;",
        "`\\01`;",
        "`\\1`;",
        "`\\07`;",
        "`${1}\\1`;",
        "`\\0${1}\\8`;",
    ]);
    assert_all_accepted(&["`\\0`;", "`\\0a`;", "`\\\\1`;", "tag`\\1`;", "tag`\\8`;", "tag`\\00`;"]);
    assert_eq!(
        evaluate("(function(s) { return s[0] === undefined && s.raw[0] === '\\\\1'; })`\\1`"),
        Value::Bool(true)
    );
}
