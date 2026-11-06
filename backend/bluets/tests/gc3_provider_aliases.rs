// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Authored and automatic JSX imports must coexist in physical CommonJS output.

use serde_json::Value;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
#[path = "oracle_support/gc3_cases.rs"]
mod cases;
use cases::{compile_case_output, Temporary};

fn compile_case(registry: &Value, expected: &Value, root: &Temporary, index: usize) -> PathBuf {
    compile_case_output(registry, expected, root, index).0
}

#[test]
fn imported_global_augmentations_keep_their_normal_graph_owner() {
    for module in ["CommonJS", "ES2022"] {
        let root = Temporary::new(&format!("imported-global-{module}"));
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/typescript_oracle")
            .join(format!(
                "modambient-global-imported-bad-{}",
                module.to_ascii_lowercase()
            ));
        for entry in fs::read_dir(fixture).unwrap() {
            let entry = entry.unwrap();
            if entry
                .path()
                .extension()
                .is_some_and(|extension| extension == "ts")
            {
                fs::copy(entry.path(), root.0.join(entry.file_name())).unwrap();
            }
        }
        let config = root.0.join("tsconfig.json");
        fs::write(
            &config,
            serde_json::json!({"compilerOptions": {
            "strict": true, "module": module, "target": "ES2022", "noEmit": true,
            "types": [], "lib": ["ES2022"]
        }, "files": ["main.ts", "augment.d.ts"]})
            .to_string(),
        )
        .unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
            .args(["--project", config.to_str().unwrap(), "--diagnostics-json"])
            .output()
            .unwrap();
        assert!(!output.status.success());
        let diagnostics = String::from_utf8(output.stderr)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(diagnostics.len(), 1, "{module}: {diagnostics:?}");
        let ts = &diagnostics[0]["typescript"];
        assert_eq!(ts["code"], 2322);
        assert_eq!(
            ts["message"],
            "Type 'string' is not assignable to type 'number'."
        );
        assert_eq!(ts["span"]["module"], "main.ts");
        assert_eq!(ts["relatedInformation"][0]["span"]["module"], "dep.ts");
    }
}

