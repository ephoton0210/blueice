// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! K.12.2.5: file-change builds and bounded watch process lifecycle.

use serde_json::json;
use std::env;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

#[path = "oracle_support/project_paths.rs"]
mod project_paths;

const MATRIX: &str = include_str!("fixtures/project_build_watch/build-watch-matrix.tsv");
const STEPS: [&str; 5] = ["cold", "edit", "add", "burst", "delete"];
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
        "bluets-projwatch-{label}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&root).unwrap();
    fs::canonicalize(root).unwrap()
}

fn materialize(root: &Path) {
    fs::create_dir_all(root.join("dep/src")).unwrap();
    let dep = json!({
        "compilerOptions": {
            "composite": true,
            "declaration": true,
            "module": "commonjs",
            "rootDir": "src",
            "outDir": "out"
        },
        "include": ["src/**/*.ts"]
    });
    let main = json!({
        "compilerOptions": {"composite": true, "declaration": true, "module": "commonjs"},
        "files": ["main.ts"],
        "references": [{"path": "./dep"}]
    });
    fs::write(
        root.join("dep/tsconfig.json"),
        serde_json::to_string_pretty(&dep).unwrap() + "\n",
    )
    .unwrap();
    fs::write(root.join("dep/src/base.ts"), "export const value = 1;\n").unwrap();
    fs::write(
        root.join("tsconfig.json"),
        serde_json::to_string_pretty(&main).unwrap() + "\n",
    )
    .unwrap();
    fs::write(
        root.join("main.ts"),
        "import { value } from './dep/src/base';\nexport const result = value + 1;\n",
    )
    .unwrap();
}

fn mutate(root: &Path, step: &str) {
    match step {
        "edit" => fs::write(root.join("dep/src/base.ts"), "export const value = 2;\n").unwrap(),
        "add" => fs::write(root.join("dep/src/extra.ts"), "export const extra = 3;\n").unwrap(),
        "burst" => {
            for value in [4, 5, 6] {
                fs::write(
                    root.join("dep/src/base.ts"),
                    format!("export const value = {value};\n"),
                )
                .unwrap();
            }
        }
        "delete" => fs::remove_file(root.join("dep/src/extra.ts")).unwrap(),
        _ => unreachable!(),
    }
}

