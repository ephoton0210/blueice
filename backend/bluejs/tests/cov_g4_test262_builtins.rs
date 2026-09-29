// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Test262's Object, Reflect, Proxy, Function, Iterator, PlainTime and
//! typed-array constructor fixtures.

mod cov_g4_common;
use cov_g4_common::test262::run_directory;

fn check(relative: &str) {
    let failures = run_directory(relative, &[]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn object() {
    check("built-ins/Object");
}

#[test]
fn reflect() {
    check("built-ins/Reflect");
}

#[test]
fn proxy() {
    check("built-ins/Proxy");
}

#[test]
fn function() {
    check("built-ins/Function");
}

#[test]
fn iterator() {
    check("built-ins/Iterator");
}

#[test]
fn plain_time() {
    check("built-ins/Temporal/PlainTime");
    check("intl402/Temporal/PlainTime");
}

#[test]
fn typed_array_constructors() {
    check("built-ins/TypedArrayConstructors");
}
