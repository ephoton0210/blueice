// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! K.8.1 native inferred-declaration decisions and actual emitted-program observations.

use serde_json::Value;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::{env, fs};

fn corpus() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/declaration_inference")
}

fn cases() -> Vec<Value> {
    let reference: Value = serde_json::from_str(include_str!(
        "fixtures/declaration_inference/reference.json"
    ))
    .unwrap();
    assert_eq!(reference["typescript"], "5.9.3");
    reference["cases"].as_array().unwrap().clone()
}

fn accepted(case: &Value) -> bool {
    case["diagnostics"].as_array().unwrap().is_empty()
}

struct Temporary(PathBuf);

impl Temporary {
    fn new(name: &str) -> Self {
        let root = fs::canonicalize(env::temp_dir()).unwrap().join(format!(
            "bluets-dts-inference-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        Self(root)
    }

    fn copy(&self, case: &Value) -> PathBuf {
        let id = case["id"].as_str().unwrap();
        let directory = self.0.join(id);
        fs::create_dir_all(&directory).unwrap();
        for name in case["sources"]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .chain(std::iter::once("tsconfig.json"))
        {
            fs::copy(corpus().join(id).join(name), directory.join(name)).unwrap();
        }
        directory
    }
}

impl Drop for Temporary {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn declaration_inference_matrix_retains_all_cartesian_configurations() {
    let cases = cases();
    assert_eq!(cases.len(), 68);
    assert_eq!(cases.iter().filter(|case| accepted(case)).count(), 65);
    let recorded: BTreeSet<_> = cases
        .iter()
        .map(|case| case["id"].as_str().unwrap().to_string())
        .collect();
    let discovered: BTreeSet<_> = fs::read_dir(corpus())
        .unwrap()
        .map(|entry| entry.unwrap())
        .filter(|entry| entry.file_type().unwrap().is_dir())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(recorded, discovered);
    assert_eq!(recorded.len(), 68);
    let tuples: BTreeSet<_> = cases
        .iter()
        .map(|case| {
            (
                case["form"].as_str().unwrap(),
                case["options"]["target"].as_str().unwrap(),
                case["options"]["module"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(tuples.len(), 68);
    let matrix: String = cases
        .iter()
        .map(|case| {
            format!(
                "{}/main.ts\t{}\n",
                case["id"].as_str().unwrap(),
                if accepted(case) { "accept" } else { "reject" }
            )
        })
        .collect();
    assert_eq!(
        fs::read_to_string(corpus().join("declaration-inference-checker-matrix.tsv"))
            .unwrap()
            .replace("\r\n", "\n"),
        matrix
    );
    for case in cases {
        let id = case["id"].as_str().unwrap();
        let config: Value = serde_json::from_str(
            &fs::read_to_string(corpus().join(id).join("tsconfig.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(config["compilerOptions"], case["options"]);
        let files: BTreeSet<_> = config["files"]
            .as_array()
            .unwrap()
            .iter()
            .map(|file| file.as_str().unwrap())
            .collect();
        let sources: BTreeSet<_> = case["sources"]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(files, sources);
        if accepted(&case) {
            assert_eq!(case["files"], serde_json::json!(["main.d.ts", "main.js"]));
        } else {
            assert_eq!(case["emitSkipped"], true);
            assert_eq!(case["files"], serde_json::json!([]));
        }
    }
}

#[test]
fn declaration_verdicts_and_primary_origins_match_native_typescript() {
    let root = Temporary::new("decisions");
    let mut failures = Vec::new();
    for case in cases() {
        let directory = root.copy(&case);
        let output = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
            .arg("--project")
            .arg(directory.join("tsconfig.json"))
            .args(["--noEmit", "--diagnostics-json"])
            .output()
            .unwrap();
        let id = case["id"].as_str().unwrap();
        let diagnostics = String::from_utf8_lossy(&output.stderr)
            .lines()
            .map(serde_json::from_str::<Value>)
            .collect::<Result<Vec<_>, _>>();
        let Ok(diagnostics) = diagnostics else {
            failures.push(format!("{id}: unstructured diagnostics"));
            continue;
        };
        if output.status.success() != accepted(&case) {
            failures.push(format!("{id}: verdict differs: {diagnostics:?}"));
            continue;
        }
        if accepted(&case) {
            assert!(diagnostics.is_empty());
            continue;
        }
        let expected = &case["diagnostics"][0];
        let Some(actual) = diagnostics.first().map(|d| &d["typescript"]) else {
            failures.push(format!("{id}: missing primary diagnostic"));
            continue;
        };
        let source = fs::read_to_string(directory.join("main.ts")).unwrap();
        let prefix = &source[..expected["start"].as_u64().unwrap() as usize];
        let line = prefix.bytes().filter(|byte| *byte == b'\n').count() + 1;
        let column = prefix.rsplit('\n').next().unwrap().encode_utf16().count() + 1;
        if actual["code"] != expected["code"]
            || actual["message"] != expected["message"]
            || actual["position"]["line"] != line
            || actual["position"]["column"] != column
            || actual["position"]["length"] != expected["length"]
            || actual["span"]["module"]
                .as_str()
                .is_none_or(|m| m.rsplit('/').next() != Some("main.ts"))
        {
            failures.push(format!("{id}: primary diagnostic differs: {actual}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3, Acorn 8.15.0 and Node"]
fn emitted_declarations_preserve_native_execution_declarations_and_target_syntax() {
    let root = Temporary::new("emit");
    let mut failures = Vec::new();
    for case in cases().into_iter().filter(accepted) {
        let directory = root.copy(&case);
        let output = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
            .arg("--project")
            .arg(directory.join("tsconfig.json"))
            .args(["--diagnostics-json"])
            .output()
            .unwrap();
        let id = case["id"].as_str().unwrap();
        if !output.status.success() {
            failures.push(format!(
                "{id}: build refused: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
            continue;
        }
        let declaration = fs::read_to_string(directory.join("out/main.d.ts")).unwrap();
        if declaration != case["declaration"].as_str().unwrap() {
            failures.push(format!("{id}: exact declaration differs"));
        }
        let observed = Command::new("node")
            .arg(corpus().join("observe.cjs"))
            .arg(directory.join("out/main.js"))
            .arg(case["options"]["target"].as_str().unwrap())
            .arg(case["options"]["module"].as_str().unwrap())
            .arg(id)
            .output()
            .unwrap();
        if !observed.status.success() {
            failures.push(format!(
                "{id}: observation failed: {}",
                String::from_utf8_lossy(&observed.stderr)
            ));
            continue;
        }
        let actual: Value = serde_json::from_slice(&observed.stdout).unwrap();
        let runtime_category = case["runtimeError"]
            .as_str()
            .map(|s| s.split(':').next().unwrap());
        if actual["syntaxAccepted"] != case["syntaxError"].is_null()
            || actual["typescriptParseErrors"] != case["typescriptParseErrors"]
            || actual["proposalFeatures"] != case["proposalFeatures"]
            || actual["observation"] != case["observation"]
            || actual["runtimeErrorCategory"] != serde_json::json!(runtime_category)
            || actual["proposalExecution"] != case["proposalExecution"]
        {
            failures.push(format!(
                "{id}: emitted syntax/runtime observations differ: {actual}"
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3, Acorn 8.15.0 and Node"]
fn recorded_declaration_inference_match_the_live_native_compiler() {
    let recorder = Path::new(env!("CARGO_MANIFEST_DIR")).join(
        "../../development/browser_core/phase-18-bluets/tools/record_declaration_inference.cjs",
    );
    let output = Command::new("node").arg(recorder).output().unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn checked_namespace_variables_retain_their_inferred_public_types() {
    let compiled = blueice_bluets::compile(
        "memory:///main.ts",
        &blueice_bluets::MapLoader::from([blueice_bluets::ModuleSource::new(
            "memory:///main.ts",
            "export namespace N { export const answer = 42; }",
        )]),
        blueice_bluets::CompilerOptions::default(),
    );
    assert!(!compiled.has_errors(), "{:?}", compiled.diagnostics);
    let symbols = &compiled.checked.as_ref().unwrap().modules["memory:///main.ts"].symbols;
    let answer = symbols
        .iter()
        .find(|symbol| symbol.name == "N.answer")
        .unwrap_or_else(|| panic!("missing qualified variable: {symbols:?}"));
    assert_eq!(
        answer.value_type,
        Some(blueice_bluets::Type::Literal("42".into())),
        "{symbols:?}"
    );
}
