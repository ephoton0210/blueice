// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! K.12.2.3: incremental artifacts equal a clean build after seeded edits.

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

const STEPS: usize = 24;
const PROJECTS: [&str; 3] = ["p0", "p1", "."];
const MATRIX: &str =
    include_str!("fixtures/project_build_equivalence/build-equivalence-matrix.tsv");
static NEXT: AtomicU64 = AtomicU64::new(0);

#[derive(Clone)]
struct Model {
    values: [u32; 3],
    comments: [u32; 3],
    es2020: [bool; 3],
}

impl Default for Model {
    fn default() -> Self {
        Self {
            values: [1, 2, 3],
            comments: [0; 3],
            es2020: [false; 3],
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Row {
    step: usize,
    rebuilt: Vec<String>,
}

impl Row {
    fn record(&self) -> String {
        format!("{}\t{}\n", self.step, self.rebuilt.join(";"))
    }
}

fn temporary(label: &str) -> PathBuf {
    let root = env::temp_dir().join(format!(
        "bluets-projequiv-{label}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&root).unwrap();
    fs::canonicalize(root).unwrap()
}

fn source(model: &Model, index: usize) -> String {
    let body = match index {
        0 => format!("export const value = {};", model.values[0]),
        1 => format!(
            "import {{ value }} from '../p0/main';\nexport const value1 = value + {};",
            model.values[1]
        ),
        2 => format!(
            "import {{ value1 }} from './p1/main';\nexport const result = value1 + {};",
            model.values[2]
        ),
        _ => unreachable!(),
    };
    format!("{body} // edit {}\n", model.comments[index])
}

fn config(model: &Model, index: usize) -> String {
    let references = match index {
        0 => Vec::new(),
        1 => vec![json!({"path": "../p0"})],
        2 => vec![json!({"path": "./p1"})],
        _ => unreachable!(),
    };
    serde_json::to_string_pretty(&json!({
        "compilerOptions": {
            "composite": true,
            "declaration": true,
            "module": "commonjs",
            "target": if model.es2020[index] { "ES2020" } else { "ES2022" }
        },
        "files": ["main.ts"],
        "references": references
    }))
    .unwrap()
        + "\n"
}

fn materialize(root: &Path, model: &Model) {
    for (index, project) in PROJECTS.iter().enumerate() {
        let directory = root.join(project);
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join("main.ts"), source(model, index)).unwrap();
        fs::write(directory.join("tsconfig.json"), config(model, index)).unwrap();
    }
}

fn next_edit(seed: &mut u64) -> (usize, usize) {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 7;
    *seed ^= *seed << 17;
    ((*seed % 3) as usize, ((*seed >> 32) % 3) as usize)
}

fn mutate(root: &Path, model: &mut Model, project: usize, kind: usize) {
    let directory = root.join(PROJECTS[project]);
    if kind == 2 {
        model.es2020[project] = !model.es2020[project];
        fs::write(directory.join("tsconfig.json"), config(model, project)).unwrap();
    } else {
        if kind == 0 {
            model.values[project] += 1;
        } else {
            model.comments[project] += 1;
        }
        fs::write(directory.join("main.ts"), source(model, project)).unwrap();
    }
}

fn report(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn build(executable: &Path, root: &Path) -> Vec<String> {
    let output = Command::new(executable)
        .current_dir(root)
        .args(["--build", "tsconfig.json", "--verbose", "--pretty", "false"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", report(&output));
    report(&output)
        .lines()
        .filter_map(|line| {
            line.strip_prefix("Building project: ").or_else(|| {
                line.split("Building project '")
                    .nth(1)
                    .and_then(|rest| rest.split('\'').next())
            })
        })
        .map(|project| project_paths::relative_to(root, project))
        .collect()
}

fn artifacts(root: &Path) -> Vec<(String, Vec<u8>)> {
    PROJECTS
        .iter()
        .flat_map(|project| {
            ["main.js", "main.d.ts"].map(|name| {
                let relative = format!("{project}/{name}");
                (relative.clone(), fs::read(root.join(relative)).unwrap())
            })
        })
        .collect()
}

fn run(executable: &Path, label: &str, pause_for_tsc: bool) -> Vec<Row> {
    let incremental = temporary(&format!("{label}-incremental"));
    let mut model = Model::default();
    materialize(&incremental, &model);
    let mut rows = vec![Row {
        step: 0,
        rebuilt: build(executable, &incremental),
    }];
    let mut visited = BTreeSet::new();
    let mut kinds = BTreeSet::new();
    let mut seed = 0x6a09_e667_f3bc_c909u64;
    for step in 1..=STEPS {
        let (project, kind) = next_edit(&mut seed);
        visited.insert(project);
        kinds.insert(kind);
        if pause_for_tsc {
            thread::sleep(Duration::from_millis(1100));
        }
        mutate(&incremental, &mut model, project, kind);
        let rebuilt = build(executable, &incremental);
        let clean = temporary(&format!("{label}-clean-{step}"));
        materialize(&clean, &model);
        assert_eq!(
            build(executable, &clean).len(),
            PROJECTS.len(),
            "step {step}: clean build must include all projects"
        );
        assert_eq!(
            artifacts(&incremental),
            artifacts(&clean),
            "step {step}, project {}, edit kind {kind}",
            PROJECTS[project]
        );
        fs::remove_dir_all(clean).unwrap();
        rows.push(Row { step, rebuilt });
    }
    assert_eq!(visited.len(), PROJECTS.len());
    assert_eq!(kinds.len(), 3);
    fs::remove_dir_all(incremental).unwrap();
    rows
}

fn matrix() -> Vec<Row> {
    MATRIX
        .lines()
        .map(|line| {
            let (step, projects) = line.split_once('\t').unwrap();
            Row {
                step: step.parse().unwrap(),
                rebuilt: if projects == "-" {
                    Vec::new()
                } else {
                    projects.split(';').map(str::to_string).collect()
                },
            }
        })
        .collect()
}

#[test]
fn recorded_matrix_has_a_cold_build_and_24_edits() {
    let rows = matrix();
    assert_eq!(rows.len(), STEPS + 1);
    assert_eq!(
        rows.iter().map(|row| row.step).collect::<Vec<_>>(),
        (0..=STEPS).collect::<Vec<_>>()
    );
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3"]
fn pinned_seeded_edits_match_clean_outputs_and_ordered_rebuilds() {
    let tsc = PathBuf::from(env::var_os("BLUEICE_BLUETSC_ORACLE").unwrap());
    let version = Command::new(&tsc).arg("--version").output().unwrap();
    assert_eq!(
        String::from_utf8_lossy(&version.stdout).trim(),
        "Version 5.9.3"
    );
    let rows = run(&tsc, "oracle", true);
    if env::var("BLUEICE_WRITE_PROJECT_EQUIVALENCE_MATRIX").as_deref() == Ok("1") {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/project_build_equivalence/build-equivalence-matrix.tsv");
        fs::write(path, rows.iter().map(Row::record).collect::<String>()).unwrap();
    } else {
        assert_eq!(rows, matrix());
    }
}

#[test]
fn bluetsc_seeded_edits_match_clean_outputs_and_pinned_order() {
    let rows = run(Path::new(env!("CARGO_BIN_EXE_bluetsc")), "bluetsc", false);
    assert_eq!(rows, matrix());
}
