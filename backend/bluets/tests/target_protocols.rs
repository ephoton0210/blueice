// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Iterator closing and generator completion boundaries for authored target helpers.

use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/target_protocols")
}

fn reference() -> Value {
    let record: Value =
        serde_json::from_str(include_str!("fixtures/target_protocols/reference.json")).unwrap();
    assert_eq!(record["typescript"], "5.9.3");
    assert_eq!(record["acorn"], "8.15.0");
    assert_eq!(record["native_observations"], 37);
    assert_eq!(
        record["native_stdout"].as_str().unwrap().lines().count(),
        37
    );
    let cases = record["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 22);
    let targets = [
        "ES5", "ES2015", "ES2016", "ES2017", "ES2018", "ES2019", "ES2020", "ES2021", "ES2022",
        "ES2023", "ESNext",
    ];
    let mut observed = BTreeSet::new();
    for case in cases {
        let target = case["target"].as_str().unwrap();
        let module = case["module"].as_str().unwrap();
        assert!(targets.contains(&target));
        assert!(matches!(module, "commonjs" | "ES2022"));
        assert!(observed.insert((target, module)));
        assert_eq!(case["accepts"], true);
        assert_eq!(
            case["flags"],
            json!([
                "--target",
                target,
                "--module",
                module,
                "--lib",
                "ES2020",
                "--strict",
                "--skipLibCheck",
                "--downlevelIteration",
                "true"
            ])
        );
    }
    assert_eq!(observed.len(), 22);
    record
}

fn report(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn temporary_root(name: &str) -> PathBuf {
    let directory = fs::canonicalize(std::env::temp_dir())
        .unwrap()
        .join(format!("bluets-protocol-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&directory);
    fs::create_dir_all(&directory).unwrap();
    directory
}

fn build(case: &Value, directory: &Path) -> Output {
    fs::create_dir_all(directory).unwrap();
    fs::copy(fixtures().join("input.ts"), directory.join("main.ts")).unwrap();
    fs::write(
        directory.join("package.json"),
        json!({"type":if case["module"] == "commonjs" { "commonjs" } else { "module" }})
            .to_string(),
    )
    .unwrap();
    let config = directory.join("tsconfig.json");
    fs::write(
        &config,
        json!({"compilerOptions":{
            "target":case["target"],"module":case["module"],"lib":["ES2020"],
            "strict":true,"skipLibCheck":true,"downlevelIteration":true,
            "declaration":true,"noEmitOnError":true,"outDir":"blue"
        },"files":["main.ts"]})
        .to_string(),
    )
    .unwrap();
    Command::new(env!("CARGO_BIN_EXE_bluetsc"))
        .arg("--project")
        .arg(config)
        .output()
        .unwrap()
}

#[test]
fn protocol_projects_emit_the_pinned_declarations() {
    let record = reference();
    let root = temporary_root("emit");
    let mut failures = Vec::new();
    for (index, case) in record["cases"].as_array().unwrap().iter().enumerate() {
        let directory = root.join(index.to_string());
        let built = build(case, &directory);
        let label = format!("{}/{}", case["target"], case["module"]);
        if !built.status.success() {
            failures.push(format!("{label}: build failed: {}", report(&built)));
            continue;
        }
        let declaration = fs::read_to_string(directory.join("blue/main.d.ts"))
            .unwrap()
            .replace("\r\n", "\n");
        if declaration != case["declaration_text"].as_str().unwrap() {
            failures.push(format!("{label}: declarations differ: {declaration:?}"));
        }
    }
    fs::remove_dir_all(root).unwrap();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3, Acorn 8.15.0 and Node"]
fn recorded_protocol_observations_match_pinned_tools() {
    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../development/browser_core/phase-18-bluets/tools/record_target_protocols.cjs");
    let output = Command::new("node").arg(script).output().unwrap();
    assert!(output.status.success(), "{}", report(&output));
}

#[test]
#[ignore = "requires pinned Acorn 8.15.0 and Node"]
fn authored_target_output_matches_native_protocol_semantics() {
    let record = reference();
    let native = Command::new("node")
        .arg(fixtures().join("native.mjs"))
        .output()
        .unwrap();
    assert!(native.status.success(), "{}", report(&native));
    let expected = record["native_stdout"].as_str().unwrap();
    assert_eq!(
        String::from_utf8_lossy(&native.stdout).replace("\r\n", "\n"),
        expected
    );
    let syntax = fixtures()
        .parent()
        .unwrap()
        .join("oracle_support/assert_target_syntax.cjs");
    let root = temporary_root("runtime");
    let mut failures = Vec::new();
    for (index, case) in record["cases"].as_array().unwrap().iter().enumerate() {
        let directory = root.join(index.to_string());
        let built = build(case, &directory);
        let label = format!("{}/{}", case["target"], case["module"]);
        if !built.status.success() {
            failures.push(format!("{label}: build failed: {}", report(&built)));
            continue;
        }
        let file = directory.join("blue/main.js");
        let parsed = Command::new("node")
            .arg(&syntax)
            .arg(&file)
            .arg(case["target"].as_str().unwrap())
            .arg(case["module"].as_str().unwrap())
            .output()
            .unwrap();
        if !parsed.status.success() {
            failures.push(format!(
                "{label}: target syntax differs: {}",
                report(&parsed)
            ));
        }
        let executed = Command::new("node").arg(file).output().unwrap();
        if !executed.status.success()
            || String::from_utf8_lossy(&executed.stdout).replace("\r\n", "\n") != expected
        {
            failures.push(format!(
                "{label}: native semantics differ: {}",
                report(&executed)
            ));
        }
    }
    fs::remove_dir_all(root).unwrap();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
