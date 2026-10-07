// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! K.3.3 actual CLI presentation, artifacts, execution and exit statuses.
use serde_json::{json, Value};
use std::{fs, path::Path, process::Command};

fn reference() -> Value {
    serde_json::from_str(include_str!(
        "fixtures/diagnostics/presentation-reference.json"
    ))
    .unwrap()
}

fn artifacts(root: &Path, current: &Path, result: &mut Vec<String>) {
    if !current.exists() {
        return;
    }
    for entry in fs::read_dir(current).unwrap() {
        let entry = entry.unwrap();
        if entry.file_type().unwrap().is_dir() {
            artifacts(root, &entry.path(), result);
        } else {
            result.push(
                entry
                    .path()
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/"),
            );
        }
    }
}

#[test]
fn cli_presentation_and_emission_match_pinned_observations() {
    let refusals: Vec<Value> = serde_json::from_str(include_str!(
        "fixtures/diagnostics/presentation-policy-refusals.json"
    ))
    .unwrap();
    let mut failures = Vec::new();
    for (index, row) in reference()["cases"].as_array().unwrap().iter().enumerate() {
        let root = std::env::temp_dir().join(format!(
            "bluets-presentation-{}-{index}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("main.ts"), row["source"].as_str().unwrap()).unwrap();
        for (name, source) in row["extraSources"].as_object().unwrap() {
            fs::write(root.join(name), source.as_str().unwrap()).unwrap();
        }
        fs::write(
            root.join("tsconfig.json"),
            row["configText"].as_str().unwrap(),
        )
        .unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
            .current_dir(&root)
            .env("FORCE_COLOR", "0")
            .args(
                row["args"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|arg| arg.as_str().unwrap()),
            )
            .output()
            .unwrap();
        let mut fields = Vec::new();
        let text = String::from_utf8_lossy(&output.stdout)
            .replace(&*root.to_string_lossy(), "<project>")
            .replace('\\', "/");
        let stdout = text
            .split('\n')
            .filter(|line| {
                let Some((name, value)) = line.split_once(':') else {
                    return true;
                };
                if !row["summaryFields"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|field| field["name"] == name)
                {
                    return true;
                }
                let value = value.trim();
                let (number, unit) = if let Some(number) = value.strip_suffix('K') {
                    (number, "K")
                } else if let Some(number) = value.strip_suffix('s') {
                    (number, "s")
                } else {
                    (value, "count")
                };
                let measurement = number.parse::<f64>().expect("numeric summary measurement");
                assert!(measurement.is_finite() && measurement >= 0.0);
                fields.push(json!({"name":name,"unit":unit}));
                false
            })
            .collect::<Vec<_>>()
            .join("\n");
        let mut assets = Vec::new();
        artifacts(&root.join("out"), &root.join("out"), &mut assets);
        assets.sort();
        let runtime = if root.join("out/main.js").is_file() {
            let run = Command::new("node")
                .args(["--input-type=module", "--eval"])
                .arg(fs::read_to_string(root.join("out/main.js")).unwrap())
                .output()
                .unwrap();
            assert!(
                run.status.success(),
                "{}",
                String::from_utf8_lossy(&run.stderr)
            );
            json!(String::from_utf8(run.stdout).unwrap())
        } else {
            Value::Null
        };
        let actual = json!({"exit":output.status.code(),"stdout":stdout,"stderr":String::from_utf8_lossy(&output.stderr),"summaryFields":fields,"assets":assets,"runtime":runtime});
        let mut expected = json!({"exit":row["exit"],"stdout":row["stdout"],"stderr":row["stderr"],"summaryFields":row["summaryFields"],"assets":row["assets"],"runtime":row["runtime"]});
        if let Some(refusal) = refusals
            .iter()
            .find(|refusal| refusal["name"] == row["name"])
        {
            let report = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
                .current_dir(&root)
                .args(["--project", ".", "--diagnostics-json"])
                .output()
                .unwrap();
            let diagnostic: Value = serde_json::from_slice(&report.stderr).unwrap();
            assert_eq!(diagnostic["btsCode"], refusal["btsCode"]);
            assert_eq!(diagnostic["rawMessage"], refusal["rawMessage"]);
            assert_eq!(diagnostic["typescript"]["code"], 5023);
            for field in ["exit", "assets", "runtime"] {
                expected[field] = refusal[field].clone();
            }
        }
        if actual != expected {
            failures.push(format!(
                "{}: expected {expected}; observed {actual}",
                row["name"]
            ));
        }
        assert_eq!(
            fs::read_to_string(root.join("main.ts")).unwrap(),
            row["source"]
        );
        for (name, source) in row["extraSources"].as_object().unwrap() {
            assert_eq!(
                fs::read_to_string(root.join(name)).unwrap(),
                source.as_str().unwrap()
            );
        }
        fs::remove_dir_all(root).unwrap();
    }
    assert!(
        failures.is_empty(),
        "{} presentation failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
#[ignore = "requires BLUEICE_BLUETSC_ORACLE to point to TypeScript 5.9.3"]
fn recorded_presentation_matches_pinned_typescript() {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join(
        "../../development/browser_core/phase-18-bluets/tools/record_diagnostic_presentation.cjs",
    );
    let output = Command::new("node").arg(script).output().unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
