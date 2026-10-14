// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Primary coordinates at the configuration and native project boundaries.
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::{env, fs, path::Path, process::Command};

fn copy(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        if entry.file_type().unwrap().is_dir() {
            copy(&entry.path(), &to.join(entry.file_name()));
        } else {
            fs::copy(entry.path(), to.join(entry.file_name())).unwrap();
        }
    }
}

#[test]
fn project_position_record_covers_every_existing_matrix() {
    let record: Value =
        serde_json::from_str(include_str!("fixtures/diagnostics/project-positions.json")).unwrap();
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let actual = record["cases"]
        .as_array()
        .unwrap()
        .iter()
        .map(|case| case["id"].as_str().unwrap().to_string())
        .collect::<BTreeSet<_>>();
    let mut expected = BTreeSet::new();
    for (directory, filename, prefix) in [
        ("project_config", "config-checker-matrix.tsv", "config"),
        ("cli_surface", "cli-checker-matrix.tsv", "cli"),
        (
            "option_combinations",
            "options-checker-matrix.tsv",
            "options",
        ),
    ] {
        for line in fs::read_to_string(fixtures.join(directory).join(filename))
            .unwrap()
            .lines()
        {
            let (id, _) = line.split_once('\t').unwrap();
            let id = if prefix == "cli" {
                id.strip_prefix("cli-").unwrap()
            } else {
                id
            };
            expected.insert(format!("{prefix}:{id}"));
        }
    }
    assert_eq!(actual.len(), record["cases"].as_array().unwrap().len());
    assert_eq!(actual, expected);
}

#[test]
fn public_coordinates_count_utf16_and_original_line_breaks() {
    use blueice_bluets::{compile, CompilerOptions, MapLoader, ModuleSource};
    let record: Value =
        serde_json::from_str(include_str!("fixtures/diagnostics/project-positions.json")).unwrap();
    for case in record["coordinateCases"].as_array().unwrap() {
        let source = case["source"].as_str().unwrap();
        let result = compile(
            "memory:///main.ts",
            &MapLoader::from([ModuleSource::new("memory:///main.ts", source)]),
            CompilerOptions::default(),
        );
        assert_eq!(result.diagnostics.len(), 1);
        let diagnostic = result.diagnostics[0].to_json();
        assert_eq!(diagnostic["typescript"]["code"], case["first"]["code"]);
        assert_eq!(
            diagnostic["typescript"]["position"],
            case["first"]["position"]
        );
        let start = source.find("absent").unwrap();
        assert_eq!(diagnostic["span"]["start"], start);
        assert_eq!(diagnostic["span"]["end"], start + "absent".len());
        assert!(!diagnostic.to_string().contains("🧊"));
    }
}

#[test]
fn primary_project_positions_match_every_existing_matrix() {
    let record: Value =
        serde_json::from_str(include_str!("fixtures/diagnostics/project-positions.json")).unwrap();
    assert_eq!(record["version"], "5.9.3");
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let root = env::temp_dir().join(format!("bluets-project-positions-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let mut failures = Vec::new();
    let mut counts = [0; 3];
    for (index, case) in record["cases"].as_array().unwrap().iter().enumerate() {
        let directory = root.join(index.to_string());
        let matrix = case["matrix"].as_str().unwrap();
        match matrix {
            "config" => {
                counts[0] += 1;
                copy(
                    &fixtures
                        .join("project_config")
                        .join(case["entry"].as_str().unwrap()),
                    &directory,
                );
            }
            "cli" => {
                counts[1] += 1;
                copy(&fixtures.join("cli_surface/project"), &directory);
                let filename = directory.join("tsconfig.json");
                let mut config: Value =
                    serde_json::from_slice(&fs::read(&filename).unwrap()).unwrap();
                match case["variant"].as_str().unwrap_or("") {
                    "error" => fs::write(
                        directory.join("src/value.ts"),
                        "export const value: number = \"wrong\";\n",
                    )
                    .unwrap(),
                    "config-noemit" => config["compilerOptions"]["noEmit"] = json!(true),
                    "config-listfiles" => config["compilerOptions"]["listFiles"] = json!(true),
                    "selectors" => {
                        config["include"] = json!(["src/**/*.ts"]);
                        config["exclude"] = json!(["src/ignore*"]);
                    }
                    "in-place" => {
                        config["compilerOptions"]
                            .as_object_mut()
                            .unwrap()
                            .remove("outDir");
                    }
                    _ => {}
                }
                fs::write(filename, serde_json::to_vec_pretty(&config).unwrap()).unwrap();
            }
            "options" => {
                counts[2] += 1;
                fs::create_dir_all(directory.join("src")).unwrap();
                for (filename, source) in record["optionSources"].as_object().unwrap() {
                    fs::write(directory.join(filename), source.as_str().unwrap()).unwrap();
                }
                fs::write(
                    directory.join("tsconfig.json"),
                    serde_json::to_vec(
                        &json!({"compilerOptions": case["options"], "files": ["src/main.ts"]}),
                    )
                    .unwrap(),
                )
                .unwrap();
            }
            _ => panic!("unknown matrix"),
        }
        let mut args = case["args"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap())
            .collect::<Vec<_>>();
        // Position replay does not publish artifacts. Existing CLI emit/run
        // observations remain independently covered by cli_surface.rs.
        if case["accepts"] == true && !args.contains(&"--showConfig") {
            args.push("--noEmit");
        }
        args.push("--diagnostics-json");
        let output = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
            .current_dir(&directory)
            .args(args)
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&output.stderr);
        let diagnostics = text
            .lines()
            .map(serde_json::from_str::<Value>)
            .collect::<Result<Vec<_>, _>>();
        let id = case["id"].as_str().unwrap();
        match diagnostics {
            Ok(actual) if case["first"].is_null() => {
                if !actual.is_empty() || !output.status.success() {
                    failures.push(format!("{id}: unexpected diagnostic {text}"));
                }
            }
            Ok(actual) => {
                let primary = actual.first().map(|value| &value["typescript"]);
                if !primary.is_some_and(|value| {
                    value["code"] == case["first"]["code"]
                        && value["position"] == case["first"]["position"]
                        && (case["first"]["file"].is_null()
                            || value["span"]["module"] == case["first"]["file"])
                }) {
                    failures.push(format!(
                        "{id}: expected {}; received {primary:?}",
                        case["first"]
                    ));
                }
            }
            Err(_) => failures.push(format!("{id}: unstructured diagnostic")),
        }
    }
    fs::remove_dir_all(root).unwrap();
    assert_eq!(counts, [69, 30, 768]);
    let count = failures.len();
    failures.truncate(20);
    assert!(
        failures.is_empty(),
        "{count} project position mismatches:\n{}",
        failures.join("\n")
    );
}

#[test]
fn native_known_subset_option_refusals_do_not_invent_unknown_option_errors() {
    let output = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
        .args(["--target", "es2022", "--diagnostics-json"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let diagnostic: Value = serde_json::from_slice(&output.stderr).unwrap();
    assert!(diagnostic["typescript"].is_null());
    assert!(diagnostic["noTypeScriptCounterpart"].as_str().is_some());
    assert_eq!(
        diagnostic["rawMessage"],
        "unknown compiler option `--target`"
    );
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3"]
fn recorded_project_positions_match_pinned_typescript() {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../development/browser_core/phase-18-bluets/tools/record_project_diagnostic_positions.cjs");
    let output = Command::new("node").arg(script).output().unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
