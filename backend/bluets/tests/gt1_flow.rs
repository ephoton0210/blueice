// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! G-T1's frozen predicate, exceptional-flow and lexical-capture observations.

use blueice_bluets::{compile, CompilerOptions, EcmaTarget, MapLoader, ModuleSource};
use serde_json::{json, Value};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

#[path = "oracle_support/union_order.rs"]
mod union_order;

fn observations() -> (Value, Value) {
    let input = serde_json::from_str(include_str!("fixtures/gt1_flow/cases.json")).unwrap();
    let reference = serde_json::from_str(include_str!("fixtures/gt1_flow/reference.json")).unwrap();
    (input, reference)
}

#[test]
fn first_stage_retains_raw_native_and_bluets_order_observations() {
    let raw: Value =
        serde_json::from_str(include_str!("fixtures/gt1_flow/stage1-order.json")).unwrap();
    assert_eq!(raw["typescript"], "5.9.3");
    let (_, reference) = observations();
    let row = &raw["observations"][0];
    assert_eq!(row["id"], "finally-return-override");
    let native = reference["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["id"] == row["id"])
        .unwrap();
    assert_eq!(row["native"], native["declaration"]);
    assert!(union_order::only_literal_return_union_order(
        row["bluets"].as_str().unwrap(),
        row["native"].as_str().unwrap()
    ));
}

#[test]
fn predicates_exceptional_flow_and_captures_match_native_types() {
    let (input, reference) = observations();
    assert_eq!(reference["typescript"], "5.9.3");
    assert_eq!(input["options"], reference["options"]);
    assert_eq!(input["cases"].as_array().unwrap().len(), 45);
    assert_eq!(reference["cases"].as_array().unwrap().len(), 45);
    let mut failures = Vec::new();
    for (case, expected) in input["cases"]
        .as_array()
        .unwrap()
        .iter()
        .zip(reference["cases"].as_array().unwrap())
    {
        assert_eq!(case["id"], expected["id"]);
        let source = case["source"].as_str().unwrap();
        let compilation = compile(
            "entry.ts",
            &MapLoader::from([ModuleSource::new("entry.ts", source)]),
            CompilerOptions {
                target: EcmaTarget::Es2020,
                declaration: true,
                ..CompilerOptions::default()
            },
        );
        let actual: Vec<_> = compilation.diagnostics.iter().map(|diagnostic| {
            let Some(native) = &diagnostic.typescript else { return diagnostic.to_json(); };
            let related: Vec<_> = native.related_information.iter().map(|r| json!({
                "code":r.code,"message":r.message,"module":r.span.module,
                "start":source[..r.span.start].encode_utf16().count(),
                "length":source[r.span.start..r.span.end].encode_utf16().count(),"related":[]
            })).collect();
            json!({"code":native.code,"message":native.message,"module":native.span.module,
                "start":source[..native.span.start].encode_utf16().count(),
                "length":source[native.span.start..native.span.end].encode_utf16().count(),"related":related})
        }).collect();
        if json!(actual) != expected["diagnostics"]
            || compilation.output.is_some() != expected["accepts"].as_bool().unwrap()
        {
            failures.push(format!(
                "{}: diagnostics expected {}, got {}; output={}",
                case["id"],
                expected["diagnostics"],
                json!(actual),
                compilation.output.is_some()
            ));
            continue;
        }
        if let Some(output) = compilation.output {
            let declaration = output.artifacts["entry.ts"]
                .declaration
                .as_deref()
                .expect("declaration output");
            if declaration != expected["declaration"].as_str().unwrap() {
                failures.push(format!(
                    "{}: declaration expected {:?}, got {:?}",
                    case["id"], expected["declaration"], declaration
                ));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} G-T1 mismatches:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3"]
fn gt1_diagnostics_and_declarations_match_live_typescript() {
    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../development/browser_core/phase-18-bluets/tools/record_gt1_flow.cjs");
    let output = Command::new("node").arg(script).output().unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3, Acorn 8.15.0 and Node"]
fn gt1_emitted_declarations_have_native_consumers_syntax_and_execution() {
    let (input, reference) = observations();
    let mut outputs = Vec::new();
    for (case, expected) in input["cases"]
        .as_array()
        .unwrap()
        .iter()
        .zip(reference["cases"].as_array().unwrap())
    {
        if expected["accepts"] != true {
            continue;
        }
        let result = compile(
            "entry.ts",
            &MapLoader::from([ModuleSource::new(
                "entry.ts",
                case["source"].as_str().unwrap(),
            )]),
            CompilerOptions {
                target: EcmaTarget::Es2020,
                declaration: true,
                ..CompilerOptions::default()
            },
        );
        assert!(
            !result.has_errors(),
            "{}: {:?}",
            case["id"],
            result.diagnostics
        );
        let output = result.output.unwrap();
        let artifact = &output.artifacts["entry.ts"];
        outputs.push(json!({"id":case["id"],"declaration":artifact.declaration,"javascript":artifact.javascript}));
    }
    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../development/browser_core/phase-18-bluets/tools/record_gt1_flow.cjs");
    let consume = |outputs: &[Value]| {
        let mut child = Command::new("node")
            .arg(&script)
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
            .write_all(serde_json::to_string(outputs).unwrap().as_bytes())
            .unwrap();
        child.wait_with_output().unwrap()
    };
    let output = consume(&outputs);
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    // This must fail in the native type checker, not only a textual comparator.
    let declaration = outputs[0]["declaration"]
        .as_str()
        .unwrap()
        .replace(": value is string", ": boolean");
    assert_ne!(outputs[0]["declaration"], declaration);
    outputs[0]["declaration"] = Value::String(declaration);
    let widened = consume(&outputs);
    assert!(!widened.status.success());
    assert!(
        String::from_utf8_lossy(&widened.stderr).contains("TS2344"),
        "{}",
        String::from_utf8_lossy(&widened.stderr)
    );
    outputs[0]["declaration"] = reference["cases"][0]["declaration"].clone();
    let changed = outputs
        .iter_mut()
        .find(|row| row["id"] == "finally-return-override")
        .unwrap();
    let original = changed["declaration"].as_str().unwrap();
    changed["declaration"] = Value::String(original.replace("\"discarded\"", "\"changed\""));
    let changed_member = consume(&outputs);
    assert!(!changed_member.status.success());
    assert!(
        String::from_utf8_lossy(&changed_member.stderr).contains("TS2344"),
        "{}",
        String::from_utf8_lossy(&changed_member.stderr)
    );
}

#[test]
fn finalizer_duplication_respects_the_owner_work_budget() {
    let source = |depth| {
        let mut body = "value = 'next';".to_string();
        for _ in 0..depth {
            body = format!("try {{ value = 'next'; }} finally {{ {body} }}");
        }
        format!("export function read(value: string | number) {{ {body} return value; }}")
    };
    let mut options = CompilerOptions::default();
    options.limits.max_type_expansions = 1;
    let below = compile(
        "entry.ts",
        &MapLoader::from([ModuleSource::new("entry.ts", source(2))]),
        options.clone(),
    );
    assert!(!below.has_errors(), "{:?}", below.diagnostics);
    let above = compile(
        "entry.ts",
        &MapLoader::from([ModuleSource::new("entry.ts", source(10))]),
        options,
    );
    assert!(above.output.is_none());
    assert!(
        above.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == blueice_bluets::DiagnosticCode::ResourceLimit
                && diagnostic.message.contains("control-flow")
                && diagnostic.message.contains("bounded")
        }),
        "{:?}",
        above.diagnostics
    );
}
