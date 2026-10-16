// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! K.12.1.6: referenced projects stay inside the canonical owner root.

use serde_json::json;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

const MATRIX: &str = include_str!("fixtures/project_reference_owner_gate/owner-gate-matrix.tsv");
static NEXT: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Eq, PartialEq)]
struct Snapshot {
    scenario: String,
    verdict: String,
    built: Vec<String>,
}

#[cfg(unix)]
impl Snapshot {
    fn row(&self) -> String {
        format!(
            "{}\t{}\t{}\n",
            self.scenario,
            self.verdict,
            self.built.join(";")
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
                verdict: fields.next().unwrap().into(),
                built: fields
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
        "bluets-projowner-{label}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(path.join("owner")).unwrap();
    fs::canonicalize(path).unwrap()
}

fn project(path: &Path, references: &[&str], source: &str) {
    fs::create_dir_all(path).unwrap();
    let config = json!({
        "compilerOptions": {"composite": true, "declaration": true, "module": "commonjs"},
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

fn materialize(base: &Path, escape: bool) -> PathBuf {
    let owner = base.join("owner");
    let dependency = if escape {
        base.join("outside")
    } else {
        owner.join("dep")
    };
    project(&dependency, &[], "export const value = 1;\n");
    fs::write(dependency.join("marker"), "outside-marker\n").unwrap();
    if escape {
        #[cfg(unix)]
        std::os::unix::fs::symlink(&dependency, owner.join("dep")).unwrap();
    }
    project(
        &owner,
        &["./dep"],
        "import { value } from './dep/main';\nexport const result: number = value;\n",
    );
    owner
}

fn report(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn build(executable: &Path, owner: &Path) -> Output {
    Command::new(executable)
        .current_dir(owner)
        .args(["--build", "tsconfig.json", "--verbose", "--pretty", "false"])
        .output()
        .unwrap()
}

fn snapshot(scenario: &str, output: &Output, owner: &Path) -> Snapshot {
    let built = report(output)
        .lines()
        .filter_map(|line| {
            line.strip_prefix("Building project: ").or_else(|| {
                line.split("Building project '")
                    .nth(1)
                    .and_then(|rest| rest.split('\'').next())
            })
        })
        .map(|path| {
            let path = Path::new(path);
            let relative = if let Ok(relative) = path.strip_prefix(owner) {
                relative.to_string_lossy().into_owned()
            } else if let Ok(relative) = path.strip_prefix(owner.parent().unwrap()) {
                format!("../{}", relative.to_string_lossy())
            } else {
                path.to_string_lossy().into_owned()
            };
            relative.trim_start_matches("./").replace('\\', "/")
        })
        .collect();
    Snapshot {
        scenario: scenario.into(),
        verdict: if output.status.success() {
            "accept".into()
        } else {
            "reject".into()
        },
        built,
    }
}

#[cfg(unix)]
#[test]
#[ignore = "requires pinned TypeScript 5.9.3"]
fn pinned_build_records_owner_gate_difference() {
    let tsc = PathBuf::from(env::var_os("BLUEICE_BLUETSC_ORACLE").unwrap());
    let version = Command::new(&tsc).arg("--version").output().unwrap();
    assert_eq!(
        String::from_utf8_lossy(&version.stdout).trim(),
        "Version 5.9.3"
    );
    let scenarios = [("internal", false), ("symlink-escape", true)];
    let observed = scenarios
        .iter()
        .map(|(name, escape)| {
            let base = temporary(&format!("oracle-{name}"));
            let owner = materialize(&base, *escape);
            let output = build(&tsc, &owner);
            let observation = snapshot(name, &output, &owner);
            fs::remove_dir_all(base).unwrap();
            observation
        })
        .collect::<Vec<_>>();
    if env::var("BLUEICE_WRITE_PROJECT_OWNER_GATE_MATRIX").as_deref() == Ok("1") {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/project_reference_owner_gate/owner-gate-matrix.tsv");
        fs::write(path, observed.iter().map(Snapshot::row).collect::<String>()).unwrap();
    } else {
        assert_eq!(observed, matrix());
    }
}

#[test]
fn internal_reference_still_builds_and_executes() {
    let base = temporary("bluetsc-internal");
    let owner = materialize(&base, false);
    let output = build(Path::new(env!("CARGO_BIN_EXE_bluetsc")), &owner);
    assert!(output.status.success(), "{}", report(&output));
    assert_eq!(snapshot("internal", &output, &owner), matrix()[0]);
    assert!(owner.join("dep/main.d.ts").is_file());
    assert!(owner.join("main.d.ts").is_file());
    let execution = Command::new("node")
        .arg("-e")
        .arg("process.stdout.write(String(require(process.argv[1]).result))")
        .arg(owner.join("main.js"))
        .output()
        .unwrap();
    assert!(execution.status.success(), "{}", report(&execution));
    assert_eq!(execution.stdout, b"1");
    fs::remove_dir_all(base).unwrap();
}

#[cfg(unix)]
#[test]
fn symlinked_reference_outside_owner_root_is_refused_before_read_or_emit() {
    let base = temporary("bluetsc-escape");
    let owner = materialize(&base, true);
    let output = build(Path::new(env!("CARGO_BIN_EXE_bluetsc")), &owner);
    let diagnostic = report(&output);
    assert!(!output.status.success(), "{diagnostic}");
    assert!(
        diagnostic.contains("resolves outside project root"),
        "{diagnostic}"
    );
    assert!(diagnostic.len() < 1024, "{diagnostic}");
    assert!(!base.join("outside/main.d.ts").exists());
    assert!(!base.join("outside/main.js").exists());
    assert!(!owner.join("main.d.ts").exists());
    assert_eq!(
        fs::read_to_string(base.join("outside/marker")).unwrap(),
        "outside-marker\n"
    );
    fs::remove_dir_all(base).unwrap();
}
