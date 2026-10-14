// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Standard globals are materialized lazily. A strict-mode assignment to one
//! that has not been read yet must still find the (existing) global property
//! instead of reporting an unresolvable reference.
use blueice_bluejs::{compile, parse, Value, Vm};

fn check(source: &str) {
    let value = Vm::default()
        .execute(&compile(&parse(source).unwrap()).unwrap())
        .unwrap_or_else(|error| panic!("{source}: {error:?}"));
    assert_eq!(value, Value::Bool(true), "{source}");
}

#[test]
fn strict_assignment_overwrites_an_unread_standard_global() {
    for name in [
        "Symbol", "Map", "Promise", "Math", "Reflect", "Proxy", "Intl", "BigInt",
    ] {
        check(&format!(
            "'use strict'; {name} = undefined; typeof {name} === 'undefined' \
             && Object.getOwnPropertyDescriptor(globalThis, '{name}').value === undefined"
        ));
    }
}

#[test]
fn strict_compound_and_logical_assignment_see_the_standard_global() {
    check("'use strict'; var kept = Symbol; Symbol ||= 5; Symbol === kept");
    check("'use strict'; Math &&= 7; Math === 7");
}

#[test]
fn strict_assignment_to_a_missing_global_is_still_a_reference_error() {
    check(
        "'use strict'; try { definitelyMissingGlobal = 1; false } \
         catch (e) { e instanceof ReferenceError }",
    );
}
