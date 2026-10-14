// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Test262's eval and global-code fixtures: variable and function
//! declaration instantiation through direct and indirect `eval` and script
//! top level.

mod cov_g4_common;
use cov_g4_common::test262::run_directory;

fn check(relative: &str) {
    let failures = run_directory(relative, &[]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn direct_eval_code() {
    check("language/eval-code/direct");
}

#[test]
fn indirect_eval_code() {
    check("language/eval-code/indirect");
}

#[test]
fn global_code() {
    check("language/global-code");
}

#[test]
fn annex_b_eval_and_global_code() {
    check("annexB/language/eval-code");
    check("annexB/language/global-code");
}
