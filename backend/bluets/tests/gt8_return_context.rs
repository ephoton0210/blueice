// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! G-T8 observes return precision, declaration graphs and downstream consumers.
use blueice_bluets::{
    compile, CheckingOptions, CompilerOptions, DiagnosticCode, EcmaTarget, ModuleLoader,
    ModuleSource, SourceSpan,
};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

fn observations() -> (Value, Value) {
    (
        serde_json::from_str(include_str!("fixtures/gt8_return_context/cases.json")).unwrap(),
        serde_json::from_str(include_str!("fixtures/gt8_return_context/reference.json")).unwrap(),
    )
}

/// The test owner supplies only this frozen graph; resolution never performs I/O.
struct FixtureLoader(BTreeMap<String, String>);

impl ModuleLoader for FixtureLoader {
    fn load(&self, id: &str) -> Result<ModuleSource, String> {
        self.0
            .get(id.strip_prefix("memory:///").ok_or("foreign fixture id")?)
            .map(|source| ModuleSource::new(id, source))
            .ok_or_else(|| format!("unowned fixture source {id}"))
    }

    fn resolve(&self, _from: &str, specifier: &str) -> Result<String, String> {
        let name = specifier.strip_prefix("./").ok_or("foreign fixture edge")?;
        if name.contains('/') || name.contains('\\') || name.contains("..") {
            return Err("fixture edge escapes the frozen graph".into());
        }
        let stem = name.strip_suffix(".js").unwrap_or(name);
        [
            name.to_string(),
            format!("{stem}.ts"),
            format!("{stem}.d.ts"),
        ]
        .into_iter()
        .find(|name| self.0.contains_key(name))
        .map(|name| format!("memory:///{name}"))
        .ok_or_else(|| format!("unowned fixture edge {specifier}"))
    }
}

fn options(row: &Value) -> CompilerOptions {
    CompilerOptions {
        target: EcmaTarget::Es2022,
        declaration: true,
        checking: Some(CheckingOptions {
            strict_function_types: row["options"]["strictFunctionTypes"] != false,
            strict_null_checks: row["options"]["strictNullChecks"] != false,
            exact_optional_property_types: row["options"]["exactOptionalPropertyTypes"] == true,
            no_unchecked_indexed_access: row["options"]["noUncheckedIndexedAccess"] == true,
            ..CheckingOptions::default()
        }),
        ..CompilerOptions::default()
    }
}

fn loader(files: &Value) -> FixtureLoader {
    FixtureLoader(
        files
            .as_object()
            .unwrap()
            .iter()
            .map(|(name, source)| (name.clone(), source.as_str().unwrap().to_string()))
            .collect(),
    )
}

fn position(code: u32, message: &str, span: &SourceSpan, sources: &FixtureLoader) -> Value {
    let name = span
        .module
        .strip_prefix("memory:///")
        .unwrap_or(&span.module);
    let source = &sources.0[name];
    json!({
        "code": code, "message": message, "module": name,
        "start": source[..span.start].encode_utf16().count(),
        "length": source[span.start..span.end].encode_utf16().count(),
        "related": [],
    })
}

fn diagnostics(result: &blueice_bluets::Compilation, sources: &FixtureLoader) -> Value {
    json!(result
        .diagnostics
        .iter()
        .map(|diagnostic| {
            let Some(native) = &diagnostic.typescript else {
                return diagnostic.to_json();
            };
            let mut described = position(native.code, &native.message, &native.span, sources);
            described["related"] = json!(native
                .related_information
                .iter()
                .map(|related| { position(related.code, &related.message, &related.span, sources) })
                .collect::<Vec<_>>());
            described
        })
        .collect::<Vec<_>>())
}

fn outputs() -> Vec<Value> {
    let (_, reference) = observations();
    let mut failures = Vec::new();
    let mut outputs = Vec::new();
    for row in reference["cases"].as_array().unwrap() {
        let sources = loader(&row["files"]);
        let result = compile("memory:///entry.ts", &sources, options(row));
        let actual = diagnostics(&result, &sources);
        if actual != row["diagnostics"]
            || result.output.is_some() != row["accepts"].as_bool().unwrap()
        {
            failures.push(format!(
                "{}: expected {}; got {}; output={}",
                row["id"],
                row["diagnostics"],
                actual,
                result.output.is_some()
            ));
            continue;
        }
        if let Some(output) = result.output {
            let mut declarations = BTreeMap::new();
            let mut javascript = BTreeMap::new();
            for (name, artifact) in &output.artifacts {
                let name = name
                    .strip_prefix("memory:///")
                    .unwrap()
                    .strip_suffix(".ts")
                    .unwrap();
                if let Some(declaration) = &artifact.declaration {
                    declarations.insert(format!("{name}.d.ts"), declaration.clone());
                }
                javascript.insert(format!("{name}.js"), artifact.javascript.clone());
            }
            if json!(declarations) != row["declarations"] {
                failures.push(format!(
                    "{}: declaration expected {}; got {}",
                    row["id"],
                    row["declarations"],
                    json!(declarations)
                ));
            }
            for kind in ["accept", "reject"] {
                let consumer = &row["consumers"][kind];
                let mut files = declarations.clone();
                files.insert(
                    "consumer.ts".into(),
                    consumer["source"].as_str().unwrap().into(),
                );
                let sources = FixtureLoader(files);
                let result = compile("memory:///consumer.ts", &sources, options(row));
                let actual = diagnostics(&result, &sources);
                if actual != consumer["diagnostics"] {
                    failures.push(format!(
                        "{} consumer {kind}: expected {}; got {}",
                        row["id"], consumer["diagnostics"], actual
                    ));
                }
            }
            outputs.push(
                json!({"id": row["id"], "declarations": declarations, "javascript": javascript}),
            );
        }
    }
    assert!(
        failures.is_empty(),
        "{} G-T8 mismatches:\n{}",
        failures.len(),
        failures.join("\n")
    );
    outputs
}

