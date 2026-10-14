// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! K.1.5: inferred return signatures, declarations and execution match pinned tsc.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::{env, fs};

const MATRIX: &str = include_str!("fixtures/typescript_oracle/infer-return-checker-matrix.tsv");

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
                    .starts_with("infer-return-")
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
        "bluets-inferred-return-{name}-{}",
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
    if env::var_os("BLUEICE_WRITE_RETURN_MATRIX").is_some() {
        fs::write(
            fixtures().join("infer-return-checker-matrix.tsv"),
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
#[ignore = "requires the pinned TypeScript compiler and Node"]
fn inferred_returns_preserve_node_execution() {
    let tsc = pinned_tsc();
    let root = temporary("runtime");
    fs::write(root.join("package.json"), "{\"type\":\"module\"}").unwrap();
    let entry = fixtures().join("infer-return-runtime/main.ts");
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
fn inferred_return_declarations_match_typescript() {
    let tsc = pinned_tsc();
    let root = temporary("declarations");
    for name in [
        "method-default",
        "escaped-literals",
        "getter-completion",
        "completion",
        "unknown-getter",
        "inferred-setter",
        "recursion",
        "default",
        "pattern",
        "primitives",
        "unions",
        "empty",
        "freshness",
        "async",
        "generators",
        "classes",
        "accessor-pair",
        "generic",
        "closures",
        "records",
        "namespace",
        "forward",
        "local",
    ] {
        let entry = fixtures().join(format!("infer-return-decl-{name}/main.ts"));
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

#[test]
#[ignore = "requires the pinned TypeScript compiler"]
fn union_declarations_have_the_same_types_despite_literal_intern_order() {
    let tsc = pinned_tsc();
    let root = temporary("union-declarations");
    for name in ["unreachable", "finally"] {
        let entry = fixtures().join(format!("infer-return-decl-{name}/main.ts"));
        let blue = root.join(name).join("blue");
        let reference = root.join(name).join("reference");
        let built = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
            .arg("build")
            .arg(&entry)
            .args(["--declaration", "--out-dir"])
            .arg(&blue)
            .output()
            .unwrap();
        assert!(built.status.success(), "{}", report(&built));
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
                "--emitDeclarationOnly",
                "--outDir",
            ])
            .arg(&reference)
            .arg(entry)
            .output()
            .unwrap();
        assert!(built.status.success(), "{}", report(&built));
        let consumer = root.join(name).join("consumer.ts");
        fs::write(&consumer, "import type * as Blue from './blue/main';\nimport type * as Reference from './reference/main';\ntype Equal<A, B> = (<T>() => T extends A ? 1 : 2) extends (<T>() => T extends B ? 1 : 2) ? true : false;\nconst equal: Equal<typeof Blue.count, typeof Reference.count> = true;\n").unwrap();
        let checked = Command::new(&tsc)
            .args([
                "--target", "ES2022", "--lib", "ES2022", "--module", "ES2022", "--strict",
                "--pretty", "false", "--noEmit",
            ])
            .arg(consumer)
            .output()
            .unwrap();
        assert!(checked.status.success(), "{name}: {}", report(&checked));
    }
    fs::remove_dir_all(root).unwrap();
}
