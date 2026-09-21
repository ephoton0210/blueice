// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The [[Prototype]] of `%GeneratorFunction%`, `%AsyncFunction%` and
//! `%AsyncGeneratorFunction%` is the `%Function%` constructor itself, not
//! `%Function.prototype%`.
use blueice_bluejs::{compile, parse, Value, Vm};

fn assert_true(source: &str) {
    let program = parse(source).unwrap_or_else(|e| panic!("{source}: {e:?}"));
    assert_eq!(
        Vm::default()
            .execute(&compile(&program).unwrap())
            .unwrap_or_else(|e| panic!("{source}: {e:?}")),
        Value::Bool(true),
        "{source}"
    );
}

#[test]
fn every_dynamic_function_constructor_inherits_from_function() {
    for prototype_of in [
        "function* () {}",
        "async function () {}",
        "async function* () {}",
    ] {
        // Read the constructor first thing in a fresh realm, before anything
        // else has created the intrinsics.
        assert_true(&format!(
            "var C = Object.getPrototypeOf({prototype_of}).constructor;
             Object.getPrototypeOf(C) === Function && C !== Function"
        ));
    }
    assert_true(
        "var GF = Object.getPrototypeOf(function* () {}).constructor;
         GF.call === Function.prototype.call && Object.getPrototypeOf(GF) !== Function.prototype",
    );
}
