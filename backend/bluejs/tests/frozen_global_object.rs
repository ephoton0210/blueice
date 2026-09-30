// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Standard globals are materialized lazily, but the global object's own
//! properties exist from the start. Making the global object non-extensible
//! must therefore not turn their first observation into a failed definition.
use blueice_bluejs::{compile, parse, Value, Vm};

fn check(sources: &[&str]) {
    for source in sources {
        let value = Vm::default()
            .execute(&compile(&parse(source).unwrap()).unwrap())
            .unwrap_or_else(|error| panic!("{source}: {error:?}"));
        assert_eq!(value, Value::Bool(true), "{source}");
    }
}

#[test]
fn freezing_the_global_object_succeeds_and_freezes_the_standard_globals() {
    check(&[
        "Object.freeze(this) === this && Object.isFrozen(this) \
           && Object.getOwnPropertyDescriptor(this, 'Math').writable === false \
           && Object.getOwnPropertyDescriptor(this, 'Math').configurable === false",
        "Object.seal(globalThis) === globalThis && Object.isSealed(globalThis)",
        "Object.preventExtensions(globalThis) === globalThis && !Object.isExtensible(globalThis)",
    ]);
}

#[test]
fn sloppy_assignment_to_a_new_name_on_a_frozen_global_is_silently_ignored() {
    check(&[
        "Object.freeze(this); for (var i = 0; i < 3; i++) { fresh = i; } typeof fresh === 'undefined'",
        "'use strict'; Object.freeze(this); \
         try { fresh2 = 1; false } catch (e) { e instanceof TypeError || e instanceof ReferenceError }",
    ]);
}

#[test]
fn standard_globals_stay_usable_after_the_global_object_is_frozen() {
    check(&[
        "Object.freeze(this); typeof Map === 'function' && typeof Promise === 'function' \
           && new Map([[1, 2]]).get(1) === 2 && typeof Symbol.iterator === 'symbol'",
    ]);
}
