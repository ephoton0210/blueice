// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Parser-owned static regions preserve call arguments and runtime operators.

use blueice_bluets::{
    compile, CheckingOptions, CompilerOptions, Diagnostic, EcmaTarget, JavaScriptLayout,
    ModuleKind, ModuleLoader, ModuleSource, Severity, SourceSpan,
};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::process::Command;

const MAIN: &str = "memory:///main.ts";

fn input() -> Value {
    serde_json::from_str(include_str!("fixtures/erased_type_regions/cases.json")).unwrap()
}

fn reference() -> Value {
    serde_json::from_str(include_str!("fixtures/erased_type_regions/reference.json")).unwrap()
}

/// Only the authored fixture graph is available; resolution never reads a file.
struct FixtureLoader(BTreeMap<String, String>);

impl FixtureLoader {
    fn new(entry: &Value) -> Self {
        let mut files = BTreeMap::from([(
            "main.ts".to_owned(),
            entry["source"].as_str().unwrap().to_owned(),
        )]);
        if let Some(extra) = entry["files"].as_object() {
            files.extend(
                extra
                    .iter()
                    .map(|(name, text)| (name.to_owned(), text.as_str().unwrap().to_owned())),
            );
        }
        Self(files)
    }
}

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
            name.to_owned(),
            format!("{stem}.ts"),
            format!("{stem}.d.ts"),
        ]
        .into_iter()
        .find(|name| self.0.contains_key(name))
        .map(|name| format!("memory:///{name}"))
        .ok_or_else(|| format!("unowned fixture edge {specifier}"))
    }
}

fn options(profile: &Value) -> CompilerOptions {
    CompilerOptions {
        target: if profile["compilerOptions"]["target"] == "ES5" {
            EcmaTarget::Es5
        } else {
            EcmaTarget::Es2022
        },
        module_kind: if profile["compilerOptions"]["module"] == "CommonJS" {
            ModuleKind::CommonJs
        } else {
            ModuleKind::Esm
        },
        libraries: Some(vec![EcmaTarget::Es5]),
        checking: Some(CheckingOptions {
            strict_null_checks: true,
            ..CheckingOptions::default()
        }),
        javascript_layout: JavaScriptLayout::TypeScript,
        always_strict: true,
        declaration: true,
        declaration_map: true,
        source_map: true,
        inline_sources: false,
        resolver_fingerprint: "erased-type-regions-owned-fixture-v1".into(),
        ..CompilerOptions::default()
    }
}

fn description(code: u32, message: &str, span: &SourceSpan, loader: &FixtureLoader) -> Value {
    let name = span
        .module
        .strip_prefix("memory:///")
        .unwrap_or(&span.module);
    let Some(source) = loader.0.get(name) else {
        return json!({"unexpectedModule":name,"code":code,"message":message});
    };
    let before = &source[..span.start];
    json!({
        "code":code,"message":message,"file":name,
        "position":{
            "line":before.bytes().filter(|byte| *byte == b'\n').count()+1,
            "column":before.rsplit('\n').next().unwrap().encode_utf16().count()+1,
            "length":source[span.start..span.end].encode_utf16().count(),
        },
        "relatedInformation":[],
    })
}

fn diagnostic(diagnostic: &Diagnostic, loader: &FixtureLoader) -> Value {
    let Some(native) = &diagnostic.typescript else {
        let mut value = diagnostic.to_json();
        value["span"]["module"] = json!(diagnostic
            .span
            .module
            .strip_prefix("memory:///")
            .unwrap_or(&diagnostic.span.module));
        return json!({"unmatchedBlueDiagnostic":value});
    };
    let mut value = description(native.code, &native.message, &native.span, loader);
    value["relatedInformation"] = json!(native
        .related_information
        .iter()
        .map(|row| description(row.code, &row.message, &row.span, loader))
        .collect::<Vec<_>>());
    value
}

