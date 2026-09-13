// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Opt-in compatibility checks against a pinned external TypeScript compiler.
//!
//! The oracle is deliberately outside the BlueTS dependency graph.  CI (or a
//! developer) supplies the exact `tsc` executable through `BLUEICE_TSC`; this
//! test verifies that it is the version pinned below before accepting it.

use blueice_bluets::{compile, CompilerOptions, DiagnosticCode, MapLoader, ModuleSource};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

const PINNED_TYPESCRIPT_VERSION: &str = "5.9.3";
static TEST_DIRECTORY_COUNTER: AtomicU64 = AtomicU64::new(0);

/// This test is ignored in ordinary Rust builds because the reference compiler
/// is an explicitly provisioned test tool, not a BlueTS dependency.
#[test]
#[ignore = "requires BLUEICE_TSC to point to the pinned TypeScript compiler"]
fn pinned_typescript_oracle_agrees_on_supported_erasure_and_type_errors() {
    let tsc = pinned_tsc();
    assert_pinned_version(&tsc);

    let temporary = TestDirectory::new();
    let source = r#"interface Account { id: string }
function identity<T>(value: T): T { return value; }
const name: string = identity('Ada');
const account: Account = { id: name };
function label(value: Account): string { return value.id; }
console.log(label(account));
"#;
    let input = temporary.path().join("main.ts");
    fs::write(&input, source).unwrap();

    let compilation = compile(
        "main.ts",
        &MapLoader::from([ModuleSource::new("main.ts", source)]),
        CompilerOptions::default(),
    );
    assert!(
        !compilation.has_errors(),
        "BlueTS rejected the oracle's accepted fixture: {:#?}",
        compilation.diagnostics
    );
    let blueice_javascript = &compilation
        .output
        .as_ref()
        .unwrap()
        .artifacts
        .get("main.ts")
        .unwrap()
        .javascript;
    assert!(!blueice_javascript.contains("interface Account"));
    assert!(!blueice_javascript.contains(": Account"));
    let blueice_output = temporary.path().join("blueice.js");
    fs::write(&blueice_output, blueice_javascript).unwrap();

    let typescript_output = temporary.path().join("typescript");
    let emitted_by_tsc = run_tsc(&tsc, &input, &typescript_output, false);
    assert_success(
        &emitted_by_tsc,
        "the pinned TypeScript compiler rejected the fixture",
    );

    let node = env::var_os("BLUEICE_NODE").unwrap_or_else(|| "node".into());
    let blueice_result = run_node(&node, &blueice_output);
    let typescript_result = run_node(&node, &typescript_output.join("main.js"));
    assert_success(&blueice_result, "Node could not execute BlueTSC output");
    assert_success(
        &typescript_result,
        "Node could not execute TypeScript output",
    );
    assert_eq!(blueice_result.stdout, typescript_result.stdout);

    let invalid_source = "const mustBeNumber: number = 'not a number';\n";
    let invalid_input = temporary.path().join("invalid.ts");
    fs::write(&invalid_input, invalid_source).unwrap();
    let rejected_by_tsc = run_tsc(&tsc, &invalid_input, &typescript_output, true);
    assert!(
        !rejected_by_tsc.status.success(),
        "the pinned TypeScript compiler accepted an invalid assignment"
    );
    let rejected_by_bluets = compile(
        "invalid.ts",
        &MapLoader::from([ModuleSource::new("invalid.ts", invalid_source)]),
        CompilerOptions::default(),
    );
    assert!(rejected_by_bluets.has_errors());
    assert!(rejected_by_bluets
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == DiagnosticCode::TypeMismatch));
}

fn pinned_tsc() -> PathBuf {
    env::var_os("BLUEICE_TSC")
        .map(PathBuf::from)
        .expect("set BLUEICE_TSC to the TypeScript 5.9.3 tsc executable")
}

fn assert_pinned_version(tsc: &Path) {
    let output = Command::new(tsc).arg("--version").output().unwrap();
    assert_success(&output, "could not query the TypeScript compiler version");
    assert_eq!(
        String::from_utf8(output.stdout).unwrap().trim(),
        format!("Version {PINNED_TYPESCRIPT_VERSION}"),
        "BLUEICE_TSC must be the pinned oracle compiler"
    );
}

fn run_tsc(tsc: &Path, input: &Path, output: &Path, no_emit: bool) -> Output {
    let mut command = Command::new(tsc);
    command.args([
        "--target", "ES2022", "--module", "none", "--pretty", "false",
    ]);
    if no_emit {
        command.arg("--noEmit");
    } else {
        command.arg("--outDir").arg(output);
    }
    command.arg(input).output().unwrap()
}

fn run_node(node: &std::ffi::OsStr, input: &Path) -> Output {
    Command::new(node).arg(input).output().unwrap()
}

fn assert_success(output: &Output, context: &str) {
    assert!(
        output.status.success(),
        "{context}: stdout: {}; stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        let sequence = TEST_DIRECTORY_COUNTER.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = env::temp_dir().join(format!(
            "blueice-bluets-oracle-{}-{}-{sequence}",
            std::process::id(),
            nanos
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