fn report(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn row(step: &str, output: &Output, root: &Path) -> Row {
    assert!(output.status.success(), "{step}: {}", report(output));
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
    Row {
        step: step.to_string(),
        built,
    }
}

fn build(executable: &Path, root: &Path, step: &str) -> Row {
    let output = Command::new(executable)
        .current_dir(root)
        .args(["--build", "tsconfig.json", "--verbose", "--pretty", "false"])
        .output()
        .unwrap();
    row(step, &output, root)
}

fn sequence(executable: &Path, label: &str, pause_for_tsc: bool) -> Vec<Row> {
    let root = temporary(label);
    materialize(&root);
    let mut observed = vec![build(executable, &root, "cold")];
    for step in STEPS.iter().skip(1) {
        if pause_for_tsc {
            thread::sleep(Duration::from_millis(1100));
        }
        mutate(&root, step);
        observed.push(build(executable, &root, step));
    }
    fs::remove_dir_all(root).unwrap();
    observed
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
fn matrix_has_five_ordered_steps() {
    assert_eq!(
        matrix()
            .iter()
            .map(|row| row.step.as_str())
            .collect::<Vec<_>>(),
        STEPS
    );
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3"]
fn pinned_build_edit_add_burst_delete_order() {
    let tsc = PathBuf::from(env::var_os("BLUEICE_BLUETSC_ORACLE").unwrap());
    let version = Command::new(&tsc).arg("--version").output().unwrap();
    assert_eq!(
        String::from_utf8_lossy(&version.stdout).trim(),
        "Version 5.9.3"
    );
    let observed = sequence(&tsc, "oracle", true);
    if env::var("BLUEICE_WRITE_PROJECT_WATCH_MATRIX").as_deref() == Ok("1") {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/project_build_watch/build-watch-matrix.tsv");
        fs::write(path, observed.iter().map(Row::record).collect::<String>()).unwrap();
    } else {
        assert_eq!(observed, matrix());
    }
}

#[test]
fn public_build_edit_add_burst_delete_order() {
    let observed = sequence(Path::new(env!("CARGO_BIN_EXE_bluetsc")), "bluetsc", false);
    let mut expected = matrix();
    expected
        .last_mut()
        .unwrap()
        .built
        .push("tsconfig.json".into());
    assert_eq!(observed, expected);
}

#[test]
fn deleting_a_selected_source_removes_its_old_outputs() {
    let bluetsc = Path::new(env!("CARGO_BIN_EXE_bluetsc"));
    let root = temporary("delete-output");
    materialize(&root);
    let _ = build(bluetsc, &root, "cold");
    mutate(&root, "add");
    let _ = build(bluetsc, &root, "add");
    assert!(root.join("dep/out/extra.js").is_file());
    assert!(root.join("dep/out/extra.d.ts").is_file());
    mutate(&root, "delete");
    let _ = build(bluetsc, &root, "delete");
    assert!(!root.join("dep/out/extra.js").exists());
    assert!(!root.join("dep/out/extra.d.ts").exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn deleting_a_source_preserves_changed_old_output() {
    let bluetsc = Path::new(env!("CARGO_BIN_EXE_bluetsc"));
    let root = temporary("changed-output");
    materialize(&root);
    let _ = build(bluetsc, &root, "cold");
    mutate(&root, "add");
    let _ = build(bluetsc, &root, "add");
    let changed = root.join("dep/out/extra.js");
    fs::write(&changed, "// user-modified output\n").unwrap();
    mutate(&root, "delete");
    let _ = build(bluetsc, &root, "delete");
    assert_eq!(
        fs::read_to_string(changed).unwrap(),
        "// user-modified output\n"
    );
    assert!(!root.join("dep/out/extra.d.ts").exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn legacy_v3_state_rebuilds_with_output_hashes() {
    let bluetsc = Path::new(env!("CARGO_BIN_EXE_bluetsc"));
    let root = temporary("v3-migration");
    materialize(&root);
    let _ = build(bluetsc, &root, "cold");
    let path = root.join("dep/tsconfig.bluetsc.tsbuildinfo");
    let mut state: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    state["version"] = json!(3);
    state.as_object_mut().unwrap().remove("output_hashes");
    fs::write(&path, serde_json::to_vec(&state).unwrap()).unwrap();
    let output = Command::new(bluetsc)
        .current_dir(&root)
        .args(["--build", "tsconfig.json", "--verbose"])
        .output()
        .unwrap();
    let rebuilt = row("legacy", &output, &root);
    assert_eq!(rebuilt.built, vec!["dep/tsconfig.json", "tsconfig.json"]);
    assert!(report(&output).contains("invalid or foreign .tsbuildinfo"));
    let state: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(state["version"], 17);
    assert!(state["output_hashes"].as_object().unwrap().len() >= 2);
    fs::remove_dir_all(root).unwrap();
}

struct WatchProcess {
    child: Child,
    stdin: Option<ChildStdin>,
    lines: Receiver<String>,
    readers: Vec<thread::JoinHandle<()>>,
    seen: Vec<String>,
}

fn read_watch_lines<R: Read + Send + 'static>(
    stream: R,
    sender: Sender<String>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        for line in BufReader::new(stream).lines() {
            let Ok(line) = line else { break };
            if sender.send(line).is_err() {
                break;
            }
        }
    })
}

impl WatchProcess {
    fn spawn(root: &Path) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
            .current_dir(root)
            .args([
                "--build",
                "tsconfig.json",
                "--watch",
                "--verbose",
                "--pretty",
                "false",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take();
        let (sender, lines) = mpsc::channel();
        let readers = vec![
            read_watch_lines(child.stdout.take().unwrap(), sender.clone()),
            read_watch_lines(child.stderr.take().unwrap(), sender),
        ];
        Self {
            child,
            stdin,
            lines,
            readers,
            seen: Vec::new(),
        }
    }

    fn wait_cycle(&mut self, index: usize) -> Vec<String> {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut built = Vec::new();
        loop {
            assert!(
                Instant::now() < deadline,
                "watch cycle {index} timed out: {:?}",
                self.seen
            );
            if let Some(status) = self.child.try_wait().unwrap() {
                panic!(
                    "watch exited before cycle {index}: {status}; {:?}",
                    self.seen
                );
            }
            if let Ok(line) = self.lines.recv_timeout(Duration::from_millis(200)) {
                self.seen.push(line.clone());
                if let Some(project) = line.strip_prefix("Building project: ") {
                    built.push(project.replace('\\', "/"));
                }
                if line == format!("Watch build {index} complete") {
                    return built;
                }
            }
        }
    }

    fn assert_quiet(&mut self) {
        let deadline = Instant::now() + Duration::from_millis(400);
        while Instant::now() < deadline {
            if let Ok(line) = self.lines.recv_timeout(Duration::from_millis(50)) {
                self.seen.push(line.clone());
                assert!(
                    !line.starts_with("Watch build "),
                    "extra watch cycle: {line}"
                );
                assert!(
                    !line.starts_with("File change detected"),
                    "extra event: {line}"
                );
            }
        }
    }

    fn stop(mut self) {
        let mut stdin = self.stdin.take().unwrap();
        stdin.write_all(b"q\n").unwrap();
        stdin.flush().unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                assert!(status.success(), "watch shutdown status: {status}");
                break;
            }
            assert!(Instant::now() < deadline, "watch did not shut down");
            thread::sleep(Duration::from_millis(25));
        }
        for reader in self.readers.drain(..) {
            reader.join().unwrap();
        }
    }
}

impl Drop for WatchProcess {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

#[test]
fn watch_reemits_edit_add_delete_debounces_burst_and_stops() {
    let root = temporary("real-watch");
    materialize(&root);
    let mut watch = WatchProcess::spawn(&root);
    let mut expected = matrix();
    expected
        .last_mut()
        .unwrap()
        .built
        .push("tsconfig.json".into());
    assert_eq!(watch.wait_cycle(0), expected[0].built);
    for (index, step) in STEPS.iter().enumerate().skip(1) {
        mutate(&root, step);
        assert_eq!(watch.wait_cycle(index), expected[index].built, "{step}");
        match *step {
            "edit" => assert!(fs::read_to_string(root.join("dep/out/base.js"))
                .unwrap()
                .contains("= 2")),
            "add" => {
                assert!(root.join("dep/out/extra.js").is_file());
                assert!(root.join("dep/out/extra.d.ts").is_file());
            }
            "burst" => {
                assert!(fs::read_to_string(root.join("dep/out/base.js"))
                    .unwrap()
                    .contains("= 6"));
                watch.assert_quiet();
            }
            "delete" => {
                assert!(!root.join("dep/out/extra.js").exists());
                assert!(!root.join("dep/out/extra.d.ts").exists());
            }
            _ => unreachable!(),
        }
    }
    watch.stop();
    fs::remove_dir_all(root).unwrap();
}
