// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Real same-process incremental work covers every frozen lifecycle profile.

mod program_roots_lifecycle_support;

#[test]
fn all_528_library_generations_match_native_and_fresh_compilation() {
    program_roots_lifecycle_support::run(|row| {
        for key in [
            "nativeAgrees",
            "coldWarmExact",
            "parsedPartition",
            "checkedPartition",
            "cacheContract",
        ] {
            assert_eq!(row[key], true, "{key}: {row}");
        }
    });
}
