// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Global declaration instantiation across scripts, assignment to global
//! bindings, and the bindings that sloppy direct `eval` creates, shadows,
//! deletes and re-creates.

mod cov_g4_common;
use cov_g4_common::{failures, scripts};

fn starts(results: &[String], index: usize, prefix: &str) {
    assert!(
        results[index].starts_with(prefix),
        "script {index}: {:?} does not start with {prefix:?} (all: {results:?})",
        results[index]
    );
}

#[test]
fn a_script_cannot_redeclare_what_an_earlier_script_declared_lexically() {
    let results = scripts(&["let a = 1;", "var a;", "let a;", "function a() {}"]);
    starts(&results, 0, "ok");
    starts(&results, 1, "SyntaxError");
    starts(&results, 2, "SyntaxError");
    starts(&results, 3, "SyntaxError");
    let results = scripts(&["var v = 1;", "let v;"]);
    starts(&results, 1, "SyntaxError");
    let results = scripts(&[
        "Object.defineProperty(globalThis, 'locked', { value: 1 });",
        "let locked;",
    ]);
    starts(&results, 1, "SyntaxError");
}

#[test]
fn a_script_cannot_declare_what_the_global_object_refuses() {
    let results = scripts(&[
        "Object.preventExtensions(globalThis);",
        "var q;",
        "function fq() {}",
        "var Object;",
    ]);
    starts(&results, 1, "TypeError");
    starts(&results, 2, "TypeError");
    starts(&results, 3, "ok");
    let results = scripts(&[
        "Object.defineProperty(globalThis, 'frozenName', { value: 1 });",
        "function frozenName() {}",
        "Object.defineProperty(globalThis, 'writableName', { value: 1, writable: true, enumerable: true });",
        "function writableName() { return 2 } writableName()",
    ]);
    starts(&results, 1, "TypeError");
    assert_eq!(results[3], "ok 2");
}

#[test]
fn a_var_over_a_global_accessor_leaves_the_accessor_in_charge() {
    let results = scripts(&[
        "var log = []; Object.defineProperty(globalThis, 'acc', { get: function () { return 7 }, set: function (v) { log.push(v) }, configurable: true });",
        "var acc; typeof Object.getOwnPropertyDescriptor(globalThis, 'acc').get",
        "acc = 9; log.join() + ':' + acc",
    ]);
    assert_eq!(results[1], "ok function");
    assert_eq!(results[2], "ok 9:7");
}

#[test]
fn assignments_reach_the_bindings_an_earlier_script_declared() {
    let results = scripts(&[
        "var g = 1; let l = 2; const c = 3;",
        "g = 5; l = 6; g + ':' + l",
        "c = 7",
        "'use strict'; c = 8",
        "'use strict'; l = 9; l",
        "let t = (function () { throw new Error('no value') })();",
        "t = 1",
    ]);
    starts(&results, 0, "ok");
    assert_eq!(results[1], "ok 5:6");
    starts(&results, 2, "TypeError");
    starts(&results, 3, "TypeError");
    assert_eq!(results[4], "ok 9");
    starts(&results, 5, "uncaught");
    starts(&results, 6, "ReferenceError");
}

#[test]
fn symbols_keys_and_lazy_globals_are_visible_on_the_global_object() {
    assert_eq!(
        failures(
            r#"
eq('symbol key', globalThis[Symbol.toStringTag], undefined);
eq('symbol descriptor', Object.getOwnPropertyDescriptor(globalThis, Symbol.iterator), undefined);
eq('lazy standard global', typeof Object.getOwnPropertyDescriptor(globalThis, 'JSON').value, 'object');
eq('NaN', Object.getOwnPropertyDescriptor(globalThis, 'NaN').writable, false);
"#
        ),
        ""
    );
}
