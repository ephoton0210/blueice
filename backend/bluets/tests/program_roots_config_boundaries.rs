// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Native-calibrated configuration roots and static layout diagnostics.

use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);
mod program_roots_cli_identity;
const FIXTURE: &str = include_str!("fixtures/program_roots/config-program-boundaries.json");
struct Project(PathBuf);
impl Project {
    fn new(row: &Value) -> Self {
        let base = std::env::temp_dir().join(format!(
            "bluets-config-program-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(base.join("project")).unwrap();
        let result = Self(fs::canonicalize(base).unwrap());
        for (name, source) in row["files"].as_object().unwrap() {
            let file = result.root().join(name);
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(file, source.as_str().unwrap()).unwrap();
        }
        fs::write(
            result.root().join("tsconfig.json"),
            serde_json::to_vec_pretty(&row["config"]).unwrap(),
        )
        .unwrap();
        // Native used pretty JSON with a final newline; token positions agree.
        let config = result.root().join("tsconfig.json");
        let mut text = fs::read_to_string(&config).unwrap();
        text.push('\n');
        fs::write(config, text).unwrap();
        result
    }
    fn root(&self) -> PathBuf {
        self.0.join("project")
    }
}
impl Drop for Project {
    fn drop(&mut self) {
        if std::thread::panicking() {
            eprintln!("Retained config-program fixture: {}", self.0.display());
        } else {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }
}

fn diagnostic(row: &Value, root: &Path) -> Value {
    assert!(row["code"].is_number(), "missing Native counterpart: {row}");
    let file = row["span"]["module"]
        .as_str()
        .filter(|file| !file.is_empty())
        .map(|file| program_roots_cli_identity::relative_file(file, root));
    json!({"code":row["code"],"message":row["message"],"file":file,
        "position":row["position"],"relatedInformation":row["relatedInformation"]
            .as_array().into_iter().flatten().map(|row| diagnostic(row, root)).collect::<Vec<_>>()})
}

fn relocate(value: &mut Value, native: &str, actual: &str) {
    match value {
        Value::String(text) => *text = text.replace(native, actual),
        Value::Array(rows) => rows
            .iter_mut()
            .for_each(|row| relocate(row, native, actual)),
        Value::Object(rows) => rows
            .values_mut()
            .for_each(|row| relocate(row, native, actual)),
        _ => {}
    }
}

#[test]
fn config_inheritance_and_root_dir_match_complete_native_diagnostics_on_all_clis() {
    let fixture: Value = serde_json::from_str(FIXTURE).unwrap();
    for row in fixture["cases"].as_array().unwrap() {
        for arguments in [
            vec!["--project", "tsconfig.json"],
            vec!["check", "--config", "tsconfig.json"],
            vec!["--build", "tsconfig.json"],
        ] {
            let project = Project::new(row);
            let output = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
                .args(&arguments)
                .arg("--diagnostics-json")
                .env("BLUETSC_OWNER_STATE_DIR", project.0.join("owner-state"))
                .current_dir(project.root())
                .output()
                .unwrap();
            fs::write(project.0.join("stdout.log"), &output.stdout).unwrap();
            fs::write(project.0.join("stderr.log"), &output.stderr).unwrap();
            let diagnostics = String::from_utf8(output.stderr)
                .unwrap()
                .lines()
                .map(|line| {
                    let raw: Value = serde_json::from_str(line).unwrap();
                    diagnostic(&raw["typescript"], &project.root())
                })
                .collect::<Vec<_>>();
            let mut expected = row["expected"].clone();
            relocate(
                &mut expected,
                row["nativeProjectRoot"].as_str().unwrap(),
                &project.root().to_string_lossy().replace('\\', "/"),
            );
            assert_eq!(
                json!({"accepts":output.status.success(),"diagnostics":diagnostics}),
                expected,
                "{}; {arguments:?}",
                row["id"]
            );
            assert!(!project.root().join("dist").exists());
        }
    }
}

#[test]
fn showing_configuration_does_not_change_inherited_compilation_roots() {
    let fixture: Value = serde_json::from_str(FIXTURE).unwrap();
    for row in fixture["cases"].as_array().unwrap() {
        if !row["id"].as_str().unwrap().starts_with("null/") {
            continue;
        }
        let project = Project::new(row);
        let output = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
            .args(["--project", "tsconfig.json", "--showConfig"])
            .current_dir(project.root())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let shown: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(
            shown.get("files").unwrap_or(&json!([])),
            &row["shownFiles"],
            "{}",
            row["id"]
        );
    }
}
