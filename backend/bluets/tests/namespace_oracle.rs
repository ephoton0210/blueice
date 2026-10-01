// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Namespace emit against pinned TypeScript (J.3.5.3): every accepted
//! `namespace-*` program is compiled by BlueTSC and by `tsc` for each target
//! (ES2022, ES2020) and both must print the same thing under Node. That pins
//! the run-time object, merging, initialization order and what an exported
//! variable reads, rather than the text.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use blueice_bluets::{compile, CompilerOptions, EcmaTarget, MapLoader, ModuleSource};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

const MATRIX: &str = include_str!("fixtures/typescript_oracle/namespace-checker-matrix.tsv");

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/typescript_oracle")
}

/// Every single-module namespace fixture pinned TypeScript accepts, as
/// `(name, entry text)`.
fn accepted_programs() -> Vec<(String, String)> {
    MATRIX
        .lines()
        .filter_map(|line| line.split_once('\t'))
        .filter(|(path, verdict)| *verdict == "accept" && path.ends_with("/main.ts"))
        .map(|(path, _)| {
            let name = path.trim_end_matches("/main.ts").to_string();
            let text = fs::read_to_string(fixtures().join(path)).unwrap();
            (name, text)
        })
        .collect()
}

const TARGETS: &[EcmaTarget] = &[EcmaTarget::Es2022, EcmaTarget::Es2020];

#[test]
#[ignore = "requires BLUEICE_BLUETSC_ORACLE to point to the pinned TypeScript compiler"]
fn pinned_namespace_emit_prints_what_typescript_prints_for_every_target() {
    let tsc = env::var_os("BLUEICE_BLUETSC_ORACLE")
        .map(PathBuf::from)
        .expect("set BLUEICE_BLUETSC_ORACLE to the TypeScript 5.9.3 tsc executable");
    let version = Command::new(&tsc).arg("--version").output().unwrap();
    assert!(String::from_utf8_lossy(&version.stdout).contains("5.9.3"));
    let node = env::var_os("BLUEICE_NODE").unwrap_or_else(|| "node".into());
    let programs = accepted_programs();
    assert!(programs.len() >= 20, "the accepted fixtures were found");
    for (name, text) in &programs {
        for target in TARGETS {
            compare(name, text, *target, &tsc, &node);
        }
    }
}

fn compare(name: &str, text: &str, target: EcmaTarget, tsc: &Path, node: &std::ffi::OsStr) {
    let mode = format!("{name} target={}", target.as_str());
    let directory = Directory::new();
    let compiled = compile(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new("memory:///main.ts", text)]),
        CompilerOptions {
            target,
            ..CompilerOptions::default()
        },
    );
    assert!(
        compiled.output.is_some(),
        "BlueTSC rejected {mode}: {:#?}",
        compiled.diagnostics
    );
    let blue = directory.path().join("blue");
    let reference = directory.path().join("tsc");
    let input = directory.path().join("src");
    for folder in [&blue, &reference, &input] {
        fs::create_dir_all(folder).unwrap();
    }
    for folder in [&blue, &reference] {
        fs::write(folder.join("package.json"), "{\"type\":\"module\"}\n").unwrap();
    }
    fs::write(input.join("main.ts"), text).unwrap();
    let javascript = &compiled.output.as_ref().unwrap().artifacts["memory:///main.ts"].javascript;
    fs::write(blue.join("main.js"), javascript).unwrap();

    let module = if target == EcmaTarget::Es2022 {
        "ES2022"
    } else {
        "ES2020"
    };
    let emitted = Command::new(tsc)
        .args([
            "--target",
            &target.as_str().to_uppercase(),
            "--module",
            module,
            "--strict",
            "--pretty",
            "false",
            "--outDir",
        ])
        .arg(&reference)
        .arg(input.join("main.ts"))
        .output()
        .unwrap();
    assert!(
        reference.join("main.js").exists(),
        "tsc emitted nothing for {mode}: {emitted:?}"
    );

    let blue_run = run(node, &blue.join("main.js"));
    let tsc_run = run(node, &reference.join("main.js"));
    assert!(
        blue_run.status.success(),
        "{mode}: BlueTSC output failed: {blue_run:?}\n{javascript}"
    );
    assert!(
        tsc_run.status.success(),
        "{mode}: tsc output failed: {tsc_run:?}"
    );
    assert_eq!(
        String::from_utf8_lossy(&blue_run.stdout),
        String::from_utf8_lossy(&tsc_run.stdout),
        "{mode}: BlueTSC and TypeScript print different output\n--- BlueTSC output ---\n{javascript}"
    );
}

fn run(node: &std::ffi::OsStr, input: &Path) -> Output {
    Command::new(node).arg(input).output().unwrap()
}

struct Directory(PathBuf);

impl Directory {
    fn new() -> Self {
        let sequence = COUNTER.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = env::temp_dir().join(format!(
            "blueice-bluets-namespace-{}-{nanos}-{sequence}",
            std::process::id()
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
