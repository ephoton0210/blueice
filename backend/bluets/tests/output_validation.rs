// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! K.7.3 option dependencies, invalid values and command overrides.

use serde_json::Value;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::{env, fs};

fn corpus() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/output_validation")
}

fn cases() -> Vec<Value> {
    let reference: Value =
        serde_json::from_str(include_str!("fixtures/output_validation/reference.json")).unwrap();
    assert_eq!(reference["typescript"], "5.9.3");
    reference["cases"].as_array().unwrap().clone()
}

#[test]
fn output_validation_corpus_is_complete() {
    let cases = cases();
    assert_eq!(cases.len(), 18);
    let recorded: BTreeSet<_> = cases
        .iter()
        .map(|case| case["id"].as_str().unwrap().to_string())
        .collect();
    let discovered: BTreeSet<_> = fs::read_dir(corpus())
        .unwrap()
        .map(|entry| entry.unwrap())
        .filter(|entry| entry.file_type().unwrap().is_dir())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(recorded, discovered);
    let matrix: String = cases
        .iter()
        .map(|case| {
            format!(
                "{}/main.ts\t{}\n",
                case["id"].as_str().unwrap(),
                if case["reference"]["diagnostics"]
                    .as_array()
                    .unwrap()
                    .is_empty()
                {
                    "accept"
                } else {
                    "reject"
                }
            )
        })
        .collect();
    assert_eq!(
        fs::read_to_string(corpus().join("output-validation-checker-matrix.tsv"))
            .unwrap()
            .replace("\r\n", "\n"),
        matrix
    );
}

#[test]
fn output_validation_verdicts_and_diagnostic_origins_match_typescript() {
    let mut failures = Vec::new();
    for case in cases() {
        let id = case["id"].as_str().unwrap();
        let directory = corpus().join(id);
        let output = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
            .arg("--project")
            .arg(directory.join("tsconfig.json"))
            .args(
                case["flags"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_str().unwrap()),
            )
            .args(["--noEmit", "--diagnostics-json"])
            .output()
            .unwrap();
        let expected = case["reference"]["diagnostics"].as_array().unwrap();
        let actual: Vec<Value> = String::from_utf8(output.stderr)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        if output.status.success() != expected.is_empty() || actual.len() != expected.len() {
            failures.push(format!(
                "{id}: verdict/diagnostic count differs: {actual:?}"
            ));
            continue;
        }
        let source = fs::read_to_string(directory.join("tsconfig.json")).unwrap();
        for (expected, actual) in expected.iter().zip(actual) {
            let actual = &actual["typescript"];
            let start = expected["start"].as_u64().unwrap() as usize;
            let prefix = &source[..start];
            let line = prefix.bytes().filter(|byte| *byte == b'\n').count() + 1;
            let column = prefix.rsplit('\n').next().unwrap().encode_utf16().count() + 1;
            if actual["code"] != expected["code"]
                || actual["message"] != expected["message"]
                || actual["position"]["line"] != line
                || actual["position"]["column"] != column
                || actual["position"]["length"] != expected["length"]
            {
                failures.push(format!("{id}: diagnostic differs: {actual}"));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3 and Node"]
fn output_validation_observations_match_live_typescript() {
    let recorder = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../development/browser_core/phase-18-bluets/tools/record_output_validation.cjs");
    let output = Command::new("node").arg(recorder).output().unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
#[ignore = "requires Node and pinned output-option observations"]
fn output_validation_overrides_match_execution_and_declarations() {
    let root = fs::canonicalize(env::temp_dir())
        .unwrap()
        .join(format!("bluets-output-validation-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let mut failures = Vec::new();
    for case in cases().into_iter().filter(|case| {
        case["reference"]["diagnostics"]
            .as_array()
            .unwrap()
            .is_empty()
    }) {
        let id = case["id"].as_str().unwrap();
        let directory = root.join(id);
        fs::create_dir_all(&directory).unwrap();
        for file in ["main.ts", "tsconfig.json"] {
            fs::copy(corpus().join(id).join(file), directory.join(file)).unwrap();
        }
        let out = directory.join("out");
        let build = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
            .arg("--project")
            .arg(directory.join("tsconfig.json"))
            .args(
                case["flags"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_str().unwrap()),
            )
            .arg("--outDir")
            .arg(&out)
            .output()
            .unwrap();
        if !build.status.success() {
            failures.push(format!(
                "{id}: build failed: {}",
                String::from_utf8_lossy(&build.stderr)
            ));
            continue;
        }
        let execution = Command::new("node")
            .arg("-e")
            .arg("console.log(JSON.stringify(require(process.argv[1]).result))")
            .arg(out.join("main.js"))
            .output()
            .unwrap();
        if !execution.status.success()
            || serde_json::from_slice::<Value>(&execution.stdout)
                .ok()
                .as_ref()
                != Some(&case["reference"]["result"])
            || fs::read_to_string(out.join("main.d.ts")).unwrap()
                != case["reference"]["declaration"].as_str().unwrap()
        {
            failures.push(format!("{id}: runtime/declaration differs"));
        }
    }
    fs::remove_dir_all(root).unwrap();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
