// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Selected root order determines which merged declaration supplies a property.

use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);
mod program_roots_cli_identity;

struct Project(PathBuf);

impl Project {
    fn new(row: &Value) -> Self {
        let base = std::env::temp_dir().join(format!(
            "bluets-root-order-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(base.join("project")).unwrap();
        let project = Self(fs::canonicalize(base).unwrap());
        for (name, text) in row["files"].as_object().unwrap() {
            fs::write(project.root().join(name), text.as_str().unwrap()).unwrap();
        }
        fs::write(
            project.root().join("tsconfig.json"),
            serde_json::to_vec_pretty(&row["config"]).unwrap(),
        )
        .unwrap();
        project
    }

    fn root(&self) -> PathBuf {
        self.0.join("project")
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        if std::thread::panicking() {
            eprintln!("Retained root-order fixture: {}", self.0.display());
        } else {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }
}

fn diagnostic(row: &Value, root: &Path) -> Value {
    assert!(row["code"].is_number(), "missing Native counterpart: {row}");
    let file = row["span"]["module"]
        .as_str()
        .map(|file| program_roots_cli_identity::relative_file(file, root));
    json!({
        "code":row["code"],"message":row["message"],"file":file,
        "position":row["position"],
        "relatedInformation":row["relatedInformation"].as_array().into_iter().flatten()
            .map(|row| diagnostic(row, root)).collect::<Vec<_>>()
    })
}

#[test]
fn native_legacy_and_persistent_cli_preserve_selected_root_order() {
    let fixture: Value = serde_json::from_str(include_str!(
        "fixtures/program_roots/selected-root-order.json"
    ))
    .unwrap();
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
            assert_eq!(
                json!({"accepts":output.status.success(),"diagnostics":diagnostics}),
                row["expected"],
                "{}; {arguments:?}",
                row["id"]
            );
            assert!(!project.root().join("dist").exists());
        }
    }
}
