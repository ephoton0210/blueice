// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Grammar-error paths of the script/module parser and the function, class,
//! decorator, template and arrow-function productions. Every error is a
//! specified SyntaxError (`known_syntax`).

use blueice_bluejs::{parse, parse_module};

fn assert_script_errors(cases: &[(&str, &str)]) {
    for (source, message) in cases {
        let error = parse(source).unwrap_err();
        assert!(
            error.message.contains(message),
            "{source:?}: {:?} does not contain {message:?}",
            error.message
        );
        assert!(error.known_syntax, "{source:?}: {}", error.message);
    }
}

fn assert_module_errors(cases: &[(&str, &str)]) {
    for (source, message) in cases {
        let error = parse_module(source).unwrap_err();
        assert!(
            error.message.contains(message),
            "{source:?}: {:?} does not contain {message:?}",
            error.message
        );
        assert!(error.known_syntax, "{source:?}: {}", error.message);
    }
}

#[test]
fn function_headers_and_bodies_report_their_own_errors() {
    assert_script_errors(&[
        (
            "function f {}",
            "a function parameter list must begin with '('",
        ),
        ("function f(...) {}", "expected a binding target"),
        ("function f(...a b) {}", "expected RParen"),
        (
            "function f(...a = 1) {}",
            "a rest parameter cannot have a default value",
        ),
        (
            "function f(...a,) {}",
            "a rest parameter cannot have a trailing comma",
        ),
        ("function f() 1", "expected LBrace"),
        ("function f() {", "unterminated block"),
        ("({ get x })", "expected LParen"),
        ("({ set x })", "expected LParen"),
        (
            "(async function aw\\u0061it() {})",
            "the await keyword cannot contain an escape",
        ),
        (
            "(async function await() {})",
            "await cannot be used as a function name here",
        ),
        ("async function f(a = await 1) {}", "await is not allowed"),
        ("function* g(a = yield) {}", "yield is not allowed"),
        (
            "async await => 1",
            "await cannot be used as a binding identifier",
        ),
        (
            "async aw\\u0061it => 1",
            "the await keyword cannot contain an escape",
        ),
        ("x => {", "unterminated block"),
        ("x\n=> 1", "no line terminator is allowed before =>"),
        ("(x)\n=> 1", "no line terminator is allowed before =>"),
    ]);
    for source in [
        "function let() {}",
        "(x = (1)) => x",
        "async (x = 1) => x",
        "function f(a, ...b) {}",
    ] {
        assert!(parse(source).is_ok(), "{source}");
    }
}

#[test]
fn class_and_decorator_grammar_errors_are_classified() {
    assert_script_errors(&[
        (
            "class C extends B => {}",
            "a class heritage cannot be an arrow function",
        ),
        (
            "class C extends (a) => {}",
            "a class heritage cannot be an arrow function",
        ),
        ("class C extends (a", "expected RParen"),
        ("class C", "expected LBrace"),
        ("class C { @ }", "expected a decorator expression"),
        (
            "class C { accessor foo() {} }",
            "an auto-accessor cannot be a method",
        ),
        ("@(a b) class C {}", "expected RParen"),
        (
            "@a.1 class C {}",
            "a decorator must be followed by a class or class element",
        ),
        ("@a( class C {}", "expected Comma"),
        ("@(+) class C {}", "expected an expression"),
        ("@a.+ class C {}", "expected an identifier"),
        ("@d class {", "expected a property key"),
        ("@d class C {", "expected a property key"),
        (
            "@d 1",
            "a decorator must be followed by a class or class element",
        ),
        (
            "@d cl\\u0061ss C {}",
            "a decorator must be followed by a class or class element",
        ),
    ]);
    for source in [
        "@let class C {}",
        "@a.b class C {}",
        "@a.b(1) class C {}",
        "@(a) class C {}",
    ] {
        assert!(parse(source).is_ok(), "{source}");
    }
}

#[test]
fn escaped_async_and_annex_b_for_heads_and_private_names_are_classified() {
    assert_script_errors(&[
        ("\\u0061sync x => 1", "cannot contain an escape"),
        ("\\u0061sync (x) => 1", "cannot contain an escape"),
        ("class C { m() { this.#missing } }", "private"),
    ]);
    // Annex B: a call expression is a web-compatible for-in/of head in
    // sloppy code.
    for source in ["for (f() in {}) ;", "for (f() of []) ;"] {
        assert!(parse(source).is_ok(), "{source}");
    }
}

#[test]
fn parenthesized_expressions_and_assignment_targets_are_unwrapped() {
    for source in [
        "(a);",
        "((1 + 2));",
        // Only a parenthesized optional chain keeps its grouping without an
        // assignment operator.
        "(a?.b);",
        "((a?.b));",
        "(a) = 1;",
        "((a)) = 1;",
        "[(a)] = [1];",
        "({ b: (c) } = {});",
        "(a.b) = 1;",
        "(a) += 1;",
    ] {
        assert!(parse(source).is_ok(), "{source}");
    }
}

#[test]
fn template_placeholders_are_parsed_in_the_enclosing_context() {
    assert_script_errors(&[
        (
            "\"use strict\"; `${yield}`",
            "a reserved word cannot be used as an identifier reference",
        ),
        (
            "function f(){ return `${await 1}`; }",
            "unexpected expression after await identifier",
        ),
        (
            "`${yield 1}`",
            "unexpected trailing tokens after expression",
        ),
        (
            "`${a}${yield 1}${b}`",
            "unexpected trailing tokens after expression",
        ),
        ("`${a b}`", "unterminated or invalid template placeholder"),
        ("`${+}`", "unterminated or invalid template placeholder"),
    ]);
    for source in ["`${yield}`", "function* g(){ `${yield 1}` }", "`${await}`"] {
        assert!(parse(source).is_ok(), "{source}");
    }
    assert!(parse_module("`${await 1}`").is_ok());
}

#[test]
fn a_bare_hash_is_a_private_identifier_without_a_name() {
    assert_script_errors(&[
        ("#", "private identifier requires a name"),
        (
            "function # x() {}",
            "a function parameter list must begin with '('",
        ),
        ("a #", "expected ';'"),
    ]);
}

#[test]
fn module_import_and_export_clauses_report_their_errors() {
    assert_module_errors(&[
        ("import {a as} from 'x'", "expected a binding identifier"),
        ("import * as 1 from 'x'", "expected a binding identifier"),
        ("import 1 from 'x'", "expected a binding identifier"),
        ("import a from 1", "expected a module specifier string"),
        ("export * from 1", "expected a module specifier string"),
        ("import a from '\\01'", "legacy octal"),
        ("export * from '\\01'", "legacy octal"),
        ("export { a as '\\01' }", "legacy octal"),
        (
            "import a from '\\uD800'",
            "module specifier must be well-formed Unicode",
        ),
        (
            "export { a as '\\uD800' }",
            "module export name must be well-formed Unicode",
        ),
        (
            "import x from 'a' w\\u0069th {}",
            "the with keyword cannot contain an escape",
        ),
        (
            "export * fr\\u006fm 'a'",
            "the from keyword cannot contain an escape",
        ),
    ]);
    assert!(parse_module("export {a}\nfr\\u006fm").is_ok());
    assert!(parse_module("var a; export {a}\nfr\\u006fm\n").is_ok());
}
