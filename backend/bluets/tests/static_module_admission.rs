// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Module/helper semantics must not depend on executable output generation.

use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

mod program_roots_cli_identity;

fn diagnostic(row: &Value, root: &Path) -> Value {
    assert!(row["code"].is_number(), "missing Native counterpart: {row}");
    let file = row["span"]["module"]
        .as_str()
        .filter(|name| !name.is_empty())
        .map(|name| program_roots_cli_identity::relative_file(name, root));
    json!({"code":row["code"],"message":row["message"],"file":file,
        "position":row["position"],"relatedInformation":row["relatedInformation"]
            .as_array().into_iter().flatten().map(|row| diagnostic(row,root)).collect::<Vec<_>>()})
}

struct Project(PathBuf);
impl Drop for Project {
    fn drop(&mut self) {
        if std::thread::panicking() {
            eprintln!("Retained static module admission: {}", self.0.display());
        } else {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }
}

#[test]
fn no_emit_helper_semantics_match_all_168_native_boundaries_without_publication() {
    let reference: Value = serde_json::from_str(include_str!(
        "fixtures/static_admission/helper-boundaries.json"
    ))
    .unwrap();
    assert_eq!(reference["cases"].as_array().unwrap().len(), 168);
    run_reference(&reference);
}

#[test]
fn private_fields_match_all_36_native_explicit_and_default_policies() {
    let reference: Value = serde_json::from_str(include_str!(
        "fixtures/static_admission/private-policies.json"
    ))
    .unwrap();
    assert_eq!(reference["cases"].as_array().unwrap().len(), 36);
    run_reference(&reference);
}

fn run_reference(reference: &Value) {
    let source = include_str!("fixtures/ge2_module_helpers/cases.json");
    assert_eq!(
        blueice_bluets::remote_declarations::sha256_hex(source.as_bytes()),
        reference["sourceFixtureSha256"].as_str().unwrap()
    );
    let input: Value = serde_json::from_str(source).unwrap();
    let root = std::env::temp_dir().join(format!(
        "bluets-static-admission-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&root).unwrap();
    let project = Project(root.canonicalize().unwrap());
    let mut failures = Vec::new();
    for row in reference["cases"].as_array().unwrap() {
        let id = row["id"].as_str().unwrap();
        let parts = id.split('/').collect::<Vec<_>>();
        let base = input["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|case| {
                case["family"] == "helper-product"
                    && case["form"] == parts[0]
                    && case["options"]["module"] == "CommonJS"
                    && case["options"]["target"] == "ES5"
                    && case["options"]["importHelpers"] == false
                    && case["options"]["noEmitHelpers"] == false
            })
            .unwrap();
        let directory = project.0.join(id);
        fs::create_dir_all(&directory).unwrap();
        let mut files = base["files"].as_object().unwrap().clone();
        if parts[2] == "complete" {
            files.extend(input["sharedFiles"].as_object().unwrap().clone());
        }
        for (name, text) in files {
            let path = directory.join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text.as_str().unwrap()).unwrap();
        }
        let mut options = base["options"].clone();
        options["target"] = json!(parts[1]);
        options["noEmit"] = json!(true);
        options["importHelpers"] = json!(true);
        options["noEmitHelpers"] = row
            .get("noEmitHelpers")
            .cloned()
            .unwrap_or_else(|| json!(parts[2] == "absent"));
        if let Some(policy) = row["fieldPolicy"].as_str() {
            if policy == "default" {
                options
                    .as_object_mut()
                    .unwrap()
                    .remove("useDefineForClassFields");
            } else {
                options["useDefineForClassFields"] = json!(policy == "define");
            }
        }
        fs::write(
            directory.join("tsconfig.json"),
            json!({"compilerOptions":options,"files":[base["entry"]]}).to_string(),
        )
        .unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
            .args(["--project", "tsconfig.json", "--diagnostics-json"])
            .current_dir(&directory)
            .output()
            .unwrap();
        let diagnostics = String::from_utf8(output.stderr)
            .unwrap()
            .lines()
            .map(|line| {
                let raw: Value = serde_json::from_str(line).unwrap();
                diagnostic(&raw["typescript"], &directory)
            })
            .collect::<Vec<_>>();
        let observed = json!({"accepts":output.status.success(),"diagnostics":diagnostics});
        if observed != row["expected"] {
            failures.push(format!(
                "{id}: expected {}; observed {observed}",
                row["expected"]
            ));
        }
        assert!(!directory.join("out").exists(), "{id}: static output");
        assert!(!directory.join("tsconfig.bluetsc.tsbuildinfo").exists());
    }
    assert!(
        failures.is_empty(),
        "{} static boundaries differ:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn a_static_missing_helper_has_no_output_or_success_metadata() {
    use blueice_bluets::{compile, CompilerOptions, EcmaTarget, MapLoader, ModuleSource};
    let id = "program:///main.ts";
    let result = compile(
        id,
        &MapLoader::from([ModuleSource::new(
            id,
            "class Base {} export class Derived extends Base {}",
        )]),
        CompilerOptions {
            target: EcmaTarget::Es5,
            no_emit: true,
            import_helpers: true,
            ..CompilerOptions::default()
        },
    );
    assert!(result.has_errors());
    assert!(result
        .diagnostics
        .iter()
        .any(|row| row.typescript.as_ref().is_some_and(|row| row.code == 2354)));
    assert!(result.output.is_none());
    assert!(result.debug_info.is_none());
}

#[test]
fn a_script_has_no_imported_helper_requirement_or_output_authority() {
    use blueice_bluets::{compile, CompilerOptions, EcmaTarget, MapLoader, ModuleSource};
    let id = "program:///main.ts";
    let result = compile(
        id,
        &MapLoader::from([ModuleSource::new(
            id,
            "class Base {} class Derived extends Base {}",
        )]),
        CompilerOptions {
            target: EcmaTarget::Es5,
            no_emit: true,
            import_helpers: true,
            ..CompilerOptions::default()
        },
    );
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    assert!(result.output.is_none());
    assert!(result.debug_info.is_some());
}

#[test]
fn a_static_system_export_assignment_has_no_success_metadata() {
    use blueice_bluets::{compile, CompilerOptions, MapLoader, ModuleKind, ModuleSource};
    let id = "program:///main.ts";
    let result = compile(
        id,
        &MapLoader::from([ModuleSource::new(
            id,
            "function read(): number { return 1; } export = read;",
        )]),
        CompilerOptions {
            module_kind: ModuleKind::System,
            no_emit: true,
            ..CompilerOptions::default()
        },
    );
    assert!(result.has_errors());
    assert!(result
        .diagnostics
        .iter()
        .any(|row| row.typescript.as_ref().is_some_and(|row| row.code == 1218)));
    assert!(result.output.is_none());
    assert!(result.debug_info.is_none());
}
