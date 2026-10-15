// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! K.12.1.1: pinned multi-project reference-graph cases at the CLI boundary.

use serde::Deserialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

const CASES: &str = include_str!("fixtures/project_references/reference-graph-cases.json");
const MATRIX: &str = include_str!("fixtures/project_references/reference-graph-matrix.tsv");
static NEXT: AtomicU64 = AtomicU64::new(0);

#[derive(Deserialize)]
struct FixtureSet {
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    name: String,
    projects: BTreeMap<String, FixtureProject>,
}

#[derive(Deserialize)]
struct FixtureProject {
    config: Value,
    sources: BTreeMap<String, String>,
}

#[derive(Debug, Eq, PartialEq)]
struct Observation {
    name: String,
    verdict: String,
    code: String,
    file: String,
    line: u32,
    column: u32,
}

impl Observation {
    fn row(&self) -> String {
        format!(
            "{}\t{}\t{}\t{}\t{}\t{}\n",
            self.name, self.verdict, self.code, self.file, self.line, self.column
        )
    }
}

fn cases() -> Vec<Case> {
    serde_json::from_str::<FixtureSet>(CASES).unwrap().cases
}

fn matrix() -> Vec<Observation> {
    MATRIX
        .lines()
        .map(|line| {
            let fields = line.split('\t').collect::<Vec<_>>();
            assert_eq!(fields.len(), 6, "{line}");
            Observation {
                name: fields[0].into(),
                verdict: fields[1].into(),
                code: fields[2].into(),
                file: fields[3].into(),
                line: fields[4].parse().unwrap(),
                column: fields[5].parse().unwrap(),
            }
        })
        .collect()
}

