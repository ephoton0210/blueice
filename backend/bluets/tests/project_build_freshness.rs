// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! K.12.1.3: rebuild decisions from pinned TypeScript 5.9.3 `--build --verbose`.

use serde_json::json;
use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::Duration;

#[path = "oracle_support/project_paths.rs"]
mod project_paths;

const MATRIX: &str = include_str!("fixtures/project_build_freshness/build-freshness-matrix.tsv");
const STEPS: [&str; 10] = [
    "projfresh-cold",
    "projfresh-noop",
    "projfresh-lib-touch",
    "projfresh-lib-config-touch",
    "projfresh-lib-comment",
    "projfresh-lib-signature",
    "projfresh-side-signature",
    "projfresh-root-signature",
    "projfresh-lib-declaration-comment",
    "projfresh-second-noop",
];
static NEXT: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Eq, PartialEq)]
struct Snapshot {
    step: String,
    built: Vec<String>,
}

#[derive(Debug, Eq, PartialEq)]
struct ArtifactSnapshot {
    step: String,
    declarations: Vec<String>,
    values: Vec<i64>,
}

impl Snapshot {
    fn row(&self) -> String {
        let built = if self.built.is_empty() {
            "-".to_string()
        } else {
            self.built.join(";")
        };
        format!("{}\t{built}\n", self.step)
    }
}

fn matrix() -> Vec<Snapshot> {
    MATRIX
        .lines()
        .map(|line| {
            let (step, built) = line.split_once('\t').expect("step and rebuilt projects");
            Snapshot {
                step: step.into(),
                built: if built == "-" {
                    Vec::new()
                } else {
                    built.split(';').map(str::to_string).collect()
                },
            }
        })
        .collect()
}

