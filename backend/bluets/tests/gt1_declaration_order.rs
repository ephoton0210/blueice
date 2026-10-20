// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Stage two compares actual bytes, not sorted unions or type-only equivalence.

use blueice_bluets::{compile, CompilerOptions, EcmaTarget, MapLoader, ModuleSource};
use serde_json::{json, Value};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

fn reference() -> Value {
    serde_json::from_str(include_str!(
        "fixtures/gt1_declaration_order/reference.json"
    ))
    .unwrap()
}

fn outputs() -> Vec<Value> {
    let reference = reference();
    assert_eq!(reference["typescript"], "5.9.3");
    assert_eq!(reference["cases"].as_array().unwrap().len(), 90);
    let mut outputs = Vec::new();
    let mut failures = Vec::new();
    for row in reference["cases"].as_array().unwrap() {
        let target = EcmaTarget::parse(row["target"].as_str().unwrap()).unwrap();
        let libraries = match row["library"].as_str().unwrap() {
            "default" => None,
            "explicit" => Some(vec![target]),
            "none" => Some(vec![]),
            other => panic!("foreign library mode: {other}"),
        };
        let source = row["source"].as_str().unwrap();
        let loader = MapLoader::from([ModuleSource::new("entry.ts", source)]);
        let mut options = CompilerOptions {
            target,
            libraries,
            declaration: true,
            ..CompilerOptions::default()
        };
        if row["library"] == "none" {
            options.ambient_declaration_modules.push(ModuleSource::new(
                "owned-globals.d.ts",
                reference["owned_globals"].as_str().unwrap(),
            ));
        }
        let compiled = compile("entry.ts", &loader, options.clone());
        assert_eq!(row["accepts"], true);
        assert_eq!(row["diagnostics"], json!([]));
        assert!(
            compiled.diagnostics.is_empty(),
            "{}: {:?}",
            row["id"],
            compiled.diagnostics
        );
        let output = compiled.output.unwrap();
        let artifact = &output.artifacts["entry.ts"];
        let declaration = artifact.declaration.as_deref().unwrap();
        if declaration != row["declaration"].as_str().unwrap() {
            failures.push(format!(
                "{}: expected {:?}, got {:?}",
                row["id"], row["declaration"], declaration
            ));
        }
        // Presentation must not alter runtime output, checking, or host grants.
        options.declaration = false;
        let runtime = compile("entry.ts", &loader, options);
        assert!(runtime.diagnostics.is_empty());
        assert_eq!(
            runtime.output.unwrap().artifacts["entry.ts"].javascript,
            artifact.javascript,
            "{}",
            row["id"]
        );
        outputs.push(json!({"id":row["id"],"declaration":declaration}));
    }
    assert!(
        failures.is_empty(),
        "{} exact declaration mismatches:\n{}",
        failures.len(),
        failures.join("\n")
    );
    outputs
}

#[test]
fn inferred_literal_declarations_match_native_bytes_for_every_library_mode() {
    outputs();
}

#[test]
fn matrix_retains_all_frozen_forms_and_profile_controls() {
    let input: Value =
        serde_json::from_str(include_str!("fixtures/gt1_declaration_order/cases.json")).unwrap();
    let reference = reference();
    let mut expected = Vec::new();
    for target in input["targets"].as_array().unwrap() {
        for mode in input["libraries"].as_array().unwrap() {
            for form in input["forms"].as_array().unwrap() {
                expected.push((
                    format!(
                        "{}-{}-{}",
                        form["id"].as_str().unwrap(),
                        target.as_str().unwrap().to_lowercase(),
                        mode.as_str().unwrap()
                    ),
                    form["source"].clone(),
                ));
            }
        }
    }
    let actual: Vec<_> = reference["cases"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| {
            (
                row["id"].as_str().unwrap().to_string(),
                row["source"].clone(),
            )
        })
        .collect();
    assert_eq!(actual, expected);
    assert_eq!(reference["profile_controls"].as_array().unwrap().len(), 33);
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3"]
fn native_declarations_and_library_allocation_facts_are_reproducible() {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join(
        "../../development/browser_core/phase-18-bluets/tools/record_gt1_declaration_order.cjs",
    );
    let output = Command::new("node").arg(script).output().unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3"]
fn exact_declarations_have_pinned_native_consumers() {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join(
        "../../development/browser_core/phase-18-bluets/tools/record_gt1_declaration_order.cjs",
    );
    let mut child = Command::new("node")
        .arg(script)
        .arg("--outputs")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&serde_json::to_vec(&outputs()).unwrap())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
