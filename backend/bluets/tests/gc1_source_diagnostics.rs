// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! G-C1's frozen nine source observations require agreement, not gap allowances.

use blueice_bluets::{compile, CompilerOptions, EcmaTarget, MapLoader, ModuleSource};
use serde_json::Value;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

const CASES: [&str; 9] = [
    "tuple-index",
    "throw-newline",
    "throw-empty",
    "rest-alias",
    "compatible-function-field",
    "setter-destructure",
    "catch-type",
    "name-type",
    "name-area",
];

fn control_options() -> CompilerOptions {
    CompilerOptions {
        target: EcmaTarget::Es2020,
        ..CompilerOptions::default()
    }
}

#[test]
fn nine_source_observations_match_complete_pinned_diagnostics() {
    let reference: Value =
        serde_json::from_str(include_str!("fixtures/diagnostics/source-families.json")).unwrap();
    let mut failures = Vec::new();
    for suffix in CASES {
        let id = format!("diagnostics-other-{suffix}");
        let cases: Vec<_> = reference["cases"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|case| case["id"] == id)
            .collect();
        assert_eq!(cases.len(), 1, "missing/duplicate frozen observation: {id}");
        let case = cases[0];
        let result = compile(
            "memory:///witness.ts",
            &MapLoader::from([ModuleSource::new(
                "memory:///witness.ts",
                case["source"].as_str().unwrap(),
            )]),
            CompilerOptions::default(),
        );
        let mut actual = Vec::new();
        for diagnostic in &result.diagnostics {
            match &diagnostic.typescript {
                Some(native) => {
                    assert!(
                        native.related_information.is_empty(),
                        "{id}: unexpected related diagnostics"
                    );
                    actual.push(serde_json::json!({
                        "code": native.code,
                        "message": native.message,
                        "start": native.span.start,
                        "length": native.span.end - native.span.start,
                    }));
                }
                None => actual.push(diagnostic.to_json()),
            }
        }
        if Value::Array(actual.clone()) != case["diagnostics"]
            || result.output.is_some() != case["accepts"].as_bool().unwrap()
        {
            failures.push(format!(
                "{suffix}: expected {}, got {}; output={}",
                case["diagnostics"],
                Value::Array(actual),
                result.output.is_some(),
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn source_form_controls_preserve_types_and_native_execution() {
    let input: Value =
        serde_json::from_str(include_str!("fixtures/gc1_source_controls/cases.json")).unwrap();
    let reference: Value =
        serde_json::from_str(include_str!("fixtures/gc1_source_controls/reference.json")).unwrap();
    let mut failures = Vec::new();
    assert_eq!(input["cases"].as_array().unwrap().len(), 18);
    assert_eq!(reference["cases"].as_array().unwrap().len(), 18);
    for (case, expected) in input["cases"]
        .as_array()
        .unwrap()
        .iter()
        .zip(reference["cases"].as_array().unwrap())
    {
        assert_eq!(case["id"], expected["id"]);
        let source = case["source"].as_str().unwrap();
        let result = compile(
            "memory:///controls.ts",
            &MapLoader::from([ModuleSource::new("memory:///controls.ts", source)]),
            control_options(),
        );
        let actual = result.diagnostics.iter().map(|d| match &d.typescript {
            Some(native) => serde_json::json!({"code":native.code,"message":native.message,"start":source[..native.span.start].encode_utf16().count(),"length":source[native.span.start..native.span.end].encode_utf16().count()}),
            None => d.to_json(),
        }).collect::<Vec<_>>();
        if Value::Array(actual.clone()) != expected["diagnostics"]
            || result.output.is_some() != expected["accepts"].as_bool().unwrap()
        {
            failures.push(format!(
                "{}: expected {}; actual={}; output={}",
                case["id"],
                expected,
                Value::Array(actual),
                result.output.is_some()
            ));
            continue;
        }
        if case["runtime"] == true {
            let output = result.output.unwrap();
            let javascript = &output.artifacts["memory:///controls.ts"].javascript;
            let mut child = Command::new("node")
                .arg("--input-type=module")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            child
                .stdin
                .take()
                .unwrap()
                .write_all(javascript.as_bytes())
                .unwrap();
            let run = child.wait_with_output().unwrap();
            let actual = serde_json::json!({"exit":run.status.code(),"stdout":String::from_utf8(run.stdout).unwrap().replace("\r\n","\n"),"stderr":String::from_utf8(run.stderr).unwrap().replace("\r\n","\n")});
            assert_eq!(actual, expected["runtime"], "{}", case["id"]);
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
#[ignore = "requires BLUEICE_BLUETSC_ORACLE to point to pinned TypeScript 5.9.3"]
fn source_controls_match_live_pinned_typescript_and_execution() {
    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../development/browser_core/phase-18-bluets/tools/record_gc1_sources.cjs");
    let output = Command::new("node").arg(script).output().unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
#[ignore = "requires BLUEICE_ACORN_ORACLE to point to pinned Acorn 8.15.0"]
fn accepted_runtime_controls_parse_at_the_selected_ecma_edition() {
    let command = std::env::var("BLUEICE_ACORN_ORACLE").expect("pinned Acorn command");
    let resolve = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/oracle_support/resolve_package.cjs");
    let input: Value =
        serde_json::from_str(include_str!("fixtures/gc1_source_controls/cases.json")).unwrap();
    for case in input["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|case| case["runtime"] == true)
    {
        let compilation = compile(
            "memory:///controls.ts",
            &MapLoader::from([ModuleSource::new(
                "memory:///controls.ts",
                case["source"].as_str().unwrap(),
            )]),
            control_options(),
        );
        assert!(
            compilation.diagnostics.is_empty(),
            "{}: {:?}",
            case["id"],
            compilation.diagnostics
        );
        let output = compilation.output.unwrap();
        let javascript = &output.artifacts["memory:///controls.ts"].javascript;
        let script = "const fs=require('node:fs');const resolve=require(process.argv[1]);const acorn=require(resolve(process.argv[2],'acorn','dist/acorn.js'));if(acorn.version!=='8.15.0')throw Error('unpinned Acorn');acorn.parse(fs.readFileSync(0,'utf8'),{ecmaVersion:2020,sourceType:'module'});";
        let mut child = Command::new("node")
            .args(["-e", script])
            .arg(&resolve)
            .arg(&command)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(javascript.as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}: {}",
            case["id"],
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
