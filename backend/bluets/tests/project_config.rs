// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! K.2.1: real project configuration is compared at the CLI boundary.

use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::env;
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

const MATRIX: &str = include_str!("fixtures/project_config/config-checker-matrix.tsv");
static NEXT: AtomicU64 = AtomicU64::new(0);

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/project_config")
}

fn cases() -> BTreeSet<String> {
    fs::read_dir(fixtures())
        .unwrap()
        .map(|entry| entry.unwrap())
        .filter(|entry| entry.path().is_dir())
        .map(|entry| entry.file_name().to_str().unwrap().to_string())
        .collect()
}

fn rows() -> Vec<(String, bool)> {
    MATRIX
        .lines()
        .map(|line| {
            let columns: Vec<_> = line.split('\t').collect();
            assert_eq!(columns.len(), 2);
            assert!(matches!(columns[1], "accept" | "reject"));
            (columns[0].to_string(), columns[1] == "accept")
        })
        .collect()
}

fn normalized(mut value: Value) -> Value {
    let options = value["compilerOptions"].as_object_mut().unwrap();
    for key in ["outDir", "rootDir", "baseUrl", "declarationDir"] {
        if let Some(Value::String(path)) = options.get_mut(key) {
            *path = path.replace('\\', "/").trim_start_matches("./").to_string();
        }
    }
    let files = value["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|path| {
            path.as_str()
                .unwrap()
                .replace('\\', "/")
                .trim_start_matches("./")
                .to_string()
        })
        .collect::<BTreeSet<_>>();
    json!({"compilerOptions": value["compilerOptions"], "files": files})
}

fn blue(root: &Path, command: &str, show: bool) -> Output {
    let mut process = Command::new(env!("CARGO_BIN_EXE_bluetsc"));
    process
        .current_dir(root)
        .args([command, "--config", "tsconfig.json"]);
    if show {
        process.arg("--showConfig");
    }
    process.output().unwrap()
}

