// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Parse rejections that the grammar mandates must be reported as specified
//! SyntaxErrors (`ParseError::known_syntax`), not as unclassified failures.
//! The Test262 adapter counts a rejection toward a negative parse-phase test
//! only when the parser knows the source is invalid, so a missing
//! classification hides a correct rejection.

use blueice_bluejs::parse;

fn assert_known_syntax_error(source: &str) {
    let error = parse(source).expect_err(source);
    assert!(error.known_syntax, "{source:?}: {error:?}");
}

#[test]
fn a_mandatory_token_that_is_missing_is_a_syntax_error() {
    for source in [
        // The head of every statement keyword requires its punctuation.
        "while 1 break;",
        "while (true",
        "if true ;",
        "if{};else{}",
        "do ; while",
        "do ; ; (1)",
        "do x; while 1",
        "for a of b;",
        "switch 1 {}",
        "with 1 {}",
        // The `for` head has exactly two semicolons.
        "for (a; b\n)",
        "for(false;false;false;) {}",
        "for(index=0; index<10; index++; index--) ;",
        "for({var index=0; index+=1;} index++<=10; index*2;) {}",
        // Object literal members.
        "y={__func;}();",
        "({1})",
        "if({1}) ;",
        "({ async foo })",
        "({ *foo })",
        "({ async *foo })",
    ] {
        assert_known_syntax_error(source);
    }
}

#[test]
fn a_token_that_cannot_begin_an_expression_is_a_syntax_error() {
    for source in [
        "var a=1,b=2; if(a>b)\nelse b=a",
        "x = else;",
        "x = case;",
        "x = default;",
        "x = in;",
        "x = instanceof;",
        "x = ??;",
        "x = ...;",
        "x = >;",
        "for = 1;",
        "if = 1;",
        "while = 1;",
        "with = 1;",
        "case = 1;",
        "default = 1;",
        "else = 1;",
        "in = 1;",
        "instanceof = 1;",
        "\n._",
        "\n.source;",
        "#field in {}",
        "x = #field;",
    ] {
        assert_known_syntax_error(source);
    }
}

#[test]
fn throw_rejects_a_line_terminator_before_its_expression() {
    assert_known_syntax_error("try {\n throw\n 1;\n} catch (e) {}");
}

#[test]
fn nullish_coalescing_cannot_mix_with_logical_operators_unparenthesized() {
    for source in [
        "0 || 0 ?? true;",
        "0 && 0 ?? true;",
        "0 ?? 0 || true;",
        "0 ?? 0 && true;",
    ] {
        assert_known_syntax_error(source);
    }
}

#[test]
fn accessor_parameter_lists_are_checked() {
    for source in [
        "({ get a(x = 1) {} })",
        "({ get a(x) {} })",
        "({ set a() {} })",
        "({ set a(...x) {} })",
        "class C { get a(x = 1) {} }",
        "class C { set a(x, y) {} }",
    ] {
        assert_known_syntax_error(source);
    }
}

#[test]
fn a_regexp_literal_cannot_contain_a_line_terminator() {
    for source in [
        "/a\nb/",
        "/a\rb/",
        "/a\u{2028}b/",
        "/a\u{2029}b/",
        "/\u{2028}/",
        "/[\n]/",
        "/\\\n/",
        "/\\\u{2028}/",
        "/\\\u{2029}/",
        "/a",
    ] {
        assert_known_syntax_error(source);
    }
}
