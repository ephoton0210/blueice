// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! K.1.2: immutable mutation checking through the public compiler CLI.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::{env, fs};

const MATRIX: &str = include_str!("fixtures/typescript_oracle/immutable-checker-matrix.tsv");

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/typescript_oracle")
}

fn entries() -> BTreeSet<String> {
    fs::read_dir(fixtures())
        .unwrap()
        .map(|entry| entry.unwrap())
        .filter(|entry| {
            entry.path().is_dir()
                && entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("immutable-")
        })
        .map(|entry| format!("{}/main.ts", entry.file_name().to_string_lossy()))
        .collect()
}

fn rows() -> Vec<(String, bool)> {
    MATRIX
        .lines()
        .map(|line| {
            let (path, verdict) = line.split_once('\t').unwrap();
            assert!(matches!(verdict, "accept" | "reject"));
            (path.to_string(), verdict == "accept")
        })
        .collect()
}

fn module_kind(path: &str) -> &str {
    if path.starts_with("immutable-require-") {
        "commonjs"
    } else {
        "ES2022"
    }
}

fn module_arguments(path: &str) -> Vec<&str> {
    if module_kind(path) == "commonjs" {
        vec!["--module", "commonjs"]
    } else {
        Vec::new()
    }
}

fn report(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[test]
fn matrix_covers_every_fixture() {
    assert!(rows().len() >= 40);
    assert_eq!(
        rows()
            .into_iter()
            .map(|(path, _)| path)
            .collect::<BTreeSet<_>>(),
        entries()
    );
}

#[test]
fn check_and_build_match_pinned_verdicts() {
    let root = env::temp_dir().join(format!("bluets-immutable-matrix-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let mut failures = Vec::new();
    for (index, (path, accepts)) in rows().into_iter().enumerate() {
        let checked = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
            .arg("check")
            .arg(fixtures().join(&path))
            .args(module_arguments(&path))
            .output()
            .unwrap();
        let out = root.join(index.to_string());
        let built = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
            .arg("build")
            .arg(fixtures().join(&path))
            .args(module_arguments(&path))
            .arg("--out-dir")
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
    let _ = fs::remove_dir_all(root);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
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
#[ignore = "requires BLUEICE_BLUETSC_ORACLE to point to the pinned TypeScript compiler"]
fn recorded_verdicts_match_pinned_typescript() {
    let tsc = pinned_tsc();
    let mut fresh = Vec::new();
    for path in entries() {
        let checked = Command::new(&tsc)
            .args([
                "--target",
                "ES2022",
                "--module",
                module_kind(&path),
                "--strict",
                "--pretty",
                "false",
                "--allowImportingTsExtensions",
                "--noEmit",
            ])
            .arg(fixtures().join(&path))
            .output()
            .unwrap();
        fresh.push((path, checked.status.success()));
    }
    if env::var_os("BLUEICE_WRITE_IMMUTABLE_MATRIX").is_some() {
        fs::write(
            fixtures().join("immutable-checker-matrix.tsv"),
            fresh
                .iter()
                .map(|(path, accepts)| {
                    format!("{path}\t{}\n", if *accepts { "accept" } else { "reject" })
                })
                .collect::<String>(),
        )
        .unwrap();
    } else {
        assert_eq!(fresh, rows());
    }
}

#[test]
#[ignore = "requires BLUEICE_BLUETSC_ORACLE to point to the pinned TypeScript compiler and Node"]
fn mutable_program_preserves_execution_and_declarations() {
    let tsc = pinned_tsc();
    let root = env::temp_dir().join(format!("bluets-immutable-runtime-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("package.json"), "{\"type\":\"module\"}").unwrap();
    let entry = fixtures().join("immutable-runtime/main.ts");
    let blue = root.join("blue");
    let reference = root.join("reference");
    let built = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
        .arg("build")
        .arg(&entry)
        .arg("--declaration")
        .arg("--out-dir")
        .arg(&blue)
        .output()
        .unwrap();
    assert!(built.status.success(), "{}", report(&built));
    let built = Command::new(tsc)
        .args([
            "--target",
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
    assert!(built.status.success(), "{}", report(&built));
    let run = |dir: &Path| {
        let result = Command::new("node")
            .arg(dir.join("main.js"))
            .output()
            .unwrap();
        assert!(result.status.success(), "{}", report(&result));
        result.stdout
    };
    assert_eq!(run(&blue), run(&reference));
    assert_eq!(
        fs::read_to_string(blue.join("main.d.ts")).unwrap(),
        fs::read_to_string(reference.join("main.d.ts")).unwrap()
    );
    fs::remove_dir_all(root).unwrap();
}
