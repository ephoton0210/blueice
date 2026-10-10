// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Public owner builds retain module/helper policy and provider input identity.

use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn copy(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let target = destination.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

fn build(config: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_bluetsc"))
        .args(["build", "--config"])
        .arg(config)
        .output()
        .unwrap()
}

#[test]
fn owner_manifests_bind_helper_selection_and_provider_bytes() {
    let root = fs::canonicalize(std::env::temp_dir())
        .unwrap()
        .join(format!("bluets-module-policy-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/module_systems/modemit-helper-options-commonjs-import-emit-provider");
    let mut fingerprints = BTreeSet::new();
    for module in ["commonjs", "amd", "umd"] {
        let directory = root.join(module);
        copy(&fixture, &directory);
        let config = directory.join("owner.json");
        let manifest_path = directory.join("published/bluetsc.manifest.json");
        for imports in [false, true] {
            for omit in [false, true] {
                fs::write(
                    &config,
                    json!({
                        "entries":["main.ts"],"outDir":"published","target":"es5",
                        "lib":["es2020"],"module":module,"moduleResolution":"node10",
                        "declaration":true,"importHelpers":imports,"noEmitHelpers":omit
                    })
                    .to_string(),
                )
                .unwrap();
                let result = build(&config);
                assert!(
                    result.status.success(),
                    "{}",
                    String::from_utf8_lossy(&result.stderr)
                );
                let bytes = fs::read(&manifest_path).unwrap();
                let manifest: Value = serde_json::from_slice(&bytes).unwrap();
                assert_eq!(manifest["module"], module);
                assert_eq!(manifest["import_helpers"], imports);
                assert_eq!(manifest["no_emit_helpers"], omit);
                assert!(fingerprints.insert(manifest["fingerprint"].as_str().unwrap().to_owned()));
                if imports && !omit {
                    let provider = directory.join("node_modules/tslib/index.d.ts");
                    let original = fs::read_to_string(&provider).unwrap();
                    fs::write(
                        &provider,
                        format!("{original}\n// owner provider revision\n"),
                    )
                    .unwrap();
                    assert!(build(&config).status.success());
                    let revised = fs::read(&manifest_path).unwrap();
                    let revised_value: Value = serde_json::from_slice(&revised).unwrap();
                    assert_ne!(manifest["fingerprint"], revised_value["fingerprint"]);
                    fs::remove_file(&provider).unwrap();
                    assert!(!build(&config).status.success());
                    assert_eq!(fs::read(&manifest_path).unwrap(), revised);
                    fs::write(&provider, original).unwrap();
                }
            }
        }
    }
    assert_eq!(fingerprints.len(), 12);
    fs::remove_dir_all(root).unwrap();
}
