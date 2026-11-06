// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Frozen JSX compositions retain complete diagnostics and raw declarations.
#[path = "oracle_support/gc3_compare.rs"]
mod compare;
use compare::family_matches_native;

#[test]
fn overload_compositions_match_complete_native_diagnostics_and_declarations() {
    family_matches_native("jsx-overload", 40);
}

#[test]
fn union_compositions_match_complete_native_diagnostics_and_declarations() {
    family_matches_native("jsx-union", 20);
}

#[test]
fn recursive_compositions_match_complete_native_diagnostics_and_declarations() {
    family_matches_native("jsx-recursive", 20);
}

#[test]
fn imported_compositions_match_complete_native_diagnostics_and_declarations() {
    family_matches_native("jsx-imported-declarations", 20);
}

#[test]
fn global_compositions_match_complete_native_diagnostics_and_declarations() {
    family_matches_native("jsx-global-declarations", 20);
}
