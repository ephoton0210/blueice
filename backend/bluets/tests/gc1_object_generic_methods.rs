// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! G-C1 object generic methods compares full inference observations at the public compiler boundary.

use blueice_bluets::{
    compile, CheckingOptions, CompilerOptions, EcmaTarget, MapLoader, ModuleKind, ModuleSource,
};
use serde_json::{json, Value};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

fn observations() -> (Value, Value) {
    (
        serde_json::from_str(include_str!(
            "fixtures/gc1_object_generic_methods/cases.json"
        ))
        .unwrap(),
        serde_json::from_str(include_str!(
            "fixtures/gc1_object_generic_methods/reference.json"
        ))
        .unwrap(),
    )
}

fn options() -> CompilerOptions {
    CompilerOptions {
        target: EcmaTarget::Es2022,
        module_kind: ModuleKind::CommonJs,
        source_map: true,
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
            options(),
        );
        let diagnostics: Vec<_> = result.diagnostics.iter().map(|d| {
            let Some(native) = &d.typescript else { return d.to_json(); };
            let describe = |code, message: &str, span: &blueice_bluets::SourceSpan| json!({
                "code":code,"message":message,"module":span.module,
                "start":source[..span.start].encode_utf16().count(),
                "length":source[span.start..span.end].encode_utf16().count(),"related":[]
            });
            let related: Vec<_> = native.related_information.iter()
                .map(|r| describe(r.code, &r.message, &r.span)).collect();
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
        "{} G-C1 object generic methods mismatches:\n{}",
        failures.len(),
        failures.join("\n")
    );
    outputs
}

#[test]
fn generic_methods_match_complete_native_observations() {
    outputs();
}

#[test]
fn optional_named_assignments_preserve_owner_readonly_data() {
    let (input, reference) = observations();
    assert_eq!(input["ownerControls"], reference["ownerControls"]);
    assert_eq!(input["ownerControls"].as_array().unwrap().len(), 3);
    for row in input["ownerControls"].as_array().unwrap() {
        for checking in [Some(CheckingOptions::default()), None] {
            for (key, accepted) in [("source", false), ("mutable", true)] {
                let mut policy = options();
                policy.checking = checking;
                let result = compile(
                    "entry.ts",
                    &MapLoader::from([ModuleSource::new("entry.ts", row[key].as_str().unwrap())]),
                    policy,
                );
                assert_eq!(
                    result.output.is_some(),
                    accepted,
                    "{} {key} {checking:?}: {:?}",
                    row["id"],
                    result.diagnostics
                );
                if accepted {
                    assert!(result.diagnostics.is_empty());
                } else {
                    assert!(
                        result
                            .diagnostics
                            .iter()
                            .any(|diagnostic| diagnostic.message.contains("readonly")),
                        "{}: {:?}",
                        row["id"],
                        result.diagnostics
                    );
                }
            }
        }
    }
}

#[test]
fn frozen_cases_retain_sources_options_and_verdicts() {
    let (input, reference) = observations();
    assert_eq!(reference["typescript"], "5.9.3");
    let inputs = input["cases"].as_array().unwrap();
    let rows = reference["cases"].as_array().unwrap();
    assert_eq!(inputs.len(), 53);
    assert_eq!(rows.len(), inputs.len());
    for (form, row) in inputs.iter().zip(rows) {
        for key in ["id", "source", "accepts", "invoke"] {
            assert_eq!(row[key], form[key]);
        }
        assert_eq!(row["options"], input["options"]);
    }
}

fn recorder() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../development/browser_core/phase-18-bluets/tools/record_gc1_object_generic_methods.cjs")
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3 and Node"]
fn gc1_object_generic_methods_native_observations_are_reproducible() {
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
fn gc1_object_generic_methods_exact_declarations_have_native_consumers_syntax_and_execution() {
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
