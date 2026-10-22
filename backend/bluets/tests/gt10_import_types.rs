// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! G-T10 observes return precision, declaration graphs and downstream consumers.
use blueice_bluets::{
    compile, CheckingOptions, CompilerOptions, DiagnosticCode, EcmaTarget, ModuleLoader,
    ModuleSource, SourceSpan,
};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

#[path = "oracle_support/gt10_boundaries.rs"]
mod boundaries;

fn observations() -> (Value, Value) {
    (
        serde_json::from_str(include_str!("fixtures/gt10_import_types/cases.json")).unwrap(),
        serde_json::from_str(include_str!("fixtures/gt10_import_types/reference.json")).unwrap(),
    )
}

/// The test owner supplies only this frozen graph; resolution never performs I/O.
struct FixtureLoader(BTreeMap<String, String>);

impl ModuleLoader for FixtureLoader {
    fn resolution_error_typescript_code(&self, _message: &str) -> Option<u32> {
        Some(2307)
    }
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
        module_kind: if row["options"]["module"] == "CommonJS" {
            blueice_bluets::ModuleKind::CommonJs
        } else {
            blueice_bluets::ModuleKind::Esm
        },
        verbatim_module_syntax: row["options"]["verbatimModuleSyntax"] == true,
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
                files.extend(
                    sources
                        .0
                        .iter()
                        .filter(|(name, _)| name.ends_with(".d.ts"))
                        .map(|(name, text)| (name.clone(), text.clone())),
                );
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
        "{} G-T10 mismatches:\n{}",
        failures.len(),
        failures.join("\n")
    );
    outputs
}

#[test]
fn import_types_match_native_graphs_and_downstream_diagnostics() {
    outputs();
}

#[test]
fn matrix_preserves_every_native_program_and_module_configuration() {
    let (input, reference) = observations();
    assert_eq!(input["forms"].as_array().unwrap().len(), 30);
    assert_eq!(input["rejections"].as_array().unwrap().len(), 9);
    assert_eq!(input["owner_controls"], reference["owner_controls"]);
    let mut rows = reference["cases"].as_array().unwrap().iter();
    let mut ids = BTreeSet::new();
    for form in input["forms"]
        .as_array()
        .unwrap()
        .iter()
        .chain(input["rejections"].as_array().unwrap())
    {
        let modules = form.get("modules").unwrap_or(&input["modules"]);
        let verdicts = if form.get("source").is_some() {
            vec!["reject"]
        } else {
            vec!["accept", "reject"]
        };
        for module in modules.as_array().unwrap() {
            for verdict in &verdicts {
                let row = rows.next().unwrap();
                let id = format!(
                    "{}-{}-{verdict}",
                    form["id"].as_str().unwrap(),
                    module.as_str().unwrap().to_lowercase()
                );
                assert!(ids.insert(id.clone()));
                assert_eq!(row["id"], id);
                assert_eq!(row["accepts"], *verdict == "accept");
                let source = if let Some(source) = form["source"].as_str() {
                    source.to_string()
                } else {
                    format!(
                        "{}\n{}",
                        form["prefix"].as_str().unwrap(),
                        form[*verdict].as_str().unwrap()
                    )
                };
                let mut files = form["files"].as_object().unwrap().clone();
                files.insert(
                    "entry.ts".into(),
                    json!(format!("// UTF-16 control: 😀\n{source}\n")),
                );
                assert_eq!(row["files"], json!(files));
                let mut expected = input["options"].as_object().unwrap().clone();
                if let Some(overrides) = form["options"].as_object() {
                    expected.extend(overrides.clone());
                }
                expected.insert("module".into(), module.clone());
                assert_eq!(row["options"], json!(expected));
                assert_eq!(
                    row["diagnostics"].as_array().unwrap().is_empty(),
                    *verdict == "accept"
                );
                if *verdict == "accept" {
                    for kind in ["accept", "reject"] {
                        assert_eq!(
                            row["consumers"][kind]["diagnostics"]
                                .as_array()
                                .unwrap()
                                .is_empty(),
                            kind == "accept"
                        );
                    }
                }
            }
        }
    }
    assert!(rows.next().is_none());
    assert_eq!(ids.len(), 133);
    assert_eq!(
        reference["cases"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|row| row["accepts"] == true)
            .count(),
        58
    );
    assert_eq!(
        reference["cases"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|row| row.get("runtime").is_some())
            .count(),
        15
    );
}

fn recorder() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../development/browser_core/phase-18-bluets/tools/record_gt10_import_types.cjs")
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3 and Node"]
fn gt10_native_import_type_graphs_are_reproducible() {
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
fn gt10_exact_declarations_have_native_consumers_elision_and_execution() {
    let bytes = serde_json::to_vec(&outputs()).unwrap();
    let mut child = Command::new("node")
        .arg(recorder())
        .arg("--outputs")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(&bytes).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
