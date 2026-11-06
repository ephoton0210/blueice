// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Async super property and invocation phases retain original source roles.
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
fn lowered_async_super_property_and_call_keep_distinct_original_roles() {
    class_cli::run(
        |row| {
            row["form"] == "ge1_downlevel/async-super-method"
                && matches!(
                    inputs::resolve(row)["options"]["target"].as_str(),
                    Some("ES2015" | "ES2016")
                )
        },
        16,
        "async-super-boundaries",
        "verify_ge5_async_super_boundaries.cjs",
    );
}

#[test]
fn es5_empty_async_super_signature_keeps_the_original_async_entry_phase() {
    class_cli::run(
        |row| {
            row["form"] == "ge1_downlevel/async-super-method"
                && inputs::resolve(row)["options"]["target"] == "ES5"
        },
        11,
        "async-super-es5-signature",
        "verify_ge5_async_super_boundaries.cjs",
    );
}
