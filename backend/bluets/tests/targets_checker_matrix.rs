// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! K.7.1 target evidence from TypeScript 5.9.3 through the public CLI.

use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::{env, fs};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/typescript_oracle")
}

fn reference() -> Value {
    let value: Value = serde_json::from_str(include_str!(
        "fixtures/typescript_oracle/targets-reference.json"
    ))
    .unwrap();
    assert_eq!(value["version"], "5.9.3");
    value
}

fn cases() -> Vec<Value> {
    reference()["cases"].as_array().unwrap().clone()
}

fn module_kind(case: &Value) -> &'static str {
    let flags = case["flags"].as_array().unwrap();
    let flag = flags.windows(2).find(|pair| pair[0] == "--module").unwrap();
    match flag[1].as_str().unwrap() {
        "commonjs" => "commonjs",
        "ES2022" => "es2022",
        other => panic!("unsupported reference module: {other}"),
    }
}

fn report(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn temporary_root(name: &str) -> PathBuf {
    let root = fs::canonicalize(env::temp_dir())
        .unwrap()
        .join(format!("bluets-target-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    root
}

fn prepare(case: &Value, directory: &Path, emit: bool) -> PathBuf {
    fs::create_dir_all(directory).unwrap();
    fs::copy(
        fixtures().join(case["entry"].as_str().unwrap()),
        directory.join("main.ts"),
    )
    .unwrap();
    fs::write(
        directory.join("package.json"),
        json!({"type":if module_kind(case) == "commonjs" { "commonjs" } else { "module" }})
            .to_string(),
    )
    .unwrap();
    let mut options = json!({});
    let flags = case["flags"].as_array().unwrap();
    let mut index = 0;
    while index < flags.len() {
        let name = flags[index].as_str().unwrap().strip_prefix("--").unwrap();
        index += 1;
        let next = flags.get(index).and_then(Value::as_str);
        options[name] = if next.is_none_or(|value| value.starts_with("--")) {
            Value::Bool(true)
        } else {
            index += 1;
            match (name, next.unwrap()) {
                (_, "true") => Value::Bool(true),
                (_, "false") => Value::Bool(false),
                ("lib", value) => json!(value.split(',').collect::<Vec<_>>()),
                (_, value) => json!(value),
            }
        };
    }
    options["noEmit"] = json!(!emit);
    if emit {
        options["declaration"] = json!(true);
        options["noEmitOnError"] = json!(true);
        options["outDir"] = json!(directory.join("blue"));
    }
    let config = directory.join("tsconfig.json");
    fs::write(
        &config,
        json!({"compilerOptions":options,"files":["main.ts"]}).to_string(),
    )
    .unwrap();
    config
}

#[test]
fn matrix_covers_every_target_and_form() {
    let reference = reference();
    let cases = cases();
    assert_eq!(reference["targets"].as_array().unwrap().len(), 11);
    assert_eq!(reference["forms"].as_array().unwrap().len(), 29);
    assert_eq!(reference["modules"].as_array().unwrap().len(), 2);
    assert_eq!(cases.len(), 638);
    assert_eq!(
        cases.iter().filter(|case| case["accepts"] == true).count(),
        600
    );
    assert_eq!(
        cases.iter().filter(|case| case["runtime"] == true).count(),
        600
    );
    assert_eq!(
        cases
            .iter()
            .filter(|case| case["declaration"] == true)
            .count(),
        600
    );
    for target in reference["targets"].as_array().unwrap() {
        for form in reference["forms"].as_array().unwrap() {
            for module in reference["modules"].as_array().unwrap() {
                assert_eq!(
                    cases
                        .iter()
                        .filter(|case| case["target"] == *target
                            && case["form"] == *form
                            && module_kind(case) == module.as_str().unwrap().to_ascii_lowercase())
                        .count(),
                    1,
                    "{target}/{form}/{module}"
                );
            }
        }
    }
    let recorded: BTreeSet<_> = cases
        .iter()
        .map(|case| case["entry"].as_str().unwrap().to_string())
        .collect();
    let discovered = fs::read_dir(fixtures())
        .unwrap()
        .map(|entry| entry.unwrap())
        .filter(|entry| {
            entry.path().is_dir()
                && entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("emittarget-")
        })
        .map(|entry| format!("{}/main.ts", entry.file_name().to_string_lossy()))
        .collect();
    assert_eq!(recorded.len(), cases.len());
    assert_eq!(recorded, discovered);
    for case in &cases {
        let source = fixtures().join(case["entry"].as_str().unwrap());
        let flags = fs::read_to_string(source.parent().unwrap().join("flags.txt")).unwrap();
        let recorded = case["flags"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            flags.split_whitespace().collect::<Vec<_>>(),
            recorded,
            "{}",
            case["entry"]
        );
    }
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
        include_str!("fixtures/typescript_oracle/targets-checker-matrix.tsv").replace("\r\n", "\n")
    );
}

#[test]
fn target_projects_match_pinned_verdicts_and_primary_diagnostics() {
    let root = temporary_root("check");
    let mut failures = Vec::new();
    for (index, case) in cases().into_iter().enumerate() {
        let config = prepare(&case, &root.join(index.to_string()), false);
        let output = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
            .arg("--project")
            .arg(config)
            .arg("--diagnostics-json")
            .output()
            .unwrap();
        let entry = case["entry"].as_str().unwrap();
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
        if output.status.success() != (case["accepts"] == true)
            || (case["accepts"] == true && !diagnostics.is_empty())
        {
            failures.push(format!(
                "{entry}: expected accept={}: {diagnostics:?}",
                case["accepts"]
            ));
            continue;
        }
        if case["accepts"] == true {
            continue;
        }
        let Some(primary) = diagnostics.first().map(|item| &item["typescript"]) else {
            failures.push(format!("{entry}: missing primary diagnostic"));
            continue;
        };
        let related = primary["relatedInformation"].as_array().into_iter().flatten().map(|item|
            json!({"code":item["code"],"message":item["message"],"module":item["span"]["module"],"position":item["position"],"related":[]})).collect::<Vec<_>>();
        let actual = json!({"code":primary["code"],"message":primary["message"],"module":primary["span"]["module"],"position":primary["position"],"related":related});
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
        "{} target mismatches:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn target_manifests_record_helper_version_and_distinct_identity() {
    let root = temporary_root("manifest");
    let source = root.join("main.ts");
    fs::write(&source, "export const value: number = 42;\n").unwrap();
    let mut failures = Vec::new();
    let mut fingerprints = BTreeSet::new();
    let mut versions = BTreeSet::new();
    for target in reference()["targets"].as_array().unwrap() {
        let target = target.as_str().unwrap().to_ascii_lowercase();
        for module in ["commonjs", "es2022"] {
            let output = root.join(format!("{target}-{module}"));
            let built = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
                .arg("build")
                .arg(&source)
                .arg("--project-root")
                .arg(&root)
                .args(["--target", &target, "--module", module])
                .arg("--out-dir")
                .arg(&output)
                .output()
                .unwrap();
            if !built.status.success() {
                failures.push(format!("{target}: {}", report(&built)));
                continue;
            }
            let manifest: Value = serde_json::from_str(
                &fs::read_to_string(output.join("bluetsc.manifest.json")).unwrap(),
            )
            .unwrap();
            if manifest["target"] != target {
                failures.push(format!("{target}: manifest target differs: {manifest}"));
            }
            match manifest["target_helper_version"].as_str() {
                Some(version) if !version.is_empty() => {
                    versions.insert(version.to_string());
                }
                _ => failures.push(format!("{target}: missing target helper version")),
            }
            if !fingerprints.insert(manifest["fingerprint"].as_str().unwrap().to_string()) {
                failures.push(format!("{target}: reused another target's fingerprint"));
            }
        }
    }
    fs::remove_dir_all(root).unwrap();
    assert_eq!(
        versions.len(),
        1,
        "all targets use one versioned helper family: {failures:?}"
    );
    assert_eq!(fingerprints.len(), 22, "{failures:?}");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn target_projects_emit_the_pinned_declarations_without_optional_tools() {
    let root = temporary_root("emit");
    let mut failures = Vec::new();
    for (index, case) in cases()
        .into_iter()
        .filter(|case| case["accepts"] == true)
        .enumerate()
    {
        let directory = root.join(index.to_string());
        let config = prepare(&case, &directory, true);
        let built = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
            .arg("--project")
            .arg(config)
            .output()
            .unwrap();
        let entry = case["entry"].as_str().unwrap();
        if !built.status.success() {
            failures.push(format!("{entry}: build failed: {}", report(&built)));
            continue;
        }
        match fs::read_to_string(directory.join("blue/main.js")) {
            Ok(javascript) if !javascript.is_empty() => {}
            result => failures.push(format!("{entry}: missing JavaScript output: {result:?}")),
        }
        match fs::read_to_string(directory.join("blue/main.d.ts")) {
            Ok(declaration)
                if declaration.replace("\r\n", "\n")
                    == case["declaration_text"].as_str().unwrap() => {}
            result => failures.push(format!("{entry}: declarations differ: {result:?}")),
        }
    }
    fs::remove_dir_all(root).unwrap();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3, Acorn 8.15.0 and Node"]
fn recorded_targets_match_pinned_typescript() {
    let recorder = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../development/browser_core/phase-18-bluets/tools/record_targets.cjs");
    let output = Command::new("node").arg(recorder).output().unwrap();
    assert!(output.status.success(), "{}", report(&output));
}

fn syntax(file: &Path, target: &str, module: &str) -> Output {
    let wrapper = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/oracle_support/assert_target_syntax.cjs");
    Command::new("node")
        .arg(wrapper)
        .arg(file)
        .args([target, module])
        .output()
        .unwrap()
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3, Acorn 8.15.0 and Node"]
fn target_outputs_preserve_execution_declarations_and_syntax_edition() {
    // Regenerate all reference observations before trusting the frozen expectations.
    recorded_targets_match_pinned_typescript();
    let root = temporary_root("runtime");
    let mut failures = Vec::new();
    for (index, case) in cases()
        .into_iter()
        .filter(|case| case["accepts"] == true)
        .enumerate()
    {
        let directory = root.join(index.to_string());
        let config = prepare(&case, &directory, true);
        let built = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
            .arg("--project")
            .arg(config)
            .output()
            .unwrap();
        let entry = case["entry"].as_str().unwrap();
        if !built.status.success() {
            failures.push(format!("{entry}: build failed: {}", report(&built)));
            continue;
        }
        let file = directory.join("blue/main.js");
        let parsed = syntax(&file, case["target"].as_str().unwrap(), module_kind(&case));
        if !parsed.status.success() {
            failures.push(format!(
                "{entry}: target syntax differs: {}",
                report(&parsed)
            ));
        }
        let node = Command::new("node").arg(file).output().unwrap();
        if !node.status.success()
            || String::from_utf8_lossy(&node.stdout).replace("\r\n", "\n")
                != case["stdout"].as_str().unwrap()
        {
            failures.push(format!("{entry}: execution differs: {}", report(&node)));
        }
        let declaration = fs::read_to_string(directory.join("blue/main.d.ts"))
            .unwrap()
            .replace("\r\n", "\n");
        if declaration != case["declaration_text"].as_str().unwrap() {
            failures.push(format!("{entry}: declarations differ: {declaration:?}"));
        }
    }
    fs::remove_dir_all(root).unwrap();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

fn owned_profile_cases() -> Vec<Value> {
    let cases = cases()
        .into_iter()
        .filter(|case| {
            case["accepts"] == true && matches!(case["target"].as_str(), Some("ES2020" | "ES2022"))
        })
        .collect::<Vec<_>>();
    assert_eq!(cases.len(), 112);
    cases
}

fn build_owned_profile(case: &Value, directory: &Path) -> Output {
    prepare(case, directory, false);
    let target = case["target"].as_str().unwrap().to_ascii_lowercase();
    // The native route selects the existing owned library profile. Keep the
    // separate project replay's explicit TypeScript lib and checking flags.
    Command::new(env!("CARGO_BIN_EXE_bluetsc"))
        .arg("build")
        .arg(directory.join("main.ts"))
        .arg("--project-root")
        .arg(directory)
        .args(["--target", &target, "--module", module_kind(case)])
        .arg("--declaration")
        .arg("--out-dir")
        .arg(directory.join("blue"))
        .output()
        .unwrap()
}

#[test]
fn owned_target_profiles_emit_the_pinned_declarations_without_optional_tools() {
    let root = temporary_root("owned-emit");
    let mut failures = Vec::new();
    for (index, case) in owned_profile_cases().into_iter().enumerate() {
        let directory = root.join(index.to_string());
        let built = build_owned_profile(&case, &directory);
        let entry = case["entry"].as_str().unwrap();
        if !built.status.success() {
            failures.push(format!("{entry}: owned build failed: {}", report(&built)));
            continue;
        }
        let javascript = fs::read_to_string(directory.join("blue/main.js")).unwrap();
        assert!(!javascript.is_empty(), "{entry}: empty JavaScript");
        let declaration = fs::read_to_string(directory.join("blue/main.d.ts"))
            .unwrap()
            .replace("\r\n", "\n");
        if declaration != case["declaration_text"].as_str().unwrap() {
            failures.push(format!(
                "{entry}: owned declarations differ: {declaration:?}"
            ));
        }
    }
    fs::remove_dir_all(root).unwrap();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
#[ignore = "requires pinned Acorn 8.15.0 and Node"]
fn owned_target_profiles_match_runtime_declarations_and_syntax() {
    let root = temporary_root("owned-runtime");
    let mut failures = Vec::new();
    for (index, case) in owned_profile_cases().into_iter().enumerate() {
        let directory = root.join(index.to_string());
        let built = build_owned_profile(&case, &directory);
        let entry = case["entry"].as_str().unwrap();
        if !built.status.success() {
            failures.push(format!("{entry}: owned build failed: {}", report(&built)));
            continue;
        }
        let file = directory.join("blue/main.js");
        let parsed = syntax(&file, case["target"].as_str().unwrap(), module_kind(&case));
        if !parsed.status.success() {
            failures.push(format!(
                "{entry}: owned syntax differs: {}",
                report(&parsed)
            ));
        }
        let node = Command::new("node").arg(file).output().unwrap();
        if !node.status.success()
            || String::from_utf8_lossy(&node.stdout).replace("\r\n", "\n")
                != case["stdout"].as_str().unwrap()
        {
            failures.push(format!(
                "{entry}: owned execution differs: {}",
                report(&node)
            ));
        }
        let declaration = fs::read_to_string(directory.join("blue/main.d.ts"))
            .unwrap()
            .replace("\r\n", "\n");
        if declaration != case["declaration_text"].as_str().unwrap() {
            failures.push(format!(
                "{entry}: owned declarations differ: {declaration:?}"
            ));
        }
    }
    fs::remove_dir_all(root).unwrap();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
#[ignore = "requires pinned Acorn 8.15.0 and Node"]
fn existing_es2020_logical_assignment_obeys_the_syntax_floor() {
    let case = cases()
        .into_iter()
        .find(|case| {
            case["target"] == "ES2020"
                && case["form"] == "logical-assign"
                && module_kind(case) == "commonjs"
        })
        .unwrap();
    let root = temporary_root("logical-assignment");
    prepare(&case, &root, false);
    // This direct route also checks the existing owned ES2020 library profile.
    let built = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
        .arg("build")
        .arg(root.join("main.ts"))
        .arg("--project-root")
        .arg(&root)
        .args(["--target", "es2020", "--module", "commonjs"])
        .arg("--out-dir")
        .arg(root.join("blue"))
        .output()
        .unwrap();
    assert!(built.status.success(), "{}", report(&built));
    let file = root.join("blue/main.js");
    let node = Command::new("node").arg(&file).output().unwrap();
    assert!(node.status.success(), "{}", report(&node));
    assert_eq!(
        String::from_utf8_lossy(&node.stdout).replace("\r\n", "\n"),
        case["stdout"].as_str().unwrap()
    );
    let parsed = syntax(&file, "ES2020", "commonjs");
    fs::remove_dir_all(root).unwrap();
    assert!(parsed.status.success(), "{}", report(&parsed));
}