fn temporary(label: &str) -> PathBuf {
    let root = env::temp_dir().join(format!(
        "bluets-projref-{label}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&root).unwrap();
    root
}

fn materialize(case: &Case, root: &Path) {
    fs::create_dir_all(root).unwrap();
    for (directory, project) in &case.projects {
        let path = root.join(directory);
        fs::create_dir_all(&path)
            .unwrap_or_else(|error| panic!("create project directory {}: {error}", path.display()));
        let config = serde_json::to_string_pretty(&project.config).unwrap() + "\n";
        fs::write(path.join("tsconfig.json"), config).unwrap();
        for (name, source) in &project.sources {
            let file = path.join(name);
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(file, source).unwrap();
        }
    }
}

fn report(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn observation(case: &Case, root: &Path, output: &Output) -> Observation {
    let diagnostic = report(output)
        .lines()
        .find_map(|line| {
            let (prefix, rest) = line.split_once("error TS")?;
            let code = rest
                .chars()
                .take_while(char::is_ascii_digit)
                .collect::<String>();
            if code.is_empty() {
                return None;
            }
            let prefix = prefix.trim().trim_end_matches(':');
            let Some((file, position)) = prefix.rsplit_once('(') else {
                return Some((code, "-".to_string(), 0, 0));
            };
            let Some(position) = position.strip_suffix(')') else {
                return Some((code, "-".to_string(), 0, 0));
            };
            let (line, column) = position.split_once(',')?;
            let file = Path::new(file.trim())
                .strip_prefix(root)
                .unwrap_or_else(|_| Path::new(file.trim()))
                .to_string_lossy()
                .replace('\\', "/")
                .trim_start_matches("./")
                .to_string();
            Some((code, file, line.parse().ok()?, column.parse().ok()?))
        })
        .unwrap_or_else(|| ("-".into(), "-".into(), 0, 0));
    Observation {
        name: case.name.clone(),
        verdict: if output.status.success() {
            "accept".into()
        } else {
            "reject".into()
        },
        code: diagnostic.0,
        file: diagnostic.1,
        line: diagnostic.2,
        column: diagnostic.3,
    }
}

fn pinned_tsc() -> PathBuf {
    let path = PathBuf::from(
        env::var_os("BLUEICE_BLUETSC_ORACLE")
            .expect("set BLUEICE_BLUETSC_ORACLE to the pinned TypeScript 5.9.3 tsc"),
    );
    let version = Command::new(&path).arg("--version").output().unwrap();
    assert_eq!(
        String::from_utf8_lossy(&version.stdout).trim(),
        "Version 5.9.3"
    );
    path
}

#[test]
fn reference_graph_matrix_has_eight_rejects_and_a_positive_graph() {
    let cases = cases();
    let expected = matrix();
    assert_eq!(cases.len(), 9);
    assert_eq!(expected.len(), cases.len());
    assert_eq!(
        cases.iter().map(|case| &case.name).collect::<BTreeSet<_>>(),
        expected
            .iter()
            .map(|row| &row.name)
            .collect::<BTreeSet<_>>()
    );
    assert_eq!(
        expected
            .iter()
            .filter(|row| row.verdict == "reject")
            .count(),
        8
    );
    assert_eq!(
        expected
            .iter()
            .filter(|row| row.verdict == "accept")
            .count(),
        1
    );
}

#[test]
#[ignore = "requires the pinned TypeScript 5.9.3 compiler"]
fn recorded_reference_graph_matrix_matches_pinned_typescript_build() {
    let tsc = pinned_tsc();
    let root = temporary("oracle");
    let observed = cases()
        .iter()
        .map(|case| {
            let directory = root.join(&case.name);
            materialize(case, &directory);
            let output = Command::new(&tsc)
                .current_dir(&directory)
                .args(["--build", "tsconfig.json", "--pretty", "false"])
                .output()
                .unwrap();
            observation(case, &directory, &output)
        })
        .collect::<Vec<_>>();
    fs::remove_dir_all(root).unwrap();
    if env::var("BLUEICE_WRITE_PROJECT_REFERENCE_MATRIX").as_deref() == Ok("1") {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/project_references/reference-graph-matrix.tsv");
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
fn public_build_matches_pinned_reference_graph_verdicts_and_primary_positions() {
    let root = temporary("bluetsc");
    let mut failures = Vec::new();
    for (case, expected) in cases().iter().zip(matrix()) {
        let directory = root.join(&case.name);
        materialize(case, &directory);
        let output = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
            .current_dir(&directory)
            .args(["--build", "tsconfig.json", "--pretty", "false"])
            .output()
            .unwrap();
        let actual = observation(case, &directory, &output);
        if actual != expected {
            failures.push(format!(
                "{}: expected {expected:?}, got {actual:?}; {}",
                case.name,
                report(&output)
            ));
        }
    }
    fs::remove_dir_all(root).unwrap();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn non_build_project_command_retains_the_reference_refusal() {
    let root = temporary("non-build-refusal");
    let case = cases()
        .into_iter()
        .find(|case| case.name == "projref-valid-chain")
        .unwrap();
    materialize(&case, &root);
    let output = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
        .current_dir(&root)
        .args(["--project", "tsconfig.json", "--pretty", "false"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(report(&output).contains("references` requires the K.12 build graph"));
    let composite_only = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
        .current_dir(&root)
        .args(["--project", "dep/tsconfig.json", "--pretty", "false"])
        .output()
        .unwrap();
    assert!(!composite_only.status.success());
    assert!(report(&composite_only).contains("unsupported compiler option `composite`"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn build_denies_references_outside_the_owner_root() {
    let root = temporary("owner-refusal");
    let project = root.join("project");
    fs::create_dir_all(&project).unwrap();
    fs::write(
        project.join("tsconfig.json"),
        r#"{"files":["main.ts"],"compilerOptions":{"composite":true,"declaration":true},"references":[{"path":"../outside"}]}"#,
    )
    .unwrap();
    fs::write(project.join("main.ts"), "export const main = 1;\n").unwrap();
    let outside = root.join("outside");
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("tsconfig.json"), "not TypeScript JSON").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
        .current_dir(&project)
        .args(["--build", "tsconfig.json", "--pretty", "false"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(report(&output).contains("outside project root"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
#[ignore = "requires the pinned TypeScript 5.9.3 compiler and Node"]
fn valid_reference_graph_emits_the_same_executable_exports_as_typescript() {
    let case = cases()
        .into_iter()
        .find(|case| case.name == "projref-valid-chain")
        .unwrap();
    let root = temporary("emit-oracle");
    let typescript = root.join("typescript");
    let bluets = root.join("bluets");
    materialize(&case, &typescript);
    materialize(&case, &bluets);
    for (directory, executable) in [
        (&typescript, pinned_tsc()),
        (&bluets, PathBuf::from(env!("CARGO_BIN_EXE_bluetsc"))),
    ] {
        let output = Command::new(executable)
            .current_dir(directory)
            .args(["--build", "tsconfig.json", "--pretty", "false"])
            .output()
            .unwrap();
        assert!(output.status.success(), "{}", report(&output));
    }
    let execute = |directory: &Path| {
        let output = Command::new("node")
            .current_dir(directory)
            .args([
                "-e",
                "process.stdout.write(JSON.stringify({root:require('./root.js').root,dep:require('./dep/dep.js').dep}))",
            ])
            .output()
            .unwrap();
        assert!(output.status.success(), "{}", report(&output));
        output.stdout
    };
    assert_eq!(execute(&bluets), execute(&typescript));
    fs::remove_dir_all(root).unwrap();
}
