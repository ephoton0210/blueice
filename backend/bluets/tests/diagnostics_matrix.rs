// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! K.3 records codes and message templates from every existing checker matrix.

use serde_json::Value;
use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn reference() -> Value {
    serde_json::from_str(include_str!("fixtures/diagnostics/reference.json")).unwrap()
}

fn legacy_flags(flags: &[Value]) -> Vec<String> {
    let mut result = Vec::new();
    let mut flags = flags.iter().map(|flag| flag.as_str().unwrap());
    while let Some(flag) = flags.next() {
        match flag {
            "--strict" | "--allowImportingTsExtensions" | "--noEmit" => {}
            "--lib" => {
                flags.next().unwrap();
            }
            "--module" | "--target" => {
                result.push(flag.to_string());
                result.push(flags.next().unwrap().to_ascii_lowercase());
            }
            "--experimentalDecorators" => result.push("--experimental-decorators".to_string()),
            "--esModuleInterop" => result.push("--es-module-interop".to_string()),
            "--jsxFactory" => result.push("--jsx-factory".to_string()),
            "--jsxFragmentFactory" => result.push("--jsx-fragment-factory".to_string()),
            "--jsxImportSource" => result.push("--jsx-import-source".to_string()),
            value => result.push(value.to_string()),
        }
    }
    result
}

#[test]
fn record_covers_every_existing_checker_matrix() {
    let reference = reference();
    assert_eq!(reference["version"], "5.9.3");
    let cases = reference["cases"].as_array().unwrap();
    let ids = cases
        .iter()
        .map(|case| case["id"].as_str().unwrap().to_string())
        .collect::<BTreeSet<_>>();
    assert_eq!(ids.len(), cases.len());
    let mut expected = BTreeSet::new();
    for directory in ["typescript_oracle", "strictness"] {
        for entry in fs::read_dir(fixtures().join(directory)).unwrap() {
            let path = entry.unwrap().path();
            let Some(matrix) = path
                .file_name()
                .and_then(|name| name.to_str())
                .and_then(|name| name.strip_suffix("-checker-matrix.tsv"))
            else {
                continue;
            };
            for line in fs::read_to_string(&path).unwrap().lines() {
                let (entry, _) = line.split_once('\t').unwrap();
                expected.insert(format!("{matrix}:{entry}"));
            }
        }
    }
    assert_eq!(ids, expected);
    let recorded = include_str!("fixtures/diagnostics/diagnostics-checker-matrix.tsv")
        .lines()
        .map(|line| line.split_once('\t').unwrap())
        .collect::<Vec<_>>();
    assert_eq!(recorded.len(), cases.len());
    for (case, (id, verdict)) in cases.iter().zip(recorded) {
        assert_eq!(case["id"], id);
        assert_eq!(case["accepts"], verdict == "accept");
    }
}

#[test]
fn primary_codes_and_message_templates_match_the_pinned_corpus() {
    let reference = reference();
    let no_counterpart: Vec<Value> = serde_json::from_str(include_str!(
        "fixtures/diagnostics/no-typescript-counterpart.json"
    ))
    .unwrap();
    let mut failures = Vec::new();
    for case in reference["cases"].as_array().unwrap() {
        let id = case["id"].as_str().unwrap();
        let mut command = Command::new(env!("CARGO_BIN_EXE_bluetsc"));
        if let Some(config) = case["config"].as_str() {
            command.args(["--project"]).arg(fixtures().join(config));
            command.arg("--noEmit");
        } else {
            command.arg("check").arg(
                fixtures()
                    .join("typescript_oracle")
                    .join(case["entry"].as_str().unwrap()),
            );
            command.args(legacy_flags(case["flags"].as_array().unwrap()));
        }
        let output = command.arg("--diagnostics-json").output().unwrap();
        let text = String::from_utf8_lossy(&output.stderr);
        let diagnostics = text
            .lines()
            .map(serde_json::from_str::<Value>)
            .collect::<Result<Vec<_>, _>>();
        let Ok(diagnostics) = diagnostics else {
            failures.push(format!(
                "{id}: diagnostics are not structured JSON: {}",
                text.lines().next().unwrap_or("empty response")
            ));
            continue;
        };
        let expected = &case["first"];
        if let Some(recorded) = no_counterpart.iter().find(|entry| entry["id"] == id) {
            let Some(primary) = diagnostics.first() else {
                failures.push(format!("{id}: missing recorded subset refusal"));
                continue;
            };
            if primary["btsCode"] != recorded["btsCode"]
                || primary["rawMessage"] != recorded["message"]
                || !primary["typescript"].is_null()
                || primary["noTypeScriptCounterpart"].as_str().is_none()
            {
                failures.push(format!("{id}: recorded subset refusal changed: {primary}"));
            }
            continue;
        }
        if expected.is_null() {
            if !output.status.success() || !diagnostics.is_empty() {
                failures.push(format!(
                    "{id}: accepted program has a non-refusal diagnostic"
                ));
            }
            continue;
        }
        let Some(primary) = diagnostics.first() else {
            failures.push(format!("{id}: missing primary diagnostic"));
            continue;
        };
        let actual = &primary["typescript"];
        let expected_template = reference["templates"]
            .as_array()
            .unwrap()
            .iter()
            .find(|template| template["code"] == expected["code"])
            .unwrap();
        if actual["code"] != expected["code"]
            || actual["messageTemplate"] != expected_template["message"]
            || !primary["btsCode"]
                .as_str()
                .is_some_and(|code| code.starts_with("BTS"))
        {
            failures.push(format!(
                "{id}: expected {expected_template}; received {primary}"
            ));
        }
    }
    let count = failures.len();
    failures.truncate(20);
    assert!(
        failures.is_empty(),
        "{count} diagnostic mismatches:\n{}",
        failures.join("\n")
    );
}

#[test]
#[ignore = "requires BLUEICE_BLUETSC_ORACLE to point to the pinned TypeScript compiler"]
fn recorded_codes_messages_and_spans_match_pinned_typescript() {
    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../development/browser_core/phase-18-bluets/tools/record_diagnostics.cjs");
    let output = Command::new("node").arg(script).output().unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
