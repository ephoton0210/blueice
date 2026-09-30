// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Test262's lexical and syntactic fixtures: literals, identifiers, reserved
//! words, automatic semicolon insertion, directive prologues, block scoping,
//! destructuring and the Annex B language extensions.

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
fn lexical_grammar() {
    check(&[
        "language/asi",
        "language/comments",
        "language/directive-prologue",
        "language/future-reserved-words",
        "language/identifiers",
        "language/keywords",
        "language/line-terminators",
        "language/literals",
        "language/punctuators",
        "language/reserved-words",
        "language/white-space",
    ]);
}

#[test]
fn scoping_and_declarations() {
    check(&[
        "language/block-scope",
        "language/computed-property-names",
        "language/destructuring",
        "language/identifier-resolution",
        "language/rest-parameters",
        "language/statementList",
        "language/types",
    ]);
}

#[test]
fn annex_b_language() {
    // The one stale fixture contradicts the current specification (see the
    // conformance runner's `STALE_CORPUS_FIXTURES`).
    let failures = run_directory("annexB/language", &["block-decl-func-skip-arguments.js"]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