#[test]
fn call_argument_semantics_match_every_frozen_native_observation() {
    let input = input();
    let reference = reference();
    let controls: Value = serde_json::from_str(include_str!(
        "fixtures/erased_type_regions/discovery-controls.json"
    ))
    .unwrap();
    let mut differences = Vec::new();
    let rows = reference["cases"].as_array().unwrap();
    assert_eq!(rows.len(), 136);
    for wanted in rows {
        let entry = input["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"] == wanted["form"])
            .unwrap();
        let profile = input["profiles"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"] == wanted["profile"])
            .unwrap();
        let loader = FixtureLoader::new(entry);
        let result = compile(MAIN, &loader, options(profile));
        let mut diagnostics: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|row| row.severity == Severity::Error)
            .map(|row| diagnostic(row, &loader))
            .collect();
        diagnostics.sort_by_key(|row| {
            (
                row["file"].as_str().unwrap_or("").to_owned(),
                row["position"]["line"].as_u64(),
                row["position"]["column"].as_u64(),
                row["position"]["length"].as_u64(),
                row["code"].as_u64(),
            )
        });
        let control = controls["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"] == wanted["id"]);
        let expected_accepts = control.map_or(wanted["accepts"].as_bool().unwrap(), |row| {
            row["exitCode"] == 0
        });
        let expected_diagnostics =
            control.map_or(&wanted["diagnostics"], |row| &row["diagnostics"]);
        if control.is_none() && wanted["accepts"] == true {
            if let Some(output) = &result.output {
                for (name, text) in wanted["outputs"].as_object().unwrap() {
                    if let Some(stem) = name.strip_suffix(".d.ts") {
                        let id = format!("memory:///{stem}.ts");
                        let text = text.as_str().unwrap();
                        let expected = text
                            .strip_suffix(&format!("//# sourceMappingURL={name}.map"))
                            .unwrap();
                        let actual = output
                            .artifacts
                            .get(&id)
                            .and_then(|artifact| artifact.declaration.as_deref());
                        if actual != Some(expected) {
                            differences.push(format!(
                                "{} {name}: actual declaration={actual:?}, Native={expected:?}",
                                wanted["id"]
                            ));
                        }
                    }
                }
            }
        }
        if result.output.is_some() != expected_accepts
            || json!(diagnostics) != *expected_diagnostics
        {
            differences.push(format!(
                "{}: output={}, actual={diagnostics:?}, Native={}",
                wanted["id"],
                result.output.is_some(),
                wanted["diagnostics"]
            ));
        }
    }
    assert!(differences.is_empty(), "{}", differences.join("\n"));
}

#[test]
fn callable_type_arguments_do_not_duplicate_the_outer_call_diagnostic() {
    let inputs: Value =
        serde_json::from_str(include_str!("fixtures/callable_utilities/cases.json")).unwrap();
    let native: Value =
        serde_json::from_str(include_str!("fixtures/callable_utilities/reference.json")).unwrap();
    let entry = inputs["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == "function-fixed-reject")
        .unwrap();
    let expected = native["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == "es5-commonjs/function-fixed-reject")
        .unwrap();
    let loader = FixtureLoader::new(entry);
    for profile in input()["profiles"].as_array().unwrap() {
        let result = compile(MAIN, &loader, options(profile));
        let diagnostics: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|row| row.severity == Severity::Error)
            .map(|row| diagnostic(row, &loader))
            .collect();
        assert!(result.output.is_none());
        assert_eq!(json!(diagnostics), expected["diagnostics"]);
    }
}

#[test]
fn runtime_comparisons_keep_their_real_argument_separator() {
    let source = "declare const a: number; declare const b: number; declare const c: number; \
        declare function two(value: boolean, other: number): number; \
        export const output = two(a < b, c);\n";
    let entry = json!({"source":source});
    for profile in input()["profiles"].as_array().unwrap() {
        let result = compile(MAIN, &FixtureLoader::new(&entry), options(profile));
        assert!(!result.has_errors(), "{:?}", result.diagnostics);
        assert_eq!(
            result.checked.unwrap().modules[MAIN]
                .symbols
                .iter()
                .find(|row| row.name == "output")
                .unwrap()
                .value_type,
            Some(blueice_bluets::Type::Number)
        );
    }
}

#[test]
fn concrete_constructor_uses_the_same_parser_owned_argument_boundaries() {
    let source = "class Box { constructor(public value: Record<string, number>) {} }\n\
        export const result = new Box({count: 1} as Record<string, number>);\n";
    let entry = json!({"source":source});
    for profile in input()["profiles"].as_array().unwrap() {
        let result = compile(MAIN, &FixtureLoader::new(&entry), options(profile));
        assert!(!result.has_errors(), "{:?}", result.diagnostics);
        assert!(result.output.is_some());
    }
}

#[test]
#[ignore = "requires pinned TypeScript and complete producer/consumer/map/execution verification"]
fn call_argument_outputs_match_the_complete_native_contract() {
    let compiler = std::env::var_os("BLUEICE_ERASED_TYPE_REGIONS_COMPILER")
        .unwrap_or_else(|| env!("CARGO_BIN_EXE_bluetsc").into());
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(
        "../../development/browser_core/phase-18-bluets/tools/verify_erased_type_regions.cjs",
    );
    let root = std::env::var_os("BLUEICE_ERASED_TYPE_REGIONS_OUTPUT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::env::temp_dir().join(format!("bluets-erased-regions-{}", std::process::id()))
        });
    for surface in ["legacy", "native"] {
        let mut command = Command::new("node");
        command
            .arg("--no-maglev")
            .arg(&script)
            .arg(&compiler)
            .arg(root.join(surface));
        if surface == "native" {
            command.arg("--native");
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "{surface}: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
