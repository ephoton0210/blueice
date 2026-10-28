// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! G-C1: exact, owner-local explainFiles reasons from pinned TypeScript.

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::{env, fs};

fn materialize(label: &str, input: &Value, case: &Value) -> PathBuf {
    let root = env::temp_dir().join(format!("bluets-gc1-{label}-{}", std::process::id()));
    fs::create_dir(&root).unwrap();
    for (name, source) in case["files"].as_object().unwrap() {
        assert!(Path::new(name)
            .components()
            .all(|part| matches!(part, std::path::Component::Normal(_))));
        let target = root.join(name);
        assert!(target.starts_with(&root));
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(
            target,
            source
                .as_str()
                .unwrap()
                .replace("@globals", input["globals"].as_str().unwrap()),
        )
        .unwrap();
    }
    let mut config = case["config"].clone();
    let mut options = input["options"].as_object().unwrap().clone();
    if let Some(local) = config["compilerOptions"].as_object() {
        options.extend(local.clone());
    }
    config["compilerOptions"] = Value::Object(options);
    fs::write(
        root.join("tsconfig.json"),
        serde_json::to_vec_pretty(&config).unwrap(),
    )
    .unwrap();
    fs::canonicalize(root).unwrap()
}

#[test]
fn explain_files_config_and_false_override_are_observational_only() {
    let input: Value =
        serde_json::from_str(include_str!("fixtures/gc1_explain/cases.json")).unwrap();
    let reference: Value =
        serde_json::from_str(include_str!("fixtures/gc1_explain/reference.json")).unwrap();
    let case = &input["cases"][1];
    let root = materialize("flag-controls", &input, case);
    let config_path = root.join("tsconfig.json");
    let mut config: Value = serde_json::from_slice(&fs::read(&config_path).unwrap()).unwrap();
    config["compilerOptions"]["explainFiles"] = Value::Bool(true);
    fs::write(&config_path, serde_json::to_vec_pretty(&config).unwrap()).unwrap();
    let before = fs::read(&config_path).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
        .current_dir(&root)
        .args(["--project", "tsconfig.json", "--pretty", "false"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        normalize(&output.stdout, &root),
        reference["cases"][1]["stdout"]
    );
    for arguments in [vec!["--explainFiles", "false"], vec!["--showConfig"]] {
        let output = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
            .current_dir(&root)
            .args(["--project", "tsconfig.json", "--pretty", "false"])
            .args(&arguments)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stderr.is_empty());
        if arguments[0] == "--showConfig" {
            assert!(serde_json::from_slice::<Value>(&output.stdout).is_ok());
        } else {
            assert!(output.stdout.is_empty());
        }
    }
    assert_eq!(fs::read(&config_path).unwrap(), before);
    assert!(!root.join("entry.js").exists());
    assert!(!root.join("dep.js").exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn explain_files_does_not_follow_or_describe_outside_owner_input() {
    let input: Value =
        serde_json::from_str(include_str!("fixtures/gc1_explain/cases.json")).unwrap();
    let mut case = input["cases"][0].clone();
    case["files"]["entry.ts"] =
        Value::String("import { secret } from '../outside'; export const value = secret;".into());
    let container = env::temp_dir().join(format!("bluets-gc1-owner-{}", std::process::id()));
    fs::create_dir(&container).unwrap();
    let outside = container.join("outside.ts");
    fs::write(
        &outside,
        "export const secret = 'GC1_PRIVATE_SOURCE_BODY';\n",
    )
    .unwrap();
    // The configured project is a child; the sibling is deliberately not granted.
    let initial = materialize("owner-input", &input, &case);
    let root = container.join("project");
    fs::rename(&initial, &root).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
        .current_dir(&root)
        .args([
            "--project",
            "tsconfig.json",
            "--pretty",
            "false",
            "--explainFiles",
        ])
        .output()
        .unwrap();
    let report = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!output.status.success());
    assert!(!report.contains("GC1_PRIVATE_SOURCE_BODY"), "{report}");
    let normalized = report.replace('\\', "/");
    for path in [&outside, &fs::canonicalize(&outside).unwrap()] {
        assert!(
            !normalized.contains(&path.to_string_lossy().replace('\\', "/")),
            "outside-owner path disclosed: {report}"
        );
    }
    assert!(
        !report
            .lines()
            .any(|line| line == "../outside.ts" || line.contains("Referenced via '../outside'")),
        "{report}"
    );
    assert!(!root.join("entry.js").exists());
    assert_eq!(
        fs::read_to_string(&outside).unwrap(),
        "export const secret = 'GC1_PRIVATE_SOURCE_BODY';\n"
    );
    fs::remove_dir_all(container).unwrap();
}

#[test]
fn pre_fix_build_state_cannot_bypass_the_new_checker() {
    let input: Value =
        serde_json::from_str(include_str!("fixtures/gc1_explain/cases.json")).unwrap();
    let root = materialize("checker-state-epoch", &input, &input["cases"][0]);
    let path = root.join("tsconfig.json");
    let mut config: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    config["compilerOptions"]
        .as_object_mut()
        .unwrap()
        .remove("noEmit");
    config["compilerOptions"]["composite"] = Value::Bool(true);
    config["compilerOptions"]["declaration"] = Value::Bool(true);
    fs::write(&path, serde_json::to_vec_pretty(&config).unwrap()).unwrap();
    let run = || {
        Command::new(env!("CARGO_BIN_EXE_bluetsc"))
            .current_dir(&root)
            .args(["--build", "tsconfig.json", "--verbose", "--pretty", "false"])
            .output()
            .unwrap()
    };
    let cold = run();
    assert!(
        cold.status.success(),
        "{}{}",
        String::from_utf8_lossy(&cold.stdout),
        String::from_utf8_lossy(&cold.stderr)
    );
    let state_path = root.join("tsconfig.bluetsc.tsbuildinfo");
    let mut state: Value = serde_json::from_slice(&fs::read(&state_path).unwrap()).unwrap();
    assert_eq!(state["version"], 18);
    let warm = run();
    assert!(warm.status.success());
    assert!(!String::from_utf8_lossy(&warm.stdout).contains("Building project:"));
    for old_epoch in [4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17] {
        state["version"] = Value::from(old_epoch);
        fs::write(&state_path, serde_json::to_vec(&state).unwrap()).unwrap();
        let rebuilt = run();
        assert!(
            rebuilt.status.success(),
            "{}{}",
            String::from_utf8_lossy(&rebuilt.stdout),
            String::from_utf8_lossy(&rebuilt.stderr)
        );
        assert!(String::from_utf8_lossy(&rebuilt.stdout).contains("Building project:"));
        let current: Value = serde_json::from_slice(&fs::read(&state_path).unwrap()).unwrap();
        assert_eq!(current["version"], 18);
    }
    fs::remove_dir_all(root).unwrap();
}

fn normalize(text: &[u8], root: &Path) -> String {
    let root = root.to_string_lossy().replace('\\', "/");
    String::from_utf8(text.to_vec())
        .unwrap()
        .replace('\\', "/")
        .replace(&root, "<project>")
        .replace(root.strip_prefix("//?/").unwrap_or(&root), "<project>")
        .replace("\r\n", "\n")
}

#[test]
fn complete_explain_files_output_matches_four_pinned_reason_families() {
    let input: Value =
        serde_json::from_str(include_str!("fixtures/gc1_explain/cases.json")).unwrap();
    let reference: Value =
        serde_json::from_str(include_str!("fixtures/gc1_explain/reference.json")).unwrap();
    assert_eq!(input["typescript"], "5.9.3");
    assert_eq!(reference["typescript"], input["typescript"]);
    assert_eq!(input["cases"].as_array().unwrap().len(), 4);
    assert_eq!(reference["cases"].as_array().unwrap().len(), 4);
    let mut failures = Vec::new();
    for (case, expected) in input["cases"]
        .as_array()
        .unwrap()
        .iter()
        .zip(reference["cases"].as_array().unwrap())
    {
        assert_eq!(case["id"], expected["id"]);
        let id = case["id"].as_str().unwrap();
        let root = materialize(id, &input, case);
        let output = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
            .current_dir(&root)
            .args([
                "--project",
                "tsconfig.json",
                "--pretty",
                "false",
                "--explainFiles",
            ])
            .output()
            .unwrap();
        let actual = serde_json::json!({"id":id,"exit":output.status.code(),"stdout":normalize(&output.stdout,&root),"stderr":normalize(&output.stderr,&root)});
        if actual != *expected {
            failures.push(format!("{id}: expected {expected}; got {actual}"));
        }
        fs::remove_dir_all(root).unwrap();
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
#[ignore = "requires BLUEICE_BLUETSC_ORACLE to point to pinned TypeScript 5.9.3"]
fn explain_files_observations_match_live_pinned_typescript() {
    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../development/browser_core/phase-18-bluets/tools/record_gc1_explain.cjs");
    let output = Command::new("node").arg(script).output().unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
