// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Library availability is independent of the JavaScript syntax target.

use serde_json::{json, Value};
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct Project(PathBuf);

impl Project {
    fn new(source: &str) -> Self {
        let root = fs::canonicalize(std::env::temp_dir())
            .unwrap()
            .join(format!(
                "bluets-library-options-{}-{}",
                std::process::id(),
                SEQUENCE.fetch_add(1, Ordering::Relaxed)
            ));
        fs::create_dir(&root).unwrap();
        fs::write(root.join("main.ts"), source).unwrap();
        Self(root)
    }

    fn configure(&self, options: Value) {
        fs::write(
            self.0.join("tsconfig.json"),
            json!({"files":["main.ts"],"compilerOptions":options}).to_string(),
        )
        .unwrap();
    }

    fn run(&self, arguments: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_bluetsc"))
            .args(arguments)
            .current_dir(&self.0)
            .output()
            .unwrap()
    }

    fn build(&self) -> Value {
        let result = self.run(&["build", "--config", "tsconfig.json"]);
        assert!(
            result.status.success(),
            "{}{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        serde_json::from_str(&fs::read_to_string(self.0.join("out/bluetsc.manifest.json")).unwrap())
            .unwrap()
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn selected_libraries_and_syntax_targets_match_pinned_verdicts() {
    let reference: Value = serde_json::from_str(include_str!(
        "fixtures/target_library_options/reference.json"
    ))
    .unwrap();
    assert_eq!(reference["typescriptVersion"], "5.9.3");
    for case in reference["cases"].as_array().unwrap() {
        let project = Project::new(case["source"].as_str().unwrap());
        project.configure(case["options"].clone());
        let result = project.run(&["check", "--config", "tsconfig.json"]);
        assert_eq!(
            result.status.success(),
            case["accepts"].as_bool().unwrap(),
            "{}: {}{}",
            case["id"],
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
    }
}

#[test]
fn library_selection_is_recorded_independently_in_artifact_identity() {
    let project = Project::new("export function answer(): number { return 42; }");
    project.configure(json!({"target":"es2022","module":"es2022","lib":["ES2020"],"outDir":"out"}));
    let older = project.build();
    assert_eq!(older["target"], "es2022");
    assert_eq!(older["standardLibrary"]["libraries"], json!(["es2020"]));
    project.configure(json!({"target":"es2022","module":"es2022","lib":["es2022"],"outDir":"out"}));
    let newer = project.build();
    assert_eq!(newer["standardLibrary"]["libraries"], json!(["es2022"]));
    assert_ne!(older["fingerprint"], newer["fingerprint"]);
    assert_ne!(
        older["standardLibrary"]["sourceFingerprint"],
        newer["standardLibrary"]["sourceFingerprint"]
    );
}

#[test]
fn iteration_policy_changes_the_manifest_and_fingerprint() {
    let project = Project::new("export function answer(): number { return 42; }");
    project.configure(
        json!({"target":"es2020","module":"es2022","outDir":"out","downlevelIteration":false}),
    );
    let indexed = project.build();
    assert_eq!(indexed["downlevelIteration"], false);
    project.configure(
        json!({"target":"es2020","module":"es2022","outDir":"out","downlevelIteration":true}),
    );
    let iterator = project.build();
    assert_eq!(iterator["downlevelIteration"], true);
    assert_ne!(indexed["fingerprint"], iterator["fingerprint"]);
}

#[test]
fn library_case_and_es6_alias_are_normalized_in_show_config() {
    let project = Project::new("export function answer(): number { return 42; }");
    project
        .configure(json!({"target":"es2022","lib":["ES2015","ES2020"],"downlevelIteration":true}));
    let result = project.run(&["check", "--config", "tsconfig.json", "--showConfig"]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let shown: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(shown["compilerOptions"]["lib"], json!(["es6", "es2020"]));
    assert_eq!(shown["compilerOptions"]["downlevelIteration"], true);
}
