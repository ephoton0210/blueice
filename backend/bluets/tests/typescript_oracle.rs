// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Opt-in compatibility checks against a pinned external TypeScript compiler.
//!
//! The fixtures are deliberately narrow: each one is already part of BlueTS's
//! documented language matrix. The oracle never makes a new syntax supported.

use blueice_bluets::{compile, CompilerOptions, DiagnosticCode, MapLoader, ModuleSource};
use serde_json::Value;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

const PINNED_TYPESCRIPT_VERSION: &str = "5.9.3";
static TEST_DIRECTORY_COUNTER: AtomicU64 = AtomicU64::new(0);

struct OracleCase {
    name: &'static str,
    modules: &'static [(&'static str, &'static str)],
    expected_stdout: Option<&'static str>,
}

const CASES: &[OracleCase] = &[
    OracleCase {
        name: "generic-property",
        modules: &[(
            "memory:///main.ts",
            include_str!("fixtures/typescript_oracle/generic-property/main.ts"),
        )],
        expected_stdout: Some("Ada\n"),
    },
    OracleCase {
        name: "optional-default",
        modules: &[(
            "memory:///main.ts",
            include_str!("fixtures/typescript_oracle/optional-default/main.ts"),
        )],
        expected_stdout: Some("2\n"),
    },
    OracleCase {
        name: "generic-declaration-module",
        modules: &[
            (
                "memory:///main.ts",
                include_str!("fixtures/typescript_oracle/generic-declaration-module/main.ts"),
            ),
            (
                "memory:///types/envelope.d.ts",
                include_str!(
                    "fixtures/typescript_oracle/generic-declaration-module/types/envelope.d.ts"
                ),
            ),
        ],
        expected_stdout: Some("Ada\n"),
    },
    OracleCase {
        name: "generic-constraint-default-declaration-module",
        modules: &[
            (
                "memory:///main.ts",
                include_str!(
                    "fixtures/typescript_oracle/generic-constraint-default-declaration-module/main.ts"
                ),
            ),
            (
                "memory:///types/envelope.d.ts",
                include_str!(
                    "fixtures/typescript_oracle/generic-constraint-default-declaration-module/types/envelope.d.ts"
                ),
            ),
        ],
        expected_stdout: Some("Ada\n"),
    },
    OracleCase {
        name: "assignment-error",
        modules: &[(
            "memory:///main.ts",
            include_str!("fixtures/typescript_oracle/assignment-error/main.ts"),
        )],
        expected_stdout: None,
    },
    OracleCase {
        name: "call-argument-error",
        modules: &[(
            "memory:///main.ts",
            include_str!("fixtures/typescript_oracle/call-argument-error/main.ts"),
        )],
        expected_stdout: None,
    },
    OracleCase {
        name: "generic-constraint-error",
        modules: &[ (
            "memory:///main.ts",
            include_str!("fixtures/typescript_oracle/generic-constraint-error/main.ts"),
        )],
        expected_stdout: None,
    },
];

/// This test is ignored in ordinary Rust builds because the reference compiler
/// is an explicitly provisioned test tool, not a BlueTS dependency.
#[test]
#[ignore = "requires BLUEICE_TSC to point to the pinned TypeScript compiler"]
fn pinned_typescript_oracle_matches_the_supported_fixture_matrix() {
    let tsc = pinned_tsc();
    assert_pinned_version(&tsc);
    let node = env::var_os("BLUEICE_NODE").unwrap_or_else(|| "node".into());
    for case in CASES {
        run_case(case, &tsc, &node);
    }
}

fn run_case(case: &OracleCase, tsc: &Path, node: &std::ffi::OsStr) {
    let temporary = TestDirectory::new();
    let sources = case
        .modules
        .iter()
        .map(|(id, text)| ModuleSource::new(*id, *text))
        .collect::<Vec<_>>();
    for (id, text) in case.modules {
        let path = temporary.path().join(disk_path(id));
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    let options = CompilerOptions {
        source_map: true,
        ..CompilerOptions::default()
    };
    let compilation = compile("memory:///main.ts", &MapLoader::from(sources), options);
    let typescript_output = temporary.path().join("typescript");
    let input = temporary.path().join("main.ts");
    let tsc_output = run_tsc(
        tsc,
        &input,
        &typescript_output,
        case.expected_stdout.is_none(),
    );

    match case.expected_stdout {
        Some(expected_stdout) => {
            assert!(
                !compilation.has_errors(),
                "BlueTS rejected accepted fixture {}: {:#?}",
                case.name,
                compilation.diagnostics
            );
            assert_success(
                &tsc_output,
                &format!(
                    "the pinned TypeScript compiler rejected fixture {}",
                    case.name
                ),
            );
            let artifact = &compilation.output.as_ref().unwrap().artifacts["memory:///main.ts"];
            let blueice_output = temporary.path().join("blueice.js");
            fs::write(&blueice_output, &artifact.javascript).unwrap();
            assert_source_map(&artifact.source_map.as_ref().unwrap().to_json(), "BlueTSC");
            assert_source_map(
                &fs::read_to_string(typescript_output.join("main.js.map")).unwrap(),
                "TypeScript",
            );
            let blueice_result = run_node(node, &blueice_output);
            let typescript_result = run_node(node, &typescript_output.join("main.js"));
            assert_success(&blueice_result, "Node could not execute BlueTSC output");
            assert_success(
                &typescript_result,
                "Node could not execute TypeScript output",
            );
            assert_eq!(
                blueice_result.stdout,
                expected_stdout.as_bytes(),
                "{} BlueTSC stdout",
                case.name
            );
            assert_eq!(
                typescript_result.stdout,
                expected_stdout.as_bytes(),
                "{} TypeScript stdout",
                case.name
            );
        }
        None => {
            assert!(
                compilation.has_errors(),
                "BlueTS accepted rejected fixture {}",
                case.name
            );
            assert!(compilation
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == DiagnosticCode::TypeMismatch));
            assert!(
                !tsc_output.status.success(),
                "the pinned TypeScript compiler accepted rejected fixture {}",
                case.name
            );
        }
    }
}

fn disk_path(module_id: &str) -> &str {
    module_id
        .strip_prefix("memory:///")
        .expect("oracle fixture module IDs are memory-rooted")
}

fn assert_source_map(source_map: &str, producer: &str) {
    let value: Value = serde_json::from_str(source_map)
        .unwrap_or_else(|error| panic!("{producer} emitted invalid source-map JSON: {error}"));
    assert_eq!(value["version"], 3, "{producer} source map version");
    assert!(value["mappings"]
        .as_str()
        .is_some_and(|value| !value.is_empty()));
    assert!(value["sources"]
        .as_array()
        .is_some_and(|value| !value.is_empty()));
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
        "--target",
        "ES2022",
        "--module",
        "none",
        "--pretty",
        "false",
        "--sourceMap",
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
