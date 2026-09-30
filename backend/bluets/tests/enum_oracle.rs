// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Enum emit against pinned TypeScript (J.3.4.3): each program is compiled by
//! BlueTSC and by `tsc` under every const-enum option combination (default,
//! `preserveConstEnums`, `isolatedModules`) and both must print the same
//! thing under Node. That pins inlining, erasure and the runtime object of
//! regular, const and imported enums.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use blueice_bluets::{compile, CompilerOptions, MapLoader, ModuleSource};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// `(name, entry file, every module as (file, text))`.
type Fixture = (
    &'static str,
    &'static str,
    &'static [(&'static str, &'static str)],
);

const FIXTURES: &[Fixture] = &[
    (
        "enum-runtime-basic",
        "main.ts",
        &[(
            "main.ts",
            include_str!("fixtures/typescript_oracle/enum-runtime-basic/main.ts"),
        )],
    ),
    (
        "enum-const-runtime",
        "main.ts",
        &[(
            "main.ts",
            include_str!("fixtures/typescript_oracle/enum-const-runtime/main.ts"),
        )],
    ),
    (
        "enum-const-imported",
        "valid.ts",
        &[
            (
                "valid.ts",
                include_str!("fixtures/typescript_oracle/enum-const-imported/valid.ts"),
            ),
            (
                "colors.ts",
                include_str!("fixtures/typescript_oracle/enum-const-imported/colors.ts"),
            ),
        ],
    ),
];

/// `(preserveConstEnums, isolatedModules)`.
const MODES: &[(bool, bool)] = &[(false, false), (true, false), (false, true), (true, true)];

#[test]
#[ignore = "requires BLUEICE_BLUETSC_ORACLE to point to the pinned TypeScript compiler"]
fn pinned_enum_emit_prints_what_typescript_prints_in_every_const_enum_mode() {
    let tsc = env::var_os("BLUEICE_BLUETSC_ORACLE")
        .map(PathBuf::from)
        .expect("set BLUEICE_BLUETSC_ORACLE to the TypeScript 5.9.3 tsc executable");
    let version = Command::new(&tsc).arg("--version").output().unwrap();
    assert!(String::from_utf8_lossy(&version.stdout).contains("5.9.3"));
    let node = env::var_os("BLUEICE_NODE").unwrap_or_else(|| "node".into());
    for (name, entry, modules) in FIXTURES {
        for (preserve, isolated) in MODES {
            compare(name, entry, modules, *preserve, *isolated, &tsc, &node);
        }
    }
}

fn compare(
    name: &str,
    entry: &str,
    modules: &[(&str, &str)],
    preserve: bool,
    isolated: bool,
    tsc: &Path,
    node: &std::ffi::OsStr,
) {
    let mode = format!("{name} preserve={preserve} isolated={isolated}");
    let directory = Directory::new();
    let sources: Vec<ModuleSource> = modules
        .iter()
        .map(|(file, text)| ModuleSource::new(format!("memory:///{file}"), *text))
        .collect();
    let compiled = compile(
        &format!("memory:///{entry}"),
        &MapLoader::from(sources),
        CompilerOptions {
            preserve_const_enums: preserve,
            isolated_modules: isolated,
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
    let mut command = Command::new(tsc);
    command.args([
        "--target",
        "ES2022",
        "--module",
        "ES2022",
        "--strict",
        "--pretty",
        "false",
        "--allowImportingTsExtensions",
        "--rewriteRelativeImportExtensions",
        "--outDir",
    ]);
    command.arg(&reference);
    if preserve {
        command.arg("--preserveConstEnums");
    }
    if isolated {
        command.arg("--isolatedModules");
    }
    command.arg(input.join(entry)).output().unwrap();
    let entry_js = entry.replace(".ts", ".js");
    assert!(
        reference.join(&entry_js).exists(),
        "tsc emitted nothing for {mode}"
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
            "blueice-bluets-enum-{}-{nanos}-{sequence}",
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
