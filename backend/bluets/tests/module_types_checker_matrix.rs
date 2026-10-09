// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! K.6.2 type-only, import attributes and UMD namespace evidence through the public project CLI.

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
        "fixtures/typescript_oracle/module-types-reference.json"
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
    for file in fs::read_dir(source).unwrap() {
        let file = file.unwrap();
        if file
            .path()
            .extension()
            .is_some_and(|suffix| suffix == "ts" || suffix == "json")
        {
            fs::copy(file.path(), directory.join(file.file_name())).unwrap();
        }
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
        json!({"compilerOptions":options,"files":[Path::new(case["entry"].as_str().unwrap()).file_name().unwrap().to_str().unwrap()]}).to_string(),
    )
    .unwrap();
    config
}

#[test]
fn matrix_covers_every_module_types_fixture() {
    let cases = cases();
    assert_eq!(cases.len(), 86);
    assert_eq!(
        cases.iter().filter(|case| case["runtime"] == true).count(),
        54
    );
    assert_eq!(
        cases
            .iter()
            .filter(|case| case["declaration"] == true)
            .count(),
        60
    );
    let recorded = cases
        .iter()
        .map(|case| case["entry"].as_str().unwrap().to_string())
        .collect::<BTreeSet<_>>();
    let discovered = fs::read_dir(fixtures())
        .unwrap()
        .map(|entry| entry.unwrap())
        .filter(|entry| {
            entry.path().is_dir() && entry.file_name().to_string_lossy().starts_with("modtypes-")
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
        include_str!("fixtures/typescript_oracle/module-types-checker-matrix.tsv")
            .replace("\r\n", "\n")
    );
}

#[test]
fn module_types_match_pinned_verdicts_and_primary_diagnostics() {
    let root = env::temp_dir().join(format!("bluets-modtypes-check-{}", std::process::id()));
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
fn recorded_module_types_match_pinned_typescript() {
    let _ = pinned_tsc();
    let recorder = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../development/browser_core/phase-18-bluets/tools/record_module_types.cjs");
    let output = Command::new("node").arg(recorder).output().unwrap();
    assert!(output.status.success(), "{}", report(&output));
}

fn declarations(directory: &Path) -> BTreeMap<String, String> {
    fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.to_string_lossy().ends_with(".d.ts"))
        .map(|path| {
            (
                path.file_name().unwrap().to_string_lossy().into_owned(),
                fs::read_to_string(path).unwrap().replace("\r\n", "\n"),
            )
        })
        .collect()
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3 and Node"]
fn module_types_preserve_execution_and_declarations() {
    let tsc = pinned_tsc();
    // Configured output directories must use the same canonical root as the CLI.
    let root = fs::canonicalize(env::temp_dir())
        .unwrap()
        .join(format!("bluets-modtypes-runtime-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("package.json"), "{\"type\":\"module\"}").unwrap();
    let mut failures = Vec::new();
    for (index, case) in cases()
        .into_iter()
        .filter(|case| case["declaration"] == true)
        .enumerate()
    {
        let directory = root.join(index.to_string());
        let config = prepare(&case, &directory);
        let entry = case["entry"].as_str().unwrap();
        let blue = directory.join("blue");
        let reference = directory.join("reference");
        let mut project: Value =
            serde_json::from_str(&fs::read_to_string(&config).unwrap()).unwrap();
        project["compilerOptions"]["declaration"] = Value::Bool(true);
        project["compilerOptions"]["outDir"] = json!(blue);
        fs::write(&config, project.to_string()).unwrap();
        let built = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
            .arg("--project")
            .arg(&config)
            .output()
            .unwrap();
        let oracle = Command::new(&tsc)
            .arg("--project")
            .arg(config)
            .arg("--declaration")
            .arg("--outDir")
            .arg(&reference)
            .args([
                "--allowImportingTsExtensions",
                "--rewriteRelativeImportExtensions",
            ])
            .output()
            .unwrap();
        assert!(oracle.status.success(), "{entry}: {}", report(&oracle));
        let run = |output: &Path| {
            Command::new("node")
                .arg(output.join("main.js"))
                .output()
                .unwrap()
        };
        let expected = (case["runtime"] == true).then(|| {
            let output = run(&reference);
            assert!(output.status.success(), "{entry}: {}", report(&output));
            output
        });
        if !built.status.success() {
            failures.push(format!("{entry}: {}", report(&built)));
            continue;
        }
        if let Some(expected) = expected {
            let actual = run(&blue);
            if !actual.status.success() || actual.stdout != expected.stdout {
                failures.push(format!("{entry}: execution differs: {}", report(&actual)));
            }
        }
        let actual = declarations(&blue);
        let expected = declarations(&reference);
        if actual != expected {
            failures.push(format!(
                "{entry}: declarations differ:\nBlue: {actual:#?}\nTypeScript: {expected:#?}"
            ));
        }
    }
    fs::remove_dir_all(root).unwrap();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
