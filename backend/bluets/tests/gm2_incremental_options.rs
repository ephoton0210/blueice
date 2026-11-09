// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Explicit incremental config, observable state and owner-confined recovery.
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Temporary(PathBuf);
impl Temporary {
    fn new() -> Self {
        let path = std::env::temp_dir().canonicalize().unwrap().join(format!(
            "bluets-explicit-incremental-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}
impl Drop for Temporary {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn reference() -> Value {
    serde_json::from_str(include_str!(
        "fixtures/gm2_incremental_options/reference.json"
    ))
    .unwrap()
}
fn write(root: &Path, files: &Value) {
    for (name, text) in files.as_object().unwrap() {
        let file = root.join(name);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, text.as_str().unwrap()).unwrap();
    }
}
fn run(root: &Path) -> Output {
    run_mode(root, false)
}
fn run_mode(root: &Path, legacy: bool) -> Output {
    Command::new(env!("CARGO_BIN_EXE_bluetsc"))
        .current_dir(root)
        .args(if legacy {
            vec!["build", "--config", "tsconfig.json", "--diagnostics-json"]
        } else {
            vec![
                "--build",
                "tsconfig.json",
                "--verbose",
                "--diagnostics-json",
            ]
        })
        .output()
        .unwrap()
}
fn success(output: &Output) {
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
fn runtime_file(root: &Path, entry: &str, value: u64) {
    let script = format!(
        "console.log(require({}).value());",
        json!(format!("./{entry}"))
    );
    let result = Command::new("node")
        .current_dir(root)
        .args(["-e", &script])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&result.stdout).trim(),
        value.to_string()
    );
}
fn output_inventory(root: &Path) -> BTreeMap<String, Vec<u8>> {
    fn visit(base: &Path, directory: &Path, outputs: &mut BTreeMap<String, Vec<u8>>) {
        for item in fs::read_dir(directory).unwrap() {
            let path = item.unwrap().path();
            if path.is_dir() {
                visit(base, &path, outputs);
            } else if path
                .extension()
                .is_some_and(|ext| ext == "js" || ext == "ts" || ext == "map")
            {
                outputs.insert(
                    path.strip_prefix(base)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/"),
                    fs::read(&path).unwrap(),
                );
            }
        }
    }
    let mut outputs = BTreeMap::new();
    visit(root, &root.join("dist"), &mut outputs);
    outputs
}

#[test]
fn switching_to_incremental_retires_only_owned_publication_metadata() {
    let reference = reference();
    let row = &reference["cases"][0];
    for metadata_is_input in [false, true] {
        let temp = Temporary::new();
        write(&temp.0, &row["files"]);
        let mut config: Value =
            serde_json::from_str(row["files"]["tsconfig.json"].as_str().unwrap()).unwrap();
        config["compilerOptions"]
            .as_object_mut()
            .unwrap()
            .remove("incremental");
        fs::write(temp.0.join("tsconfig.json"), config.to_string()).unwrap();
        success(&run_mode(&temp.0, true));
        let manifest = temp.0.join("dist/bluetsc.manifest.json");
        let original_manifest = fs::read(&manifest).unwrap();
        let original_javascript = fs::read(temp.0.join("dist/entry.js")).unwrap();
        config["compilerOptions"]["incremental"] = json!(true);
        if metadata_is_input {
            config["extends"] = json!("./dist/bluetsc.manifest.json");
        }
        fs::write(temp.0.join("tsconfig.json"), config.to_string()).unwrap();
        let output = run_mode(&temp.0, true);
        if metadata_is_input {
            assert!(!output.status.success());
            assert_eq!(fs::read(&manifest).unwrap(), original_manifest);
            assert_eq!(
                fs::read(temp.0.join("dist/entry.js")).unwrap(),
                original_javascript
            );
            continue;
        }
        success(&output);
        assert!(
            !manifest.exists(),
            "stale publication metadata survived the mode change"
        );
        runtime_file(&temp.0, "dist/entry.js", 42);
        let initial = output_inventory(&temp.0);
        success(&run_mode(&temp.0, true));
        assert_eq!(output_inventory(&temp.0), initial);
        fs::write(&manifest, &original_manifest).unwrap();
        success(&run_mode(&temp.0, true));
        assert!(
            !manifest.exists(),
            "a cached build retained restored legacy metadata"
        );
        let clean = Temporary::new();
        write(&clean.0, &row["files"]);
        success(&run_mode(&clean.0, true));
        assert_eq!(output_inventory(&clean.0), initial);
    }
    let temp = Temporary::new();
    write(&temp.0, &row["files"]);
    fs::create_dir(temp.0.join("dist")).unwrap();
    let manifest = temp.0.join("dist/bluetsc.manifest.json");
    fs::write(&manifest, "{\"user\":\"data\"}").unwrap();
    success(&run_mode(&temp.0, true));
    assert_eq!(
        fs::read_to_string(&manifest).unwrap(),
        "{\"user\":\"data\"}"
    );
    #[cfg(unix)]
    {
        let outside = Temporary::new();
        let target = outside.0.join("manifest.json");
        fs::write(&target, "outside sentinel").unwrap();
        fs::remove_file(&manifest).unwrap();
        std::os::unix::fs::symlink(&target, &manifest).unwrap();
        let before = fs::read(temp.0.join("dist/entry.js")).unwrap();
        let mut config: Value =
            serde_json::from_str(row["files"]["tsconfig.json"].as_str().unwrap()).unwrap();
        config["compilerOptions"]["removeComments"] = json!(true);
        fs::write(temp.0.join("tsconfig.json"), config.to_string()).unwrap();
        assert!(!run_mode(&temp.0, true).status.success());
        assert_eq!(fs::read_to_string(target).unwrap(), "outside sentinel");
        assert_eq!(fs::read(temp.0.join("dist/entry.js")).unwrap(), before);
    }
}
fn public_state_files(root: &Path) -> Vec<String> {
    fn visit(base: &Path, root: &Path, result: &mut Vec<String>) {
        for item in fs::read_dir(root).unwrap() {
            let path = item.unwrap().path();
            if path.is_dir() {
                visit(base, &path, result);
            } else if path.extension().is_some_and(|ext| ext == "tsbuildinfo")
                && !path.to_string_lossy().ends_with(".bluetsc.tsbuildinfo")
            {
                result.push(
                    path.strip_prefix(base)
                        .unwrap()
                        .to_str()
                        .unwrap()
                        .replace('\\', "/"),
                );
            }
        }
    }
    let mut result = Vec::new();
    visit(root, root, &mut result);
    result.sort();
    result
}
#[test]
fn frozen_options_match_complete_diagnostics_paths_and_declarations() {
    for legacy in [false, true] {
        for row in reference()["cases"].as_array().unwrap() {
            let temp = Temporary::new();
            write(&temp.0, &row["files"]);
            let output = run_mode(&temp.0, legacy);
            assert_eq!(
                output.status.success(),
                row["accepts"].as_bool().unwrap(),
                "{}: {}",
                row["id"],
                String::from_utf8_lossy(&output.stderr)
            );
            if row["accepts"] == true {
                success(&output);
                let state = row["statePath"].as_str().map(|path| {
                    let path = temp.0.join(path);
                    let before = fs::metadata(&path).unwrap().modified().unwrap();
                    (path, before)
                });
                let shown = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
                    .current_dir(&temp.0)
                    .args([
                        "build",
                        "--config",
                        "tsconfig.json",
                        "--showConfig",
                        "--diagnostics-json",
                    ])
                    .output()
                    .unwrap();
                success(&shown);
                let shown: Value = serde_json::from_slice(&shown.stdout).unwrap();
                assert!(shown["compilerOptions"].is_object());
                if let Some((path, before)) = state {
                    assert_eq!(fs::metadata(path).unwrap().modified().unwrap(), before);
                }
                let paths = row["statePath"]
                    .as_str()
                    .map(|p| vec![p.to_string()])
                    .unwrap_or_default();
                assert_eq!(public_state_files(&temp.0), paths, "{}", row["id"]);
                if let Some(declaration) = row["declaration"].as_str() {
                    assert_eq!(
                        fs::read_to_string(
                            temp.0
                                .join(row["declarationPath"].as_str().unwrap_or("dist/entry.d.ts"))
                        )
                        .unwrap(),
                        declaration
                    );
                }
                if let Some(declarations) = row["declarations"].as_object() {
                    let actual = output_inventory(&temp.0)
                        .into_iter()
                        .filter(|(name, _)| name.ends_with(".d.ts"))
                        .map(|(name, bytes)| (name, String::from_utf8(bytes).unwrap()))
                        .collect::<BTreeMap<_, _>>();
                    assert_eq!(json!(actual), json!(declarations));
                }
                runtime_file(
                    &temp.0,
                    row["runtimePath"].as_str().unwrap_or("dist/entry.js"),
                    42,
                );
            } else {
                let actual = String::from_utf8_lossy(&output.stderr).lines().map(|line| {
                let d: Value = serde_json::from_str(line).unwrap(); let d = &d["typescript"];
                let start = d["span"]["start"].as_u64().unwrap() as usize;
                let source = row["files"]["tsconfig.json"].as_str().unwrap();
                json!({"code":d["code"],"message":d["message"],"module":"tsconfig.json",
                    "start":source[..start].encode_utf16().count(),"length":d["position"]["length"],"related":[]})
            }).collect::<Vec<_>>();
                assert_eq!(json!(actual), row["diagnostics"], "{}", row["id"]);
                assert!(public_state_files(&temp.0).is_empty());
                assert!(!temp.0.join("dist/entry.js").exists());
            }
        }
    }
}
#[test]
fn warm_edit_clean_and_state_recovery_use_current_owned_inputs() {
    for id in [
        "custom",
        "maps",
        "without-declarations",
        "source-map-marker",
        "transitive-source-root",
        "inherited-incremental",
    ] {
        let reference = reference();
        let row = reference["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["id"] == id)
            .unwrap();
        let temp = Temporary::new();
        write(&temp.0, &row["files"]);
        success(&run(&temp.0));
        let state = temp.0.join(row["statePath"].as_str().unwrap());
        assert!(state.is_file());
        let warm = run(&temp.0);
        success(&warm);
        assert!(String::from_utf8_lossy(&warm.stdout).contains("is up to date"));
        let source_name = row["editPath"].as_str().unwrap_or("src/entry.ts");
        let output_name = row["runtimePath"].as_str().unwrap_or("dist/entry.js");
        let source = temp.0.join(source_name);
        fs::write(
            &source,
            fs::read_to_string(&source).unwrap().replace("42", "43"),
        )
        .unwrap();
        let edited = run(&temp.0);
        success(&edited);
        assert!(String::from_utf8_lossy(&edited.stdout).contains("Building project:"));
        runtime_file(&temp.0, output_name, 43);
        let expected = output_inventory(&temp.0);
        for bytes in [
            b"{broken".as_slice(),
            b"{\"version\":\"foreign\",\"fileNames\":[\"../../secret.ts\"]}",
        ] {
            fs::write(&state, bytes).unwrap();
            let result = run(&temp.0);
            success(&result);
            assert!(String::from_utf8_lossy(&result.stdout).contains("Building project:"));
            assert!(serde_json::from_slice::<Value>(&fs::read(&state).unwrap()).is_ok());
        }
        fs::remove_file(&state).unwrap();
        let private_path = temp.0.join("tsconfig.bluetsc.tsbuildinfo");
        let mut private: Value = serde_json::from_slice(&fs::read(&private_path).unwrap()).unwrap();
        private["status"]["native_hash"] = Value::Null;
        fs::write(private_path, private.to_string()).unwrap();
        let missing = run(&temp.0);
        success(&missing);
        assert!(String::from_utf8_lossy(&missing.stdout).contains("Building project:"));
        assert!(state.is_file());
        fs::write(temp.0.join(output_name), "throw Error('tampered');").unwrap();
        success(&run(&temp.0));
        runtime_file(&temp.0, output_name, 43);
        let clean = Temporary::new();
        write(&clean.0, &row["files"]);
        fs::copy(&source, clean.0.join(source_name)).unwrap();
        success(&run(&clean.0));
        assert_eq!(output_inventory(&clean.0), expected);
    }
}
#[test]
fn configured_state_cannot_escape_its_owner() {
    let reference = reference();
    let row = &reference["cases"][1];
    let temp = Temporary::new();
    write(&temp.0, &row["files"]);
    let outside = Temporary::new();
    let path = outside.0.join("secret.tsbuildinfo");
    fs::write(&path, "sentinel").unwrap();
    let mut config: Value =
        serde_json::from_str(row["files"]["tsconfig.json"].as_str().unwrap()).unwrap();
    config["compilerOptions"]["tsBuildInfoFile"] = json!(path);
    fs::write(temp.0.join("tsconfig.json"), config.to_string()).unwrap();
    assert!(!run(&temp.0).status.success());
    assert_eq!(fs::read_to_string(path).unwrap(), "sentinel");
    assert!(!temp.0.join("dist/entry.js").exists());
}

#[test]
fn state_paths_cannot_replace_inputs_or_emitted_artifacts() {
    let reference = reference();
    let row = &reference["cases"][1];
    for destination in [
        "src/entry.ts",
        "tsconfig.json",
        "dist/entry.js",
        "dist/entry.d.ts",
        "dist/entry.js.map",
        "declarations/entry.d.ts",
        "declarations/entry.d.ts.map",
        "tsconfig.bluetsc.tsbuildinfo",
    ] {
        let temp = Temporary::new();
        write(&temp.0, &row["files"]);
        let mut config: Value =
            serde_json::from_str(row["files"]["tsconfig.json"].as_str().unwrap()).unwrap();
        config["compilerOptions"]["tsBuildInfoFile"] = json!(destination);
        if destination.starts_with("declarations/") {
            config["compilerOptions"]["declarationDir"] = json!("declarations");
        }
        fs::write(temp.0.join("tsconfig.json"), config.to_string()).unwrap();
        let before = fs::read(temp.0.join("tsconfig.json")).unwrap();
        assert!(!run(&temp.0).status.success(), "{destination}");
        assert_eq!(fs::read(temp.0.join("tsconfig.json")).unwrap(), before);
        assert_eq!(
            fs::read_to_string(temp.0.join("src/entry.ts")).unwrap(),
            row["files"]["src/entry.ts"].as_str().unwrap()
        );
        assert!(!temp.0.join("dist/entry.js").exists());
    }
}
#[cfg(unix)]
#[test]
fn state_and_parent_symlinks_cannot_publish_outside_owner() {
    use std::os::unix::fs::symlink;
    let reference = reference();
    let row = &reference["cases"][1];
    for parent in [false, true] {
        let temp = Temporary::new();
        write(&temp.0, &row["files"]);
        let outside = Temporary::new();
        let sentinel = outside.0.join("custom.tsbuildinfo");
        fs::write(&sentinel, "sentinel").unwrap();
        if parent {
            symlink(&outside.0, temp.0.join("cache")).unwrap();
        } else {
            fs::create_dir(temp.0.join("cache")).unwrap();
            symlink(&sentinel, temp.0.join("cache/custom.tsbuildinfo")).unwrap();
        }
        assert!(!run(&temp.0).status.success());
        assert_eq!(fs::read_to_string(sentinel).unwrap(), "sentinel");
        assert!(!temp.0.join("dist/entry.js").exists());
    }
}
#[test]
#[ignore = "requires pinned TypeScript 5.9.3"]
fn frozen_native_configuration_observations_are_reproducible() {
    let tool = Path::new(env!("CARGO_MANIFEST_DIR")).join(
        "../../development/browser_core/phase-18-bluets/tools/record_gm2_incremental_options.cjs",
    );
    let output = Command::new("node").arg(tool).output().unwrap();
    success(&output);
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3 and real Native state cross-use"]
fn configured_incremental_state_cross_use_preserves_maps_and_edits() {
    let oracle =
        PathBuf::from(std::env::var("BLUEICE_BLUETSC_ORACLE").expect("pinned tsc required"));
    let oracle = if oracle.file_name().is_some_and(|name| name == "tsc.cmd")
        && oracle
            .parent()
            .and_then(Path::file_name)
            .is_some_and(|name| name == ".bin")
    {
        oracle
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("typescript/bin/tsc")
    } else {
        oracle
    };
    for id in [
        "default",
        "custom",
        "maps",
        "transitive-source-root",
        "inherited-incremental",
    ] {
        let reference = reference();
        let row = reference["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["id"] == id)
            .unwrap();
        let temp = Temporary::new();
        write(&temp.0, &row["files"]);
        let native = || {
            Command::new("node")
                .arg(&oracle)
                .current_dir(&temp.0)
                .args([
                    "-p",
                    "tsconfig.json",
                    "--pretty",
                    "false",
                    "--listEmittedFiles",
                ])
                .output()
                .unwrap()
        };
        success(&native());
        let imported = run(&temp.0);
        success(&imported);
        let event = if id == "transitive-source-root" {
            "Building project:"
        } else {
            "is up to date"
        };
        assert!(
            String::from_utf8_lossy(&imported.stdout).contains(event),
            "{id}"
        );
        let source = temp
            .0
            .join(row["editPath"].as_str().unwrap_or("src/entry.ts"));
        let declaration_path = row["declarationPath"].as_str().unwrap_or("dist/entry.d.ts");
        let runtime_path = row["runtimePath"].as_str().unwrap_or("dist/entry.js");
        fs::write(
            &source,
            fs::read_to_string(&source).unwrap().replace("42", "43"),
        )
        .unwrap();
        success(&run(&temp.0));
        let declaration = fs::read(temp.0.join(declaration_path)).unwrap();
        let consumed = native();
        success(&consumed);
        // Source-only export does not authorize/discover a Native install.
        // Native -p reconstructs its library graph and re-emits; freeze that
        // owner difference separately from semantic/artifact cross-use.
        assert!(
            String::from_utf8_lossy(&consumed.stdout).contains("TSFILE:"),
            "{id}"
        );
        assert_eq!(
            fs::read(temp.0.join(declaration_path)).unwrap(),
            declaration
        );
        runtime_file(&temp.0, runtime_path, 43);
        if id == "maps" {
            fs::remove_file(temp.0.join("dist/entry.js.map")).unwrap();
            let rebuilt = run(&temp.0);
            success(&rebuilt);
            assert!(String::from_utf8_lossy(&rebuilt.stdout).contains("Building project:"));
            assert!(temp.0.join("dist/entry.js.map").is_file());
        }
    }
}
