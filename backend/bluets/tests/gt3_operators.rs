// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! G-T3 compares complete operator observations at the public compiler boundary.

use blueice_bluets::{
    compile, CheckingOptions, CompilerOptions, DiagnosticCode, EcmaTarget, MapLoader, ModuleSource,
};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

fn observations() -> (Value, Value) {
    (
        serde_json::from_str(include_str!("fixtures/gt3_operators/cases.json")).unwrap(),
        serde_json::from_str(include_str!("fixtures/gt3_operators/reference.json")).unwrap(),
    )
}

fn options(target: &str) -> CompilerOptions {
    CompilerOptions {
        target: EcmaTarget::parse(target).unwrap(),
        declaration: true,
        checking: Some(CheckingOptions::default()),
        ..CompilerOptions::default()
    }
}

fn outputs() -> Vec<Value> {
    let (_, reference) = observations();
    let mut failures = Vec::new();
    let mut outputs = Vec::new();
    for row in reference["cases"].as_array().unwrap() {
        let source = row["source"].as_str().unwrap();
        let result = compile(
            "entry.ts",
            &MapLoader::from([ModuleSource::new("entry.ts", source)]),
            options(row["options"]["target"].as_str().unwrap()),
        );
        let diagnostics: Vec<_> = result.diagnostics.iter().map(|d| {
            let Some(native) = &d.typescript else { return d.to_json(); };
            let related: Vec<_> = native.related_information.iter().map(|r| json!({
                "code":r.code,"message":r.message,"module":r.span.module,
                "start":source[..r.span.start].encode_utf16().count(),
                "length":source[r.span.start..r.span.end].encode_utf16().count(),"related":[]
            })).collect();
            json!({"code":native.code,"message":native.message,"module":native.span.module,
                "start":source[..native.span.start].encode_utf16().count(),
                "length":source[native.span.start..native.span.end].encode_utf16().count(),"related":related})
        }).collect();
        if json!(diagnostics) != row["diagnostics"]
            || result.output.is_some() != row["accepts"].as_bool().unwrap()
        {
            failures.push(format!(
                "{}: expected {}; got {}; output={}",
                row["id"],
                row["diagnostics"],
                json!(diagnostics),
                result.output.is_some()
            ));
            continue;
        }
        if let Some(output) = result.output {
            let artifact = &output.artifacts["entry.ts"];
            let declaration = artifact.declaration.as_deref().unwrap();
            if declaration != row["declaration"].as_str().unwrap() {
                failures.push(format!(
                    "{}: declaration expected {:?}, got {:?}",
                    row["id"], row["declaration"], declaration
                ));
            }
            outputs.push(
                json!({"id":row["id"],"declaration":declaration,"javascript":artifact.javascript}),
            );
        }
    }
    assert!(
        failures.is_empty(),
        "{} G-T3 mismatches:\n{}",
        failures.len(),
        failures.join("\n")
    );
    outputs
}

#[test]
fn composed_operators_queries_and_keys_match_complete_native_observations() {
    outputs();
}

#[test]
fn every_frozen_form_retains_both_verdict_controls() {
    let (input, reference) = observations();
    assert_eq!(reference["typescript"], "5.9.3");
    assert_eq!(reference["cases"].as_array().unwrap().len(), 76);
    let mut rows = reference["cases"].as_array().unwrap().iter();
    for form in input["forms"].as_array().unwrap() {
        for verdict in ["accept", "reject"] {
            let row = rows.next().unwrap();
            assert_eq!(
                row["id"],
                format!("{}-{verdict}", form["id"].as_str().unwrap())
            );
            assert_eq!(row["area"], form["area"]);
            assert_eq!(row["accepts"], verdict == "accept");
            assert_eq!(
                row["source"],
                format!(
                    "// UTF-16 control: 😀\n{}\nexport const value: {} = {};\n",
                    form["prefix"].as_str().unwrap(),
                    form["annotation"].as_str().unwrap(),
                    form[verdict].as_str().unwrap()
                )
            );
            let mut expected_options = input["options"].clone();
            if form.get("target").is_some() {
                expected_options["target"] = form["target"].clone();
            }
            assert_eq!(row["options"], expected_options);
        }
    }
    assert!(rows.next().is_none());
    assert_eq!(input["budget_controls"], reference["budget_controls"]);
    let areas: BTreeSet<_> = input["forms"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["area"].as_str().unwrap())
        .collect();
    assert_eq!(
        areas,
        BTreeSet::from(["nested", "template", "query", "keys", "symbol"])
    );
}

#[test]
fn operator_work_and_parser_depth_limits_fail_without_output_or_widening() {
    let (input, _) = observations();
    for control in input["budget_controls"].as_array().unwrap() {
        let loader = MapLoader::from([ModuleSource::new(
            "entry.ts",
            control["source"].as_str().unwrap(),
        )]);
        let accepted = compile("entry.ts", &loader, options("ES2022"));
        assert!(
            accepted.diagnostics.is_empty(),
            "{}: {:?}",
            control["id"],
            accepted.diagnostics
        );
        assert!(accepted.output.is_some());
        let mut limited = options("ES2022");
        if control["kind"] == "expansion" {
            limited.limits.max_type_expansions = 0;
        } else {
            assert_eq!(control["kind"], "depth");
            limited.limits.parser.max_type_depth = 2;
        }
        let refused = compile("entry.ts", &loader, limited);
        assert!(refused.output.is_none(), "{}", control["id"]);
        assert!(
            refused
                .diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::ResourceLimit && d.typescript.is_none()),
            "{}: {:?}",
            control["id"],
            refused.diagnostics
        );
    }
}

fn recorder() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../development/browser_core/phase-18-bluets/tools/record_gt3_operators.cjs")
}

#[test]
fn library_key_facts_do_not_grant_runtime_binding_authority() {
    let (input, _) = observations();
    for control in input["capability_controls"].as_array().unwrap() {
        let source = control["source"].as_str().unwrap();
        let mut settings = options("ES2022");
        settings.require_declared_global_calls = true;
        if control["owner"] == true {
            settings.ambient_declaration_modules.push(ModuleSource::new(
                "owner.d.ts",
                "declare function toFixed(value: number): void;",
            ));
        }
        let result = compile(
            "entry.ts",
            &MapLoader::from([ModuleSource::new("entry.ts", source)]),
            settings,
        );
        if control["accepts"] == true {
            assert!(
                result.diagnostics.is_empty(),
                "{}: {:?}",
                control["id"],
                result.diagnostics
            );
            let output = result.output.unwrap();
            assert!(!output.artifacts["entry.ts"]
                .javascript
                .contains("bluets:keyof"));
        } else {
            assert!(result.output.is_none(), "{}", control["id"]);
            assert!(
                result
                    .diagnostics
                    .iter()
                    .any(|d| d.code == DiagnosticCode::UnknownName),
                "{}: {:?}",
                control["id"],
                result.diagnostics
            );
        }
    }
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3 and Node"]
fn gt3_native_diagnostics_and_declarations_are_reproducible() {
    let keys = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../development/browser_core/phase-18-bluets/tools/record_gt3_key_profiles.cjs");
    let output = Command::new("node").arg(keys).output().unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let output = Command::new("node").arg(recorder()).output().unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3, Acorn 8.15.0 and Node"]
fn gt3_exact_declarations_have_native_consumers_syntax_and_execution() {
    let mut child = Command::new("node")
        .arg(recorder())
        .arg("--outputs")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&serde_json::to_vec(&outputs()).unwrap())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
