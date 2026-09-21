// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Early errors that apply only to strict mode code (§13.1.1, §12.9.3,
//! §12.9.4.1 and Annex B's carve-outs), as source text exercised through the
//! public parse/compile pipeline.

use blueice_bluejs::{compile, parse};

/// The source is rejected before running: a parse or a compile error.
fn is_rejected(source: &str) -> bool {
    match parse(source) {
        Err(_) => true,
        Ok(program) => compile(&program).is_err(),
    }
}

fn assert_strict_rejected(body: &str) {
    assert!(is_rejected(&format!("\"use strict\";\n{body}")), "strict: {body}");
}

fn assert_sloppy_accepted(body: &str) {
    assert!(!is_rejected(body), "sloppy: {body}");
}

#[test]
fn eval_and_arguments_cannot_be_declared_with_var_in_strict_code() {
    for body in [
        "var eval;",
        "var arguments;",
        "var a, eval, b;",
        "var eval = 1;",
        "var a = 0, arguments = 1;",
        "var [eval] = [];",
        "var {arguments} = {};",
        "for (var eval in null) {}",
        "for (var arguments of []) {}",
        "for (var eval = 0; ; ) {}",
        "function f() { var arguments; }",
        "function f() { for (var arguments in null) {} }",
        "function f() { let eval; }",
        "{ const arguments = 0; }",
    ] {
        assert_strict_rejected(body);
    }
}

#[test]
fn eval_and_arguments_are_ordinary_var_names_in_sloppy_code() {
    for body in [
        "var eval;",
        "var arguments;",
        "for (var eval in null) {}",
        "function f() { var arguments; }",
    ] {
        assert_sloppy_accepted(body);
    }
}
