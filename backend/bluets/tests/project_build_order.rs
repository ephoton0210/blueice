// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! K.12.1.2: ordered project lists from pinned TypeScript 5.9.3 `--build --verbose`.

use serde::Deserialize;
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

const CASES: &str = include_str!("fixtures/project_build_order/build-order-cases.json");
const MATRIX: &str = include_str!("fixtures/project_build_order/build-order-matrix.tsv");
static NEXT: AtomicU64 = AtomicU64::new(0);

#[derive(Deserialize)]
struct FixtureSet {
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    name: String,
    projects: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Eq, PartialEq)]
struct Observation {
    name: String,
    projects: Vec<String>,
}

#[derive(Debug, Eq, PartialEq)]
struct ArtifactSnapshot {
    declarations: Vec<String>,
    values: Vec<i64>,
}

impl Observation {
    fn row(&self) -> String {
        format!("{}\t{}\n", self.name, self.projects.join(";"))
    }
}

fn cases() -> Vec<Case> {
    serde_json::from_str::<FixtureSet>(CASES).unwrap().cases
}

fn matrix() -> Vec<Observation> {
    MATRIX
        .lines()
        .map(|line| {
            let (name, projects) = line.split_once('\t').expect("name and ordered projects");
            Observation {
                name: name.into(),
                projects: projects.split(';').map(str::to_string).collect(),
            }
        })
        .collect()
}

