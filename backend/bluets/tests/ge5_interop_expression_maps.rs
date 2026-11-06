// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Original import calls retain their complete callee and expression endpoints.
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
fn imported_calls_retain_exact_native_callee_endpoints() {
    class_cli::run(
        |row| {
            row["id"]
                == "ge2_module_helpers/interop-exportstar-cycle-cjs-es2022-i0-n0/original/external"
        },
        1,
        "interop-expression-boundary",
        "verify_ge5_interop_expression_maps.cjs",
    );
}

#[test]
fn default_function_headers_retain_exact_native_phases() {
    class_cli::run(
        |row| {
            row["id"]
                == "ge2_module_helpers/interop-exportstar-cycle-esm-es2022-i0-n0/original/external"
        },
        1,
        "interop-default-function-boundary",
        "verify_ge5_interop_expression_maps.cjs",
    );
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3; all 363 frozen interop products"]
fn all_interop_expressions_preserve_complete_native_roles() {
    class_cli::run(
        |row| row["form"] == "ge2_module_helpers/interop-exportstar-cycle",
        363,
        "interop-expression-maps",
        "verify_ge5_interop_expression_maps.cjs",
    );
}