fn temporary(label: &str) -> PathBuf {
    let path = env::temp_dir().join(format!(
        "bluets-projfresh-{label}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&path).unwrap();
    fs::canonicalize(path).unwrap()
}

fn materialize(root: &Path) {
    for (directory, references, source) in [
        (".", vec!["./app", "./side"], "export const value = 100;\n"),
        ("app", vec!["../lib"], "export const value = 20;\n"),
        ("lib", vec![], "export const value = 1;\n"),
        ("side", vec![], "export const value = 10;\n"),
    ] {
        let path = root.join(directory);
        fs::create_dir_all(&path).unwrap();
        let config = json!({
            "compilerOptions": {"composite": true, "declaration": true},
            "files": ["main.ts"],
            "references": references.iter().map(|path| json!({"path": path})).collect::<Vec<_>>()
        });
        fs::write(
            path.join("tsconfig.json"),
            serde_json::to_string_pretty(&config).unwrap() + "\n",
        )
        .unwrap();
        fs::write(path.join("main.ts"), source).unwrap();
    }
}

fn mutate(root: &Path, step: &str) {
    let (path, content) = match step {
        "projfresh-lib-touch" | "projfresh-lib-config-touch" => {
            let path = if step == "projfresh-lib-touch" {
                root.join("lib/main.ts")
            } else {
                root.join("lib/tsconfig.json")
            };
            let content = fs::read(&path).unwrap();
            fs::write(path, content).unwrap();
            return;
        }
        "projfresh-lib-comment" => (
            root.join("lib/main.ts"),
            "export const value = 1; // note\n",
        ),
        "projfresh-lib-signature" => (root.join("lib/main.ts"), "export const value = 2;\n"),
        "projfresh-side-signature" => (root.join("side/main.ts"), "export const value = 11;\n"),
        "projfresh-root-signature" => (root.join("main.ts"), "export const value = 101;\n"),
        "projfresh-lib-declaration-comment" => {
            let path = root.join("lib/main.d.ts");
            let source = fs::read_to_string(&path).unwrap();
            fs::write(path, source + "\n// declaration comment\n").unwrap();
            return;
        }
        "projfresh-noop" | "projfresh-second-noop" => return,
        "projfresh-cold" => unreachable!(),
        other => panic!("unknown freshness step {other}"),
    };
    fs::write(path, content).unwrap();
}

fn report(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn relative_project(project: &str, root: &Path) -> String {
    project_paths::relative_to(root, project)
}

fn rebuilt_projects(output: &Output, root: &Path, native: bool) -> Vec<String> {
    report(output)
        .lines()
        .filter_map(|line| {
            if native {
                return line.strip_prefix("Building project: ");
            }
            line.split("Building project '")
                .nth(1)
                .and_then(|rest| rest.split('\'').next())
        })
        .map(|project| relative_project(project, root))
        .collect()
}

fn run_sequence(
    executable: &Path,
    root: &Path,
    native: bool,
    mut observe: impl FnMut(&Path, &str),
) -> Vec<Snapshot> {
    materialize(root);
    STEPS
        .iter()
        .map(|step| {
            if *step != "projfresh-cold" {
                if !step.ends_with("noop") {
                    thread::sleep(Duration::from_millis(1100));
                }
                mutate(root, step);
            }
            let output = Command::new(executable)
                .current_dir(root)
                .args(["--build", "tsconfig.json", "--verbose", "--pretty", "false"])
                .output()
                .unwrap();
            assert!(output.status.success(), "{step}: {}", report(&output));
            let built = rebuilt_projects(&output, root, native);
            if *step == "projfresh-cold" {
                assert_eq!(built.len(), 4, "{}", report(&output));
            }
            observe(root, step);
            Snapshot {
                step: (*step).into(),
                built,
            }
        })
        .collect()
}

fn artifacts(root: &Path, step: &str) -> ArtifactSnapshot {
    let directories = [".", "app", "lib", "side"];
    let declarations = directories
        .iter()
        .map(|directory| fs::read_to_string(root.join(directory).join("main.d.ts")).unwrap())
        .collect();
    let scripts = directories
        .iter()
        .map(|directory| root.join(directory).join("main.js"))
        .collect::<Vec<_>>();
    let execution = Command::new("node")
        .arg("-e")
        .arg("process.stdout.write(JSON.stringify(process.argv.slice(1).map(path => require(path).value)))")
        .args(scripts)
        .output()
        .unwrap();
    assert!(execution.status.success(), "{step}: {}", report(&execution));
    ArtifactSnapshot {
        step: step.into(),
        declarations,
        values: serde_json::from_slice(&execution.stdout).unwrap(),
    }
}

fn pinned_tsc() -> PathBuf {
    let path = PathBuf::from(
        env::var_os("BLUEICE_BLUETSC_ORACLE")
            .expect("set BLUEICE_BLUETSC_ORACLE to pinned TypeScript 5.9.3 tsc"),
    );
    let output = Command::new(&path).arg("--version").output().unwrap();
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "Version 5.9.3"
    );
    path
}

#[test]
fn matrix_has_ten_unique_steps() {
    let recorded = matrix();
    assert_eq!(recorded.len(), STEPS.len());
    assert_eq!(
        recorded
            .iter()
            .map(|row| row.step.as_str())
            .collect::<Vec<_>>(),
        STEPS
    );
    assert_eq!(
        recorded
            .iter()
            .map(|row| &row.step)
            .collect::<BTreeSet<_>>()
            .len(),
        STEPS.len()
    );
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3"]
fn recorded_rebuilds_match_pinned_typescript() {
    let tsc = pinned_tsc();
    let root = temporary("oracle");
    let observed = run_sequence(&tsc, &root, false, |_, _| {});
    fs::remove_dir_all(root).unwrap();
    if env::var("BLUEICE_WRITE_BUILD_FRESHNESS_MATRIX").as_deref() == Ok("1") {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/project_build_freshness/build-freshness-matrix.tsv");
        fs::write(path, observed.iter().map(Snapshot::row).collect::<String>()).unwrap();
    } else {
        assert_eq!(observed, matrix());
    }
}

#[test]
fn public_build_rebuilds_match_pinned_typescript() {
    let root = temporary("bluetsc");
    let observed = run_sequence(
        Path::new(env!("CARGO_BIN_EXE_bluetsc")),
        &root,
        true,
        |_, _| {},
    );
    fs::remove_dir_all(root).unwrap();
    assert_eq!(observed, matrix());
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3 and Node"]
fn selective_rebuilds_execute_and_declare_like_typescript() {
    let tsc = pinned_tsc();
    let oracle_root = temporary("emit-oracle");
    let native_root = temporary("emit-bluetsc");
    let mut oracle_artifacts = Vec::new();
    let oracle_decisions = run_sequence(&tsc, &oracle_root, false, |root, step| {
        oracle_artifacts.push(artifacts(root, step));
    });
    let mut native_artifacts = Vec::new();
    let native_decisions = run_sequence(
        Path::new(env!("CARGO_BIN_EXE_bluetsc")),
        &native_root,
        true,
        |root, step| native_artifacts.push(artifacts(root, step)),
    );
    fs::remove_dir_all(oracle_root).unwrap();
    fs::remove_dir_all(native_root).unwrap();
    assert_eq!(oracle_decisions, matrix());
    assert_eq!(native_decisions, matrix());
    assert_eq!(native_artifacts, oracle_artifacts);
}

#[cfg(unix)]
#[test]
fn build_state_symlink_is_refused_before_read() {
    use std::os::unix::fs::symlink;

    let root = temporary("state-symlink");
    let outside = temporary("outside-state");
    materialize(&root);
    let executable = env!("CARGO_BIN_EXE_bluetsc");
    let initial = Command::new(executable)
        .current_dir(&root)
        .args(["--build", "tsconfig.json", "--pretty", "false"])
        .output()
        .unwrap();
    assert!(initial.status.success(), "{}", report(&initial));

    let marker = outside.join("private-state.json");
    fs::write(&marker, "private-state-sentinel").unwrap();
    let state = root.join("lib/tsconfig.bluetsc.tsbuildinfo");
    fs::remove_file(&state).unwrap();
    symlink(&marker, &state).unwrap();
    let rejected = Command::new(executable)
        .current_dir(&root)
        .args(["--build", "tsconfig.json", "--pretty", "false"])
        .output()
        .unwrap();
    assert!(!rejected.status.success());
    assert!(report(&rejected).contains("build state path is not a regular file"));
    assert!(!report(&rejected).contains("private-state-sentinel"));
    assert_eq!(
        fs::read_to_string(marker).unwrap(),
        "private-state-sentinel"
    );
    fs::remove_dir_all(root).unwrap();
    fs::remove_dir_all(outside).unwrap();
}

#[test]
fn forged_build_state_output_cannot_escape_project_root() {
    let root = temporary("state-escape");
    let outside = temporary("outside-output");
    materialize(&root);
    let executable = env!("CARGO_BIN_EXE_bluetsc");
    let initial = Command::new(executable)
        .current_dir(&root)
        .args(["--build", "tsconfig.json", "--pretty", "false"])
        .output()
        .unwrap();
    assert!(initial.status.success(), "{}", report(&initial));

    let marker = outside.join("private.js");
    fs::write(&marker, "private-output-sentinel").unwrap();
    let state_path = root.join("lib/tsconfig.bluetsc.tsbuildinfo");
    let mut state: serde_json::Value =
        serde_json::from_slice(&fs::read(&state_path).unwrap()).unwrap();
    state["outputs"] = json!([marker.to_string_lossy()]);
    fs::write(&state_path, serde_json::to_vec(&state).unwrap()).unwrap();

    let rebuilt = Command::new(executable)
        .current_dir(&root)
        .args(["--build", "tsconfig.json", "--verbose", "--pretty", "false"])
        .output()
        .unwrap();
    assert!(rebuilt.status.success(), "{}", report(&rebuilt));
    assert!(report(&rebuilt).contains("Building project: lib/tsconfig.json"));
    assert!(!report(&rebuilt).contains("private-output-sentinel"));
    assert_eq!(
        fs::read_to_string(marker).unwrap(),
        "private-output-sentinel"
    );
    fs::remove_dir_all(root).unwrap();
    fs::remove_dir_all(outside).unwrap();
}
