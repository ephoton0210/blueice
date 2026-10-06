// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! K.2.2: each checking flag has a pinned on/off project pair.

use serde_json::Value;
use std::{
    collections::BTreeSet,
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};
const MATRIX: &str = include_str!("fixtures/strictness/strictness-checker-matrix.tsv");
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/strictness")
}
fn rows() -> Vec<(&'static str, bool)> {
    MATRIX
        .lines()
        .map(|line| {
            let (name, verdict) = line.split_once('\t').unwrap();
            assert!(matches!(verdict, "accept" | "reject"));
            (name, verdict == "accept")
        })
        .collect()
}
#[test]
fn every_flag_has_both_switch_positions() {
    let names = fs::read_dir(root())
        .unwrap()
        .map(|p| p.unwrap())
        .filter(|p| p.path().is_dir())
        .map(|p| p.file_name().to_str().unwrap().to_string())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        names,
        rows().into_iter().map(|(n, _)| n.to_string()).collect()
    );
    assert_eq!(names.len(), 32);
    for (name, accepts) in rows() {
        let opposite = name
            .strip_suffix(if accepts { "-0" } else { "-1" })
            .unwrap();
        assert!(names.contains(&format!("{opposite}-{}", if accepts { 1 } else { 0 })));
    }
}
#[test]
fn project_checking_matches_the_recorded_strictness_matrix() {
    let mut failures = Vec::new();
    for (name, accepts) in rows() {
        let output = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
            .current_dir(root().join(name))
            .args(["check", "--config", "tsconfig.json"])
            .output()
            .unwrap();
        if output.status.success() != accepts {
            failures.push(format!(
                "{name}: expected {accepts}; {}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
#[test]
#[ignore = "requires pinned TypeScript 5.9.3"]
fn recorded_strictness_matrix_matches_pinned_typescript() {
    let tsc = env::var_os("BLUEICE_BLUETSC_ORACLE")
        .expect("set BLUEICE_BLUETSC_ORACLE to TypeScript 5.9.3");
    let version = Command::new(&tsc).arg("--version").output().unwrap();
    assert_eq!(
        String::from_utf8_lossy(&version.stdout).trim(),
        "Version 5.9.3"
    );
    let write = env::var("BLUEICE_WRITE_STRICTNESS_MATRIX").as_deref() == Ok("1");
    let mut matrix = String::new();
    for (name, _) in rows() {
        let output = Command::new(&tsc)
            .current_dir(root().join(name))
            .args([
                "--project",
                "tsconfig.json",
                "--noEmit",
                "--pretty",
                "false",
            ])
            .output()
            .unwrap();
        matrix.push_str(&format!(
            "{name}\t{}\n",
            if output.status.success() {
                "accept"
            } else {
                "reject"
            }
        ));
        let config: Value =
            serde_json::from_slice(&fs::read(root().join(name).join("tsconfig.json")).unwrap())
                .unwrap();
        assert!(config["compilerOptions"]["strict"].is_boolean());
    }
    if write {
        fs::write(root().join("strictness-checker-matrix.tsv"), matrix).unwrap();
    } else {
        assert_eq!(matrix, MATRIX);
    }
}

#[test]
fn checking_flags_preserve_javascript_and_unrelated_type_errors() {
    let directory = env::temp_dir().join(format!("bluets-strictness-emit-{}", std::process::id()));
    let _ = fs::remove_dir_all(&directory);
    fs::create_dir_all(&directory).unwrap();
    let source="export function read(value: number): number { return value + 1; }\nconsole.log(read(41));\n";
    fs::write(directory.join("main.ts"), source).unwrap();
    let mut baseline = None;
    for (name, _) in rows() {
        let config: Value =
            serde_json::from_slice(&fs::read(root().join(name).join("tsconfig.json")).unwrap())
                .unwrap();
        let mut config = config;
        config["compilerOptions"]["outDir"] = Value::String("out".to_string());
        fs::write(
            directory.join("tsconfig.json"),
            serde_json::to_vec_pretty(&config).unwrap(),
        )
        .unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
            .current_dir(&directory)
            .args(["build", "--config", "tsconfig.json"])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{name}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let javascript = fs::read(directory.join("out/main.js")).unwrap();
        if let Some(expected) = &baseline {
            assert_eq!(&javascript, expected, "{name}");
        } else {
            baseline = Some(javascript);
        }
        fs::write(
            directory.join("main.ts"),
            "export const value: number = \"wrong\";\n",
        )
        .unwrap();
        let failed = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
            .current_dir(&directory)
            .args(["check", "--config", "tsconfig.json"])
            .output()
            .unwrap();
        assert!(
            !failed.status.success(),
            "{name}: unrelated type mismatch was disabled"
        );
        fs::write(directory.join("main.ts"), source).unwrap();
    }
    fs::remove_dir_all(directory).unwrap();
}

#[test]
#[ignore = "requires Node and pinned TypeScript 5.9.3"]
fn strict_and_relaxed_valid_programs_print_the_same_result() {
    let tsc = env::var_os("BLUEICE_BLUETSC_ORACLE").expect("pinned TypeScript 5.9.3");
    let directory = env::temp_dir().join(format!("bluets-strictness-node-{}", std::process::id()));
    let _ = fs::remove_dir_all(&directory);
    fs::create_dir_all(&directory).unwrap();
    fs::write(directory.join("main.ts"),"export function read(value: number): number { return value + 1; }\nconsole.log(read(41));\n").unwrap();
    fs::write(directory.join("package.json"), "{\"type\":\"module\"}").unwrap();
    for strict in [false, true] {
        let config = serde_json::json!({"compilerOptions":{"target":"es2022","strict":strict,"declaration":true,"outDir":"blue"},"files":["main.ts"]});
        fs::write(
            directory.join("tsconfig.json"),
            serde_json::to_vec_pretty(&config).unwrap(),
        )
        .unwrap();
        let built = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
            .current_dir(&directory)
            .args(["build", "--config", "tsconfig.json"])
            .output()
            .unwrap();
        assert!(
            built.status.success(),
            "{}",
            String::from_utf8_lossy(&built.stderr)
        );
        let emitted = Command::new(&tsc)
            .current_dir(&directory)
            .args([
                "--project",
                "tsconfig.json",
                "--outDir",
                "tsc",
                "--pretty",
                "false",
            ])
            .output()
            .unwrap();
        assert!(
            emitted.status.success(),
            "{}",
            String::from_utf8_lossy(&emitted.stdout)
        );
        assert_eq!(
            fs::read(directory.join("blue/main.d.ts")).unwrap(),
            fs::read(directory.join("tsc/main.d.ts")).unwrap()
        );
        let run = |sub: &str| {
            Command::new("node")
                .arg(directory.join(sub).join("main.js"))
                .output()
                .unwrap()
        };
        let blue = run("blue");
        let reference = run("tsc");
        assert!(blue.status.success());
        assert!(reference.status.success());
        assert_eq!(blue.stdout, reference.stdout);
        assert_eq!(blue.stdout, b"42\n");
    }
    fs::remove_dir_all(directory).unwrap();
}
