// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Reproduced project prerequisites through strict public project compilation.

use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Temporary(PathBuf);
impl Temporary {
    fn new() -> Self {
        let root = std::env::temp_dir().canonicalize().unwrap().join(format!(
            "bluets-defu-prerequisites-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        fs::create_dir(root.join("src")).unwrap();
        Self(root)
    }
}
impl Drop for Temporary {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn strict_public_projects_match_native_verdicts_and_accepted_declarations() {
    compare_projects(false);
}

fn compare_projects(runtime_only: bool) {
    let input: Value =
        serde_json::from_str(include_str!("fixtures/defu_prerequisites/cases.json")).unwrap();
    let reference: Value =
        serde_json::from_str(include_str!("fixtures/defu_prerequisites/reference.json")).unwrap();
    assert_eq!(input["cases"].as_array().unwrap().len(), 49);
    assert_eq!(reference["cases"].as_array().unwrap().len(), 49);
    assert_eq!(input["profile"], reference["profile"]);
    let mut failures = Vec::new();
    for (case, row) in input["cases"]
        .as_array()
        .unwrap()
        .iter()
        .zip(reference["cases"].as_array().unwrap())
    {
        if runtime_only && case["runtimeProbe"].is_null() {
            continue;
        }
        for key in ["id", "source", "nativeExpectedAccept"] {
            assert_eq!(case[key], row[key]);
        }
        let temporary = Temporary::new();
        fs::write(
            temporary.0.join("src/main.ts"),
            case["source"].as_str().unwrap(),
        )
        .unwrap();
        let mut config = input["profile"].clone();
        config.as_object_mut().unwrap().remove("id");
        fs::write(
            temporary.0.join("tsconfig.json"),
            serde_json::to_vec_pretty(&config).unwrap(),
        )
        .unwrap();
        let result = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
            .current_dir(&temporary.0)
            .args(["build", "--config", "tsconfig.json", "--diagnostics-json"])
            .output()
            .unwrap();
        let accepted = case["nativeExpectedAccept"].as_bool().unwrap();
        if result.status.success() != accepted {
            failures.push(format!(
                "{}: expected acceptance {accepted}; {}{}",
                case["id"],
                String::from_utf8_lossy(&result.stdout),
                String::from_utf8_lossy(&result.stderr)
            ));
            continue;
        }
        let mut declarations = BTreeMap::new();
        if temporary.0.join("dist").is_dir() {
            for entry in fs::read_dir(temporary.0.join("dist")).unwrap() {
                let file = entry.unwrap().path();
                let name = file.file_name().unwrap().to_string_lossy().to_string();
                if name.ends_with(".d.ts") {
                    declarations.insert(name, fs::read_to_string(file).unwrap());
                }
            }
        }
        if serde_json::to_value(declarations).unwrap() != row["native"]["declarations"] {
            failures.push(format!("{}: complete declaration bytes differ", case["id"]));
        }
        if runtime_only {
            let execution = Command::new("node")
                .current_dir(&temporary.0)
                .args(["-e", case["runtimeProbe"].as_str().unwrap()])
                .output()
                .unwrap();
            let observed = serde_json::json!({
                "exit": execution.status.code(),
                "stdout": String::from_utf8(execution.stdout).unwrap(),
                "stderr": String::from_utf8(execution.stderr).unwrap(),
            });
            if observed != row["native"]["runtime"] {
                failures.push(format!("{}: runtime differs: {observed}", case["id"]));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} mismatches:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3 and Node"]
fn native_prerequisite_controls_remain_reproducible() {
    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../development/browser_core/phase-18-bluets/tools/record_defu_prerequisites.cjs");
    let result = Command::new("node").arg(script).output().unwrap();
    assert!(
        result.status.success(),
        "{}{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    compare_projects(true);
}
