// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Selected-root publication preserves unrelated files and verifies retirement.

use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Temporary(PathBuf);
impl Temporary {
    fn new() -> Self {
        let directory = std::env::temp_dir().canonicalize().unwrap().join(format!(
            "bluets-program-publish-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&directory).unwrap();
        Self(directory)
    }
}
impl Drop for Temporary {
    fn drop(&mut self) {
        if std::thread::panicking() {
            eprintln!("Retained failed publication fixture: {}", self.0.display());
        } else {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }
}

fn fixture() -> Value {
    serde_json::from_str(include_str!("fixtures/program_roots/owner-controls.json")).unwrap()
}

fn update(root: &Path, input: &Value, profile: &Value, state: &Value, mode: &str) {
    for (name, text) in state["files"].as_object().unwrap() {
        fs::write(root.join(name), text.as_str().unwrap()).unwrap();
    }
    let mut options = input["options"].as_object().unwrap().clone();
    options.extend(profile["compilerOptions"].as_object().unwrap().clone());
    if mode == "incremental" {
        options.insert("incremental".into(), json!(true));
    }
    let config = json!({"compilerOptions":options,"files":state["roots"]});
    fs::write(
        root.join("tsconfig.json"),
        serde_json::to_vec_pretty(&config).unwrap(),
    )
    .unwrap();
}

fn run(root: &Path, mode: &str) -> Output {
    let arguments = match mode {
        "legacy" => vec!["build", "--config", "tsconfig.json", "--diagnostics-json"],
        "native" => vec!["--project", "tsconfig.json", "--diagnostics-json"],
        "incremental" => vec![
            "--build",
            "tsconfig.json",
            "--verbose",
            "--diagnostics-json",
        ],
        _ => panic!("unknown authored CLI surface {mode}"),
    };
    let output = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
        .current_dir(root)
        .args(arguments)
        .output()
        .unwrap();
    fs::write(root.join("last-build.stdout"), &output.stdout).unwrap();
    fs::write(root.join("last-build.stderr"), &output.stderr).unwrap();
    output
}

fn physical_inventory(root: &Path) -> BTreeMap<String, Value> {
    fn visit(root: &Path, directory: &Path, result: &mut BTreeMap<String, Value>) {
        if !directory.exists() {
            return;
        }
        for entry in fs::read_dir(directory).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            let kind = entry.file_type().unwrap();
            let name = path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            if kind.is_dir() {
                visit(root, &path, result);
            } else if kind.is_symlink() {
                result.insert(name, json!({"symlink":fs::read_link(path).unwrap()}));
            } else {
                result.insert(name, json!({"bytes":fs::read(path).unwrap()}));
            }
        }
    }
    let mut result = BTreeMap::new();
    visit(root, root, &mut result);
    result
}

#[test]
fn all_cli_surfaces_retire_removed_roots_and_preserve_unrelated_output() {
    let input = fixture();
    for profile in input["profiles"].as_array().unwrap() {
        for mode in input["cliModes"].as_array().unwrap() {
            let mode = mode.as_str().unwrap();
            let temporary = Temporary::new();
            let mut previous = BTreeMap::new();
            for state in input["states"].as_array().unwrap() {
                let clean = Temporary::new();
                let root = if state["clean"] == true {
                    &clean.0
                } else {
                    &temporary.0
                };
                update(root, &input, profile, state, mode);
                if state["id"] == "cold" {
                    fs::create_dir(root.join("dist")).unwrap();
                    fs::write(
                        root.join("dist/user-owned.js"),
                        input["sentinels"]["dist/user-owned.js"].as_str().unwrap(),
                    )
                    .unwrap();
                }
                let output = run(root, mode);
                let id = format!("{}/{mode}/{}", profile["id"], state["id"]);
                assert_eq!(
                    output.status.success(),
                    state["authoredAccepts"],
                    "{id}: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
                let inventory = physical_inventory(&root.join("dist"));
                if state["authoredAccepts"] == false {
                    assert_eq!(
                        inventory, previous,
                        "{id}: failed build changed physical output"
                    );
                    continue;
                }
                if state["clean"] != true {
                    assert_eq!(
                        fs::read_to_string(root.join("dist/user-owned.js"))
                            .ok()
                            .as_deref(),
                        input["sentinels"]["dist/user-owned.js"].as_str(),
                        "{id}: unrelated output was deleted"
                    );
                }
                for name in state["expectedCurrentArtifacts"].as_array().unwrap() {
                    assert!(
                        inventory.contains_key(name.as_str().unwrap()),
                        "{id}: missing {name}"
                    );
                }
                if state["clean"] == true {
                    let generated = |entries: &BTreeMap<String, Value>| {
                        entries
                            .iter()
                            .filter(|(name, _)| {
                                state["expectedCurrentArtifacts"]
                                    .as_array()
                                    .unwrap()
                                    .contains(&json!(name))
                            })
                            .map(|(name, value)| (name.clone(), value.clone()))
                            .collect::<BTreeMap<_, _>>()
                    };
                    assert_eq!(
                        generated(&inventory),
                        generated(&previous),
                        "{id}: clean output differs from the restored generation"
                    );
                }
                if !state["roots"]
                    .as_array()
                    .unwrap()
                    .contains(&json!("removed.ts"))
                {
                    for name in [
                        "removed.js",
                        "removed.js.map",
                        "removed.d.ts",
                        "removed.d.ts.map",
                    ] {
                        assert!(
                            !inventory.contains_key(name),
                            "{id}: stale retired output {name}"
                        );
                    }
                }
                previous = inventory;
            }
        }
    }
}

#[test]
fn replaced_retirement_targets_refuse_before_any_publication() {
    retirement_tamper(false);
}

#[cfg(unix)]
#[test]
fn symlinked_retirement_targets_refuse_without_following_or_removing_them() {
    retirement_tamper(true);
}

fn retirement_tamper(symlink: bool) {
    let input = fixture();
    for profile in input["profiles"].as_array().unwrap() {
        for mode in input["cliModes"].as_array().unwrap() {
            let mode = mode.as_str().unwrap();
            let temporary = Temporary::new();
            update(&temporary.0, &input, profile, &input["states"][0], mode);
            assert!(run(&temporary.0, mode).status.success());
            let target = temporary.0.join("dist/removed.js");
            fs::remove_file(&target).unwrap();
            let outside = temporary.0.join("outside.js");
            let sentinel = "// replacement sentinel\n";
            fs::write(&outside, sentinel).unwrap();
            if symlink {
                #[cfg(unix)]
                std::os::unix::fs::symlink(&outside, &target).unwrap();
            } else {
                fs::write(&target, sentinel).unwrap();
            }
            let previous = physical_inventory(&temporary.0.join("dist"));
            update(&temporary.0, &input, profile, &input["states"][2], mode);
            let output = run(&temporary.0, mode);
            assert!(
                !output.status.success(),
                "{}/{mode}: accepted modified retirement target",
                profile["id"]
            );
            assert_eq!(physical_inventory(&temporary.0.join("dist")), previous);
            assert_eq!(fs::read_to_string(outside).unwrap(), sentinel);
        }
    }
}