fn report(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn temporary(label: &str) -> PathBuf {
    let root = env::temp_dir().join(format!(
        "bluets-config-{label}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&root).unwrap();
    root
}

fn copy_tree(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let target = destination.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

fn pinned_tsc() -> PathBuf {
    let tsc = env::var_os("BLUEICE_BLUETSC_ORACLE")
        .map(PathBuf::from)
        .expect("set BLUEICE_BLUETSC_ORACLE to TypeScript 5.9.3");
    let version = Command::new(&tsc).arg("--version").output().unwrap();
    assert_eq!(
        String::from_utf8_lossy(&version.stdout).trim(),
        "Version 5.9.3"
    );
    tsc
}

#[test]
fn matrix_covers_every_configuration() {
    assert!(rows().len() >= 25);
    assert_eq!(
        rows()
            .into_iter()
            .map(|(name, _)| name)
            .collect::<BTreeSet<_>>(),
        cases()
    );
}

#[test]
fn file_sets_and_all_normalized_options_match_typescript() {
    let mut failures = Vec::new();
    for (name, accepts) in rows() {
        let root = fixtures().join(&name);
        let output = blue(&root, "check", true);
        if output.status.success() != accepts {
            failures.push(format!("{name}: expected {accepts}: {}", report(&output)));
        } else if accepts {
            let actual = normalized(serde_json::from_slice(&output.stdout).unwrap());
            let expected: Value =
                serde_json::from_slice(&fs::read(root.join("show-config.json")).unwrap()).unwrap();
            if actual != expected {
                failures.push(format!("{name}: expected {expected}; actual {actual}"));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
#[ignore = "requires the pinned TypeScript 5.9.3 compiler"]
fn recorded_configuration_matrix_matches_pinned_typescript() {
    let tsc = pinned_tsc();
    let write = env::var("BLUEICE_WRITE_CONFIG_MATRIX").as_deref() == Ok("1");
    let mut matrix = String::new();
    for name in cases() {
        let root = fixtures().join(&name);
        let output = Command::new(&tsc)
            .current_dir(&root)
            .args([
                "--showConfig",
                "--project",
                "tsconfig.json",
                "--pretty",
                "false",
            ])
            .output()
            .unwrap();
        let verdict = if output.status.success() {
            "accept"
        } else {
            "reject"
        };
        matrix.push_str(&format!("{name}\t{verdict}\n"));
        if output.status.success() {
            let actual = normalized(serde_json::from_slice(&output.stdout).unwrap());
            let reference = root.join("show-config.json");
            if write {
                fs::write(
                    reference,
                    format!("{}\n", serde_json::to_string_pretty(&actual).unwrap()),
                )
                .unwrap();
            } else {
                let expected: Value =
                    serde_json::from_slice(&fs::read(reference).unwrap()).unwrap();
                assert_eq!(actual, expected, "{name}");
            }
        }
    }
    if write {
        fs::write(fixtures().join("config-checker-matrix.tsv"), matrix).unwrap();
    } else {
        assert_eq!(matrix.replace("\r\n", "\n"), MATRIX.replace("\r\n", "\n"));
    }
}

#[test]
fn owner_settings_override_only_present_fields_and_bind_configuration_bytes() {
    let root = temporary("owner");
    copy_tree(&fixtures().join("config-declarations"), &root);
    fs::write(
        root.join("bluetsc.json"),
        r#"{"target":"es2020","outDir":"owner-output"}"#,
    )
    .unwrap();
    let output = blue(&root, "build", false);
    assert!(output.status.success(), "{}", report(&output));
    let manifest: Value =
        serde_json::from_slice(&fs::read(root.join("owner-output/bluetsc.manifest.json")).unwrap())
            .unwrap();
    assert_eq!(manifest["target"], "es2020");
    assert_eq!(manifest["declaration"], true);
    assert_eq!(manifest["sourceMap"], true);
    let before = blue(&root, "check", false);
    assert!(before.status.success(), "{}", report(&before));
    let config = root.join("tsconfig.json");
    let text = fs::read_to_string(&config).unwrap();
    fs::write(config, format!("// changed authorized input\n{text}")).unwrap();
    let after = blue(&root, "check", false);
    assert!(after.status.success(), "{}", report(&after));
    assert_ne!(
        before.stdout, after.stdout,
        "configuration bytes enter the fingerprint"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn unknown_options_name_the_option() {
    let output = blue(&fixtures().join("config-unknown-option"), "check", true);
    assert!(!output.status.success());
    assert!(
        report(&output).contains("typoOption"),
        "{}",
        report(&output)
    );
}

#[cfg(unix)]
#[test]
fn configurations_files_and_outputs_cannot_follow_links_outside_the_owner_root() {
    let temporary = temporary("links");
    let root = temporary.join("project");
    let outside = temporary.join("outside");
    fs::create_dir_all(&outside).unwrap();
    fs::write(
        outside.join("base.json"),
        r#"{"compilerOptions":{"target":"ES2022"}}"#,
    )
    .unwrap();
    fs::write(
        outside.join("main.ts"),
        "export const answer: number = 42;\n",
    )
    .unwrap();
    copy_tree(&fixtures().join("config-basic"), &root);
    symlink(&outside, root.join("linked")).unwrap();
    for config in [
        json!({"extends":"./linked/base.json", "files":["src/main.ts"]}),
        json!({"compilerOptions":{"target":"ES2022"}, "files":["linked/main.ts"]}),
        json!({"compilerOptions":{"target":"ES2022"}, "include":["linked/**/*.ts"]}),
        json!({"compilerOptions":{"target":"ES2022","outDir":"linked/dist"}, "files":["src/main.ts"]}),
    ] {
        fs::write(root.join("tsconfig.json"), config.to_string()).unwrap();
        let output = blue(&root, "check", true);
        assert!(!output.status.success(), "escaped owner root: {config}");
        assert!(report(&output).contains("outside"), "{}", report(&output));
    }
    fs::remove_dir_all(temporary).unwrap();
}

#[test]
#[ignore = "requires pinned TypeScript and Node for emit/declaration comparison"]
fn configuration_builds_match_node_and_exact_declarations() {
    let tsc = pinned_tsc();
    let node = env::var_os("BLUEICE_NODE").unwrap_or_else(|| "node".into());
    let root = temporary("emit");
    copy_tree(&fixtures().join("config-declarations"), &root);
    let output = blue(&root, "build", false);
    assert!(output.status.success(), "{}", report(&output));
    let reference = root.join("reference");
    let output = Command::new(tsc)
        .current_dir(&root)
        .args([
            "--project",
            "tsconfig.json",
            "--pretty",
            "false",
            "--outDir",
        ])
        .arg(&reference)
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", report(&output));
    assert_eq!(
        fs::read_to_string(root.join("build/main.d.ts")).unwrap(),
        fs::read_to_string(reference.join("main.d.ts")).unwrap()
    );
    let mut results = Vec::new();
    for directory in [root.join("build"), reference] {
        fs::write(directory.join("package.json"), r#"{"type":"module"}"#).unwrap();
        let output = Command::new(&node)
            .current_dir(directory)
            .args(["--input-type=module", "-e"])
            .arg("import { answer } from './main.js'; console.log(answer);")
            .output()
            .unwrap();
        assert!(output.status.success(), "{}", report(&output));
        results.push(output.stdout);
    }
    assert_eq!(results[0], b"42\n");
    assert_eq!(results[0], results[1]);
    fs::remove_dir_all(root).unwrap();
}
