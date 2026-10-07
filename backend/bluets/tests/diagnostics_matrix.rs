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
fn primary_codes_templates_and_positions_match_the_pinned_corpus() {
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
        for diagnostic in &diagnostics {
            if diagnostic["typescript"].is_null()
                && diagnostic["noTypeScriptCounterpart"].as_str().is_none()
            {
                failures.push(format!("{id}: unmapped diagnostic: {diagnostic}"));
            }
        }
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
        let expected_position = serde_json::json!({
            "line": expected["line"], "column": expected["column"], "length": expected["length"],
        });
        if actual["position"] != expected_position {
            failures.push(format!(
                "{id}: expected position {expected_position}; received {}",
                actual["position"]
            ));
        }
        let source_root = if let Some(config) = case["config"].as_str() {
            Path::new(config).parent().unwrap().to_path_buf()
        } else {
            Path::new("typescript_oracle")
                .join(case["entry"].as_str().unwrap())
                .parent()
                .unwrap()
                .to_path_buf()
        };
        let expected_module = Path::new(expected["file"].as_str().unwrap())
            .strip_prefix(source_root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        if actual["span"]["module"] != expected_module {
            failures.push(format!(
                "{id}: expected source module {expected_module}; received {}",
                actual["span"]["module"]
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
    let catalog = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../development/browser_core/phase-18-bluets/tools/record_diagnostic_catalog.cjs");
    let output = Command::new("node").arg(catalog).output().unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
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

#[test]
fn primary_messages_and_related_information_match_pinned_typescript() {
    let mut failures = Vec::new();
    let gaps: Value = serde_json::from_str(include_str!(
        "fixtures/diagnostics/presentation-type-fact-gaps.json"
    ))
    .unwrap();
    assert_eq!(gaps["version"], "5.9.3");
    let mut observed_gaps = BTreeSet::new();
    for case in reference()["cases"].as_array().unwrap() {
        let expected = &case["first"];
        if expected.is_null() {
            continue;
        }
        let mut command = Command::new(env!("CARGO_BIN_EXE_bluetsc"));
        let source_root = if let Some(config) = case["config"].as_str() {
            command
                .arg("--project")
                .arg(fixtures().join(config))
                .arg("--noEmit");
            Path::new(config).parent().unwrap().to_path_buf()
        } else {
            let entry = Path::new("typescript_oracle").join(case["entry"].as_str().unwrap());
            command.arg("check").arg(fixtures().join(&entry));
            command.args(legacy_flags(case["flags"].as_array().unwrap()));
            entry.parent().unwrap().to_path_buf()
        };
        let output = command.arg("--diagnostics-json").output().unwrap();
        let diagnostics = String::from_utf8_lossy(&output.stderr)
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .collect::<Vec<_>>();
        let actual = &diagnostics[0]["typescript"];
        let related = expected["related"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|item| {
                let file = item["file"].as_str().unwrap();
                let module = if file.starts_with("<typescript-lib>/") {
                    file.to_string()
                } else {
                    Path::new(file)
                        .strip_prefix(&source_root)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/")
                };
                serde_json::json!({"code":item["code"],"message":item["message"],"module":module,
                "position":{"line":item["line"],"column":item["column"],"length":item["length"]}})
            })
            .collect::<Vec<_>>();
        let actual_related = actual["relatedInformation"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|item| {
                serde_json::json!({"code":item["code"],"message":item["message"],
                "module":item["span"]["module"],"position":item["position"]})
            })
            .collect::<Vec<_>>();
        // The library API exposes virtual module identities, not the oracle's
        // absolute corpus root. Normalize only that root in embedded type names.
        let expected_message = expected["message"].as_str().unwrap().replace(
            &format!(
                "<fixtures>/{}/",
                source_root.to_string_lossy().replace('\\', "/")
            ),
            "",
        );
        let compared_message = if let Some(gap) = gaps["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|gap| gap["id"] == case["id"])
        {
            let source = fs::read(
                fixtures()
                    .join("typescript_oracle")
                    .join(case["entry"].as_str().unwrap()),
            )
            .unwrap();
            let digest = ring::digest::digest(&ring::digest::SHA256, &source);
            let hash = digest
                .as_ref()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>();
            assert_eq!(hash, gap["sourceSha256"]);
            assert_eq!(gap["gap"], "G-T1");
            assert_eq!(gap["nextLeaf"], "K.4.1");
            assert_eq!(expected_message, gap["typescriptMessage"]);
            assert_eq!(actual["code"], gap["code"]);
            assert_ne!(gap["blueMessage"], gap["typescriptMessage"]);
            observed_gaps.insert(gap["id"].as_str().unwrap().to_string());
            gap["blueMessage"].as_str().unwrap()
        } else {
            &expected_message
        };
        if actual["message"] != compared_message || actual_related != related {
            failures.push(format!(
                "{}: expected message {} and related {:?}; received {} and {:?}",
                case["id"], expected["message"], related, actual["message"], actual_related
            ));
        }
    }
    assert_eq!(
        observed_gaps,
        gaps["cases"]
            .as_array()
            .unwrap()
            .iter()
            .map(|gap| gap["id"].as_str().unwrap().to_string())
            .collect()
    );
    assert_eq!(observed_gaps.len(), 3);
    let count = failures.len();
    failures.truncate(12);
    assert!(
        failures.is_empty(),
        "{count} presentation mismatches:\n{}",
        failures.join("\n")
    );
}