#[test]
fn authored_and_automatic_provider_imports_produce_valid_commonjs_modules() {
    let registry: Value =
        serde_json::from_str(include_str!("fixtures/gc3_compositions/registry.json")).unwrap();
    let reference: Value =
        serde_json::from_str(include_str!("fixtures/gc3_compositions/reference.json")).unwrap();
    assert_eq!(reference["typescript"], "5.9.3");
    let root = Temporary::new("syntax");
    let mut failures = Vec::new();
    for (index, id) in [
        "jsx-provider-conditional-barrel-valid/conditional-barrel-valid-prod-node16-cjs",
        "jsx-provider-conditional-barrel-wrong-props/conditional-barrel-wrong-props-prod-node16-cjs",
        "jsx-provider-types-only-runtime-missing/types-only-runtime-missing-prod-node16-cjs",
    ]
    .into_iter()
    .enumerate()
    {
        let expected = reference["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|case| case["id"] == id)
            .unwrap();
        assert_eq!(expected["options"]["module"], "Node16");
        let directory = compile_case(&registry, expected, &root, index);
        for expected_syntax in expected["syntax"].as_array().unwrap() {
            assert_eq!(expected_syntax["parseDiagnostics"], serde_json::json!([]));
            assert!(expected_syntax["acornError"].is_null());
            let name = expected_syntax["file"].as_str().unwrap();
            let emitted = directory.join(name);
            assert!(emitted.is_file(), "{id}: missing emitted {name}");
            let checked = Command::new("node")
                .arg("--check")
                .arg(&emitted)
                .env_remove("NODE_OPTIONS")
                .env_remove("NODE_PATH")
                .output()
                .unwrap();
            if !checked.status.success() {
                failures.push(format!(
                    "{id}/{name}: Native syntax is valid; Blue syntax failed: {}",
                    String::from_utf8_lossy(&checked.stderr)
                ));
            }
        }

    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn provider_declarations_retain_existing_import_paths_in_production_and_development() {
    let registry: Value =
        serde_json::from_str(include_str!("fixtures/gc3_compositions/registry.json")).unwrap();
    let reference: Value =
        serde_json::from_str(include_str!("fixtures/gc3_compositions/reference.json")).unwrap();
    let cases = reference["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|case| {
            matches!(
                case["form"].as_str(),
                Some(
                    "jsx-provider-conditional-barrel-valid"
                        | "jsx-provider-conditional-barrel-wrong-props"
                        | "jsx-provider-types-only-runtime-missing"
                )
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(cases.len(), 12);
    let root = Temporary::new("declarations");
    let mut failures = Vec::new();
    for (index, expected) in cases.into_iter().enumerate() {
        let directory = compile_case(&registry, expected, &root, index);
        for (name, source) in expected["outputs"].as_object().unwrap() {
            if !name.ends_with(".d.ts") && !name.ends_with(".d.cts") && !name.ends_with(".d.mts") {
                continue;
            }
            let actual = fs::read_to_string(directory.join(name)).unwrap();
            if actual != source.as_str().unwrap() {
                failures.push(format!(
                    "{}/{}: raw declaration differs\nBlue: {}\nNative: {}",
                    expected["id"].as_str().unwrap(),
                    name,
                    actual,
                    source.as_str().unwrap()
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn configured_global_jsx_roots_preserve_exact_declarations() {
    let registry: Value =
        serde_json::from_str(include_str!("fixtures/gc3_compositions/registry.json")).unwrap();
    let reference: Value =
        serde_json::from_str(include_str!("fixtures/gc3_compositions/reference.json")).unwrap();
    let cases = reference["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|case| case["form"] == "jsx-provider-missing-provider-global-fallback")
        .collect::<Vec<_>>();
    assert_eq!(cases.len(), 4);
    let root = Temporary::new("global-roots");
    for (index, expected) in cases.into_iter().enumerate() {
        let directory = compile_case(&registry, expected, &root, index);
        for (name, source) in expected["outputs"].as_object().unwrap() {
            if name.ends_with(".d.ts") {
                assert_eq!(
                    fs::read_to_string(directory.join(name)).unwrap(),
                    source.as_str().unwrap(),
                    "{}/{name}",
                    expected["id"].as_str().unwrap()
                );
            }
        }
    }
}

#[test]
fn missing_runtime_providers_match_frozen_complete_diagnostics() {
    let registry: Value =
        serde_json::from_str(include_str!("fixtures/gc3_compositions/registry.json")).unwrap();
    let reference: Value =
        serde_json::from_str(include_str!("fixtures/gc3_compositions/reference.json")).unwrap();
    let cases = reference["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|case| {
            matches!(
                case["form"].as_str(),
                Some(
                    "jsx-provider-missing-runtime-module"
                        | "jsx-provider-missing-runtime-declarations"
                        | "jsx-provider-missing-provider-global-fallback"
                )
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(cases.len(), 12);
    let root = Temporary::new("missing-diagnostics");
    let mut failures = Vec::new();
    for (index, expected) in cases.into_iter().enumerate() {
        assert_eq!(expected["verdict"], "reject");
        let (directory, compiled) = compile_case_output(&registry, expected, &root, index);
        let form = registry["forms"]
            .as_array()
            .unwrap()
            .iter()
            .find(|form| form["id"] == expected["form"])
            .unwrap();
        let mut actual = Vec::new();
        for line in String::from_utf8_lossy(&compiled.stderr)
            .lines()
            .filter(|line| !line.is_empty())
        {
            let diagnostic: Value = serde_json::from_str(line).unwrap();
            let ts = &diagnostic["typescript"];
            assert!(
                ts.is_object(),
                "{}: missing TypeScript counterpart: {diagnostic}",
                expected["id"]
            );
            let physical = ts["span"]["module"].as_str().unwrap().replace('\\', "/");
            let prefix = directory.to_string_lossy().replace('\\', "/") + "/";
            let file = physical.strip_prefix(&prefix).unwrap_or(&physical);
            let source = form["files"][file]
                .as_str()
                .expect("diagnostic belongs to original authored source");
            let start = ts["span"]["start"].as_u64().unwrap() as usize;
            let end = ts["span"]["end"].as_u64().unwrap() as usize;
            let before = &source[..start];
            let start_utf16 = before.encode_utf16().count();
            let length = source[start..end].encode_utf16().count();
            assert_eq!(
                ts["position"],
                serde_json::json!({
                    "line": before.bytes().filter(|byte| *byte == b'\n').count() + 1,
                    "column": before.rsplit('\n').next().unwrap().encode_utf16().count() + 1,
                    "length": length,
                })
            );
            assert_eq!(ts["relatedInformation"], serde_json::json!([]));
            assert_eq!(diagnostic["severity"], "error");
            actual.push(serde_json::json!({
                "code": ts["code"], "category": "Error", "file": file,
                "start": start_utf16, "length": length,
                "message": ts["message"].as_str().unwrap().replace(directory.to_str().unwrap(), "<root>"),
                "related": [],
            }));
        }
        if compiled.status.success() || serde_json::json!(actual) != expected["diagnostics"] {
            failures.push(format!(
                "{}: accepted={}\nBlue: {}\nNative: {}",
                expected["id"].as_str().unwrap(),
                compiled.status.success(),
                serde_json::json!(actual),
                expected["diagnostics"]
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
