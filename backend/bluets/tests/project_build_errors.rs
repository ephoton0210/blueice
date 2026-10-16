// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! K.12.1.5: a failed project blocks dependents but not independent branches.

use serde_json::json;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

#[path = "oracle_support/project_paths.rs"]
mod project_paths;

const MATRIX: &str = include_str!("fixtures/project_build_errors/build-error-matrix.tsv");
static NEXT: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Eq, PartialEq)]
struct Snapshot {
    scenario: String,
    exit: i32,
    events: Vec<String>,
}

impl Snapshot {
    fn row(&self) -> String {
        format!(
            "{}\t{}\t{}\n",
            self.scenario,
            self.exit,
            self.events.join(";")
        )
    }
}

fn matrix() -> Vec<Snapshot> {
    MATRIX
        .lines()
        .map(|line| {
            let mut fields = line.split('\t');
            Snapshot {
                scenario: fields.next().unwrap().into(),
                exit: fields.next().unwrap().parse().unwrap(),
                events: fields
                    .next()
                    .unwrap()
                    .split(';')
                    .map(str::to_string)
                    .collect(),
            }
        })
        .collect()
}

fn temporary(label: &str) -> PathBuf {
    let path = env::temp_dir().join(format!(
        "bluets-projerr-{label}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&path).unwrap();
    fs::canonicalize(path).unwrap()
}

fn project(root: &Path, name: &str, references: &[&str], valid: bool) {
    let directory = root.join(name);
    fs::create_dir_all(&directory).unwrap();
    let config = json!({
        "compilerOptions": {"composite": true, "declaration": true, "noEmitOnError": true},
        "files": ["main.ts"],
        "references": references.iter().map(|path| json!({"path": path})).collect::<Vec<_>>()
    });
    fs::write(
        directory.join("tsconfig.json"),
        serde_json::to_string_pretty(&config).unwrap() + "\n",
    )
    .unwrap();
    let mut source = String::new();
    for (index, reference) in references.iter().enumerate() {
        source.push_str(&format!(
            "import {{ value as dependency{index} }} from '{reference}/main';\n"
        ));
    }
    source.push_str(if valid {
        "export const value = 1;\n"
    } else {
        "export const value: string = 42;\n"
    });
    fs::write(directory.join("main.ts"), source).unwrap();
}

fn materialize(root: &Path, scenario: &str) {
    match scenario {
        "chain" => {
            project(root, "bad", &[], false);
            project(root, "app", &["../bad"], true);
            project(root, ".", &["./app"], true);
        }
        "parallel" => {
            project(root, "bad", &[], false);
            project(root, "good", &[], true);
            project(root, ".", &["./bad", "./good"], true);
        }
        "branch" => {
            project(root, "bad", &[], false);
            project(root, "left", &["../bad"], true);
            project(root, "good", &[], true);
            project(root, "right", &["../good"], true);
            project(root, ".", &["./left", "./right"], true);
        }
        other => panic!("unknown project error scenario {other}"),
    }
}

fn report(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn relative(root: &Path, path: &str) -> String {
    project_paths::relative_to(root, path)
}

fn events(output: &Output, root: &Path) -> Vec<String> {
    report(output)
        .lines()
        .filter_map(|line| {
            if let Some(project) = line.strip_prefix("Building project: ") {
                return Some(format!("build:{}", relative(root, project)));
            }
            if let Some(project) = line
                .split("Building project '")
                .nth(1)
                .and_then(|rest| rest.split('\'').next())
            {
                return Some(format!("build:{}", relative(root, project)));
            }
            let (kind, rest) = if let Some(rest) = line.split("Skipping build of project '").nth(1)
            {
                ("skip", rest)
            } else if let Some(rest) = line.split("Project '").nth(1) {
                if !rest.contains("can't be built because its dependency '") {
                    return None;
                }
                ("blocked", rest)
            } else {
                return None;
            };
            let project = relative(root, rest.split('\'').next()?);
            let dependency = relative(
                root,
                rest.split("its dependency '").nth(1)?.split('\'').next()?,
            );
            let reason = if rest.contains("was not built") {
                "not-built"
            } else if rest.contains("has errors") {
                "errors"
            } else {
                return None;
            };
            Some(format!("{kind}:{project}:{dependency}:{reason}"))
        })
        .collect()
}

fn run(executable: &Path, scenario: &str, label: &str) -> Snapshot {
    let root = temporary(label);
    materialize(&root, scenario);
    let output = Command::new(executable)
        .current_dir(&root)
        .args([
            "--build",
            "tsconfig.json",
            "--verbose",
            "--stopBuildOnErrors",
            "--pretty",
            "false",
        ])
        .output()
        .unwrap();
    let observed = Snapshot {
        scenario: scenario.into(),
        exit: output.status.code().unwrap_or(-1),
        events: events(&output, &root),
    };
    let diagnostics = report(&output);
    assert!(diagnostics.contains("error TS2322:"), "{diagnostics}");
    assert!(!diagnostics.contains("error TS6305:"), "{diagnostics}");
    assert!(!root.join("bad/main.d.ts").exists(), "{diagnostics}");
    assert!(!root.join("main.d.ts").exists(), "{diagnostics}");
    if scenario == "chain" {
        assert!(!root.join("app/main.d.ts").exists(), "{diagnostics}");
    }
    if scenario != "chain" {
        assert!(root.join("good/main.d.ts").is_file(), "{}", report(&output));
    }
    if scenario == "branch" {
        assert!(!root.join("left/main.d.ts").exists(), "{diagnostics}");
        assert!(root.join("right/main.d.ts").is_file(), "{diagnostics}");
    }
    fs::remove_dir_all(root).unwrap();
    observed
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3"]
fn record_pinned_project_error_behavior() {
    let tsc = PathBuf::from(env::var_os("BLUEICE_BLUETSC_ORACLE").unwrap());
    let version = Command::new(&tsc).arg("--version").output().unwrap();
    assert_eq!(
        String::from_utf8_lossy(&version.stdout).trim(),
        "Version 5.9.3"
    );
    let observed = ["chain", "parallel", "branch"]
        .iter()
        .map(|scenario| run(&tsc, scenario, &format!("oracle-{scenario}")))
        .collect::<Vec<_>>();
    if env::var("BLUEICE_WRITE_PROJECT_BUILD_ERRORS_MATRIX").as_deref() == Ok("1") {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/project_build_errors/build-error-matrix.tsv");
        fs::write(path, observed.iter().map(Snapshot::row).collect::<String>()).unwrap();
    } else {
        assert_eq!(observed, matrix());
    }
}

#[test]
fn public_build_project_error_behavior() {
    let bluetsc = Path::new(env!("CARGO_BIN_EXE_bluetsc"));
    let observed = ["chain", "parallel", "branch"]
        .iter()
        .map(|scenario| run(bluetsc, scenario, &format!("bluetsc-{scenario}")))
        .collect::<Vec<_>>();
    assert_eq!(observed, matrix());
}
