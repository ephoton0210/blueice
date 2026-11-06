// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Native storage and publication phases of exported decorated class values.
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
fn decorated_commonjs_class_values_keep_native_storage_and_publication_phases() {
    class_cli::run(
        |row| {
            matches!(
                row["form"].as_str().unwrap(),
                "ge3_decorator_compositions/class-expression-named"
                    | "ge3_decorator_compositions/class-expression-anonymous"
                    | "ge3_decorator_compositions/class-expression-computed-heritage"
            ) && inputs::resolve(row)["options"]["module"] == "CommonJS"
        },
        57,
        "class-value-publication",
        "verify_ge5_class_value_publication.cjs",
    );
}
