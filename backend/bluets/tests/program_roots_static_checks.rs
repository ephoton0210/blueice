// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Static declaration consumers acquire no executable import or output grant.

use serde_json::{json, Value};
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Temporary(PathBuf);
impl Temporary {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "bluets-program-static-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        Self(root)
    }

    fn write(&self, row: &Value, no_emit: bool) {
        for (name, source) in row["files"].as_object().unwrap() {
            let path = self.0.join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, source.as_str().unwrap()).unwrap();
        }
        let mut config = row["config"].clone();
        config["compilerOptions"]["noEmit"] = json!(no_emit);
        fs::write(
            self.0.join("tsconfig.json"),
            serde_json::to_vec_pretty(&config).unwrap(),
        )
        .unwrap();
    }

    fn run(&self, legacy: bool, check: bool) -> Output {
        let arguments = if legacy {
            vec![
                if check { "check" } else { "build" },
                "--config",
                "tsconfig.json",
                "--diagnostics-json",
            ]
        } else {
            vec!["--project", "tsconfig.json", "--diagnostics-json"]
        };
        let output = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
            .args(arguments)
            .current_dir(&self.0)
            .output()
            .unwrap();
        fs::write(self.0.join("stdout.log"), &output.stdout).unwrap();
        fs::write(self.0.join("stderr.log"), &output.stderr).unwrap();
        output
    }
}
impl Drop for Temporary {
    fn drop(&mut self) {
        if std::thread::panicking() {
            eprintln!("Retained static-check fixture: {}", self.0.display());
        } else {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }
}

fn fixture() -> Value {
    serde_json::from_str(include_str!("fixtures/program_roots/static-consumers.json")).unwrap()
}

fn diagnostic(row: &Value) -> Value {
    assert!(row["code"].is_number(), "missing Native counterpart: {row}");
    json!({
        "code":row["code"],"message":row["message"],"file":row["span"]["module"],
        "position":row["position"],
        "relatedInformation":row["relatedInformation"].as_array().into_iter().flatten()
            .map(diagnostic).collect::<Vec<_>>()
    })
}

#[test]
fn declaration_consumers_match_native_without_publishing_on_either_cli() {
    let input = fixture();
    for legacy in [false, true] {
        for row in input["cases"].as_array().unwrap() {
            let temporary = Temporary::new();
            temporary.write(row, true);
            let output = temporary.run(legacy, true);
            let diagnostics = String::from_utf8(output.stderr)
                .unwrap()
                .lines()
                .map(|line| {
                    let raw: Value = serde_json::from_str(line).unwrap();
                    diagnostic(&raw["typescript"])
                })
                .collect::<Vec<_>>();
            assert_eq!(
                json!({"accepts":output.status.success(),"diagnostics":diagnostics}),
                row["expected"],
                "{}; legacy={legacy}",
                row["id"]
            );
            assert!(!temporary.0.join("dist").exists(), "static check published");
        }
    }
}

#[test]
fn the_same_declarations_still_refuse_executable_publication() {
    let input = fixture();
    let cases = input["cases"].as_array().unwrap();
    for legacy in [false, true] {
        for row in cases.iter().filter(|row| {
            row["id"].as_str().unwrap().starts_with("es5-cjs-none/")
                && row["expected"]["accepts"] == true
        }) {
            let temporary = Temporary::new();
            temporary.write(row, false);
            let output = temporary.run(legacy, false);
            assert!(!output.status.success(), "build accepted {}", row["id"]);
            assert!(
                String::from_utf8(output.stderr)
                    .unwrap()
                    .contains("declaration modules are type-only"),
                "missing executable import refusal for {}",
                row["id"]
            );
            assert!(!temporary.0.join("dist").exists());
        }
    }
}
