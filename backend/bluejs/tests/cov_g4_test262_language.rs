// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Test262's statement fixtures (everything but `class`), and the
//! assignment, function and function-code fixtures that share the
//! environment machinery with them.

mod cov_g4_common;
use cov_g4_common::test262::run_directory;

fn check(relatives: &[&str]) {
    let mut failures = Vec::new();
    for relative in relatives {
        failures.extend(run_directory(relative, &[]));
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn loops() {
    check(&[
        "language/statements/for",
        "language/statements/for-in",
        "language/statements/for-of",
        "language/statements/while",
        "language/statements/do-while",
    ]);
}

#[test]
fn for_await_of() {
    check(&["language/statements/for-await-of"]);
}

#[test]
fn control_flow() {
    check(&[
        "language/statements/if",
        "language/statements/switch",
        "language/statements/try",
        "language/statements/labeled",
        "language/statements/break",
        "language/statements/continue",
        "language/statements/return",
        "language/statements/throw",
        "language/statements/block",
        "language/statements/empty",
        "language/statements/expression",
        "language/statements/debugger",
        "language/statements/with",
    ]);
}

#[test]
fn declarations() {
    check(&[
        "language/statements/variable",
        "language/statements/let",
        "language/statements/const",
        "language/statements/using",
        "language/statements/await-using",
        "language/statements/function",
        "language/statements/generators",
        "language/statements/async-function",
        "language/statements/async-generator",
    ]);
}

#[test]
fn assignment_and_function_code() {
    check(&[
        "language/expressions/assignment",
        "language/expressions/compound-assignment",
        "language/expressions/assignmenttargettype",
        "language/function-code",
        "language/arguments-object",
    ]);
}
