// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Test262's ArrayBuffer, SharedArrayBuffer, DataView and Atomics fixtures.

mod cov_g4_common;
use cov_g4_common::test262::run_directory;

fn check(relative: &str) {
    let failures = run_directory(relative, &[]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn array_buffer() {
    check("built-ins/ArrayBuffer");
}

#[test]
fn shared_array_buffer() {
    check("built-ins/SharedArrayBuffer");
}

#[test]
fn data_view() {
    check("built-ins/DataView");
}

#[test]
fn atomics_wait() {
    check("built-ins/Atomics/wait");
}

#[test]
fn atomics_wait_async() {
    check("built-ins/Atomics/waitAsync");
}

#[test]
fn atomics_notify() {
    check("built-ins/Atomics/notify");
}

#[test]
fn atomics_operations() {
    for operation in [
        "add",
        "and",
        "compareExchange",
        "exchange",
        "isLockFree",
        "load",
        "or",
        "pause",
        "store",
        "sub",
        "xor",
    ] {
        check(&format!("built-ins/Atomics/{operation}"));
    }
}

#[test]
fn typed_array_statics_and_views() {
    for directory in [
        "from",
        "of",
        "prototype/set",
        "prototype/subarray",
        "prototype/buffer",
        "prototype/byteLength",
        "prototype/byteOffset",
        "prototype/length",
        "prototype/entries",
        "prototype/keys",
        "prototype/values",
    ] {
        check(&format!("built-ins/TypedArray/{directory}"));
    }
}
