// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Original class-method completion cannot leak into an apply wrapper.
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
fn lowered_class_method_completion_stops_before_the_generated_apply_wrapper() {
    class_cli::run(
        |row| {
            row["form"] == "ge1_downlevel/async-class-method"
                && matches!(
                    inputs::resolve(row)["options"]["target"].as_str(),
                    Some("ES2015" | "ES2016")
                )
        },
        16,
        "async-class-method-boundaries",
        "verify_ge5_async_class_method_boundaries.cjs",
    );
}

#[test]
fn es5_class_method_function_keyword_retains_the_original_async_signature() {
    class_cli::run(
        |row| {
            row["form"] == "ge1_downlevel/async-class-method"
                && inputs::resolve(row)["options"]["target"] == "ES5"
        },
        11,
        "async-class-method-es5-signature",
        "verify_ge5_async_class_method_es5_signature.cjs",
    );
}
