// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Legacy accessor eligibility precedes the paired-decorator semantic check.

use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

mod program_roots_cli_identity;

fn reference() -> Value {
    let source = include_str!("fixtures/ge3_decorator_compositions/cases.json");
    let original = include_str!("fixtures/ge3_decorator_compositions/reference.json");
    let data: Value = serde_json::from_str(include_str!(
        "fixtures/static_admission/accessor-decorators.json"
    ))
    .unwrap();
    assert_eq!(data["cases"].as_array().unwrap().len(), 24);
    assert_eq!(
        data["sourceFixtureSha256"],
        blueice_bluets::remote_declarations::sha256_hex(source.as_bytes())
    );
    assert_eq!(
        data["referenceFixtureSha256"],
        blueice_bluets::remote_declarations::sha256_hex(original.as_bytes())
    );
    data
}

fn diagnostic(row: &Value, root: &Path) -> Value {
    assert!(row["code"].is_number(), "unmapped diagnostic: {row}");
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
            eprintln!("Retained accessor admission: {}", self.0.display());
        } else {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }
}

fn compare(family: &str, expected_cases: usize) {
    let reference = reference();
    let selected = reference["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["family"] == family)
        .collect::<Vec<_>>();
    assert_eq!(selected.len(), expected_cases);
    let root = std::env::temp_dir().join(format!(
        "bluets-accessor-admission-{family}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&root).unwrap();
    let project = Project(root.canonicalize().unwrap());
    let mut failures = Vec::new();
    for (index, row) in selected.into_iter().enumerate() {
        for no_emit in [false, true] {
            let directory = project.0.join(format!("{index}-{no_emit}"));
            fs::create_dir(&directory).unwrap();
            for (name, source) in row["files"].as_object().unwrap() {
                let path = directory.join(name);
                fs::create_dir_all(path.parent().unwrap()).unwrap();
                fs::write(path, source.as_str().unwrap()).unwrap();
            }
            let mut options = row["options"].clone();
            options["noEmit"] = json!(no_emit);
            fs::write(
                directory.join("tsconfig.json"),
                json!({"compilerOptions":options,"files":[row["entry"]]}).to_string(),
            )
            .unwrap();
            let output = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
                .current_dir(&directory)
                .args(["--project", "tsconfig.json", "--diagnostics-json"])
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
            let actual = json!({"accepts":output.status.success(),"diagnostics":diagnostics});
            if actual != row["expected"] {
                failures.push(format!(
                    "{}; noEmit={no_emit}: expected {}; actual {actual}",
                    row["id"], row["expected"]
                ));
            }
            for name in [
                "out",
                "types",
                "tsconfig.bluetsc.tsbuildinfo",
                "tsconfig.tsbuildinfo",
            ] {
                assert!(
                    !directory.join(name).exists(),
                    "{}: unexpected {name}",
                    row["id"]
                );
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} differences:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn all_sixteen_original_private_accessor_cases_keep_complete_native_diagnostics() {
    compare("original-ge3-private", 16);
}

#[test]
fn eight_legal_accessor_pairs_keep_their_native_1207_error() {
    compare("legal-pair-control", 8);
}

#[test]
fn static_private_accessor_refusal_has_no_output_or_success_metadata() {
    use blueice_bluets::{compile, CompilerOptions, EcmaTarget, MapLoader, ModuleSource};
    let source = "function dec(...args: any[]): any {}\nexport class C { @dec get #value(): number { return 1; } @dec set #value(n: number) {} }";
    let id = "program:///main.ts";
    let result = compile(
        id,
        &MapLoader::from([ModuleSource::new(id, source)]),
        CompilerOptions {
            target: EcmaTarget::Es2022,
            no_emit: true,
            experimental_decorators: true,
            ..CompilerOptions::default()
        },
    );
    assert!(result.has_errors());
    assert!(result.output.is_none());
    assert!(result.debug_info.is_none());
    let codes = result
        .diagnostics
        .iter()
        .filter_map(|diagnostic| {
            diagnostic
                .typescript
                .as_ref()
                .map(|typescript| typescript.code)
        })
        .collect::<Vec<_>>();
    assert_eq!(codes.iter().filter(|code| **code == 1206).count(), 2);
    assert!(!codes.contains(&1207), "{codes:?}");
}
