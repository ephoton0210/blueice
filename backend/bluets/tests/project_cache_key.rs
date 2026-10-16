// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! K.12.2.1: source, compiler-option and graph/config edits miss build state.

use serde_json::json;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

#[path = "oracle_support/project_paths.rs"]
mod project_paths;

const MATRIX: &str = include_str!("fixtures/project_cache_key/cache-key-matrix.tsv");
static NEXT: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Eq, PartialEq)]
struct Snapshot {
    step: String,
    built: Vec<String>,
}

impl Snapshot {
    fn row(&self) -> String {
        format!("{}\t{}\n", self.step, self.built.join(";"))
    }
}

fn matrix() -> Vec<Snapshot> {
    MATRIX
        .lines()
        .map(|line| {
            let (step, built) = line.split_once('\t').unwrap();
            Snapshot {
                step: step.into(),
                built: built.split(';').map(str::to_string).collect(),
            }
        })
        .collect()
}

fn temporary(label: &str) -> PathBuf {
    let path = env::temp_dir().join(format!(
        "bluets-projkey-{label}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&path).unwrap();
    fs::canonicalize(path).unwrap()
}

fn config(references: &[&str], target: &str) -> String {
    let value = json!({
        "compilerOptions": {
            "composite": true,
            "declaration": true,
            "module": "commonjs",
            "target": target
        },
        "files": ["main.ts"],
        "references": references.iter().map(|path| json!({"path": path})).collect::<Vec<_>>()
    });
    serde_json::to_string_pretty(&value).unwrap() + "\n"
}

fn materialize(root: &Path) {
    for dependency in ["dep_a", "dep_b"] {
        let path = root.join(dependency);
        fs::create_dir_all(&path).unwrap();
        fs::write(path.join("tsconfig.json"), config(&[], "ES2022")).unwrap();
        fs::write(path.join("main.ts"), "export const value = 1;\n").unwrap();
    }
    fs::write(root.join("tsconfig.json"), config(&["./dep_a"], "ES2022")).unwrap();
    fs::write(root.join("main.ts"), "export const result = 1;\n").unwrap();
}

fn report(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn snapshot(step: &str, output: &Output, root: &Path) -> Snapshot {
    let built = report(output)
        .lines()
        .filter_map(|line| {
            line.strip_prefix("Building project: ").or_else(|| {
                line.split("Building project '")
                    .nth(1)
                    .and_then(|rest| rest.split('\'').next())
            })
        })
        .map(|project| project_paths::relative_to(root, project))
        .collect();
    Snapshot {
        step: step.into(),
        built,
    }
}

fn build(executable: &Path, root: &Path) -> Output {
    Command::new(executable)
        .current_dir(root)
        .args(["--build", "tsconfig.json", "--verbose", "--pretty", "false"])
        .output()
        .unwrap()
}

fn run(executable: &Path, scenario: &str, label: &str) -> Vec<Snapshot> {
    let root = temporary(label);
    materialize(&root);
    let first = build(executable, &root);
    assert!(first.status.success(), "{}", report(&first));
    let cold = snapshot(&format!("{scenario}-cold"), &first, &root);
    match scenario {
        "source" => fs::write(root.join("main.ts"), "export const result = 2;\n").unwrap(),
        "options" => fs::write(root.join("tsconfig.json"), config(&["./dep_a"], "ES2020")).unwrap(),
        "graph" => fs::write(root.join("tsconfig.json"), config(&["./dep_b"], "ES2022")).unwrap(),
        other => panic!("unknown cache-key scenario {other}"),
    }
    let rebuilt = build(executable, &root);
    assert!(rebuilt.status.success(), "{}", report(&rebuilt));
    let changed = snapshot(&format!("{scenario}-changed"), &rebuilt, &root);
    let execution = Command::new("node")
        .arg("-e")
        .arg("process.stdout.write(String(require(process.argv[1]).result))")
        .arg(root.join("main.js"))
        .output()
        .unwrap();
    assert!(execution.status.success(), "{}", report(&execution));
    assert_eq!(
        execution.stdout,
        if scenario == "source" { b"2" } else { b"1" }
    );
    assert!(root.join("main.d.ts").is_file());
    assert!(root.join("dep_a/main.d.ts").is_file());
    if scenario == "graph" {
        assert!(root.join("dep_b/main.d.ts").is_file());
    }
    fs::remove_dir_all(root).unwrap();
    vec![cold, changed]
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3"]
fn pinned_cache_key_matrix() {
    let tsc = PathBuf::from(env::var_os("BLUEICE_BLUETSC_ORACLE").unwrap());
    let version = Command::new(&tsc).arg("--version").output().unwrap();
    assert_eq!(
        String::from_utf8_lossy(&version.stdout).trim(),
        "Version 5.9.3"
    );
    let observed = ["source", "options", "graph"]
        .iter()
        .flat_map(|scenario| run(&tsc, scenario, &format!("oracle-{scenario}")))
        .collect::<Vec<_>>();
    if env::var("BLUEICE_WRITE_PROJECT_CACHE_KEY_MATRIX").as_deref() == Ok("1") {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/project_cache_key/cache-key-matrix.tsv");
        fs::write(path, observed.iter().map(Snapshot::row).collect::<String>()).unwrap();
    } else {
        assert_eq!(observed, matrix());
    }
}

#[test]
fn source_bytes_change_misses_build_state() {
    let observed = run(
        Path::new(env!("CARGO_BIN_EXE_bluetsc")),
        "source",
        "bluetsc-source",
    );
    assert_eq!(observed, matrix()[0..2]);
}

#[test]
fn compiler_option_change_misses_build_state() {
    let observed = run(
        Path::new(env!("CARGO_BIN_EXE_bluetsc")),
        "options",
        "bluetsc-options",
    );
    assert_eq!(observed, matrix()[2..4]);
}

#[test]
fn graph_configuration_change_misses_build_state() {
    let observed = run(
        Path::new(env!("CARGO_BIN_EXE_bluetsc")),
        "graph",
        "bluetsc-graph",
    );
    assert_eq!(observed, matrix()[4..6]);
}
