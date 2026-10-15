// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! K.12.2.4: malformed or foreign `.tsbuildinfo` never reuses stale output.

use serde_json::json;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

const MATRIX: &str =
    include_str!("fixtures/project_build_corrupt_state/build-corrupt-state-matrix.tsv");
const CASES: [&str; 2] = ["truncated", "foreign"];
static NEXT: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, PartialEq, Eq)]
struct Row {
    step: String,
    built: Vec<String>,
}

impl Row {
    fn record(&self) -> String {
        format!(
            "{}\t{}\n",
            self.step,
            if self.built.is_empty() {
                "-".to_string()
            } else {
                self.built.join(";")
            }
        )
    }
}

fn temporary(label: &str) -> PathBuf {
    let root = env::temp_dir().join(format!(
        "bluets-projcorrupt-{label}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&root).unwrap();
    fs::canonicalize(root).unwrap()
}

fn materialize(root: &Path) {
    fs::create_dir_all(root.join("dep")).unwrap();
    let dep = json!({
        "compilerOptions": {"composite": true, "declaration": true, "module": "commonjs"},
        "files": ["main.ts"]
    });
    let root_config = json!({
        "compilerOptions": {"composite": true, "declaration": true, "module": "commonjs"},
        "files": ["main.ts"],
        "references": [{"path": "./dep"}]
    });
    fs::write(
        root.join("dep/tsconfig.json"),
        serde_json::to_string_pretty(&dep).unwrap() + "\n",
    )
    .unwrap();
    fs::write(root.join("dep/main.ts"), "export const dependency = 1;\n").unwrap();
    fs::write(
        root.join("tsconfig.json"),
        serde_json::to_string_pretty(&root_config).unwrap() + "\n",
    )
    .unwrap();
    fs::write(
        root.join("main.ts"),
        "import { dependency } from './dep/main';\nexport const value = dependency + 1;\n",
    )
    .unwrap();
}

fn report(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn build(executable: &Path, root: &Path, step: &str) -> (Row, String) {
    let output = Command::new(executable)
        .current_dir(root)
        .args(["--build", "tsconfig.json", "--verbose", "--pretty", "false"])
        .output()
        .unwrap();
    let message = report(&output);
    assert!(output.status.success(), "{step}: {message}");
    let built = message
        .lines()
        .filter_map(|line| {
            line.strip_prefix("Building project: ").or_else(|| {
                line.split("Building project '")
                    .nth(1)
                    .and_then(|rest| rest.split('\'').next())
            })
        })
        .map(|project| {
            Path::new(project)
                .strip_prefix(root)
                .unwrap_or_else(|_| Path::new(project))
                .to_string_lossy()
                .trim_start_matches("./")
                .replace('\\', "/")
        })
        .collect();
    (
        Row {
            step: step.to_string(),
            built,
        },
        message,
    )
}

fn artifacts(root: &Path) -> Vec<Vec<u8>> {
    ["dep/main.js", "dep/main.d.ts", "main.js", "main.d.ts"]
        .iter()
        .map(|name| fs::read(root.join(name)).unwrap())
        .collect()
}

fn corrupt(path: &Path, case: &str) {
    let bytes = match case {
        "truncated" => b"{\"version\":".as_slice(),
        "foreign" => {
            b"{\"version\":\"0.0.0\",\"fileNames\":[],\"secret\":\"FOREIGN_SENTINEL\"}".as_slice()
        }
        _ => unreachable!(),
    };
    fs::write(path, bytes).unwrap();
}

fn run(executable: &Path, label: &str, case: &str, native: bool) -> Vec<Row> {
    let root = temporary(label);
    materialize(&root);
    let (cold, _) = build(executable, &root, &format!("{case}-cold"));
    let state = root.join(if native {
        "tsconfig.bluetsc.tsbuildinfo"
    } else {
        "tsconfig.tsbuildinfo"
    });
    assert!(state.is_file(), "{case}: expected .tsbuildinfo state");
    let (noop, _) = build(executable, &root, &format!("{case}-noop"));
    corrupt(&state, case);
    fs::write(root.join("main.js"), "exports.value = -999;\n").unwrap();
    let (repaired, message) = build(executable, &root, &format!("{case}-repaired"));
    if native {
        assert!(
            message.contains("invalid or foreign .tsbuildinfo"),
            "{message}"
        );
        assert!(!message.contains("FOREIGN_SENTINEL"), "{message}");
        assert!(
            message.len() < 1024,
            "diagnostic must be bounded: {message}"
        );
    }
    let clean = temporary(&format!("{label}-clean"));
    materialize(&clean);
    let (clean_built, _) = build(executable, &clean, "clean");
    assert_eq!(
        clean_built.built,
        vec!["dep/tsconfig.json", "tsconfig.json"]
    );
    assert_eq!(
        artifacts(&root),
        artifacts(&clean),
        "{case}: recovered output differs from clean build"
    );
    let (reused, _) = build(executable, &root, &format!("{case}-reused"));
    fs::remove_dir_all(root).unwrap();
    fs::remove_dir_all(clean).unwrap();
    vec![cold, noop, repaired, reused]
}

fn matrix() -> Vec<Row> {
    MATRIX
        .lines()
        .map(|line| {
            let (step, built) = line.split_once('\t').unwrap();
            Row {
                step: step.to_string(),
                built: if built == "-" {
                    Vec::new()
                } else {
                    built.split(';').map(str::to_string).collect()
                },
            }
        })
        .collect()
}

#[test]
fn matrix_covers_two_four_step_recovery_sequences() {
    assert_eq!(
        matrix()
            .iter()
            .map(|row| row.step.as_str())
            .collect::<Vec<_>>(),
        [
            "truncated-cold",
            "truncated-noop",
            "truncated-repaired",
            "truncated-reused",
            "foreign-cold",
            "foreign-noop",
            "foreign-repaired",
            "foreign-reused",
        ]
    );
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3"]
fn pinned_corrupt_build_state_matrix() {
    let tsc = PathBuf::from(env::var_os("BLUEICE_BLUETSC_ORACLE").unwrap());
    let version = Command::new(&tsc).arg("--version").output().unwrap();
    assert_eq!(
        String::from_utf8_lossy(&version.stdout).trim(),
        "Version 5.9.3"
    );
    let observed = CASES
        .iter()
        .flat_map(|case| run(&tsc, &format!("oracle-{case}"), case, false))
        .collect::<Vec<_>>();
    if env::var("BLUEICE_WRITE_PROJECT_CORRUPT_STATE_MATRIX").as_deref() == Ok("1") {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/project_build_corrupt_state/build-corrupt-state-matrix.tsv");
        fs::write(path, observed.iter().map(Row::record).collect::<String>()).unwrap();
    } else {
        assert_eq!(observed, matrix());
    }
}

#[test]
fn truncated_state_rebuilds_without_reusing_wrong_output() {
    assert_eq!(
        run(
            Path::new(env!("CARGO_BIN_EXE_bluetsc")),
            "bluetsc-truncated",
            "truncated",
            true
        ),
        matrix()[0..4]
    );
}

#[test]
fn foreign_state_rebuilds_without_reusing_wrong_output() {
    assert_eq!(
        run(
            Path::new(env!("CARGO_BIN_EXE_bluetsc")),
            "bluetsc-foreign",
            "foreign",
            true
        ),
        matrix()[4..8]
    );
}

#[test]
fn truncated_dependency_state_rebuilds_the_dependent_conservatively() {
    let bluetsc = Path::new(env!("CARGO_BIN_EXE_bluetsc"));
    let root = temporary("truncated-dependency");
    materialize(&root);
    let _ = build(bluetsc, &root, "cold");
    fs::write(
        root.join("dep/tsconfig.bluetsc.tsbuildinfo"),
        b"{\"version\":",
    )
    .unwrap();
    fs::write(root.join("dep/main.js"), "exports.dependency = -999;\n").unwrap();
    let (repaired, message) = build(bluetsc, &root, "repaired");
    assert_eq!(repaired.built, vec!["dep/tsconfig.json", "tsconfig.json"]);
    assert!(
        message.contains("invalid or foreign .tsbuildinfo"),
        "{message}"
    );
    let clean = temporary("truncated-dependency-clean");
    materialize(&clean);
    let _ = build(bluetsc, &clean, "clean");
    assert_eq!(artifacts(&root), artifacts(&clean));
    let (reused, _) = build(bluetsc, &root, "reused");
    assert!(reused.built.is_empty());
    fs::remove_dir_all(root).unwrap();
    fs::remove_dir_all(clean).unwrap();
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3"]
fn actual_typescript_build_info_is_foreign_to_bluetsc() {
    let tsc = PathBuf::from(env::var_os("BLUEICE_BLUETSC_ORACLE").unwrap());
    let oracle = temporary("actual-foreign-oracle");
    materialize(&oracle);
    let _ = build(&tsc, &oracle, "oracle-cold");
    let foreign = fs::read(oracle.join("tsconfig.tsbuildinfo")).unwrap();
    let actual: serde_json::Value = serde_json::from_slice(&foreign).unwrap();
    assert_eq!(actual["version"], "5.9.3");

    let bluetsc = Path::new(env!("CARGO_BIN_EXE_bluetsc"));
    let root = temporary("actual-foreign-bluetsc");
    materialize(&root);
    let _ = build(bluetsc, &root, "bluetsc-cold");
    fs::write(root.join("tsconfig.bluetsc.tsbuildinfo"), foreign).unwrap();
    fs::write(root.join("main.js"), "exports.value = -999;\n").unwrap();
    let (repaired, message) = build(bluetsc, &root, "actual-foreign-repaired");
    assert_eq!(repaired.built, vec!["tsconfig.json"]);
    assert!(
        message.contains("invalid or foreign .tsbuildinfo"),
        "{message}"
    );
    assert!(message.len() < 1024, "{message}");
    let clean = temporary("actual-foreign-clean");
    materialize(&clean);
    let _ = build(bluetsc, &clean, "clean");
    assert_eq!(artifacts(&root), artifacts(&clean));
    let state: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("tsconfig.bluetsc.tsbuildinfo")).unwrap())
            .unwrap();
    assert_eq!(state["producer"], "bluetsc");
    assert_eq!(state["version"], 4);
    fs::remove_dir_all(oracle).unwrap();
    fs::remove_dir_all(root).unwrap();
    fs::remove_dir_all(clean).unwrap();
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3"]
fn pinned_typescript_rebuilds_after_bluetsc_state() {
    let tsc = PathBuf::from(env::var_os("BLUEICE_BLUETSC_ORACLE").unwrap());
    let bluetsc = Path::new(env!("CARGO_BIN_EXE_bluetsc"));
    let root = temporary("typescript-after-bluetsc");
    materialize(&root);
    let _ = build(bluetsc, &root, "bluetsc-cold");
    let (rebuilt, _) = build(&tsc, &root, "typescript-after-bluetsc");
    assert_eq!(rebuilt.built, vec!["dep/tsconfig.json", "tsconfig.json"]);
    fs::remove_dir_all(root).unwrap();
}
