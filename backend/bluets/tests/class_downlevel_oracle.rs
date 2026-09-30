// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Target- and option-dependent class emit (J.3.3): every `class-downlevel-*`
//! program is compiled by BlueTSC and by the pinned TypeScript compiler for
//! each combination of target (ES2022, ES2020) and `useDefineForClassFields`
//! (default, true, false), and the two outputs must print exactly the same
//! thing under Node. That pins the observable semantics, so property order,
//! which properties exist, and initialization order, of native fields,
//! constructor assignment and `Object.defineProperty`, rather than the text.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use blueice_bluets::{compile, CompilerOptions, EcmaTarget, MapLoader, ModuleSource};

const PINNED_TYPESCRIPT_VERSION: &str = "5.9.3";
static COUNTER: AtomicUsize = AtomicUsize::new(0);

const FIXTURES: &[(&str, &str)] = &[
    (
        "class-downlevel-fields",
        include_str!("fixtures/typescript_oracle/class-downlevel-fields/main.ts"),
    ),
    (
        "class-downlevel-static-order",
        include_str!("fixtures/typescript_oracle/class-downlevel-static-order/main.ts"),
    ),
    (
        "class-downlevel-no-constructor",
        include_str!("fixtures/typescript_oracle/class-downlevel-no-constructor/main.ts"),
    ),
    (
        "class-downlevel-accessors-and-methods",
        include_str!("fixtures/typescript_oracle/class-downlevel-accessors-and-methods/main.ts"),
    ),
];

/// `(target, useDefineForClassFields)`; `None` is the target's default.
const MODES: &[(EcmaTarget, Option<bool>)] = &[
    (EcmaTarget::Es2022, None),
    (EcmaTarget::Es2022, Some(false)),
    (EcmaTarget::Es2020, None),
    (EcmaTarget::Es2020, Some(true)),
    (EcmaTarget::Es2020, Some(false)),
];

#[test]
#[ignore = "requires BLUEICE_BLUETSC_ORACLE to point to the pinned TypeScript compiler"]
fn pinned_class_downlevel_emit_prints_what_typescript_prints_in_every_mode() {
    let tsc = env::var_os("BLUEICE_BLUETSC_ORACLE")
        .map(PathBuf::from)
        .expect("set BLUEICE_BLUETSC_ORACLE to the TypeScript 5.9.3 tsc executable");
    let version = Command::new(&tsc).arg("--version").output().unwrap();
    assert_eq!(
        String::from_utf8(version.stdout).unwrap().trim(),
        format!("Version {PINNED_TYPESCRIPT_VERSION}")
    );
    let node = env::var_os("BLUEICE_NODE").unwrap_or_else(|| "node".into());
    for (name, source) in FIXTURES {
        for (target, define) in MODES {
            compare(name, source, *target, *define, &tsc, &node);
        }
    }
}

fn compare(
    name: &str,
    source: &str,
    target: EcmaTarget,
    define: Option<bool>,
    tsc: &Path,
    node: &std::ffi::OsStr,
) {
    let mode = format!("{name} target={} define={define:?}", target.as_str());
    let directory = Directory::new();
    fs::write(
        directory.path().join("package.json"),
        "{\"type\":\"module\"}\n",
    )
    .unwrap();

    let compiled = compile(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new("memory:///main.ts", source)]),
        CompilerOptions {
            target,
            use_define_for_class_fields: define,
            ..CompilerOptions::default()
        },
    );
    assert!(
        compiled.output.is_some(),
        "BlueTSC rejected {mode}: {:#?}",
        compiled.diagnostics
    );
    let blue = directory.path().join("blue");
    fs::create_dir_all(&blue).unwrap();
    fs::write(blue.join("package.json"), "{\"type\":\"module\"}\n").unwrap();
    let javascript = &compiled.output.as_ref().unwrap().artifacts["memory:///main.ts"].javascript;
    fs::write(blue.join("main.js"), javascript).unwrap();

    let input = directory.path().join("main.ts");
    fs::write(&input, source).unwrap();
    let out = directory.path().join("tsc");
    fs::create_dir_all(&out).unwrap();
    fs::write(out.join("package.json"), "{\"type\":\"module\"}\n").unwrap();
    let mut command = Command::new(tsc);
    let module = if target == EcmaTarget::Es2022 {
        "ES2022"
    } else {
        "ES2020"
    };
    command.args([
        "--target",
        &target.as_str().to_uppercase(),
        "--module",
        module,
    ]);
    command
        .args(["--strict", "--pretty", "false", "--outDir"])
        .arg(&out);
    if let Some(define) = define {
        command.args([
            "--useDefineForClassFields",
            if define { "true" } else { "false" },
        ]);
    }
    let reference = command.arg(&input).output().unwrap();
    // `console` has no declaration without the DOM/node lib; TypeScript still
    // emits, so only a missing emit is a failure here.
    assert!(
        out.join("main.js").exists(),
        "tsc emitted nothing for {mode}: {reference:?}"
    );

    let blue_run = run(node, &blue.join("main.js"));
    let tsc_run = run(node, &out.join("main.js"));
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
            "blueice-bluets-downlevel-{}-{nanos}-{sequence}",
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
