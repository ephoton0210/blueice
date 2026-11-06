// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! All constructor identity products cross the public CLI twice before comparison.
#[test]
#[ignore = "requires pinned TypeScript 5.9.3; all 172 frozen metadata class products"]
fn all_metadata_class_dependency_products_match_native_roles() {
    super::class_cli::run(
        |row| {
            matches!(
                row["form"].as_str(),
                Some(
                    "ge3_decorator_compositions/imported-class"
                        | "ge3_decorator_compositions/renamed-class"
                        | "ge3_decorator_compositions/namespace-class"
                        | "ge3_decorator_compositions/default-class"
                )
            )
        },
        172,
        "metadata-classes",
        "verify_ge5_metadata_classes.cjs",
    );
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3; ten adjacent ES5 legacy wrapper products"]
fn adjacent_legacy_wrappers_keep_their_complete_native_class_roles() {
    super::class_cli::run(
        |row| {
            row["profile"] == "external"
                && matches!(
                    row["origin"]["id"].as_str(),
                    Some(
                        "legacy-parameter-decorator-cjs-es5-i0-n0"
                            | "legacy-parameter-decorator-esm-es5-i0-n0"
                            | "legacy-parameter-decorator-amd-es5-i0-n0"
                            | "legacy-parameter-decorator-umd-es5-i0-n0"
                            | "legacy-parameter-decorator-system-es5-i0-n0"
                    )
                )
        },
        10,
        "metadata-class-adjacent",
        "verify_ge5_metadata_class_adjacent.cjs",
    );
}
