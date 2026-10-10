// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! K.7.3 public output decisions and actual Node/declaration observations.

use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::{env, fs};

fn corpus() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/output_options")
}

fn cases() -> Vec<Value> {
    let reference: Value =
        serde_json::from_str(include_str!("fixtures/output_options/reference.json")).unwrap();
    assert_eq!(reference["typescript"], "5.9.3");
    reference["cases"].as_array().unwrap().clone()
}

fn accepted(case: &Value) -> bool {
    case["reference"]["diagnostics"]
        .as_array()
        .unwrap()
        .is_empty()
}

fn temporary(name: &str) -> PathBuf {
    let root = fs::canonicalize(env::temp_dir())
        .unwrap()
        .join(format!("bluets-output-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    root
}

fn copy_case(case: &Value, root: &Path) -> PathBuf {
    let id = case["id"].as_str().unwrap();
    let directory = root.join(id);
    fs::create_dir_all(&directory).unwrap();
    for file in ["main.ts", "tsconfig.json"] {
        fs::copy(corpus().join(id).join(file), directory.join(file)).unwrap();
    }
    directory
}

#[test]
fn output_matrix_retains_every_native_configuration() {
    let cases = cases();
    assert_eq!(cases.len(), 78);
    assert_eq!(cases.iter().filter(|case| accepted(case)).count(), 72);
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
    assert_eq!(recorded.len(), 78);
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
        fs::read_to_string(corpus().join("output-options-checker-matrix.tsv"))
            .unwrap()
            .replace("\r\n", "\n"),
        matrix
    );
    for case in cases {
        let directory = corpus().join(case["id"].as_str().unwrap());
        let config: Value =
            serde_json::from_str(&fs::read_to_string(directory.join("tsconfig.json")).unwrap())
                .unwrap();
        for name in ["strict", "sourceMap", "declaration", "noEmitOnError"] {
            assert_eq!(config["compilerOptions"][name], true);
        }
        assert!(directory.join("main.ts").is_file());
    }
}

#[test]
fn output_verdicts_and_removed_option_origins_match_typescript() {
    let root = temporary("decisions");
    let mut failures = Vec::new();
    for case in cases() {
        let directory = copy_case(&case, &root);
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
            failures.push(format!("{id}: decision differs: {diagnostics:?}"));
            continue;
        }
        if accepted(&case) {
            assert!(diagnostics.is_empty());
            continue;
        }
        let expected = &case["reference"]["diagnostics"][0];
        let actual = diagnostics.first().map(|value| &value["typescript"]);
        let Some(actual) = actual else {
            failures.push(format!("{id}: missing primary diagnostic"));
            continue;
        };
        let source = fs::read_to_string(directory.join("tsconfig.json")).unwrap();
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
                .is_none_or(|module| module.rsplit('/').next() != Some("tsconfig.json"))
        {
            failures.push(format!("{id}: primary diagnostic differs: {actual}"));
        }
    }
    fs::remove_dir_all(root).unwrap();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
#[ignore = "requires Node and pinned output-option observations"]
fn emitted_output_options_preserve_runtime_declarations_and_maps() {
    let root = temporary("execution");
    let mut failures = Vec::new();
    for case in cases().into_iter().filter(accepted) {
        let directory = copy_case(&case, &root);
        let out = directory.join("blue");
        let build = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
            .arg("--project")
            .arg(directory.join("tsconfig.json"))
            .arg("--outDir")
            .arg(&out)
            .output()
            .unwrap();
        let id = case["id"].as_str().unwrap();
        if !build.status.success() {
            failures.push(format!(
                "{id}: build failed: {}",
                String::from_utf8_lossy(&build.stderr)
            ));
            continue;
        }
        let config: Value =
            serde_json::from_str(&fs::read_to_string(directory.join("tsconfig.json")).unwrap())
                .unwrap();
        let runtime = Command::new("node")
            .arg(corpus().join("observe.cjs"))
            .arg(&out)
            .arg(config["compilerOptions"]["module"].as_str().unwrap())
            .output()
            .unwrap();
        if !runtime.status.success() {
            failures.push(format!("{id}: emitted execution failed"));
            continue;
        }
        let observation: Value = serde_json::from_slice(&runtime.stdout).unwrap();
        if observation != case["reference"]["observation"] {
            failures.push(format!("{id}: runtime differs: {observation}"));
        }
        let declaration = fs::read_to_string(out.join("main.d.ts")).unwrap();
        if declaration != case["reference"]["declaration"].as_str().unwrap() {
            failures.push(format!("{id}: declaration differs"));
        }
        let javascript = fs::read_to_string(out.join("main.js")).unwrap();
        let expected = &case["reference"]["javascript"];
        let actual = json!({
            "bom": javascript.starts_with('\u{feff}'),
            "ordinaryComment": javascript.contains("retained ordinary comment"),
            "publicComment": javascript.contains("Public answer."),
            "internalComment": javascript.contains("@internal"),
            "literalRetained": javascript.contains("// literal /* retained */"),
            "sourceMappingURL": javascript.lines().find(|line| line.contains("sourceMappingURL")),
        });
        for name in [
            "bom",
            "ordinaryComment",
            "publicComment",
            "internalComment",
            "literalRetained",
            "sourceMappingURL",
        ] {
            if actual[name] != expected[name] {
                failures.push(format!("{id}: {name} differs"));
            }
        }
        let crlf = case["options"]["newLine"] == "CRLF";
        if crlf && javascript.replace("\r\n", "").contains('\n')
            || !crlf && javascript.contains("\r\n")
        {
            failures.push(format!("{id}: newline bytes differ"));
        }
        let map: Value =
            serde_json::from_str(&fs::read_to_string(out.join("main.js.map")).unwrap()).unwrap();
        for name in ["file", "sourceRoot", "sources", "sourcesContent"] {
            if map[name] != case["reference"]["sourceMap"][name] {
                failures.push(format!("{id}: source map {name} differs"));
            }
        }
        assert_eq!(map["version"], 3);
        assert!(!map["mappings"].as_str().unwrap().is_empty());
        let mut files: Vec<_> = fs::read_dir(&out)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        files.sort();
        if serde_json::to_value(files).unwrap() != case["reference"]["files"] {
            failures.push(format!("{id}: output file set differs"));
        }
    }
    fs::remove_dir_all(root).unwrap();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3 and Node"]
fn recorded_output_options_match_live_typescript() {
    let recorder = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../development/browser_core/phase-18-bluets/tools/record_output_options.cjs");
    let output = Command::new("node").arg(recorder).output().unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
