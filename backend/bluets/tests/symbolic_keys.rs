// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! P0-E exercises symbolic keys through public CLI projects and API limits.

use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

const INPUT: &str = include_str!("fixtures/symbolic_keys/cases.json");
const REFERENCE: &str = include_str!("fixtures/symbolic_keys/reference.json");
const BOUNDARIES: &str = include_str!("fixtures/symbolic_keys/boundary-controls.json");
const FROZEN: &str = include_str!("fixtures/symbolic_keys/FROZEN.json");
static NEXT: AtomicU64 = AtomicU64::new(0);

#[test]
fn composed_key_algebra_retains_native_parentheses_order_and_relations() {
    let input: Value =
        serde_json::from_str(include_str!("fixtures/symbolic_keys/algebra-controls.json")).unwrap();
    let frozen: Value = serde_json::from_str(FROZEN).unwrap();
    let digest = ring::digest::digest(
        &ring::digest::SHA256,
        include_bytes!("fixtures/symbolic_keys/algebra-controls.json"),
    );
    let digest = digest
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    assert_eq!(
        digest,
        frozen["supplementarySha256"]["algebra-controls.json"]
            .as_str()
            .unwrap()
    );
    let rows = input["cases"].as_array().unwrap();
    assert_eq!(rows.len(), 16);
    let mut failures = Vec::new();
    for row in rows {
        let id = row["id"].as_str().unwrap();
        let project = Project::new(&format!("algebra/{id}"));
        fs::write(
            project.root.join("main.ts"),
            row["source"].as_str().unwrap(),
        )
        .unwrap();
        fs::write(
            project.root.join("tsconfig.json"),
            serde_json::to_vec_pretty(
                &json!({"compilerOptions":input["options"],"files":["main.ts"]}),
            )
            .unwrap(),
        )
        .unwrap();
        let built = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
            .current_dir(&project.root)
            .args(["build", "--config", "tsconfig.json", "--diagnostics-json"])
            .output()
            .unwrap();
        let diagnostics = String::from_utf8_lossy(&built.stderr)
            .lines()
            .map(|line| diagnostic(&serde_json::from_str::<Value>(line).unwrap()))
            .collect::<Vec<_>>();
        let mut artifacts = BTreeMap::new();
        files(
            &project.root.join("dist"),
            &project.root.join("dist"),
            &mut artifacts,
        );
        let declarations = artifacts
            .iter()
            .filter(|(n, _)| n.ends_with(".d.ts"))
            .map(|(n, t)| (n.clone(), t.clone()))
            .collect::<BTreeMap<_, _>>();
        let actual = json!({"accepts":built.status.success(),"diagnostics":diagnostics,"declarations":declarations});
        let published = row["expected"]["accepts"] == false && !artifacts.is_empty();
        if project.retain {
            fs::write(project.root.join("build.stdout"), &built.stdout).unwrap();
            fs::write(project.root.join("build.stderr"), &built.stderr).unwrap();
            fs::write(project.root.join("observation.json"),serde_json::to_vec_pretty(&json!({"id":id,"observed":actual,"agrees":actual==row["expected"],"publishedOnRejectedNativeInput":published})).unwrap()).unwrap();
        }
        if actual != row["expected"] || published {
            failures.push(format!(
                "{id}: observed={actual}; Native={}",
                row["expected"]
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} algebra differences:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

struct Project {
    root: PathBuf,
    retain: bool,
}

impl Project {
    fn new(id: &str) -> Self {
        let (root, retain) = match std::env::var_os("BLUEICE_SYMBOLIC_KEYS_OBSERVATIONS") {
            Some(parent) => (PathBuf::from(parent).join(id), true),
            None => (
                std::env::temp_dir().join(format!(
                    "bluets-symbolic-keys-{}-{}",
                    std::process::id(),
                    NEXT.fetch_add(1, Ordering::Relaxed)
                )),
                false,
            ),
        };
        fs::create_dir_all(root.parent().unwrap()).unwrap();
        fs::create_dir(&root).expect("every observation needs a fresh project");
        Self {
            root: fs::canonicalize(root).unwrap(),
            retain,
        }
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
        "relatedInformation": value["relatedInformation"].as_array().into_iter().flatten()
            .map(diagnostic).collect::<Vec<_>>()
    })
}

fn files(root: &Path, directory: &Path, output: &mut BTreeMap<String, String>) {
    if !directory.exists() {
        return;
    }
    for entry in fs::read_dir(directory).unwrap() {
        let file = entry.unwrap().path();
        if file.is_dir() {
            files(root, &file, output);
        } else {
            let name = file
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            output.insert(name, fs::read_to_string(file).unwrap());
        }
    }
}

#[test]
fn frozen_contract_retains_the_full_matrix_and_native_products() {
    let input: Value = serde_json::from_str(INPUT).unwrap();
    let reference: Value = serde_json::from_str(REFERENCE).unwrap();
    let frozen: Value = serde_json::from_str(FROZEN).unwrap();
    for (name, bytes) in [
        ("cases.json", INPUT),
        ("reference.json", REFERENCE),
        ("boundary-controls.json", BOUNDARIES),
    ] {
        let digest = ring::digest::digest(&ring::digest::SHA256, bytes.as_bytes());
        let digest = digest
            .as_ref()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        assert_eq!(digest, frozen["sha256"][name].as_str().unwrap(), "{name}");
    }
    let forms = input["cases"].as_array().unwrap();
    assert_eq!(forms.len(), 64);
    assert_eq!(
        forms
            .iter()
            .filter(|row| row["origin"] == "unchanged-discovery-32")
            .count(),
        32
    );
    assert_eq!(input["profiles"].as_array().unwrap().len(), 4);
    let rows = reference["cases"].as_array().unwrap();
    assert_eq!(rows.len(), 256);
    assert_eq!(
        rows.iter()
            .map(|row| row["id"].as_str().unwrap())
            .collect::<BTreeSet<_>>()
            .len(),
        256
    );
    assert_eq!(reference["accepted"], 172);
    assert_eq!(reference["rejected"], 84);
    assert_eq!(reference["declarationFiles"], 176);
    assert_eq!(reference["consumers"], 344);
    assert_eq!(reference["syntax"], 176);
    assert_eq!(reference["runtime"], 24);
    assert_eq!(reference["authoringErrors"], json!([]));
    assert_eq!(reference["conclusion"], "independently-calibrated");
}

#[test]
fn public_projects_match_every_native_verdict_diagnostic_and_declaration_byte() {
    let input: Value = serde_json::from_str(INPUT).unwrap();
    let reference: Value = serde_json::from_str(REFERENCE).unwrap();
    let expected = reference["cases"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| (row["id"].as_str().unwrap(), row))
        .collect::<BTreeMap<_, _>>();
    let mut failures = Vec::new();
    let mut count = 0;
    for profile in input["profiles"].as_array().unwrap() {
        for entry in input["cases"].as_array().unwrap() {
            let id = format!(
                "{}/{}",
                profile["id"].as_str().unwrap(),
                entry["id"].as_str().unwrap()
            );
            let project = Project::new(&id);
            fs::write(
                project.root.join("main.ts"),
                entry["source"].as_str().unwrap(),
            )
            .unwrap();
            let mut roots = vec!["main.ts"];
            for (name, text) in entry["files"].as_object().into_iter().flatten() {
                let path = project.root.join(name);
                fs::create_dir_all(path.parent().unwrap()).unwrap();
                fs::write(path, text.as_str().unwrap()).unwrap();
                roots.push(name);
            }
            let mut options = input["options"].as_object().unwrap().clone();
            options.extend(profile["compilerOptions"].as_object().unwrap().clone());
            options.insert("outDir".into(), json!("dist"));
            fs::write(
                project.root.join("tsconfig.json"),
                serde_json::to_vec_pretty(&json!({
                    "compilerOptions": options, "files": roots
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
            let mut artifacts = BTreeMap::new();
            files(
                &project.root.join("dist"),
                &project.root.join("dist"),
                &mut artifacts,
            );
            let declarations = artifacts
                .iter()
                .filter(|(name, _)| name.ends_with(".d.ts"))
                .map(|(name, text)| (name.clone(), text.clone()))
                .collect::<BTreeMap<_, _>>();
            let observed = json!({"accepts":output.status.success(),"diagnostics":diagnostics,"declarations":declarations});
            let wanted = expected[id.as_str()];
            let wanted = json!({"accepts":wanted["accepts"],"diagnostics":wanted["diagnostics"],"declarations":wanted["declarations"]});
            let rejected_output = wanted["accepts"] == false && !artifacts.is_empty();
            if project.retain {
                fs::write(project.root.join("build.stdout"), &output.stdout).unwrap();
                fs::write(project.root.join("build.stderr"), &output.stderr).unwrap();
                fs::write(
                    project.root.join("observation.json"),
                    serde_json::to_vec_pretty(&json!({
                        "id":id,"accepts":observed["accepts"],"diagnostics":observed["diagnostics"],
                        "declarations":observed["declarations"],"agrees":observed==wanted,
                        "publishedOnRejectedNativeInput":rejected_output
                    }))
                    .unwrap(),
                )
                .unwrap();
            }
            if observed != wanted || rejected_output {
                failures.push(format!(
                    "{id}: observed={observed}; Native={wanted}; rejected-output={rejected_output}"
                ));
            }
            count += 1;
        }
    }
    assert_eq!(count, 256);
    assert!(
        failures.is_empty(),
        "{} complete differences:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn owner_runtime_and_configured_limits_have_no_native_comparison_credit() {
    use blueice_bluets::EcmaTarget;
    use blueice_bluets::{
        compile, CheckingOptions, CompilerOptions, DiagnosticCode, MapLoader, ModuleSource,
    };
    let controls: Value = serde_json::from_str(BOUNDARIES).unwrap();
    let rows = controls["controls"].as_array().unwrap();
    assert_eq!(rows.len(), 12);
    for row in rows {
        let mut options = CompilerOptions {
            checking: Some(CheckingOptions::default()),
            libraries: Some(vec![EcmaTarget::Es5]),
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
                result.has_errors() && result.output.is_none(),
                "{} acquired runtime authority",
                row["id"]
            );
            continue;
        }
        let normal = compile("memory:///main.ts", &loader, options.clone());
        assert!(
            normal.diagnostics.is_empty() && normal.output.is_some(),
            "{}: {:?}",
            row["id"],
            normal.diagnostics
        );
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
                .any(|d| d.code == DiagnosticCode::ResourceLimit && d.typescript.is_none()),
            "{}: {:?}",
            row["id"],
            result.diagnostics
        );
    }
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3, Acorn 8.15.0 and Node"]
fn native_producers_consumers_syntax_and_execution_are_reproducible() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for (script, arguments) in [
        ("record_symbolic_keys.cjs", vec!["--verify"]),
        ("verify_symbolic_keys.cjs", vec![]),
    ] {
        let result = Command::new("node")
            .arg(
                repo.join("development/browser_core/phase-18-bluets/tools")
                    .join(script),
            )
            .args(arguments)
            .env(
                "BLUEICE_SYMBOLIC_KEYS_COMPILER",
                env!("CARGO_BIN_EXE_bluetsc"),
            )
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{script}: {}{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
    }
}
