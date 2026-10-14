// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! K.6.3 static module, augmentation and reference evidence through the public project CLI.

use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::{env, fs};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/typescript_oracle")
}

fn cases() -> Vec<Value> {
    let reference: Value = serde_json::from_str(include_str!(
        "fixtures/typescript_oracle/ambient-reference.json"
    ))
    .unwrap();
    assert_eq!(reference["version"], "5.9.3");
    reference["cases"].as_array().unwrap().clone()
}

fn report(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn prepare(case: &Value, directory: &Path) -> PathBuf {
    fs::create_dir_all(directory).unwrap();
    let source = fixtures()
        .join(case["entry"].as_str().unwrap())
        .parent()
        .unwrap()
        .to_path_buf();
    for file in case["sources"].as_array().unwrap() {
        let relative = Path::new(file.as_str().unwrap());
        let destination = directory.join(relative);
        fs::create_dir_all(destination.parent().unwrap()).unwrap();
        fs::copy(source.join(relative), destination).unwrap();
    }
    let mut options = json!({"target":"ES2022","module":"ES2022","strict":true,
        "allowImportingTsExtensions":true});
    let flags = case["flags"].as_array().unwrap();
    let mut index = 0;
    while index < flags.len() {
        let name = flags[index].as_str().unwrap().strip_prefix("--").unwrap();
        index += 1;
        let value = flags.get(index).and_then(Value::as_str);
        options[name] = if value.is_none_or(|value| value.starts_with("--")) {
            Value::Bool(true)
        } else {
            index += 1;
            match value.unwrap() {
                "true" => Value::Bool(true),
                "false" => Value::Bool(false),
                value => json!(value),
            }
        };
    }
    fs::write(
        directory.join("package.json"),
        json!({"type":
            if options["module"] == "commonjs" { "commonjs" } else { "module" }
        })
        .to_string(),
    )
    .unwrap();
    let config = directory.join("tsconfig.json");
    fs::write(
        &config,
        json!({"compilerOptions":options,"files":case["files"]}).to_string(),
    )
    .unwrap();
    config
}

#[test]
fn matrix_covers_every_ambient_fixture() {
    let cases = cases();
    assert_eq!(cases.len(), 80);
    assert_eq!(
        cases.iter().filter(|case| case["runtime"] == true).count(),
        44
    );
    assert_eq!(
        cases
            .iter()
            .filter(|case| case["declaration"] == true)
            .count(),
        44
    );
    let recorded = cases
        .iter()
        .map(|case| case["entry"].as_str().unwrap().to_string())
        .collect::<BTreeSet<_>>();
    let discovered = fs::read_dir(fixtures())
        .unwrap()
        .map(|entry| entry.unwrap())
        .filter(|entry| {
            entry.path().is_dir()
                && entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("modambient-")
        })
        .map(|entry| {
            format!(
                "{}/{}",
                entry.file_name().to_string_lossy(),
                if entry.path().join("main.ts").is_file() {
                    "main.ts"
                } else {
                    "main.d.ts"
                }
            )
        })
        .collect();
    assert_eq!(recorded.len(), cases.len());
    assert_eq!(recorded, discovered);
    let matrix = cases
        .iter()
        .map(|case| {
            format!(
                "{}\t{}\n",
                case["entry"].as_str().unwrap(),
                if case["accepts"] == true {
                    "accept"
                } else {
                    "reject"
                }
            )
        })
        .collect::<String>();
    assert_eq!(
        matrix,
        include_str!("fixtures/typescript_oracle/ambient-checker-matrix.tsv").replace("\r\n", "\n")
    );
}

#[test]
fn ambient_match_pinned_verdicts_and_primary_diagnostics() {
    let root = env::temp_dir().join(format!("bluets-modambient-check-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let mut failures = Vec::new();
    for (index, case) in cases().into_iter().enumerate() {
        let config = prepare(&case, &root.join(index.to_string()));
        let output = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
            .arg("--project")
            .arg(config)
            .args(["--noEmit", "--diagnostics-json"])
            .output()
            .unwrap();
        let entry = case["entry"].as_str().unwrap();
        let accepts = case["accepts"] == true;
        let diagnostics = String::from_utf8_lossy(&output.stderr)
            .lines()
            .map(serde_json::from_str::<Value>)
            .collect::<Result<Vec<_>, _>>();
        let Ok(diagnostics) = diagnostics else {
            failures.push(format!(
                "{entry}: unstructured diagnostics: {}",
                report(&output)
            ));
            continue;
        };
        if output.status.success() != accepts || (accepts && !diagnostics.is_empty()) {
            failures.push(format!(
                "{entry}: expected accept={accepts}: {diagnostics:?}"
            ));
            continue;
        }
        if accepts {
            continue;
        }
        let Some(primary) = diagnostics.first().map(|item| &item["typescript"]) else {
            failures.push(format!("{entry}: missing primary diagnostic"));
            continue;
        };
        let related = primary["relatedInformation"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|item| {
                json!({"code":item["code"],"message":item["message"],
                    "module":item["span"]["module"],"position":item["position"],"related":[]})
            })
            .collect::<Vec<_>>();
        let actual = json!({"code":primary["code"],"message":primary["message"],
            "module":primary["span"]["module"],"position":primary["position"],"related":related});
        if actual != case["first"] {
            failures.push(format!(
                "{entry}: expected {}; received {actual}",
                case["first"]
            ));
        }
    }
    fs::remove_dir_all(root).unwrap();
    assert!(
        failures.is_empty(),
        "{} module type mismatches:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

fn pinned_tsc() -> PathBuf {
    let tsc = env::var_os("BLUEICE_BLUETSC_ORACLE")
        .map(PathBuf::from)
        .expect("set BLUEICE_BLUETSC_ORACLE to TypeScript 5.9.3");
    let version = Command::new(&tsc).arg("--version").output().unwrap();
    assert!(version.status.success(), "{}", report(&version));
    assert_eq!(
        String::from_utf8_lossy(&version.stdout).trim(),
        "Version 5.9.3"
    );
    tsc
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3 and Node"]
fn recorded_ambient_match_pinned_typescript() {
    let _ = pinned_tsc();
    let recorder = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../development/browser_core/phase-18-bluets/tools/record_ambient.cjs");
    let output = Command::new("node").arg(recorder).output().unwrap();
    assert!(output.status.success(), "{}", report(&output));
}

fn declarations(directory: &Path) -> BTreeMap<String, String> {
    fn walk(root: &Path, directory: &Path, result: &mut BTreeMap<String, String>) {
        for entry in fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(root, &path, result);
            } else if path.to_string_lossy().ends_with(".d.ts") {
                result.insert(
                    path.strip_prefix(root)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/"),
                    fs::read_to_string(path).unwrap().replace("\r\n", "\n"),
                );
            }
        }
    }
    let mut result = BTreeMap::new();
    walk(directory, directory, &mut result);
    result
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3 and Node"]
fn ambient_forms_preserve_execution_and_generated_declarations() {
    let tsc = pinned_tsc();
    let root = fs::canonicalize(env::temp_dir())
        .unwrap()
        .join(format!("bluets-ambient-runtime-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let mut references = Vec::new();
    for (index, case) in cases()
        .into_iter()
        .filter(|case| case["accepts"] == true)
        .enumerate()
    {
        let directory = root.join(index.to_string());
        let config = prepare(&case, &directory);
        let reference = directory.join("reference");
        let mut project: Value =
            serde_json::from_str(&fs::read_to_string(&config).unwrap()).unwrap();
        project["compilerOptions"]["declaration"] = Value::Bool(true);
        project["compilerOptions"]["outDir"] = json!(directory.join("blue"));
        fs::write(&config, project.to_string()).unwrap();
        let built = Command::new(&tsc)
            .arg("--project")
            .arg(&config)
            .arg("--outDir")
            .arg(&reference)
            .args(["--rewriteRelativeImportExtensions", "--noEmitOnError"])
            .output()
            .unwrap();
        assert!(
            built.status.success(),
            "{}: {}",
            case["entry"],
            report(&built)
        );
        let node = Command::new("node")
            .arg(reference.join("main.js"))
            .output()
            .unwrap();
        assert!(
            node.status.success(),
            "{}: {}",
            case["entry"],
            report(&node)
        );
        assert_eq!(node.stdout, b"42\n", "{}", case["entry"]);
        let input_declarations = case["sources"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|name| name.as_str())
            .filter(|name| name.ends_with(".d.ts"))
            .map(|name| {
                (
                    name.to_string(),
                    fs::read_to_string(directory.join(name))
                        .unwrap()
                        .replace("\r\n", "\n"),
                )
            })
            .collect::<BTreeMap<_, _>>();
        references.push((
            case,
            config,
            directory.join("blue"),
            node.stdout,
            declarations(&reference),
            input_declarations,
        ));
    }
    let mut failures = Vec::new();
    for (case, config, blue, stdout, expected, inputs) in references {
        let entry = case["entry"].as_str().unwrap();
        let built = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
            .arg("--project")
            .arg(config)
            .output()
            .unwrap();
        if !built.status.success() {
            failures.push(format!("{entry}: {}", report(&built)));
            continue;
        }
        let node = Command::new("node")
            .arg(blue.join("main.js"))
            .output()
            .unwrap();
        if !node.status.success() || node.stdout != stdout {
            failures.push(format!("{entry}: execution differs: {}", report(&node)));
        }
        let mut actual = declarations(&blue);
        // Native owner publication retains declaration inputs separately from generated output.
        for (name, original) in &inputs {
            if !expected.contains_key(name) {
                if let Some(retained) = actual.remove(name) {
                    assert_eq!(
                        retained, *original,
                        "{entry}: retained declaration input {name}"
                    );
                }
            }
        }
        if actual != expected {
            failures.push(format!(
                "{entry}: generated declarations differ: {actual:#?} != {expected:#?}"
            ));
        }
    }
    fs::remove_dir_all(root).unwrap();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
