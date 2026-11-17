// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Ordinary build cleanup preserves changed files; sealed publication refuses.

use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    base: PathBuf,
    project: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let base = std::env::temp_dir().canonicalize().unwrap().join(format!(
            "bluets-retirement-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let project = base.join("project");
        fs::create_dir_all(project.join("src")).unwrap();
        fs::write(project.join("src/a.ts"), "export const a = 1;\n").unwrap();
        fs::write(project.join("src/b.ts"), "export const b = 2;\n").unwrap();
        Self { base, project }
    }

    fn configure(&self, mode: &str, roots: &[&str]) {
        let mut options = json!({
            "composite": true, "declaration": true, "declarationMap": true,
            "sourceMap": true, "rootDir": "src", "outDir": "dist",
            "target": "ES5", "module": "CommonJS"
        });
        if mode == "project" {
            options.as_object_mut().unwrap().remove("composite");
        }
        if mode == "incremental" {
            options["incremental"] = json!(true);
        }
        fs::write(
            self.project.join("tsconfig.json"),
            serde_json::to_vec_pretty(&json!({"compilerOptions":options,"files":roots})).unwrap(),
        )
        .unwrap();
    }

    fn run(&self, mode: &str) -> Output {
        Command::new(env!("CARGO_BIN_EXE_bluetsc"))
            .env("BLUETSC_OWNER_STATE_DIR", self.base.join("owner-state"))
            .current_dir(&self.project)
            .args([
                if mode == "project" {
                    "--project"
                } else {
                    "--build"
                },
                "tsconfig.json",
                "--pretty",
                "false",
            ])
            .output()
            .unwrap()
    }

    fn receipt(&self) -> Vec<u8> {
        let paths = fs::read_dir(self.base.join("owner-state"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect::<Vec<_>>();
        assert_eq!(paths.len(), 1);
        fs::read(&paths[0]).unwrap()
    }

    fn remove_root(&self, mode: &str, delete_source: bool) {
        fs::write(self.project.join("src/a.ts"), "export const a = 3;\n").unwrap();
        if delete_source {
            fs::remove_file(self.project.join("src/b.ts")).unwrap();
        }
        self.configure(mode, &["src/a.ts"]);
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if std::thread::panicking() {
            eprintln!("Retained retirement regression: {}", self.base.display());
        } else {
            fs::remove_dir_all(&self.base).unwrap();
        }
    }
}

fn inventory(directory: &Path) -> BTreeMap<String, Value> {
    fn visit(root: &Path, directory: &Path, result: &mut BTreeMap<String, Value>) {
        for entry in fs::read_dir(directory).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            let name = path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            let kind = entry.file_type().unwrap();
            if kind.is_dir() {
                result.insert(name, json!({"directory":true}));
                visit(root, &path, result);
            } else if kind.is_symlink() {
                result.insert(name, json!({"symlink":fs::read_link(path).unwrap()}));
            } else {
                result.insert(
                    name,
                    json!({"sha256":
                    blueice_bluets::remote_declarations::sha256_hex(&fs::read(path).unwrap())}),
                );
            }
        }
    }
    let mut result = BTreeMap::new();
    visit(directory, directory, &mut result);
    result
}

fn success(output: Output) {
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn ordinary_build_preserves_modified_obsolete_outputs_without_claiming_them() {
    for delete_source in [false, true] {
        for retained in ["b.js", "b.js.map", "b.d.ts", "b.d.ts.map"] {
            let fixture = Fixture::new();
            fixture.configure("ordinary", &["src/a.ts", "src/b.ts"]);
            success(fixture.run("ordinary"));
            let output = fixture.project.join("dist").join(retained);
            fs::write(&output, "// user-modified output\n").unwrap();
            fixture.remove_root("ordinary", delete_source);
            success(fixture.run("ordinary"));
            assert_eq!(
                fs::read_to_string(&output).unwrap(),
                "// user-modified output\n"
            );
            for artifact in ["b.js", "b.js.map", "b.d.ts", "b.d.ts.map"] {
                if artifact != retained {
                    assert!(!fixture.project.join("dist").join(artifact).exists());
                }
            }
            let receipt: Value = serde_json::from_slice(&fixture.receipt()).unwrap();
            assert!(receipt["outputs"]["files"]
                .get(format!("dist/{retained}"))
                .is_none());
            // A later strict publisher has no authority over this retained file.
            fs::write(fixture.project.join("src/a.ts"), "export const a = 4;\n").unwrap();
            fixture.configure("project", &["src/a.ts"]);
            success(fixture.run("project"));
            assert_eq!(
                fs::read_to_string(output).unwrap(),
                "// user-modified output\n"
            );
        }
    }
}

#[test]
fn ordinary_build_refuses_unsafe_retirement_paths_before_publication() {
    for symlink in [false, true] {
        if symlink && !cfg!(unix) {
            continue;
        }
        let fixture = Fixture::new();
        fixture.configure("ordinary", &["src/a.ts", "src/b.ts"]);
        success(fixture.run("ordinary"));
        let output = fixture.project.join("dist/b.js");
        fs::remove_file(&output).unwrap();
        let outside = fixture.base.join("outside.js");
        fs::write(&outside, "// outside bytes\n").unwrap();
        if symlink {
            #[cfg(unix)]
            std::os::unix::fs::symlink(&outside, &output).unwrap();
        } else {
            fs::create_dir(&output).unwrap();
        }
        let before = inventory(&fixture.project.join("dist"));
        let receipt = fixture.receipt();
        fixture.remove_root("ordinary", true);
        assert!(!fixture.run("ordinary").status.success());
        assert_eq!(inventory(&fixture.project.join("dist")), before);
        assert_eq!(fixture.receipt(), receipt);
        assert_eq!(fs::read_to_string(outside).unwrap(), "// outside bytes\n");
    }
}

#[test]
fn project_and_explicit_incremental_keep_strict_retirement_refusal() {
    for mode in ["project", "incremental"] {
        for delete_source in [false, true] {
            let fixture = Fixture::new();
            fixture.configure(mode, &["src/a.ts", "src/b.ts"]);
            success(fixture.run(mode));
            fs::write(fixture.project.join("dist/b.js"), "// user replacement\n").unwrap();
            let before = inventory(&fixture.project.join("dist"));
            let receipt = fixture.receipt();
            fixture.remove_root(mode, delete_source);
            assert!(!fixture.run(mode).status.success());
            assert_eq!(inventory(&fixture.project.join("dist")), before, "{mode}");
            assert_eq!(fixture.receipt(), receipt, "{mode}");
        }
    }
}
