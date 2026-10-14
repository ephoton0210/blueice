// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! K.2.3: native project CLI observations recorded from TypeScript 5.9.3.

use serde_json::{json, Value};
use std::{
    collections::BTreeSet,
    env, fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};
const REFERENCE: &str = include_str!("fixtures/cli_surface/reference.json");
const MATRIX: &str = include_str!("fixtures/cli_surface/cli-checker-matrix.tsv");
fn rows() -> Vec<Value> {
    serde_json::from_str(REFERENCE).unwrap()
}
fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/cli_surface")
}
fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &to.join(entry.file_name()));
        } else {
            fs::copy(entry.path(), to.join(entry.file_name())).unwrap();
        }
    }
}
fn files(root: &Path) -> BTreeSet<String> {
    fn visit(root: &Path, directory: &Path, files: &mut BTreeSet<String>) {
        for entry in fs::read_dir(directory).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_dir() {
                visit(root, &entry.path(), files);
            } else {
                files.insert(
                    entry
                        .path()
                        .strip_prefix(root)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/"),
                );
            }
        }
    }
    let mut result = BTreeSet::new();
    visit(root, root, &mut result);
    result
}
fn prepare(root: &Path, row: &Value) {
    copy_tree(&fixtures().join("project"), root);
    let mut config: Value =
        serde_json::from_slice(&fs::read(root.join("tsconfig.json")).unwrap()).unwrap();
    match row["variant"].as_str().unwrap_or("") {
        "error" => fs::write(
            root.join("src/value.ts"),
            "export const value: number = \"wrong\";\n",
        )
        .unwrap(),
        "config-noemit" => config["compilerOptions"]["noEmit"] = json!(true),
        "config-listfiles" => config["compilerOptions"]["listFiles"] = json!(true),
        "selectors" => {
            config["include"] = json!(["src/**/*.ts"]);
            config["exclude"] = json!(["src/ignore*"]);
        }
        "existing-output" => {
            fs::create_dir_all(root.join("out")).unwrap();
            fs::write(root.join("out/keep.txt"), "preserve this unrelated file\n").unwrap();
        }
        "in-place" => {
            config["compilerOptions"]
                .as_object_mut()
                .unwrap()
                .remove("outDir");
        }
        _ => {}
    }
    fs::write(
        root.join("tsconfig.json"),
        serde_json::to_vec_pretty(&config).unwrap(),
    )
    .unwrap();
}
fn display(root: &Path, row: &Value, output: &Output) -> Value {
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    if row["args"]
        .as_array()
        .unwrap()
        .iter()
        .any(|arg| arg == "--showConfig")
        && output.status.success()
    {
        fn normalize(value: &mut Value, root: &str) {
            match value {
                Value::String(text) => {
                    *text = text.replace(root, "<project>").replace('\\', "/");
                }
                Value::Array(values) => {
                    for value in values {
                        normalize(value, root);
                    }
                }
                Value::Object(values) => {
                    for value in values.values_mut() {
                        normalize(value, root);
                    }
                }
                _ => {}
            }
        }
        let mut config: Value = serde_json::from_str(&text).unwrap();
        normalize(&mut config, &root.to_string_lossy());
        return json!({"config":config});
    }
    let text = text
        .replace(&root.to_string_lossy().to_string(), "<project>")
        .replace('\\', "/");
    let mut emitted = text
        .lines()
        .filter_map(|line| line.strip_prefix("TSFILE: "))
        .collect::<Vec<_>>();
    emitted.sort();
    let files = text
        .lines()
        .filter(|line| line.starts_with("<project>/"))
        .collect::<Vec<_>>();
    // The embedded library catalog differs from TypeScript's installation.
    // Compare project inputs, and still reject unexpected success summaries.
    let quiet = !output.status.success()
        || (output.stderr.is_empty()
            && text.lines().all(|line| {
                line.is_empty()
                    || line.starts_with("<project>/")
                    || line.starts_with("TSFILE: ")
                    || (Path::new(line).is_absolute() && line.ends_with(".d.ts"))
            }));
    json!({"files":files,"emitted":emitted,"diagnostic":!output.status.success(),"pretty":text.contains("\u{1b}["),"quiet":quiet})
}
fn observe(executable: &Path, root: &Path, row: &Value) -> (Value, Output) {
    prepare(root, row);
    let canonical = fs::canonicalize(root).unwrap();
    let root = canonical.as_path();
    let before = files(root);
    let original = before
        .iter()
        .map(|p| (p, fs::read(root.join(p)).unwrap()))
        .collect::<Vec<_>>();
    let cwd = if row["variant"] == "nested" {
        root.join("src")
    } else {
        root.to_path_buf()
    };
    let output = Command::new(executable)
        .current_dir(cwd)
        .env("FORCE_COLOR", "0")
        .args(
            row["args"]
                .as_array()
                .unwrap()
                .iter()
                .map(|arg| arg.as_str().unwrap()),
        )
        .output()
        .unwrap();
    let assets = files(root).difference(&before).cloned().collect::<Vec<_>>();
    (
        json!({"exit":output.status.code().unwrap(),"display":display(root,row,&output),"assets":assets,"inputs_preserved":original.iter().all(|(p,contents)| fs::read(root.join(p)).ok().as_ref()==Some(contents))}),
        output,
    )
}
#[test]
fn native_cli_matches_the_recorded_project_observations() {
    let root = env::temp_dir().join(format!("bluets-native-cli-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let mut failures = Vec::new();
    for (index, row) in rows().iter().enumerate() {
        let (observed, output) = observe(
            Path::new(env!("CARGO_BIN_EXE_bluetsc")),
            &root.join(index.to_string()),
            row,
        );
        let expected = json!({"exit":row["exit"],"display":row["display"],"assets":row["assets"],"inputs_preserved":row["inputs_preserved"]});
        if observed != expected {
            failures.push(format!(
                "{}: expected {expected}; observed {observed}; {}{}",
                row["name"],
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ));
        }
    }
    fs::remove_dir_all(root).unwrap();
    assert!(
        failures.is_empty(),
        "{} native CLI mismatches\n{}",
        failures.len(),
        failures.join("\n")
    );
}
#[test]
fn serialized_project_paths_normalize_windows_roots() {
    let root = Path::new(r"\\?\C:\Users\runneradmin\Temp\project");
    let mut output = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
        .arg("--help")
        .output()
        .unwrap();
    assert!(output.status.success());
    output.stdout = serde_json::to_vec(&json!({
        "exclude": [format!(r"{}\out", root.display())],
        "files": ["./src/main.ts"]
    }))
    .unwrap();
    assert_eq!(
        display(root, &json!({"args": ["--showConfig"]}), &output),
        json!({"config": {"exclude": ["<project>/out"], "files": ["./src/main.ts"]}})
    );
}

#[test]
fn cli_matrix_records_every_named_invocation() {
    let cases = rows();
    assert_eq!(cases.len(), 30);
    let matrix = cases
        .iter()
        .map(|row| {
            format!(
                "cli-{}\t{}\n",
                row["name"].as_str().unwrap(),
                if row["exit"] == 0 { "accept" } else { "reject" }
            )
        })
        .collect::<String>();
    assert_eq!(
        matrix.lines().collect::<Vec<_>>(),
        MATRIX.lines().collect::<Vec<_>>()
    );
}

#[test]
fn import_only_declarations_retain_the_module_boundary() {
    let compilation = blueice_bluets::compile(
        "memory:///main.ts",
        &blueice_bluets::MapLoader::from([
            blueice_bluets::ModuleSource::new(
                "memory:///main.ts",
                "import { value } from './value.ts'; console.log(value);",
            ),
            blueice_bluets::ModuleSource::new(
                "memory:///value.ts",
                "export const value: number = 1;",
            ),
        ]),
        blueice_bluets::CompilerOptions {
            declaration: true,
            ..blueice_bluets::CompilerOptions::default()
        },
    );
    assert!(!compilation.has_errors(), "{:?}", compilation.diagnostics);
    assert_eq!(
        compilation.output.unwrap().artifacts["memory:///main.ts"]
            .declaration
            .as_deref(),
        Some("export {};\n")
    );
}

#[test]
fn native_emission_rejects_input_and_configuration_collisions() {
    let root = env::temp_dir().join(format!("bluets-native-collisions-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    copy_tree(&fixtures().join("project"), &root);
    fs::write(
        root.join("src/main.ts"),
        "import type { Value } from './main.d.ts'; export const value: Value = 1;",
    )
    .unwrap();
    let declaration = "export type Value = number;\n";
    fs::write(root.join("src/main.d.ts"), declaration).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
        .current_dir(&root)
        .args(["--project", "."])
        .output()
        .unwrap();
    // With outDir, the original declaration remains separate from its output.
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let mut config: Value =
        serde_json::from_slice(&fs::read(root.join("tsconfig.json")).unwrap()).unwrap();
    config["compilerOptions"]
        .as_object_mut()
        .unwrap()
        .remove("outDir");
    fs::write(
        root.join("tsconfig.json"),
        serde_json::to_vec(&config).unwrap(),
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
        .current_dir(&root)
        .args(["--project", "."])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert_eq!(
        fs::read_to_string(root.join("src/main.d.ts")).unwrap(),
        declaration
    );
    assert!(!root.join("src/main.js").exists());
    fs::write(
        root.join("src/main.ts"),
        "export const value: number = 1;\n",
    )
    .unwrap();
    config["files"] = json!(["main.ts"]);
    let protected = serde_json::to_vec(&config).unwrap();
    fs::write(root.join("src/main.js.map"), &protected).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
        .current_dir(&root)
        .args(["--project", "src/main.js.map"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert_eq!(fs::read(root.join("src/main.js.map")).unwrap(), protected);
    assert!(!root.join("src/main.js").exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn native_emission_rejects_duplicate_destinations_before_writing() {
    let root = env::temp_dir().join(format!("bluets-native-duplicate-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    copy_tree(&fixtures().join("project"), &root);
    fs::write(root.join("src/main.tsx"), "export const other: number = 1;").unwrap();
    let mut config: Value =
        serde_json::from_slice(&fs::read(root.join("tsconfig.json")).unwrap()).unwrap();
    config["files"] = json!(["src/main.ts", "src/main.tsx"]);
    fs::write(
        root.join("tsconfig.json"),
        serde_json::to_vec(&config).unwrap(),
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
        .current_dir(&root)
        .args(["--project", "."])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(!root.join("out").exists());
    assert!(String::from_utf8_lossy(&output.stderr).contains("same output path"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3 and Node"]
fn native_output_runs_and_declares_like_typescript() {
    let tsc =
        PathBuf::from(env::var_os("BLUEICE_BLUETSC_ORACLE").expect("set BLUEICE_BLUETSC_ORACLE"));
    let root = env::temp_dir().join(format!("bluets-native-execution-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    for name in ["list-emits", "in-place-emits"] {
        let row = rows().into_iter().find(|r| r["name"] == name).unwrap();
        let blue = root.join(name).join("blue");
        let reference = root.join(name).join("reference");
        let (_, actual) = observe(Path::new(env!("CARGO_BIN_EXE_bluetsc")), &blue, &row);
        let (_, expected) = observe(&tsc, &reference, &row);
        assert!(actual.status.success() && expected.status.success());
        let directory = if name == "list-emits" { "out" } else { "src" };
        let run = |path: &Path| {
            let result = Command::new("node")
                .arg(path.join(directory).join("main.js"))
                .output()
                .unwrap();
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
            result.stdout
        };
        assert_eq!(run(&blue), run(&reference));
        assert_eq!(run(&blue), b"42\n");
        for file in ["main.d.ts", "value.d.ts"] {
            assert_eq!(
                fs::read(blue.join(directory).join(file)).unwrap(),
                fs::read(reference.join(directory).join(file)).unwrap()
            );
        }
    }
    fs::remove_dir_all(root).unwrap();
}
#[test]
#[ignore = "requires pinned TypeScript 5.9.3"]
fn recorded_native_cli_observations_match_pinned_typescript() {
    let tsc =
        PathBuf::from(env::var_os("BLUEICE_BLUETSC_ORACLE").expect("set BLUEICE_BLUETSC_ORACLE"));
    let version = Command::new(&tsc).arg("--version").output().unwrap();
    assert_eq!(
        String::from_utf8_lossy(&version.stdout).trim(),
        "Version 5.9.3"
    );
    let root = env::temp_dir().join(format!("bluets-native-cli-oracle-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let mut recorded = Vec::new();
    for (index, mut row) in rows().into_iter().enumerate() {
        let (observed, _) = observe(&tsc, &root.join(index.to_string()), &row);
        for (key, value) in observed.as_object().unwrap() {
            row[key] = value.clone();
        }
        recorded.push(row);
    }
    fs::remove_dir_all(root).unwrap();
    if env::var("BLUEICE_WRITE_CLI_MATRIX").as_deref() == Ok("1") {
        fs::write(
            fixtures().join("reference.json"),
            serde_json::to_string_pretty(&recorded).unwrap() + "\n",
        )
        .unwrap();
        fs::write(
            fixtures().join("cli-checker-matrix.tsv"),
            recorded
                .iter()
                .map(|r| {
                    format!(
                        "cli-{}\t{}\n",
                        r["name"].as_str().unwrap(),
                        if r["exit"] == 0 { "accept" } else { "reject" }
                    )
                })
                .collect::<String>(),
        )
        .unwrap();
    } else {
        assert_eq!(recorded, rows());
    }
}
