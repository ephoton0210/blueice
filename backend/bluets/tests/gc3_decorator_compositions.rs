// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Frozen direct compositions retain complete diagnostics and raw declarations.
#[path = "oracle_support/gc3_compare.rs"]
mod compare;
use compare::family_matches_native;

#[test]
fn metadata_compositions_match_native() {
    family_matches_native("symbol-metadata", 20);
}
#[test]
fn replacement_compositions_match_native() {
    family_matches_native("replacement-context-inference", 16);
}
#[test]
fn abstract_constraint_compositions_match_native() {
    family_matches_native("abstract-generic-constraints", 8);
}
#[test]
fn receiver_compositions_match_native() {
    family_matches_native("accessor-receivers", 24);
}
#[test]
fn legacy_compositions_match_native() {
    family_matches_native("legacy-applicable", 8);
}
