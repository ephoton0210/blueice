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
const DEFERRED: &str =
    include_str!("fixtures/typescript_oracle/namespace-checker-matrix-deferred.txt");

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/typescript_oracle")
}

/// One program: its name, entry file, and every module as `(file, text)`.
type Program = (String, String, Vec<(String, String)>);

/// Every namespace fixture pinned TypeScript accepts and BlueTSC builds: a
/// directory with a `main.ts`, or one with several entries (`*valid.ts`) beside
/// the modules they import.
fn accepted_programs() -> Vec<Program> {
    let deferred: Vec<&str> = DEFERRED.lines().collect();
    MATRIX
        .lines()
        .filter_map(|line| line.split_once('\t'))
        .filter(|(path, verdict)| *verdict == "accept" && !deferred.contains(path))
        .map(|(path, _)| {
            let (directory, entry) = path.split_once('/').unwrap();
            let modules = fs::read_dir(fixtures().join(directory))
                .unwrap()
                .map(|file| file.unwrap().file_name().into_string().unwrap())
                .filter(|file| file.ends_with(".ts"))
                .filter(|file| {
                    // Other entries of the directory are separate programs.
                    file == entry
                        || !(file.ends_with("valid.ts")
                            || file.ends_with("error.ts")
                            || file == "main.ts")
                })
                .map(|file| {
                    let text = fs::read_to_string(fixtures().join(directory).join(&file)).unwrap();
                    (file, text)
                })
                .collect();
            (path.replace('/', ":"), entry.to_string(), modules)
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
    for (name, entry, modules) in &programs {
        for target in TARGETS {
            compare(name, entry, modules, *target, &tsc, &node);
        }
    }
}

fn compare(
    name: &str,
    entry: &str,
    modules: &[(String, String)],
    target: EcmaTarget,
    tsc: &Path,
    node: &std::ffi::OsStr,
) {
    let mode = format!("{name} target={}", target.as_str());
    let directory = Directory::new();
    let sources: Vec<ModuleSource> = modules
        .iter()
        .map(|(file, text)| ModuleSource::new(format!("memory:///{file}"), text.as_str()))
        .collect();
    let compiled = compile(
        &format!("memory:///{entry}"),
        &MapLoader::from(sources),
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
    for (file, text) in modules {
        fs::write(input.join(file), text).unwrap();
    }
    for (id, artifact) in &compiled.output.as_ref().unwrap().artifacts {
        let relative = id.strip_prefix("memory:///").unwrap().replace(".ts", ".js");
        fs::write(blue.join(relative), &artifact.javascript).unwrap();
    }

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
            "--allowImportingTsExtensions",
            "--rewriteRelativeImportExtensions",
            "--outDir",
        ])
        .arg(&reference)
        .arg(input.join(entry))
        .output()
        .unwrap();
    let entry_js = entry.replace(".ts", ".js");
    assert!(
        reference.join(&entry_js).exists(),
        "tsc emitted nothing for {mode}: {emitted:?}"
    );

    let blue_run = run(node, &blue.join(&entry_js));
    let tsc_run = run(node, &reference.join(&entry_js));
    assert!(
        blue_run.status.success(),
        "{mode}: BlueTSC output failed: {blue_run:?}"
    );
    assert!(
        tsc_run.status.success(),
        "{mode}: tsc output failed: {tsc_run:?}"
    );
    assert_eq!(
        String::from_utf8_lossy(&blue_run.stdout),
        String::from_utf8_lossy(&tsc_run.stdout),
        "{mode}: BlueTSC and TypeScript print different output"
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
