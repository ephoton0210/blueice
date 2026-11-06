// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Complete frozen imported string enum metadata products.
#[path = "oracle_support/ge5_class_cli.rs"]
mod class_cli;
#[allow(dead_code)]
#[path = "oracle_support/ge5_cli.rs"]
mod cli;
#[allow(dead_code)]
#[path = "oracle_support/ge5_declarations.rs"]
mod declarations;
#[allow(dead_code)]
#[path = "oracle_support/gd2_maps.rs"]
mod decoded;
#[allow(dead_code)]
#[path = "oracle_support/ge5_inputs.rs"]
mod inputs;
#[allow(dead_code)]
#[path = "oracle_support/ge5_javascript_maps.rs"]
mod javascript_maps;
#[allow(dead_code)]
#[path = "oracle_support/project_paths.rs"]
mod project_paths;

#[test]
#[ignore = "requires pinned TypeScript 5.9.3; all 43 frozen imported string enum products"]
fn all_imported_string_enum_products_match_native_roles() {
    class_cli::run(
        |row| row["form"] == "ge3_decorator_compositions/imported-string-enum",
        43,
        "metadata-string-enum",
        "verify_ge5_metadata_string_enum.cjs",
    );
}
