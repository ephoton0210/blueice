// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! K.1.4: ECMAScript library verdicts use the same target and `--lib` in tsc.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::{env, fs};

const MATRIX: &str = include_str!("fixtures/typescript_oracle/lib-checker-matrix.tsv");

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/typescript_oracle")
}

fn entries() -> BTreeSet<String> {
    fs::read_dir(fixtures())
        .unwrap()
        .map(|entry| entry.unwrap())
        .filter(|entry| {
            entry.path().is_dir() && entry.file_name().to_string_lossy().starts_with("lib-")
        })
        .map(|entry| format!("{}/main.ts", entry.file_name().to_string_lossy()))
        .collect()
}

fn rows() -> Vec<(String, String, bool)> {
    MATRIX
        .lines()
        .map(|line| {
            let fields: Vec<_> = line.split('\t').collect();
            assert_eq!(fields.len(), 3);
            assert!(matches!(fields[1], "es2020" | "es2022"));
            assert!(matches!(fields[2], "accept" | "reject"));
            (
                fields[0].to_string(),
                fields[1].to_string(),
                fields[2] == "accept",
            )
        })
        .collect()
}

fn report(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
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

fn temporary(name: &str) -> PathBuf {
    let root = env::temp_dir().join(format!(
        "bluets-standard-library-{name}-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    root
}

#[test]
fn matrix_covers_every_fixture() {
    assert!(rows().len() >= 60);
    assert_eq!(
        rows()
            .into_iter()
            .map(|(path, _, _)| path)
            .collect::<BTreeSet<_>>(),
        entries()
    );
}

#[test]
fn check_and_build_match_pinned_verdicts() {
    let root = temporary("matrix");
    let mut failures = Vec::new();
    for (index, (path, target, accepts)) in rows().into_iter().enumerate() {
        let checked = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
            .arg("check")
            .arg(fixtures().join(&path))
            .args(["--target", &target])
            .output()
            .unwrap();
        let out = root.join(index.to_string());
        let built = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
            .arg("build")
            .arg(fixtures().join(&path))
            .args(["--target", &target, "--out-dir"])
            .arg(&out)
            .output()
            .unwrap();
        if checked.status.success() != accepts
            || built.status.success() != accepts
            || out.exists() != accepts
        {
            failures.push(format!(
                "{path}: expected {accepts}; check: {}; build: {}",
                report(&checked),
                report(&built)
            ));
        }
    }
    fs::remove_dir_all(root).unwrap();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
#[ignore = "requires BLUEICE_BLUETSC_ORACLE to point to the pinned TypeScript compiler"]
fn recorded_verdicts_match_pinned_typescript() {
    let tsc = pinned_tsc();
    let mut fresh = Vec::new();
    for (path, target, _) in rows() {
        let checked = Command::new(&tsc)
            .args([
                "--target",
                &target,
                "--lib",
                &target,
                "--module",
                "ES2022",
                "--strict",
                "--pretty",
                "false",
                "--allowImportingTsExtensions",
                "--noEmit",
            ])
            .arg(fixtures().join(&path))
            .output()
            .unwrap();
        fresh.push((path, target, checked.status.success()));
    }
    if env::var_os("BLUEICE_WRITE_LIB_MATRIX").is_some() {
        fs::write(
            fixtures().join("lib-checker-matrix.tsv"),
            fresh
                .iter()
                .map(|(path, target, accepts)| {
                    format!(
                        "{path}\t{target}\t{}\n",
                        if *accepts { "accept" } else { "reject" }
                    )
                })
                .collect::<String>(),
        )
        .unwrap();
    } else {
        assert_eq!(fresh, rows());
    }
}

#[test]
fn manifest_records_library_identity_without_publishing_declarations() {
    let root = temporary("manifest");
    let entry = root.join("main.ts");
    fs::write(&entry, "export const value: number = 1;").unwrap();
    let mut identities = Vec::new();
    for target in ["es2020", "es2022"] {
        let out = root.join(target);
        let built = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
            .arg("build")
            .arg(&entry)
            .args(["--target", target, "--out-dir"])
            .arg(&out)
            .output()
            .unwrap();
        assert!(built.status.success(), "{}", report(&built));
        let manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(out.join("bluetsc.manifest.json")).unwrap()).unwrap();
        let library = &manifest["standardLibrary"];
        assert_eq!(library["version"], "blue-ts-ecma-lib-v2");
        assert!(library["sources"]
            .as_array()
            .unwrap()
            .iter()
            .any(|source| source == "ecma-additions.v2.d.ts"));
        assert_eq!(library["target"], target);
        assert!(!library["sources"].as_array().unwrap().is_empty());
        assert!(!library["sourceFingerprint"].as_str().unwrap().is_empty());
        assert_eq!(manifest["declarationModules"], serde_json::json!([]));
        let files: BTreeSet<_> = fs::read_dir(&out)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(
            files,
            ["main.js", "bluetsc.manifest.json"]
                .map(std::ffi::OsString::from)
                .into_iter()
                .collect()
        );
        identities.push(library["sourceFingerprint"].clone());
    }
    assert_ne!(identities[0], identities[1]);
    fs::remove_dir_all(root).unwrap();
}

#[test]
#[ignore = "requires the pinned TypeScript compiler and Node"]
fn library_calls_preserve_node_execution() {
    let tsc = pinned_tsc();
    let root = temporary("runtime");
    fs::write(root.join("package.json"), "{\"type\":\"module\"}").unwrap();
    let entry = fixtures().join("lib-runtime/main.ts");
    let blue = root.join("blue");
    let reference = root.join("reference");
    let built = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
        .arg("build")
        .arg(&entry)
        .arg("--out-dir")
        .arg(&blue)
        .output()
        .unwrap();
    assert!(built.status.success(), "{}", report(&built));
    let built = Command::new(&tsc)
        .args([
            "--target", "ES2022", "--lib", "ES2022", "--module", "ES2022", "--strict", "--pretty",
            "false", "--outDir",
        ])
        .arg(&reference)
        .arg(entry)
        .output()
        .unwrap();
    assert!(built.status.success(), "{}", report(&built));
    let run = |dir: &Path| {
        let output = Command::new("node")
            .arg(dir.join("main.js"))
            .output()
            .unwrap();
        assert!(output.status.success(), "{}", report(&output));
        output.stdout
    };
    assert_eq!(run(&blue), run(&reference));
    fs::remove_dir_all(root).unwrap();
}

#[test]
#[ignore = "requires BLUEICE_BLUETSC_ORACLE to point to the pinned TypeScript compiler"]
fn library_inferred_declarations_match_typescript() {
    let tsc = pinned_tsc();
    let root = temporary("declarations");
    for name in [
        "methods",
        "instances",
        "collections",
        "symbols",
        "shadowed-symbol",
    ] {
        let entry = fixtures().join(format!("stdlib-decl-{name}/main.ts"));
        let blue = root.join(name).join("blue");
        let reference = root.join(name).join("reference");
        let built = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
            .arg("build")
            .arg(&entry)
            .args(["--declaration", "--out-dir"])
            .arg(&blue)
            .output()
            .unwrap();
        assert!(built.status.success(), "{name}: {}", report(&built));
        let built = Command::new(&tsc)
            .args([
                "--target",
                "ES2022",
                "--lib",
                "ES2022",
                "--module",
                "ES2022",
                "--strict",
                "--pretty",
                "false",
                "--declaration",
                "--outDir",
            ])
            .arg(&reference)
            .arg(entry)
            .output()
            .unwrap();
        assert!(built.status.success(), "{name}: {}", report(&built));
        assert_eq!(
            fs::read_to_string(blue.join("main.d.ts")).unwrap(),
            fs::read_to_string(reference.join("main.d.ts")).unwrap(),
            "{name}"
        );
    }
    fs::remove_dir_all(root).unwrap();
}
