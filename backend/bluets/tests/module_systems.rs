// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! K.7.2 module decisions from the public project CLI and pinned TypeScript.

use serde_json::Value;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::{env, fs};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/module_systems")
}

fn cases() -> Vec<Value> {
    let reference: Value =
        serde_json::from_str(include_str!("fixtures/module_systems/reference.json")).unwrap();
    assert_eq!(reference["typescript"], "5.9.3");
    reference["cases"].as_array().unwrap().clone()
}

fn accepts(case: &Value) -> bool {
    case["reference"]["diagnostics"]
        .as_array()
        .unwrap()
        .is_empty()
}

fn copy_project(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let target = destination.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_project(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3 and Node"]
fn recorded_module_decisions_and_declarations_match_native_typescript() {
    let recorder = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../development/browser_core/phase-18-bluets/tools/record_module_systems.cjs");
    let output = Command::new("node").arg(recorder).output().unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn module_matrix_retains_all_native_configurations() {
    let cases = cases();
    assert_eq!(cases.len(), 95);
    assert_eq!(cases.iter().filter(|case| accepts(case)).count(), 77);
    for (family, count) in [
        ("module", 3),
        ("per-file", 24),
        ("helper-options", 32),
        ("static-contexts", 36),
    ] {
        assert_eq!(
            cases.iter().filter(|case| case["family"] == family).count(),
            count,
            "{family}"
        );
    }
    let discovered: BTreeSet<_> = fs::read_dir(fixtures())
        .unwrap()
        .map(|entry| entry.unwrap())
        .filter(|entry| entry.file_type().unwrap().is_dir())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    let recorded: BTreeSet<_> = cases
        .iter()
        .map(|case| case["id"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(recorded, discovered);
    assert_eq!(recorded.len(), 95);
    let matrix: String = cases
        .iter()
        .map(|case| {
            format!(
                "{}/{}\t{}\n",
                case["id"].as_str().unwrap(),
                case["entry"].as_str().unwrap(),
                if accepts(case) { "accept" } else { "reject" }
            )
        })
        .collect();
    assert_eq!(
        fs::read_to_string(fixtures().join("module-systems-checker-matrix.tsv"))
            .unwrap()
            .replace("\r\n", "\n"),
        matrix
    );
    for case in cases {
        let directory = fixtures().join(case["id"].as_str().unwrap());
        let config: Value =
            serde_json::from_str(&fs::read_to_string(directory.join("tsconfig.json")).unwrap())
                .unwrap();
        assert_eq!(
            config["compilerOptions"]["module"],
            case["reference"]["module"]
        );
        assert_eq!(config["compilerOptions"]["strict"], true);
        assert_eq!(config["compilerOptions"]["declaration"], true);
        assert_eq!(config["compilerOptions"]["noEmitOnError"], true);
        for file in config["files"].as_array().unwrap() {
            assert!(directory.join(file.as_str().unwrap()).is_file());
        }
    }
}

#[test]
fn module_verdicts_and_primary_diagnostics_match_pinned_typescript() {
    let root = fs::canonicalize(env::temp_dir())
        .unwrap()
        .join(format!("bluets-module-decisions-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let mut failures = Vec::new();
    for case in cases() {
        let id = case["id"].as_str().unwrap();
        let directory = root.join(id);
        copy_project(&fixtures().join(id), &directory);
        let output = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
            .arg("--project")
            .arg(directory.join("tsconfig.json"))
            .args(["--noEmit", "--diagnostics-json"])
            .output()
            .unwrap();
        let diagnostics = String::from_utf8_lossy(&output.stderr)
            .lines()
            .map(serde_json::from_str::<Value>)
            .collect::<Result<Vec<_>, _>>();
        let Ok(diagnostics) = diagnostics else {
            failures.push(format!(
                "{id}: unstructured diagnostic: {}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ));
            continue;
        };
        if output.status.success() != accepts(&case) || (accepts(&case) && !diagnostics.is_empty())
        {
            failures.push(format!(
                "{id}: expected accept={}: {diagnostics:?}",
                accepts(&case)
            ));
            continue;
        }
        if accepts(&case) {
            continue;
        }
        let expected = &case["reference"]["diagnostics"][0];
        let Some(actual) = diagnostics.first().map(|item| &item["typescript"]) else {
            failures.push(format!("{id}: missing primary diagnostic"));
            continue;
        };
        if actual["code"] != expected["code"] || actual["message"] != expected["message"] {
            failures.push(format!("{id}: expected {expected}; received {actual}"));
            continue;
        }
        let expected_file = expected["file"]
            .as_str()
            .unwrap_or_else(|| case["entry"].as_str().unwrap());
        let source = fs::read_to_string(directory.join(expected_file)).unwrap();
        let start = expected["start"].as_u64().unwrap() as usize;
        let prefix = &source[..start];
        let line = prefix.bytes().filter(|byte| *byte == b'\n').count() + 1;
        let column = prefix.rsplit('\n').next().unwrap().chars().count() + 1;
        let actual_file = actual["span"]["module"]
            .as_str()
            .and_then(|module| module.rsplit('/').next());
        if actual_file != Some(expected_file)
            || actual["position"]["line"] != line
            || actual["position"]["column"] != column
            || actual["position"]["length"] != expected["length"]
        {
            failures.push(format!("{id}: primary origin differs: {actual}"));
        }
    }
    fs::remove_dir_all(root).unwrap();
    assert!(
        failures.is_empty(),
        "{} module decisions differ:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
