// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! G-T7/G-T2 receiver utilities through strict public project compilation.

use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Temporary(PathBuf);
impl Temporary {
    fn new() -> Self {
        let root = std::env::temp_dir().canonicalize().unwrap().join(format!(
            "bluets-receivers-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        Self(root)
    }
}
impl Drop for Temporary {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn diagnostic(value: &Value) -> Value {
    let value = value.get("typescript").unwrap_or(value);
    if value.is_null() {
        return json!({"missingNativeCounterpart": true});
    }
    json!({
        "code": value["code"], "message": value["message"],
        "file": value["span"].get("module"), "position": value["position"],
        "relatedInformation": value["relatedInformation"].as_array().into_iter().flatten().map(diagnostic).collect::<Vec<_>>()
    })
}

#[test]
fn utility_projects_match_complete_native_diagnostics_and_exact_declarations() {
    let input: Value =
        serde_json::from_str(include_str!("fixtures/receiver_utilities/cases.json")).unwrap();
    let reference: Value =
        serde_json::from_str(include_str!("fixtures/receiver_utilities/reference.json")).unwrap();
    assert_eq!(input["cases"].as_array().unwrap().len(), 70);
    assert_eq!(input["profiles"].as_array().unwrap().len(), 3);
    assert_eq!(reference["cases"].as_array().unwrap().len(), 210);
    let mut failures = Vec::new();
    let mut observed = 0;
    for profile in input["profiles"].as_array().unwrap() {
        for case in input["cases"].as_array().unwrap() {
            let expected = &reference["cases"][observed];
            let id = format!(
                "{}/{}",
                profile["id"].as_str().unwrap(),
                case["id"].as_str().unwrap()
            );
            assert_eq!(expected["id"], id);
            observed += 1;
            let temporary = Temporary::new();
            fs::write(
                temporary.0.join("main.ts"),
                case["source"].as_str().unwrap(),
            )
            .unwrap();
            let mut options = input["options"].as_object().unwrap().clone();
            options.extend(profile["compilerOptions"].as_object().unwrap().clone());
            options.insert("outDir".into(), json!("dist"));
            fs::write(
                temporary.0.join("tsconfig.json"),
                serde_json::to_vec(&json!({"compilerOptions":options,"files":["main.ts"]}))
                    .unwrap(),
            )
            .unwrap();
            let output = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
                .current_dir(&temporary.0)
                .args(["build", "--config", "tsconfig.json", "--diagnostics-json"])
                .output()
                .unwrap();
            let diagnostics = String::from_utf8(output.stderr)
                .unwrap()
                .lines()
                .map(|line| diagnostic(&serde_json::from_str::<Value>(line).unwrap()))
                .collect::<Vec<_>>();
            let mut declarations = BTreeMap::new();
            if temporary.0.join("dist").is_dir() {
                for entry in fs::read_dir(temporary.0.join("dist")).unwrap() {
                    let file = entry.unwrap().path();
                    let name = file.file_name().unwrap().to_string_lossy().to_string();
                    if name.ends_with(".d.ts") {
                        declarations.insert(name, fs::read_to_string(file).unwrap());
                    }
                }
            }
            if !expected["accepts"].as_bool().unwrap()
                && temporary.0.join("dist").is_dir()
                && fs::read_dir(temporary.0.join("dist"))
                    .unwrap()
                    .next()
                    .is_some()
            {
                failures.push(format!(
                    "{id}: a rejected noEmitOnError project published artifacts"
                ));
            }
            if output.status.success() != expected["accepts"].as_bool().unwrap()
                || json!(diagnostics) != expected["diagnostics"]
                || json!(declarations) != expected["declarations"]
            {
                failures.push(format!(
                    "{id}: expected {}; actual accepts={}, diagnostics={}, declarations={}",
                    expected,
                    output.status.success(),
                    json!(diagnostics),
                    json!(declarations)
                ));
            }
        }
    }
    assert_eq!(observed, 210);
    assert!(
        failures.is_empty(),
        "{} mismatches:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3, Acorn 8.15.0 and Node"]
fn utility_native_observations_consumers_syntax_and_runtime_are_reproducible() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for script in [
        "record_receiver_utilities.cjs",
        "verify_receiver_utilities.cjs",
    ] {
        let output = Command::new("node")
            .arg(
                root.join("development/browser_core/phase-18-bluets/tools")
                    .join(script),
            )
            .env(
                "BLUEICE_RECEIVER_UTILITIES_COMPILER",
                env!("CARGO_BIN_EXE_bluetsc"),
            )
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{script}: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn utility_names_preserve_owner_precedence_and_supply_no_runtime_authority() {
    use blueice_bluets::{compile, CompilerOptions, MapLoader, ModuleSource};
    for name in ["ThisParameterType", "OmitThisParameter"] {
        let source = format!("export const value: {name}<number> = {{ owner: 1 }};");
        let options = CompilerOptions {
            ambient_declaration_modules: vec![ModuleSource::new(
                "memory:///owner.d.ts",
                format!("type {name}<T> = {{owner: T}};"),
            )],
            ..CompilerOptions::default()
        };
        let result = compile(
            "memory:///main.ts",
            &MapLoader::from([ModuleSource::new("memory:///main.ts", &source)]),
            options,
        );
        assert!(!result.has_errors(), "{name}: {:?}", result.diagnostics);
        assert_eq!(
            result
                .project
                .modules
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            vec!["memory:///main.ts", "memory:///owner.d.ts"]
        );
        assert_eq!(result.output.unwrap().artifacts.len(), 1);
        let local = format!("type {name}<T> = {{owner: T}};\n{source}");
        let result = compile(
            "memory:///main.ts",
            &MapLoader::from([ModuleSource::new("memory:///main.ts", local)]),
            CompilerOptions::default(),
        );
        assert!(!result.has_errors(), "{name}: {:?}", result.diagnostics);
        let options = CompilerOptions {
            require_declared_global_calls: true,
            ..CompilerOptions::default()
        };
        let result = compile(
            "memory:///main.ts",
            &MapLoader::from([ModuleSource::new("memory:///main.ts", format!("{name}();"))]),
            options,
        );
        assert!(
            result.has_errors(),
            "a type-only library name acquired a value binding: {name}"
        );
    }
}
