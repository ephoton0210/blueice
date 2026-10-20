// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! G-T5 compares full inference observations at the public compiler boundary.

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
        serde_json::from_str(include_str!("fixtures/gt5_compositions/cases.json")).unwrap(),
        serde_json::from_str(include_str!("fixtures/gt5_compositions/reference.json")).unwrap(),
    )
}

fn options(row: &Value) -> CompilerOptions {
    CompilerOptions {
        target: EcmaTarget::Es2022,
        declaration: true,
        checking: Some(CheckingOptions {
            exact_optional_property_types: row["options"]["exactOptionalPropertyTypes"] == true,
            no_unchecked_indexed_access: row["options"]["noUncheckedIndexedAccess"] == true,
            ..CheckingOptions::default()
        }),
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
            options(row),
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
        "{} G-T5 mismatches:\n{}",
        failures.len(),
        failures.join("\n")
    );
    outputs
}

#[test]
fn computed_tuple_const_and_flow_compositions_match_complete_native_observations() {
    outputs();
}

#[test]
fn matrix_retains_every_native_pair_and_configuration() {
    let (input, reference) = observations();
    assert_eq!(reference["typescript"], "5.9.3");
    assert_eq!(reference["cases"].as_array().unwrap().len(), 86);
    let mut rows = reference["cases"].as_array().unwrap().iter();
    let mut areas = BTreeSet::new();
    for form in input["forms"].as_array().unwrap() {
        areas.insert(form["area"].as_str().unwrap());
        for verdict in ["accept", "reject"] {
            let row = rows.next().unwrap();
            let mut expected = input["options"].clone();
            if let Some(overrides) = form["options"].as_object() {
                expected.as_object_mut().unwrap().extend(overrides.clone());
            }
            assert_eq!(
                row["id"],
                format!("{}-{verdict}", form["id"].as_str().unwrap())
            );
            assert_eq!(row["area"], form["area"]);
            assert_eq!(row["options"], expected);
            assert_eq!(row["accepts"], verdict == "accept");
            if verdict == "accept" {
                assert_eq!(row["consumer"], form["consumer"]);
            }
            assert_eq!(
                row["source"],
                format!(
                    "// UTF-16 control: 😀\n{}\n{}\n",
                    form["prefix"].as_str().unwrap(),
                    form[verdict].as_str().unwrap()
                )
            );
        }
    }
    assert!(rows.next().is_none());
    assert_eq!(areas, BTreeSet::from(["symbol", "tuple", "const", "flow"]));
}

#[test]
fn composition_work_and_depth_limits_fail_without_output_or_invented_diagnostics() {
    let (input, _) = observations();
    for control in input["budget_controls"].as_array().unwrap() {
        let loader = MapLoader::from([ModuleSource::new(
            "entry.ts",
            control["source"].as_str().unwrap(),
        )]);
        let accepted = compile("entry.ts", &loader, options(&json!({})));
        assert!(
            accepted.diagnostics.is_empty(),
            "{}: {:?}",
            control["id"],
            accepted.diagnostics
        );
        assert!(accepted.output.is_some());
        let mut limited = options(&json!({}));
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
        .join("../../development/browser_core/phase-18-bluets/tools/record_gt5_compositions.cjs")
}

#[test]
fn readonly_casts_retain_owner_qualifiers_without_a_blanket_never_refusal() {
    let (input, reference) = observations();
    assert_eq!(input["readonly_controls"], reference["readonly_controls"]);
    assert_eq!(input["readonly_controls"].as_array().unwrap().len(), 3);
    for row in input["readonly_controls"].as_array().unwrap() {
        for checking in [Some(CheckingOptions::default()), None] {
            let mut policy = options(&json!({}));
            policy.checking = checking;
            for (key, accepted) in [("source", false), ("mutable", true)] {
                let result = compile(
                    "entry.ts",
                    &MapLoader::from([ModuleSource::new("entry.ts", row[key].as_str().unwrap())]),
                    policy.clone(),
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
#[ignore = "requires pinned TypeScript 5.9.3 and Node"]
fn gt5_native_observations_are_reproducible() {
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
fn gt5_exact_declarations_have_native_consumers_syntax_and_execution() {
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
