// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Frozen P0-D comparisons use public CLI projects and independent API limits.

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

#[test]
fn owner_metadata_and_configured_limits_remain_separate_from_native_comparisons() {
    use blueice_bluets::{
        compile, CheckingOptions, CompilerOptions, DiagnosticCode, MapLoader, ModuleSource,
    };
    let controls: Value = serde_json::from_str(include_str!(
        "fixtures/mapped_inference/boundary-controls.json"
    ))
    .unwrap();
    let mut observed = 0;
    for row in controls["controls"].as_array().unwrap() {
        observed += 1;
        let mut options = CompilerOptions {
            checking: Some(CheckingOptions::default()),
            declaration: true,
            ..CompilerOptions::default()
        };
        let loader = MapLoader::from([ModuleSource::new(
            "memory:///main.ts",
            row["source"].as_str().unwrap(),
        )]);
        if let Some(declaration) = row["declaration"].as_str() {
            options.ambient_declaration_modules =
                vec![ModuleSource::new("memory:///owner.d.ts", declaration)];
        }
        if row["kind"] == "runtime-authority" {
            options.require_declared_global_calls = true;
            let result = compile("memory:///main.ts", &loader, options);
            assert!(
                result.has_errors(),
                "{} acquired runtime authority",
                row["id"]
            );
            assert!(
                result.output.is_none(),
                "{} published refused output",
                row["id"]
            );
            continue;
        }
        let normal = compile("memory:///main.ts", &loader, options.clone());
        assert!(
            normal.diagnostics.is_empty(),
            "{}: {:?}",
            row["id"],
            normal.diagnostics
        );
        assert!(normal.output.is_some(), "{} normal output", row["id"]);
        if row["kind"] == "ambient-owner" {
            assert_eq!(
                normal
                    .project
                    .modules
                    .keys()
                    .map(String::as_str)
                    .collect::<Vec<_>>(),
                vec!["memory:///main.ts", "memory:///owner.d.ts"]
            );
            continue;
        }
        if let Some(work) = row["limited"]["max_type_expansions"].as_u64() {
            options.limits.max_type_expansions = work as usize;
        } else {
            options.limits.parser.max_type_depth =
                row["limited"]["parser.max_type_depth"].as_u64().unwrap() as usize;
        }
        let result = compile("memory:///main.ts", &loader, options);
        assert!(
            result.output.is_none(),
            "{} published limited output",
            row["id"]
        );
        assert!(
            result
                .diagnostics
                .iter()
                .any(|row| row.code == DiagnosticCode::ResourceLimit && row.typescript.is_none()),
            "{}: {:?}",
            row["id"],
            result.diagnostics
        );
    }
    assert_eq!(observed, 12);
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3, Acorn 8.15.0 and Node"]
fn native_observations_consumers_syntax_and_runtime_are_reproducible() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for (script, arguments) in [
        ("record_mapped_inference.cjs", vec!["--verify"]),
        ("verify_mapped_callback_context.cjs", Vec::new()),
        (
            "verify_mapped_callback_context.cjs",
            vec!["generic-argument-controls.json"],
        ),
        ("verify_mapped_inference.cjs", Vec::new()),
    ] {
        let output = Command::new("node")
            .arg(
                root.join("development/browser_core/phase-18-bluets/tools")
                    .join(script),
            )
            .args(arguments)
            .env(
                "BLUEICE_MAPPED_INFERENCE_COMPILER",
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

impl Project {
    fn new(id: &str) -> Self {
        let (root, retain) = match std::env::var_os("BLUEICE_MAPPED_INFERENCE_OBSERVATIONS") {
            Some(capture) => {
                let root = PathBuf::from(capture).join(id);
                fs::create_dir_all(root.parent().unwrap()).unwrap();
                (root, true)
            }
            None => (
                std::env::temp_dir().canonicalize().unwrap().join(format!(
                    "bluets-p0d-{}-{}",
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
fn literal_precision_and_ordinary_copy_widening_match_native() {
    let fixture: Value = serde_json::from_str(include_str!(
        "fixtures/mapped_inference/literal-preservation-controls.json"
    ))
    .unwrap();
    let rows = fixture["cases"].as_array().unwrap();
    assert_eq!(rows.len(), 4);
    for row in rows {
        let id = row["id"].as_str().unwrap();
        let project = Project::new(&format!("literal-preservation/{id}"));
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
        let diagnostics = String::from_utf8_lossy(&output.stderr)
            .lines()
            .map(|line| diagnostic(&serde_json::from_str::<Value>(line).unwrap()))
            .collect::<Vec<_>>();
        let dist = project.root.join("dist");
        let mut emitted = BTreeMap::new();
        declarations(&dist, &dist, &mut emitted);
        if project.retain {
            fs::write(project.root.join("build.stdout"), &output.stdout).unwrap();
            fs::write(project.root.join("build.stderr"), &output.stderr).unwrap();
            fs::write(project.root.join("observation.json"), serde_json::to_vec_pretty(&json!({
                "id":id,"accepts":output.status.success(),"diagnostics":diagnostics,"declarations":emitted
            })).unwrap()).unwrap();
        }
        assert_eq!(
            output.status.success(),
            row["accepts"].as_bool().unwrap(),
            "{id}"
        );
        assert_eq!(json!(diagnostics), row["diagnostics"], "{id}");
        assert_eq!(json!(emitted), row["declarations"], "{id}");
    }
}

#[test]
fn generic_key_identity_retains_returned_method_callback_context() {
    let fixture: Value = serde_json::from_str(include_str!(
        "fixtures/mapped_inference/callback-context-controls.json"
    ))
    .unwrap();
    check_native_supplement(&fixture, "callback-context", 4);
}

#[test]
fn scoped_generic_arguments_preserve_precise_callable_results() {
    let fixture: Value = serde_json::from_str(include_str!(
        "fixtures/mapped_inference/generic-argument-controls.json"
    ))
    .unwrap();
    check_native_supplement(&fixture, "generic-arguments", 12);
}

fn check_native_supplement(fixture: &Value, group: &str, expected: usize) {
    assert_eq!(fixture["cases"].as_array().unwrap().len(), expected);
    let mut failures = Vec::new();
    for row in fixture["cases"].as_array().unwrap() {
        let id = row["id"].as_str().unwrap();
        let project = Project::new(&format!("{group}/{id}"));
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
        let diagnostics = String::from_utf8_lossy(&output.stderr)
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
                    "id": id, "accepts": output.status.success(), "diagnostics": diagnostics,
                    "declarations": emitted
                }))
                .unwrap(),
            )
            .unwrap();
        }
        if output.status.success() != row["accepts"].as_bool().unwrap()
            || json!(diagnostics) != row["diagnostics"]
            || json!(emitted) != row["declarations"]
        {
            failures.push(format!(
                "{id}: diagnostics={diagnostics:?}; declarations={emitted:?}"
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn related_parameter_projections_retain_owner_boundaries() {
    let mut observed = 0;
    for source in [
        include_str!("fixtures/mapped_inference/projection-owner-controls.json"),
        include_str!("fixtures/mapped_inference/projection-identity-controls.json"),
        include_str!("fixtures/mapped_inference/projection-mutation-controls.json"),
    ] {
        let fixture: Value = serde_json::from_str(source).unwrap();
        let rows = fixture["cases"].as_array().unwrap();
        observed += rows.len();
        for row in rows {
            let id = row["id"].as_str().unwrap();
            let project = Project::new(&format!("projection-owners/{id}"));
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
            let diagnostics = String::from_utf8_lossy(&output.stderr)
                .lines()
                .map(|line| diagnostic(&serde_json::from_str::<Value>(line).unwrap()))
                .collect::<Vec<_>>();
            let dist = project.root.join("dist");
            let mut emitted = BTreeMap::new();
            declarations(&dist, &dist, &mut emitted);
            if project.retain {
                fs::write(project.root.join("build.stdout"), &output.stdout).unwrap();
                fs::write(project.root.join("build.stderr"), &output.stderr).unwrap();
                fs::write(project.root.join("observation.json"), serde_json::to_vec_pretty(&json!({
                "id":id,"accepts":output.status.success(),"diagnostics":diagnostics,"declarations":emitted
            })).unwrap()).unwrap();
            }
            assert_eq!(
                output.status.success(),
                row["blueAccepts"].as_bool().unwrap(),
                "{id}"
            );
            if row["blueAccepts"] == true {
                assert_eq!(json!(diagnostics), row["diagnostics"], "{id}");
                assert_eq!(json!(emitted), row["declarations"], "{id}");
            } else {
                assert_eq!(
                    diagnostics
                        .iter()
                        .map(|row| row["code"].as_u64().unwrap())
                        .collect::<Vec<_>>(),
                    vec![2322],
                    "{id}: intentional owner refusal"
                );
                assert!(
                    !dist.is_dir() || fs::read_dir(&dist).unwrap().next().is_none(),
                    "{id}: refused output"
                );
            }
        }
    }
    assert_eq!(observed, 13);
}

#[test]
fn all_frozen_projects_match_complete_diagnostics_and_exact_declarations() {
    let matrices = [(
        include_str!("fixtures/mapped_inference/cases.json"),
        include_str!("fixtures/mapped_inference/reference.json"),
        101,
        404,
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
                if let Some(extra) = case["compilerOptions"].as_object() {
                    options.extend(extra.clone());
                }
                options.insert("outDir".into(), json!("dist"));
                fs::write(
                    project.root.join("tsconfig.json"),
                    serde_json::to_vec_pretty(&json!({"compilerOptions": options, "files": files}))
                        .unwrap(),
                )
                .unwrap();
                let output = Command::new(
                    std::env::var_os("BLUEICE_MAPPED_INFERENCE_COMPILER")
                        .unwrap_or_else(|| env!("CARGO_BIN_EXE_bluetsc").into()),
                )
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
    assert_eq!(ids.len(), 404);
    assert!(
        failures.is_empty(),
        "{agreed}/404 complete agreements; {} mismatches; first twelve:\n{}",
        failures.len(),
        failures
            .iter()
            .take(12)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}
