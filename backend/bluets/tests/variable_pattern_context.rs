// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Pinned flat variable binding patterns through the public compiler through the public compiler.

use blueice_bluets::{compile, CheckingOptions, CompilerOptions, MapLoader, ModuleSource};
use serde_json::Value;
use std::{env, path::Path, process::Command};

#[test]
fn variable_patterns_match_pinned_diagnostics() {
    let reference: Value = serde_json::from_str(include_str!(
        "fixtures/variable-pattern-context-reference.json"
    ))
    .unwrap();
    assert_eq!(reference["version"], "5.9.3");
    assert_eq!(reference["cases"].as_array().unwrap().len(), 10);
    let mut failures = Vec::new();
    for case in reference["cases"].as_array().unwrap() {
        let source = case["source"].as_str().unwrap();
        let result = compile(
            "memory:///main.ts",
            &MapLoader::from([ModuleSource::new("memory:///main.ts", source)]),
            CompilerOptions {
                checking: Some(CheckingOptions {
                    no_implicit_any: true,
                    ..CheckingOptions::default()
                }),
                ..CompilerOptions::default()
            },
        );
        assert_eq!(
            result.has_errors(),
            !case["accepts"].as_bool().unwrap(),
            "{}: {:#?}",
            case["name"],
            result.diagnostics
        );
        assert_eq!(
            result.output.is_some(),
            case["accepts"].as_bool().unwrap(),
            "{}",
            case["name"]
        );
        let diagnostics: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.severity == blueice_bluets::Severity::Error)
            .map(|diagnostic| {
                let counterpart = diagnostic.typescript.as_ref();
                let span = counterpart.map_or(&diagnostic.span, |counterpart| &counterpart.span);
                (
                    counterpart.map(|counterpart| counterpart.code),
                    span.start,
                    span.end - span.start,
                    counterpart.map(|counterpart| counterpart.message.clone()),
                )
            })
            .collect();
        let expected: Vec<_> = case["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .map(|diagnostic| {
                (
                    Some(diagnostic["code"].as_u64().unwrap() as u32),
                    diagnostic["start"].as_u64().unwrap() as usize,
                    diagnostic["length"].as_u64().unwrap() as usize,
                    Some(diagnostic["message"].as_str().unwrap().to_string()),
                )
            })
            .collect();
        if diagnostics != expected {
            failures.push(format!(
                "{}: expected {expected:?}, got {diagnostics:?}",
                case["name"]
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3"]
fn variable_pattern_reference_matches_live_typescript() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let output = Command::new("node")
        .arg(fixtures.join("oracle_support/record_function_contexts.cjs"))
        .arg(env::var_os("BLUEICE_BLUETSC_ORACLE").expect("set BLUEICE_BLUETSC_ORACLE"))
        .arg(fixtures.join("variable-pattern-context-reference.json"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let actual: Value = serde_json::from_slice(&output.stdout).unwrap();
    let expected: Value = serde_json::from_str(include_str!(
        "fixtures/variable-pattern-context-reference.json"
    ))
    .unwrap();
    assert_eq!(actual, expected);
}

#[test]
fn variable_pattern_boundaries_remain_precise() {
    for source in [
        "export const {value} = {value:42};",
        "declare const {value}: {value:number};",
        "const {left:{value}} = {left:{value:42}};",
        "const {...rest,value} = {value:42};",
        "const {...rest = {}} = {value:42};",
        "const {value} = {value:42}; export {value};",
    ] {
        let result = compile(
            "memory:///main.ts",
            &MapLoader::from([ModuleSource::new("memory:///main.ts", source)]),
            CompilerOptions::default(),
        );
        assert!(result.has_errors(), "{source}: {:#?}", result.diagnostics);
        assert!(result.output.is_none(), "{source}");
        assert!(
            result
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code
                    == blueice_bluets::DiagnosticCode::UnsupportedSyntax),
            "{source}: {:#?}",
            result.diagnostics
        );
    }
}
