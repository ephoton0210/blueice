// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Frozen P0-B comparisons use public CLI projects and independent API limits.

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
        let (root, retain) = match std::env::var_os("BLUEICE_NONNULLABLE_AWAITED_OBSERVATIONS") {
            Some(capture) => {
                let root = PathBuf::from(capture).join(id);
                fs::create_dir_all(root.parent().unwrap()).unwrap();
                (root, true)
            }
            None => (
                std::env::temp_dir().canonicalize().unwrap().join(format!(
                    "bluets-p0b-{}-{}",
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
    let matrices = [
        (
            include_str!("fixtures/nonnullable_awaited/cases.json"),
            include_str!("fixtures/nonnullable_awaited/reference.json"),
            91,
            364,
        ),
        (
            include_str!("fixtures/nonnullable_awaited/operator-cases.json"),
            include_str!("fixtures/nonnullable_awaited/operator-reference.json"),
            8,
            32,
        ),
    ];
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
    assert_eq!(ids.len(), 396);
    assert!(
        failures.is_empty(),
        "{agreed}/396 complete agreements; {} mismatches; first twelve:\n{}",
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
        "fixtures/nonnullable_awaited/boundary-controls.json"
    ))
    .unwrap()
}

#[test]
fn demanded_cycles_report_native_errors_without_rejecting_guarded_or_decreasing_recursion() {
    let reference: Value = serde_json::from_str(include_str!(
        "fixtures/nonnullable_awaited/demand-reference.json"
    ))
    .unwrap();
    assert_eq!(reference["typescript"], "5.9.3");
    let mut failures = Vec::new();
    for case in reference["cases"].as_array().unwrap() {
        let id = format!("demand/{}", case["id"].as_str().unwrap());
        let project = Project::new(&id);
        fs::write(
            project.root.join("main.ts"),
            case["source"].as_str().unwrap(),
        )
        .unwrap();
        let mut options = reference["compilerOptions"].as_object().unwrap().clone();
        options.insert("outDir".into(), json!("dist"));
        fs::write(
            project.root.join("tsconfig.json"),
            serde_json::to_vec_pretty(&json!({"compilerOptions": options, "files": ["main.ts"]}))
                .unwrap(),
        )
        .unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
            .current_dir(&project.root)
            .args(["build", "--config", "tsconfig.json", "--diagnostics-json"])
            .output()
            .unwrap();
        let diagnostics = String::from_utf8_lossy(&output.stderr)
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
        let published_on_error = case["accepts"] == false
            && dist.is_dir()
            && fs::read_dir(&dist).unwrap().next().is_some();
        let agrees = output.status.success() == case["accepts"].as_bool().unwrap()
            && json!(diagnostics) == case["diagnostics"]
            && json!(emitted) == case["declarations"]
            && !published_on_error;
        let actual = json!({"id": id, "accepts": output.status.success(), "diagnostics": diagnostics, "declarations": emitted, "publishedOnRejectedNativeInput": published_on_error, "agrees": agrees});
        if project.retain {
            fs::write(project.root.join("build.stdout"), &output.stdout).unwrap();
            fs::write(project.root.join("build.stderr"), &output.stderr).unwrap();
            fs::write(
                project.root.join("observation.json"),
                serde_json::to_vec_pretty(&actual).unwrap(),
            )
            .unwrap();
        }
        if !agrees {
            failures.push(actual);
        }
    }
    assert_eq!(reference["cases"].as_array().unwrap().len(), 6);
    assert!(failures.is_empty(), "demand controls: {}", json!(failures));
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
            assert_eq!(
                result.output.unwrap().artifacts.len(),
                row["expected"]["artifacts"].as_u64().unwrap() as usize
            );
        }
    }
    assert_eq!(observed, 6);
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
    assert_eq!(observed, 9);
}

#[test]
fn declared_and_inferred_demands_refuse_configured_work_limits_without_native_errors() {
    use blueice_bluets::{
        compile, CheckingOptions, CompilerOptions, DiagnosticCode, MapLoader, ModuleSource,
    };
    let controls: Value = serde_json::from_str(include_str!(
        "fixtures/nonnullable_awaited/demand-resource-controls.json"
    ))
    .unwrap();
    for row in controls["controls"].as_array().unwrap() {
        let index = row["id"].as_str().unwrap();
        let loader = MapLoader::from([ModuleSource::new(
            "entry.ts",
            row["source"].as_str().unwrap(),
        )]);
        let limit = row["max_type_expansions"].as_u64().unwrap() as usize;
        let mut options = CompilerOptions {
            checking: Some(CheckingOptions::default()),
            declaration: true,
            ..CompilerOptions::default()
        };
        options.limits.max_type_expansions = limit;
        let refused = compile("entry.ts", &loader, options);
        assert!(refused.output.is_none(), "{index}/{limit} published output");
        assert!(
            refused.diagnostics.iter().any(|diagnostic| diagnostic.code
                == DiagnosticCode::ResourceLimit
                && diagnostic.typescript.is_none()),
            "{index}/{limit}: {:?}",
            refused.diagnostics
        );
        assert!(
            refused.diagnostics.iter().all(|diagnostic| diagnostic
                .typescript
                .as_ref()
                .is_none_or(|native| native.code != 2589)),
            "{index}/{limit} conflated a configured limit with semantic nontermination: {:?}",
            refused.diagnostics
        );
    }
    assert_eq!(controls["controls"].as_array().unwrap().len(), 6);
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3, Acorn 8.15.0 and Node"]
fn native_observations_consumers_syntax_and_runtime_are_reproducible() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for (script, arguments) in [
        ("record_nonnullable_awaited.cjs", Vec::new()),
        ("record_nonnullable_awaited.cjs", vec!["--operators"]),
        ("verify_nonnullable_awaited.cjs", Vec::new()),
    ] {
        let output = Command::new("node")
            .arg(
                root.join("development/browser_core/phase-18-bluets/tools")
                    .join(script),
            )
            .args(arguments)
            .env(
                "BLUEICE_NONNULLABLE_AWAITED_COMPILER",
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
