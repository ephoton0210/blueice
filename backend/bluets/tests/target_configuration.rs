// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Target selection at each public configuration boundary, without downlevel syntax.

use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

const TARGETS: &[&str] = &[
    "es5", "es2015", "es2016", "es2017", "es2018", "es2019", "es2020", "es2021", "es2022",
    "es2023", "esnext",
];
static SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct Project(PathBuf);

impl Project {
    fn new() -> Self {
        let root = fs::canonicalize(std::env::temp_dir())
            .unwrap()
            .join(format!(
                "bluets-target-config-{}-{}",
                std::process::id(),
                SEQUENCE.fetch_add(1, Ordering::Relaxed)
            ));
        fs::create_dir(&root).unwrap();
        fs::write(
            root.join("main.ts"),
            "export function answer(): number { return 42; }\n",
        )
        .unwrap();
        Self(root)
    }

    fn write(&self, name: &str, value: Value) {
        fs::write(self.0.join(name), value.to_string()).unwrap();
    }

    fn run(&self, arguments: &[&str]) -> std::process::Output {
        Command::new(env!("CARGO_BIN_EXE_bluetsc"))
            .args(arguments)
            .current_dir(&self.0)
            .output()
            .unwrap()
    }

    fn build(&self, arguments: &[&str], target: &str) -> Value {
        let output = self.run(arguments);
        assert!(
            output.status.success(),
            "{target}/{arguments:?}: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let metadata: Value = serde_json::from_str(
            &fs::read_to_string(self.0.join("out/bluetsc.manifest.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(metadata["target"], target);
        assert_eq!(metadata["standardLibrary"]["target"], target);
        assert_eq!(
            metadata["useDefineForClassFields"],
            matches!(target, "es2022" | "es2023" | "esnext")
        );
        assert_eq!(
            fs::read_to_string(self.0.join("out/main.d.ts")).unwrap(),
            "export declare function answer(): number;\n"
        );
        metadata
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn positional_targets_are_case_insensitive_and_have_distinct_artifact_identity() {
    let project = Project::new();
    let mut identities = BTreeSet::new();
    for target in TARGETS {
        let spelling = target.to_ascii_uppercase();
        let metadata = project.build(
            &[
                "build",
                "main.ts",
                "--out-dir",
                "out",
                "--declaration",
                "--target",
                &spelling,
            ],
            target,
        );
        assert!(identities.insert(metadata["fingerprint"].as_str().unwrap().to_string()));
    }
}

#[test]
fn native_and_typescript_configs_select_each_target_and_class_field_default() {
    let native_project = Project::new();
    let typescript_project = Project::new();
    for target in TARGETS {
        native_project.write(
            "bluetsc.json",
            json!({"entries":["main.ts"],"outDir":"out","declaration":true,"target":target}),
        );
        let native = native_project.build(&["build", "--config", "bluetsc.json"], target);
        typescript_project.write(
            "tsconfig.json",
            json!({"files":["main.ts"],"compilerOptions":{
                "target":target.to_ascii_uppercase(),"module":"es2022", "declaration":true,"outDir":"out"
            }}),
        );
        let typescript = typescript_project.build(&["build", "--config", "tsconfig.json"], target);
        assert_eq!(native["standardLibrary"], typescript["standardLibrary"]);
    }
}

#[test]
fn es6_alias_selects_es2015_at_every_configuration_boundary() {
    let project = Project::new();
    project.build(
        &[
            "build",
            "main.ts",
            "--out-dir",
            "out",
            "--declaration",
            "--target",
            "ES6",
        ],
        "es2015",
    );
    project.write(
        "bluetsc.json",
        json!({"entries":["main.ts"],"target":"ES6","declaration":true,"outDir":"out"}),
    );
    project.build(&["build", "--config", "bluetsc.json"], "es2015");
    let project = Project::new();
    project.write(
        "tsconfig.json",
        json!({"files":["main.ts"],"compilerOptions":{
            "target":"ES6","module":"es2022","declaration":true,"outDir":"out"
        }}),
    );
    project.build(&["build", "--config", "tsconfig.json"], "es2015");
}

#[test]
fn omitted_typescript_target_uses_es5_without_changing_the_native_default() {
    let project = Project::new();
    project.write(
        "tsconfig.json",
        json!({"files":["main.ts"],"compilerOptions":{
            "module":"es2022","declaration":true,"outDir":"out"
        }}),
    );
    project.build(&["build", "--config", "tsconfig.json"], "es5");
    let project = Project::new();
    project.write(
        "bluetsc.json",
        json!({"entries":["main.ts"],"declaration":true,"outDir":"out"}),
    );
    project.build(&["build", "--config", "bluetsc.json"], "es2022");
}
