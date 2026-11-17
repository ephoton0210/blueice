// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Publication refusals preserve outputs even when semantic error emission is on.

use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Project(PathBuf);
impl Project {
    fn state_root(&self) -> PathBuf {
        self.0.with_extension("owner-state")
    }
    fn new() -> Self {
        let root = std::env::temp_dir().canonicalize().unwrap().join(format!(
            "bluets-publication-boundary-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        Self(root)
    }

    fn configure(&self, mode: &str, files: &[&str], no_emit_on_error: bool) {
        let mut config = json!({"compilerOptions":{
            "target":"ES5","module":"CommonJS","strict":true,
            "declaration":true,"declarationMap":true,"sourceMap":true,
            "rootDir":".","outDir":"dist","noEmitOnError":no_emit_on_error
        },"files":files});
        if mode == "incremental" {
            config["compilerOptions"]["incremental"] = json!(true);
        }
        fs::write(
            self.0.join("tsconfig.json"),
            serde_json::to_vec_pretty(&config).unwrap(),
        )
        .unwrap();
    }

    fn run(&self, mode: &str) -> Output {
        let args = match mode {
            "legacy" => vec!["build", "--config", "tsconfig.json", "--diagnostics-json"],
            "native" => vec!["--project", "tsconfig.json", "--diagnostics-json"],
            "incremental" => vec!["--build", "tsconfig.json", "--diagnostics-json"],
            _ => unreachable!(),
        };
        let output = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
            .env("BLUETSC_OWNER_STATE_DIR", self.state_root())
            .current_dir(&self.0)
            .args(args)
            .output()
            .unwrap();
        fs::write(self.0.join("stdout.log"), &output.stdout).unwrap();
        fs::write(self.0.join("stderr.log"), &output.stderr).unwrap();
        output
    }
}
impl Drop for Project {
    fn drop(&mut self) {
        if std::thread::panicking() {
            eprintln!("Retained publication boundary: {}", self.0.display());
        } else {
            fs::remove_dir_all(&self.0).unwrap();
            if self.state_root().exists() {
                fs::remove_dir_all(self.state_root()).unwrap();
            }
        }
    }
}

fn inventory(root: &Path) -> BTreeMap<String, Value> {
    fn visit(root: &Path, directory: &Path, result: &mut BTreeMap<String, Value>) {
        for entry in fs::read_dir(directory).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            let name = path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            if entry.file_type().unwrap().is_dir() {
                visit(root, &path, result);
            } else if entry.file_type().unwrap().is_symlink() {
                result.insert(name, json!({"link":fs::read_link(path).unwrap()}));
            } else {
                result.insert(name,json!({"sha256":blueice_bluets::remote_declarations::sha256_hex(&fs::read(path).unwrap())}));
            }
        }
    }
    let mut result = BTreeMap::new();
    visit(root, root, &mut result);
    result
}

#[test]
fn retirement_refusal_preserves_public_state_when_emit_on_error_is_enabled() {
    for mode in ["legacy", "native", "incremental"] {
        for symlink in [false, true] {
            if symlink && !cfg!(unix) {
                continue;
            }
            let project = Project::new();
            fs::write(project.0.join("main.ts"), "export const value: number = 1;").unwrap();
            fs::write(
                project.0.join("removed.ts"),
                "export const removed: number = 2;",
            )
            .unwrap();
            project.configure(mode, &["main.ts", "removed.ts"], false);
            assert!(project.run(mode).status.success());
            let target = project.0.join("dist/removed.js");
            fs::remove_file(&target).unwrap();
            if symlink {
                let outside = project.0.join("outside.js");
                fs::write(&outside, "// untouched outside bytes\n").unwrap();
                #[cfg(unix)]
                std::os::unix::fs::symlink(outside, &target).unwrap();
            } else {
                fs::write(target, "// externally replaced bytes\n").unwrap();
            }
            let before = inventory(&project.0.join("dist"));
            project.configure(mode, &["main.ts"], false);
            assert!(
                !project.run(mode).status.success(),
                "{mode}; symlink={symlink}"
            );
            assert_eq!(
                inventory(&project.0.join("dist")),
                before,
                "{mode}; symlink={symlink}"
            );
        }
    }
}

#[test]
fn unowned_entries_survive_an_actual_changed_source_rebuild() {
    for mode in ["legacy", "native", "incremental"] {
        let project = Project::new();
        fs::write(project.0.join("main.ts"), "export const value: number = 1;").unwrap();
        project.configure(mode, &["main.ts"], true);
        assert!(project.run(mode).status.success());
        fs::create_dir(project.0.join("dist/user-data")).unwrap();
        fs::write(
            project.0.join("dist/user-data/binary"),
            [0, 255, 128, 13, 10],
        )
        .unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink("user-data/binary", project.0.join("dist/user-link")).unwrap();
        fs::write(project.0.join("main.ts"), "export const value: number = 2;").unwrap();
        assert!(project.run(mode).status.success());
        assert_eq!(
            fs::read(project.0.join("dist/user-data/binary")).unwrap(),
            [0, 255, 128, 13, 10]
        );
        #[cfg(unix)]
        assert_eq!(
            fs::read_link(project.0.join("dist/user-link")).unwrap(),
            Path::new("user-data/binary")
        );
    }
}

#[test]
fn output_parent_collisions_preserve_the_prior_generation() {
    for mode in ["legacy", "native", "incremental"] {
        let project = Project::new();
        fs::write(project.0.join("main.ts"), "export const value: number = 1;").unwrap();
        project.configure(mode, &["main.ts"], true);
        assert!(project.run(mode).status.success());
        let before = inventory(&project.0.join("dist"));
        fs::create_dir(project.0.join("branch.js")).unwrap();
        fs::write(
            project.0.join("branch.ts"),
            "export const branch: number = 1;",
        )
        .unwrap();
        fs::write(
            project.0.join("branch.js/leaf.ts"),
            "export const leaf: number = 1;",
        )
        .unwrap();
        project.configure(mode, &["main.ts", "branch.ts", "branch.js/leaf.ts"], true);
        assert!(!project.run(mode).status.success());
        assert_eq!(inventory(&project.0.join("dist")), before, "{mode}");
    }
}

#[test]
fn missing_only_roots_match_native_exits_without_retiring_prior_outputs() {
    let native: Value =
        serde_json::from_str(include_str!("fixtures/program_roots/missing-roots.json")).unwrap();
    for row in native["cases"].as_array().unwrap() {
        let project = Project::new();
        fs::write(project.0.join("main.ts"), "export const value: number = 1;").unwrap();
        let binary = env!("CARGO_BIN_EXE_bluetsc");
        let cold = Command::new(binary)
            .env("BLUETSC_OWNER_STATE_DIR", project.state_root())
            .current_dir(&project.0)
            .args([
                "main.ts",
                "--target",
                "es5",
                "--module",
                "commonjs",
                "--declaration",
                "true",
                "--outDir",
                "dist",
            ])
            .output()
            .unwrap();
        assert!(
            cold.status.success(),
            "{}",
            String::from_utf8_lossy(&cold.stderr)
        );
        let before = inventory(&project.0.join("dist"));
        let receipts = fs::read_dir(project.state_root())
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect::<Vec<_>>();
        assert_eq!(receipts.len(), 1);
        let receipt_path = &receipts[0];
        let receipt = fs::read(receipt_path).unwrap();
        let output = Command::new(binary)
            .env("BLUETSC_OWNER_STATE_DIR", project.state_root())
            .current_dir(&project.0)
            .args(
                row["args"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|arg| arg.as_str().unwrap()),
            )
            .arg("--diagnostics-json")
            .output()
            .unwrap();
        fs::write(project.0.join("stdout.log"), &output.stdout).unwrap();
        fs::write(project.0.join("stderr.log"), &output.stderr).unwrap();
        assert_eq!(
            output.status.code(),
            Some(row["exit"].as_i64().unwrap() as i32),
            "{row}"
        );
        let diagnostics = String::from_utf8(output.stderr)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0]["typescript"]["code"], 6053);
        assert_eq!(inventory(&project.0.join("dist")), before);
        assert_eq!(fs::read(receipt_path).unwrap(), receipt);
    }
}
