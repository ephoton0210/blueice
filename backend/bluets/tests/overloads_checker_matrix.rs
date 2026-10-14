// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! K.4.4 call, construct and method overload evidence at the public compiler boundary.

use blueice_bluets::{
    compile, CheckingOptions, CompilerOptions, MapLoader, ModuleLoader, ModuleSource,
};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::{env, fs};

struct FixtureLoader(MapLoader);

impl ModuleLoader for FixtureLoader {
    fn load(&self, module_id: &str) -> Result<ModuleSource, String> {
        self.0.load(module_id)
    }

    fn resolve(&self, _from_module: &str, specifier: &str) -> Result<String, String> {
        let sibling = specifier
            .strip_prefix("./")
            .ok_or_else(|| format!("fixture import is not a sibling: {specifier}"))?;
        Ok(sibling
            .strip_suffix(".js")
            .map_or_else(|| sibling.to_string(), |stem| format!("{stem}.ts")))
    }
}

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/typescript_oracle")
}

fn cases() -> Vec<Value> {
    let reference: Value = serde_json::from_str(include_str!(
        "fixtures/typescript_oracle/overloads-reference.json"
    ))
    .unwrap();
    assert_eq!(reference["version"], "5.9.3");
    reference["cases"].as_array().unwrap().clone()
}

fn position(position: &Option<blueice_bluets::TypeScriptPosition>) -> Value {
    position.as_ref().map_or(
        Value::Null,
        |position| json!({"line":position.line,"column":position.column,"length":position.length}),
    )
}

#[test]
fn matrix_covers_every_overloads_fixture() {
    let cases = cases();
    assert_eq!(cases.len(), 74);
    assert_eq!(
        cases.iter().filter(|case| case["runtime"] == true).count(),
        6
    );
    let recorded = cases
        .iter()
        .map(|case| case["entry"].as_str().unwrap().to_string())
        .collect::<BTreeSet<_>>();
    let discovered = fs::read_dir(fixtures())
        .unwrap()
        .map(|entry| entry.unwrap())
        .filter(|entry| {
            entry.path().is_dir() && entry.file_name().to_string_lossy().starts_with("over-")
        })
        .map(|entry| format!("{}/main.ts", entry.file_name().to_string_lossy()))
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
        include_str!("fixtures/typescript_oracle/overloads-checker-matrix.tsv")
            .replace("\r\n", "\n")
    );
}

#[test]
fn overloads_match_pinned_verdicts_and_primary_diagnostics() {
    let mut failures = Vec::new();
    for case in cases() {
        let entry = case["entry"].as_str().unwrap();
        let directory = fixtures().join(entry).parent().unwrap().to_path_buf();
        let sources = fs::read_dir(directory)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().is_some_and(|extension| extension == "ts"))
            .map(|path| {
                ModuleSource::new(
                    path.file_name().unwrap().to_string_lossy().into_owned(),
                    fs::read_to_string(path).unwrap(),
                )
            });
        let result = compile(
            "main.ts",
            &FixtureLoader(MapLoader::from(sources)),
            CompilerOptions {
                checking: Some(CheckingOptions::default()),
                ..CompilerOptions::default()
            },
        );
        let accepts = case["accepts"] == true;
        if result.has_errors() == accepts || result.output.is_some() != accepts {
            failures.push(format!(
                "{entry}: expected accept={accepts}: {:?}",
                result.diagnostics
            ));
            continue;
        }
        if accepts {
            continue;
        }
        let Some(primary) = result
            .diagnostics
            .first()
            .and_then(|item| item.typescript.as_ref())
        else {
            failures.push(format!(
                "{entry}: missing TypeScript primary: {:?}",
                result.diagnostics
            ));
            continue;
        };
        let related = primary
            .related_information
            .iter()
            .map(|item| {
                json!({"code":item.code,"message":item.message,"module":item.span.module,
                "position":position(&item.position),"related":[]})
            })
            .collect::<Vec<_>>();
        let actual = json!({"code":primary.code,"message":primary.message,
            "module":primary.span.module,"position":position(&primary.position),"related":related});
        if actual != case["first"] {
            failures.push(format!(
                "{entry}: expected {}; received {actual}",
                case["first"]
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} overload mismatches:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

fn report(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
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
fn recorded_overloads_matches_pinned_typescript() {
    let _ = pinned_tsc();
    let recorder = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../development/browser_core/phase-18-bluets/tools/record_overloads.cjs");
    let output = Command::new("node").arg(recorder).output().unwrap();
    assert!(output.status.success(), "{}", report(&output));
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3 and Node"]
fn overloads_preserves_execution_and_declarations() {
    let tsc = pinned_tsc();
    let root = env::temp_dir().join(format!("bluets-overloads-runtime-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("package.json"), "{\"type\":\"module\"}").unwrap();
    let mut failures = Vec::new();
    for (index, case) in cases()
        .into_iter()
        .filter(|case| case["runtime"] == true)
        .enumerate()
    {
        let entry = case["entry"].as_str().unwrap();
        let blue = root.join(index.to_string()).join("blue");
        let reference = root.join(index.to_string()).join("reference");
        let built = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
            .arg("build")
            .arg(fixtures().join(entry))
            .arg("--declaration")
            .arg("--out-dir")
            .arg(&blue)
            .output()
            .unwrap();
        let oracle = Command::new(&tsc)
            .args([
                "--target",
                "ES2022",
                "--module",
                "ES2022",
                "--strict",
                "--pretty",
                "false",
                "--declaration",
                "--allowImportingTsExtensions",
                "--rewriteRelativeImportExtensions",
                "--outDir",
            ])
            .arg(&reference)
            .arg(fixtures().join(entry))
            .output()
            .unwrap();
        assert!(oracle.status.success(), "{entry}: {}", report(&oracle));
        if !built.status.success() {
            failures.push(format!("{entry}: {}", report(&built)));
            continue;
        }
        let run = |directory: &Path| {
            Command::new("node")
                .arg(directory.join("main.js"))
                .output()
                .unwrap()
        };
        let actual = run(&blue);
        let expected = run(&reference);
        assert!(expected.status.success(), "{entry}: {}", report(&expected));
        if !actual.status.success() || actual.stdout != expected.stdout {
            failures.push(format!("{entry}: execution differs: {}", report(&actual)));
        }
        let declarations = |directory: &Path| {
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
                .collect::<std::collections::BTreeMap<_, _>>()
        };
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
