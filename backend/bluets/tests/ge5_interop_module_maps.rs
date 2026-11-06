// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Original star syntax and elided module imports keep complete Native roles.
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
#[path = "oracle_support/project_paths.rs"]
mod project_paths;

#[test]
fn original_star_and_elided_imports_match_complete_native_module_roles() {
    class_cli::run(
        |row| {
            matches!(row["id"].as_str(), Some(
            "ge2_module_helpers/interop-exportstar-cycle-esm-es5-i0-n0/original/external"
            | "ge2_module_helpers/interop-exportstar-cycle-amd-es5-i0-n0/crlfAstral/external"
            | "ge2_module_helpers/interop-exportstar-cycle-system-es5-i0-n0/crlfAstral/external"
        ))
        },
        3,
        "interop-module-boundary",
        "verify_ge5_interop_module_acceptance.cjs",
    );
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3; all 363 frozen interop modules"]
fn all_interop_modules_preserve_complete_native_roles() {
    class_cli::run(
        |row| row["form"] == "ge2_module_helpers/interop-exportstar-cycle",
        363,
        "interop-module-maps",
        "verify_ge5_interop_module_acceptance.cjs",
    );
}
