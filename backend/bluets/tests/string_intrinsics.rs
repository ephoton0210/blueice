// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Frozen P0-C comparisons use public CLI projects and independent API limits.

use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Project {
    root: PathBuf,
    retain: bool,
}

impl Project {
    fn new(id: &str) -> Self {
        let (root, retain) = match std::env::var_os("BLUEICE_STRING_INTRINSICS_OBSERVATIONS") {
            Some(capture) => {
                let root = PathBuf::from(capture).join(id);
                fs::create_dir_all(root.parent().unwrap()).unwrap();
                (root, true)
            }
            None => (
                std::env::temp_dir().canonicalize().unwrap().join(format!(
                    "bluets-p0c-{}-{}",
                    std::process::id(),
                    NEXT.fetch_add(1, Ordering::Relaxed)
                )),
                false,
            ),
        };
        fs::create_dir(&root).expect("each observation must have a fresh project directory");
        Self { root, retain }
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        if !self.retain {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
}

fn diagnostic(value: &Value) -> Value {
    let value = value.get("typescript").unwrap_or(value);
    if value.is_null() {
        return json!({"missingNativeCounterpart": true});
    }
    json!({
        "code": value["code"], "message": value["message"],
        "file": value["span"].get("module"), "position": value["position"],
        "relatedInformation": value["relatedInformation"].as_array().into_iter().flatten().map(diagnostic).collect::<Vec<_>>()
    })
}

fn declarations(root: &Path, directory: &Path, result: &mut BTreeMap<String, String>) {
    if !directory.is_dir() {
        return;
    }
    for entry in fs::read_dir(directory).unwrap() {
        let file = entry.unwrap().path();
        if file.is_dir() {
            declarations(root, &file, result);
        } else {
            let name = file
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            if [".d.ts", ".d.mts", ".d.cts"]
                .iter()
                .any(|suffix| name.ends_with(suffix))
            {
                result.insert(name, fs::read_to_string(file).unwrap());
            }
        }
    }
}

#[test]
fn all_frozen_projects_match_complete_diagnostics_and_exact_declarations() {
    let matrices = [(
        include_str!("fixtures/string_intrinsics/cases.json"),
        include_str!("fixtures/string_intrinsics/reference.json"),
        245,
        980,
    )];
    let mut failures = Vec::new();
    let mut ids = BTreeSet::new();
    let mut agreed = 0;
    for (input, reference, forms, comparisons) in matrices {
        let input: Value = serde_json::from_str(input).unwrap();
        let reference: Value = serde_json::from_str(reference).unwrap();
        assert_eq!(input["cases"].as_array().unwrap().len(), forms);
        assert_eq!(input["profiles"].as_array().unwrap().len(), 4);
        assert_eq!(reference["cases"].as_array().unwrap().len(), comparisons);
        let mut observed = 0;
        for profile in input["profiles"].as_array().unwrap() {
            for case in input["cases"].as_array().unwrap() {
                let expected = &reference["cases"][observed];
                observed += 1;
                let id = format!(
                    "{}/{}",
                    profile["id"].as_str().unwrap(),
                    case["id"].as_str().unwrap()
                );
                assert_eq!(expected["id"], id);
                assert!(ids.insert(id.clone()));
                let project = Project::new(&id);
                fs::write(
                    project.root.join("main.ts"),
                    case["source"].as_str().unwrap(),
                )
                .unwrap();
                let mut files = vec![json!("main.ts")];
                if let Some(additional) = case["files"].as_object() {
                    for (name, source) in additional {
                        let name_path = Path::new(name);
                        assert!(
                            !name_path.is_absolute()
                                && !name_path
                                    .components()
                                    .any(|part| matches!(part, std::path::Component::ParentDir))
                        );
                        let file = project.root.join(name);
                        fs::create_dir_all(file.parent().unwrap()).unwrap();
                        fs::write(file, source.as_str().unwrap()).unwrap();
                        files.push(json!(name));
                    }
                }
                let mut options = input["options"].as_object().unwrap().clone();
                options.extend(profile["compilerOptions"].as_object().unwrap().clone());
                options.insert("outDir".into(), json!("dist"));
                fs::write(
                    project.root.join("tsconfig.json"),
                    serde_json::to_vec_pretty(&json!({"compilerOptions": options, "files": files}))
                        .unwrap(),
                )
                .unwrap();
                let output = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
                    .current_dir(&project.root)
                    .args(["build", "--config", "tsconfig.json", "--diagnostics-json"])
                    .output()
                    .unwrap();
                let actual = String::from_utf8_lossy(&output.stderr)
                    .lines()
                    .map(|line| {
                        serde_json::from_str::<Value>(line)
                            .map(|value| diagnostic(&value))
                            .unwrap_or_else(|_| json!({"unstructured": line}))
                    })
                    .collect::<Vec<_>>();
                let dist = project.root.join("dist");
                let mut emitted = BTreeMap::new();
                declarations(&dist, &dist, &mut emitted);
                let publishes_on_error = expected["accepts"] == false
                    && dist.is_dir()
                    && fs::read_dir(&dist).unwrap().next().is_some();
                let matches = output.status.success() == expected["accepts"].as_bool().unwrap()
                    && json!(actual) == expected["diagnostics"]
                    && json!(emitted) == expected["declarations"]
                    && !publishes_on_error;
                if project.retain {
                    fs::write(project.root.join("build.stdout"), &output.stdout).unwrap();
                    fs::write(project.root.join("build.stderr"), &output.stderr).unwrap();
                    fs::write(project.root.join("observation.json"), serde_json::to_vec_pretty(&json!({"id": id, "accepts": output.status.success(), "diagnostics": actual, "declarations": emitted, "publishedOnRejectedNativeInput": publishes_on_error, "agrees": matches})).unwrap()).unwrap();
                }
                if matches {
                    agreed += 1;
                } else {
                    failures.push(format!("{id}: accepts={}, diagnostics={}, declarations={}, publishedOnError={publishes_on_error}", output.status.success(), json!(actual), json!(emitted)));
                }
            }
        }
        assert_eq!(observed, comparisons);
    }
    assert_eq!(ids.len(), 980);
    assert!(
        failures.is_empty(),
        "{agreed}/980 complete agreements; {} mismatches; first twelve:\n{}",
        failures.len(),
        failures
            .iter()
            .take(12)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}

fn boundaries() -> Value {
    serde_json::from_str(include_str!(
        "fixtures/string_intrinsics/boundary-controls.json"
    ))
    .unwrap()
}

#[test]
fn owner_declarations_keep_precedence_and_static_names_grant_no_runtime_binding() {
    use blueice_bluets::{compile, CompilerOptions, MapLoader, ModuleSource};
    let mut observed = 0;
    for row in boundaries()["controls"].as_array().unwrap() {
        let kind = row["kind"].as_str().unwrap();
        if kind == "resource-limit" {
            continue;
        }
        observed += 1;
        let mut options = CompilerOptions::default();
        let source = match kind {
            "ambient-owner" => {
                options.ambient_declaration_modules = vec![ModuleSource::new(
                    "memory:///owner.d.ts",
                    row["declaration"].as_str().unwrap(),
                )];
                row["source"].as_str().unwrap().to_string()
            }
            "local-owner" => format!(
                "{}\n{}",
                row["declaration"].as_str().unwrap(),
                row["source"].as_str().unwrap()
            ),
            "runtime-authority" => {
                options.require_declared_global_calls = true;
                row["source"].as_str().unwrap().to_string()
            }
            _ => panic!("unknown frozen boundary: {kind}"),
        };
        let result = compile(
            "memory:///main.ts",
            &MapLoader::from([ModuleSource::new("memory:///main.ts", source)]),
            options,
        );
        if kind == "runtime-authority" {
            assert!(
                result.has_errors(),
                "{} acquired a runtime binding",
                row["id"]
            );
        } else {
            assert!(
                result.diagnostics.is_empty(),
                "{}: {:?}",
                row["id"],
                result.diagnostics
            );
            assert_eq!(
                json!(result.project.modules.keys().collect::<Vec<_>>()),
                row["expected"]["projectModules"]
            );
            assert!(result.output.is_some(), "{} owner output", row["id"]);
        }
    }
    assert_eq!(observed, 8);
}

#[test]
fn configured_resource_refusals_remain_separate_from_native_instantiation_errors() {
    use blueice_bluets::{
        compile, CheckingOptions, CompilerOptions, DiagnosticCode, MapLoader, ModuleSource,
    };
    let mut observed = 0;
    for row in boundaries()["controls"].as_array().unwrap() {
        if row["kind"] != "resource-limit" {
            continue;
        }
        observed += 1;
        let loader = MapLoader::from([ModuleSource::new(
            "entry.ts",
            row["source"].as_str().unwrap(),
        )]);
        let mut options = CompilerOptions {
            checking: Some(CheckingOptions::default()),
            declaration: true,
            ..CompilerOptions::default()
        };
        let normal = compile("entry.ts", &loader, options.clone());
        assert!(
            normal.diagnostics.is_empty(),
            "{} normal: {:?}",
            row["id"],
            normal.diagnostics
        );
        assert!(normal.output.is_some(), "{} normal output", row["id"]);
        if let Some(value) = row["limited"]["max_type_expansions"].as_u64() {
            options.limits.max_type_expansions = value as usize;
        } else {
            options.limits.parser.max_type_depth =
                row["limited"]["parser.max_type_depth"].as_u64().unwrap() as usize;
        }
        let refused = compile("entry.ts", &loader, options);
        assert!(
            refused.output.is_none(),
            "{} published limited output",
            row["id"]
        );
        assert!(
            refused.diagnostics.iter().any(|diagnostic| diagnostic.code
                == DiagnosticCode::ResourceLimit
                && diagnostic.typescript.is_none()),
            "{}: {:?}",
            row["id"],
            refused.diagnostics
        );
    }
    assert_eq!(observed, 12);
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3, Acorn 8.15.0 and Node"]
fn native_observations_consumers_syntax_and_runtime_are_reproducible() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for (script, arguments) in [
        ("record_string_intrinsics.cjs", vec!["--verify"]),
        ("verify_string_intrinsics.cjs", Vec::new()),
    ] {
        let output = Command::new("node")
            .arg(
                root.join("development/browser_core/phase-18-bluets/tools")
                    .join(script),
            )
            .args(arguments)
            .env(
                "BLUEICE_STRING_INTRINSICS_COMPILER",
                env!("CARGO_BIN_EXE_bluetsc"),
            )
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{script}: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn generated_literal_array_contexts_and_call_controls_match_native() {
    let fixture: Value = serde_json::from_str(include_str!(
        "fixtures/string_intrinsics/literal-context-regressions.json"
    ))
    .unwrap();
    let rows = fixture["cases"].as_array().unwrap();
    assert_eq!(rows.len(), 10);
    let mut ids = BTreeSet::new();
    for row in rows {
        let id = row["id"].as_str().unwrap();
        assert!(ids.insert(id));
        let project = Project::new(&format!("literal-context-regressions/{id}"));
        fs::write(
            project.root.join("main.ts"),
            row["source"].as_str().unwrap(),
        )
        .unwrap();
        fs::write(
            project.root.join("tsconfig.json"),
            serde_json::to_vec_pretty(&json!({
                "compilerOptions": fixture["options"], "files": ["main.ts"]
            }))
            .unwrap(),
        )
        .unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
            .current_dir(&project.root)
            .args(["build", "--config", "tsconfig.json", "--diagnostics-json"])
            .output()
            .unwrap();
        let actual = String::from_utf8_lossy(&output.stderr)
            .lines()
            .map(|line| diagnostic(&serde_json::from_str::<Value>(line).unwrap()))
            .collect::<Vec<_>>();
        let dist = project.root.join("dist");
        let mut emitted = BTreeMap::new();
        declarations(&dist, &dist, &mut emitted);
        if project.retain {
            fs::write(project.root.join("build.stdout"), &output.stdout).unwrap();
            fs::write(project.root.join("build.stderr"), &output.stderr).unwrap();
            fs::write(
                project.root.join("observation.json"),
                serde_json::to_vec_pretty(&json!({
                    "id": id, "accepts": output.status.success(),
                    "diagnostics": actual, "declarations": emitted
                }))
                .unwrap(),
            )
            .unwrap();
        }
        assert_eq!(
            output.status.success(),
            row["accepts"].as_bool().unwrap(),
            "{id}: verdict"
        );
        assert_eq!(json!(actual), row["diagnostics"], "{id}: diagnostics");
        assert_eq!(json!(emitted), row["declarations"], "{id}: declarations");
        if !output.status.success() {
            assert!(
                !dist.is_dir() || fs::read_dir(&dist).unwrap().next().is_none(),
                "{id}: rejected input published output"
            );
        }
    }
}