fn temporary(label: &str) -> PathBuf {
    let root = env::temp_dir().join(format!(
        "bluets-projorder-{label}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&root).unwrap();
    root
}

fn materialize(case: &Case, root: &Path) {
    fs::create_dir_all(root).unwrap();
    for (directory, references) in &case.projects {
        let path = root.join(directory);
        fs::create_dir_all(&path).unwrap();
        let references = references
            .iter()
            .map(|path| json!({"path": path}))
            .collect::<Vec<_>>();
        let config = json!({
            "compilerOptions": {"composite": true, "declaration": true},
            "files": ["main.ts"],
            "references": references
        });
        fs::write(
            path.join("tsconfig.json"),
            serde_json::to_string_pretty(&config).unwrap() + "\n",
        )
        .unwrap();
        fs::write(path.join("main.ts"), "export const value = 1;\n").unwrap();
    }
}

fn report(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn ordered_projects(output: &Output) -> Vec<String> {
    let report = report(output);
    let mut in_list = false;
    let mut projects = Vec::new();
    for line in report.lines() {
        if line.contains("Projects in this build:") {
            in_list = true;
            continue;
        }
        if !in_list {
            continue;
        }
        if let Some(project) = line.trim().strip_prefix("* ") {
            projects.push(project.trim_start_matches("./").replace('\\', "/"));
        } else if !line.trim().is_empty() {
            break;
        }
    }
    projects
}

fn built_projects(output: &Output) -> Vec<String> {
    report(output)
        .lines()
        .filter_map(|line| line.strip_prefix("Building project: "))
        .map(str::to_string)
        .collect()
}

fn pinned_tsc() -> PathBuf {
    let path = PathBuf::from(
        env::var_os("BLUEICE_BLUETSC_ORACLE")
            .expect("set BLUEICE_BLUETSC_ORACLE to pinned TypeScript 5.9.3 tsc"),
    );
    let version = Command::new(&path).arg("--version").output().unwrap();
    assert_eq!(
        String::from_utf8_lossy(&version.stdout).trim(),
        "Version 5.9.3"
    );
    path
}

fn artifact_snapshot(root: &Path, projects: &[String]) -> ArtifactSnapshot {
    let mut declarations = Vec::new();
    let mut scripts = Vec::new();
    for project in projects {
        let directory = root.join(Path::new(project).parent().unwrap());
        declarations.push(fs::read_to_string(directory.join("main.d.ts")).unwrap());
        scripts.push(directory.join("main.js"));
    }
    let execution = Command::new("node")
        .arg("-e")
        .arg("process.stdout.write(JSON.stringify(process.argv.slice(1).map(path => require(path).value)))")
        .args(&scripts)
        .output()
        .unwrap();
    assert!(execution.status.success(), "{}", report(&execution));
    ArtifactSnapshot {
        declarations,
        values: serde_json::from_slice(&execution.stdout).unwrap(),
    }
}

#[test]
fn matrix_names_five_distinct_graphs() {
    let cases = cases();
    let recorded = matrix();
    assert_eq!(cases.len(), 5);
    assert_eq!(recorded.len(), 5);
    assert_eq!(
        cases.iter().map(|case| &case.name).collect::<BTreeSet<_>>(),
        recorded
            .iter()
            .map(|row| &row.name)
            .collect::<BTreeSet<_>>()
    );
    assert!(cases.iter().any(|case| case.name == "projorder-chain"));
    assert!(cases.iter().any(|case| case.name == "projorder-diamond"));
    assert!(cases
        .iter()
        .any(|case| case.name == "projorder-independent-leaves"));
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3"]
fn recorded_build_order_matches_pinned_typescript_verbose() {
    let tsc = pinned_tsc();
    let root = temporary("oracle");
    let observed = cases()
        .iter()
        .map(|case| {
            let directory = root.join(&case.name);
            materialize(case, &directory);
            let output = Command::new(&tsc)
                .current_dir(&directory)
                .args(["--build", "tsconfig.json", "--verbose", "--pretty", "false"])
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}: {}",
                case.name,
                report(&output)
            );
            let projects = ordered_projects(&output);
            assert_eq!(projects.len(), case.projects.len(), "{}", report(&output));
            Observation {
                name: case.name.clone(),
                projects,
            }
        })
        .collect::<Vec<_>>();
    fs::remove_dir_all(root).unwrap();
    if env::var("BLUEICE_WRITE_BUILD_ORDER_MATRIX").as_deref() == Ok("1") {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/project_build_order/build-order-matrix.tsv");
        fs::write(
            path,
            observed.iter().map(Observation::row).collect::<String>(),
        )
        .unwrap();
    } else {
        assert_eq!(observed, matrix());
    }
}

#[test]
fn public_build_lists_and_compiles_projects_in_pinned_order() {
    let root = temporary("bluetsc");
    let mut failures = Vec::new();
    for (case, expected) in cases().iter().zip(matrix()) {
        let directory = root.join(&case.name);
        materialize(case, &directory);
        let output = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
            .current_dir(&directory)
            .args(["--build", "tsconfig.json", "--verbose", "--pretty", "false"])
            .output()
            .unwrap();
        let listed = ordered_projects(&output);
        let built = built_projects(&output);
        if !output.status.success() || listed != expected.projects || built != expected.projects {
            failures.push(format!(
                "{}: expected {:?}, listed {listed:?}, built {built:?}; {}",
                case.name,
                expected.projects,
                report(&output)
            ));
        }
    }
    fs::remove_dir_all(root).unwrap();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3 and Node"]
fn ordered_build_artifacts_execute_and_declare_like_typescript() {
    let tsc = pinned_tsc();
    let root = temporary("artifacts");
    for (case, expected) in cases().iter().zip(matrix()) {
        let oracle = root.join(&case.name).join("tsc");
        let native = root.join(&case.name).join("bluetsc");
        materialize(case, &oracle);
        materialize(case, &native);
        let oracle_output = Command::new(&tsc)
            .current_dir(&oracle)
            .args(["--build", "tsconfig.json", "--pretty", "false"])
            .output()
            .unwrap();
        assert!(
            oracle_output.status.success(),
            "{}: {}",
            case.name,
            report(&oracle_output)
        );
        let native_output = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
            .current_dir(&native)
            .args(["--build", "tsconfig.json", "--pretty", "false"])
            .output()
            .unwrap();
        assert!(
            native_output.status.success(),
            "{}: {}",
            case.name,
            report(&native_output)
        );
        let oracle_artifacts = artifact_snapshot(&oracle, &expected.projects);
        let native_artifacts = artifact_snapshot(&native, &expected.projects);
        assert_eq!(native_artifacts, oracle_artifacts, "{}", case.name);
        assert_eq!(
            native_artifacts.values,
            vec![1; expected.projects.len()],
            "{}",
            case.name
        );
    }
    fs::remove_dir_all(root).unwrap();
}