#[test]
fn return_context_matches_native_graphs_and_downstream_diagnostics() {
    outputs();
}

#[test]
fn matrix_preserves_all_native_pairs_and_consumer_controls() {
    let (input, reference) = observations();
    assert_eq!(reference["typescript"], "5.9.3");
    assert_eq!(input["forms"].as_array().unwrap().len(), 40);
    let mut rows = reference["cases"].as_array().unwrap().iter();
    let mut areas = BTreeSet::new();
    let mut executions = 0;
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
            let mut files = form["files"].as_object().cloned().unwrap_or_default();
            files.insert(
                "entry.ts".into(),
                json!(format!(
                    "// UTF-16 control: 😀\n{}\n{}\n",
                    form["prefix"].as_str().unwrap(),
                    form[verdict].as_str().unwrap()
                )),
            );
            assert_eq!(row["files"], json!(files));
            if verdict == "accept" {
                for kind in ["accept", "reject"] {
                    let consumer = &row["consumers"][kind];
                    assert_eq!(
                        consumer["source"],
                        format!(
                            "// UTF-16 consumer: 😀\n{}\n",
                            form[format!("consumer_{kind}")].as_str().unwrap()
                        )
                    );
                    assert_eq!(
                        consumer["diagnostics"].as_array().unwrap().is_empty(),
                        kind == "accept"
                    );
                }
                if row.get("runtime").is_some() {
                    executions += 1;
                }
            }
        }
    }
    assert!(rows.next().is_none());
    assert_eq!(executions, 4);
    assert_eq!(
        areas,
        BTreeSet::from([
            "freshness",
            "context",
            "member",
            "async",
            "generator",
            "recursion"
        ])
    );
}

#[test]
fn return_inference_work_and_depth_limits_do_not_produce_output() {
    let (input, reference) = observations();
    assert_eq!(input["budget_controls"], reference["budget_controls"]);
    for control in input["budget_controls"].as_array().unwrap() {
        let sources = FixtureLoader(BTreeMap::from([(
            "entry.ts".into(),
            control["source"].as_str().unwrap().to_string(),
        )]));
        let normal = compile("memory:///entry.ts", &sources, options(&json!({})));
        assert!(
            normal.diagnostics.is_empty(),
            "{}: {:?}",
            control["id"],
            normal.diagnostics
        );
        assert!(normal.output.is_some());
        let mut limited = options(&json!({}));
        if control["kind"] == "expansion" {
            limited.limits.max_type_expansions = 1;
        } else {
            limited.limits.parser.max_type_depth = 2;
        }
        let refused = compile("memory:///entry.ts", &sources, limited);
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

#[test]
fn declaration_consumers_do_not_grant_a_runtime_counterpart() {
    let (input, reference) = observations();
    assert_eq!(input["owner_controls"], reference["owner_controls"]);
    assert_eq!(input["owner_controls"].as_array().unwrap().len(), 1);
    for control in input["owner_controls"].as_array().unwrap() {
        let mut files = reference["cases"][0]["declarations"]
            .as_object()
            .unwrap()
            .clone();
        files.insert("consumer.ts".into(), control["source"].clone());
        let sources = loader(&json!(files));
        for checking in [Some(CheckingOptions::default()), None] {
            let mut policy = options(&json!({}));
            policy.checking = checking;
            let result = compile("memory:///consumer.ts", &sources, policy);
            assert!(result.output.is_none());
            assert!(result.diagnostics.iter().any(|d| d.typescript.is_none()
                && d.message.contains("declaration modules are type-only")));
        }
    }
    let declaration = "export declare class Owned { get value(): number; read(): number; }";
    let sources = FixtureLoader(BTreeMap::from([("owned.d.ts".into(), declaration.into())]));
    let result = compile("memory:///owned.d.ts", &sources, options(&json!({})));
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    assert!(result.output.unwrap().artifacts.is_empty());
    for source in [
        "export declare class Owned { read() { return 4; } }",
        "export declare class Owned { get value() { return 4; } }",
        "export declare class Owned { value = 4; }",
        "export declare class Owned { static { console.log(4); } }",
    ] {
        let sources = FixtureLoader(BTreeMap::from([("owned.d.ts".into(), source.into())]));
        let result = compile("memory:///owned.d.ts", &sources, options(&json!({})));
        assert!(result.output.is_none(), "{source}");
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::InvalidDeclarationFile),
            "{source}: {:?}",
            result.diagnostics
        );
    }
}

fn recorder() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../development/browser_core/phase-18-bluets/tools/record_gt8_return_context.cjs")
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3 and Node"]
fn gt8_native_return_graphs_are_reproducible() {
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
fn gt8_exact_declarations_have_native_consumers_syntax_and_execution() {
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
