// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! G-E5: complete Native map authority and public declaration provenance.
#[path = "oracle_support/ge5_async_roles.rs"]
mod async_roles;
#[path = "oracle_support/ge5_cli.rs"]
mod cli;
#[path = "oracle_support/ge5_continuation_roles.rs"]
mod continuation_roles;
#[path = "oracle_support/ge5_declarations.rs"]
mod declarations;
#[path = "oracle_support/gd2_maps.rs"]
mod decoded;
#[path = "oracle_support/ge5_inputs.rs"]
mod inputs;
#[path = "oracle_support/ge5_javascript_boundaries.rs"]
mod javascript_boundaries;
#[path = "oracle_support/ge5_javascript_maps.rs"]
mod javascript_maps;
#[path = "oracle_support/ge5_javascript_roles.rs"]
mod javascript_roles;
#[path = "oracle_support/ge5_module_roles.rs"]
mod module_roles;
#[path = "oracle_support/ge5_native_defects.rs"]
mod native_defects;
#[path = "oracle_support/ge5_node_roles.rs"]
mod node_roles;
#[path = "oracle_support/project_paths.rs"]
mod project_paths;

use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

#[test]
fn manifest_retains_all_admitted_graphs_coordinate_products_and_profiles() {
    let input = inputs::input();
    let cases = input["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 8407);
    assert_eq!(input["repeatBuilds"], 2);
    assert_eq!(input["ownerControls"].as_array().unwrap().len(), 6);
    assert_eq!(
        cases
            .iter()
            .map(|row| row["id"].as_str().unwrap())
            .collect::<BTreeSet<_>>()
            .len(),
        cases.len()
    );
    for (suite, count) in [
        ("ge1_downlevel", 500),
        ("ge2_module_helpers", 2527),
        ("ge3_decorator_compositions", 727),
        ("gd2_declaration_maps", 196),
    ] {
        assert_eq!(
            input["parentAuthorities"][suite]["admittedConfigurations"],
            count
        );
        for variant in ["original", "crlfAstral"] {
            assert_eq!(
                cases
                    .iter()
                    .filter(|row| row["origin"]["suite"] == suite
                        && row["variant"] == variant
                        && row["profile"] == "external")
                    .count(),
                count
            );
        }
    }
    assert_eq!(
        cases
            .iter()
            .map(|row| row["form"].as_str().unwrap())
            .collect::<BTreeSet<_>>()
            .len(),
        137
    );
    assert_eq!(
        cases
            .iter()
            .filter(|row| row.get("regression").is_some())
            .count(),
        132
    );
    inputs::verify_authorities();
    let native = inputs::reference();
    assert_eq!(
        native["inputSha256"],
        inputs::sha(include_bytes!("fixtures/ge5_source_maps/cases.json"))
    );
    assert_eq!(native["cases"].as_array().unwrap().len(), cases.len());
    assert_eq!(native["repeatBuilds"], 2);
}

#[test]
fn signature_and_enum_declarations_and_every_decoded_segment_match_native() {
    let temporary = cli::Temporary::new("signature-enum");
    let input = inputs::input();
    let reference = inputs::reference();
    let native = reference["cases"].as_array().unwrap();
    let mut failures = Vec::new();
    let mut observed = 0;
    for (index, specification) in input["cases"].as_array().unwrap().iter().enumerate() {
        if specification.get("regression").is_none() {
            continue;
        }
        observed += 1;
        let row = inputs::resolve(specification);
        let expected = &native[index];
        assert_eq!(specification["id"], expected["id"]);
        let actual = cli::observe(&temporary, index, &row);
        let id = specification["id"].as_str().unwrap();
        if actual.diagnostics != expected["diagnostics"] || !actual.success {
            failures.push(format!("{id}: verdict/diagnostics: {}", actual.diagnostics));
            continue;
        }
        for (name, hash) in expected["outputs"].as_object().unwrap() {
            if ![
                ".d.ts",
                ".d.mts",
                ".d.cts",
                ".d.ts.map",
                ".d.mts.map",
                ".d.cts.map",
            ]
            .iter()
            .any(|suffix| name.ends_with(suffix))
            {
                continue;
            }
            let native_text = inputs::blob(hash.as_str().unwrap());
            let Some(text) = actual.outputs.get(name) else {
                failures.push(format!("{id}: missing {name}"));
                continue;
            };
            if name.ends_with(".map") {
                if decoded::decoded(text) != decoded::decoded(native_text) {
                    failures.push(format!("{id}: complete decoded map differs: {name}"));
                }
            } else if text != native_text {
                failures.push(format!("{id}: raw declaration differs: {name}\nNative: {native_text:?}\nBlueTSC: {text:?}"));
            }
        }
    }
    assert_eq!(observed, 132);
    assert!(
        failures.is_empty(),
        "{} G-E5 signature/enum differences; first 16:\n{}",
        failures.len(),
        failures
            .iter()
            .take(16)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
fn all_frozen_declarations_match_native_and_javascript_maps_validate_twice() {
    declarations::run();
}

#[test]
fn metadata_coordinates_and_incompatible_map_profiles_match_native() {
    declarations::metadata_regressions();
}

#[test]
fn representative_declaration_forms_match_native() {
    declarations::representative_forms();
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3; replays every Native map and both builds"]
fn complete_native_map_authority_is_reproducible() {
    let recorder = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../development/browser_core/phase-18-bluets/tools/record_ge5_maps.cjs");
    let result = Command::new("node").arg(recorder).output().unwrap();
    assert!(
        result.status.success(),
        "{}{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
}
