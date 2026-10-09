// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! JSON attribute dependencies retain owner bounds and actual asset identity.

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::{env, fs};

fn project(name: &str, resolve: bool) -> PathBuf {
    let root = fs::canonicalize(env::temp_dir())
        .unwrap()
        .join(format!("bluets-json-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("tsconfig.json"),
        json!({"compilerOptions":{
            "target":"ES2022", "module":"ESNext", "moduleResolution":"bundler",
            "strict":true, "resolveJsonModule":resolve, "noEmitOnError":true,
            "declaration":true, "outDir":"out"
        }, "files":["src/main.ts"]})
        .to_string(),
    )
    .unwrap();
    fs::write(
        root.join("src/main.ts"),
        "import data from './data.json' with {type:'json'}; export const value: number = data.nested.value;",
    )
    .unwrap();
    root
}

fn build(root: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_bluetsc"))
        .args(["build", "--config"])
        .arg(root.join("tsconfig.json"))
        .output()
        .unwrap()
}

fn report(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn fingerprint(root: &Path) -> String {
    let metadata: Value =
        serde_json::from_str(&fs::read_to_string(root.join("out/bluetsc.manifest.json")).unwrap())
            .unwrap();
    metadata["fingerprint"].as_str().unwrap().to_string()
}

#[test]
fn json_assets_keep_actual_bytes_types_and_content_fingerprints() {
    let root = project("identity", true);
    let first = "{\n  \"nested\": {\"value\": 42}, \"label\": \"你好\"\n}\n";
    fs::write(root.join("src/data.json"), first).unwrap();
    let output = build(&root);
    assert!(output.status.success(), "{}", report(&output));
    assert_eq!(
        fs::read_to_string(root.join("out/data.json")).unwrap(),
        first
    );
    let original = fingerprint(&root);
    fs::write(root.join("src/data.json"), first.replace("42", "43")).unwrap();
    let output = build(&root);
    assert!(output.status.success(), "{}", report(&output));
    assert_ne!(fingerprint(&root), original);
    assert!(fs::read_to_string(root.join("out/data.json"))
        .unwrap()
        .contains("43"));
    fs::remove_dir_all(root.join("out")).unwrap();
    fs::write(root.join("src/data.json"), "{\"nested\":{\"value\":true}}").unwrap();
    let output = build(&root);
    assert!(!output.status.success());
    assert!(report(&output).contains("TS2322"), "{}", report(&output));
    assert!(!root.join("out").exists());
    fs::write(root.join("src/data.json"), "{invalid json}").unwrap();
    assert!(!build(&root).status.success());
    assert!(!root.join("out").exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn json_resolution_requires_the_explicit_effective_option() {
    let root = project("disabled", false);
    fs::write(root.join("src/data.json"), "{\"nested\":{\"value\":42}}").unwrap();
    assert!(!build(&root).status.success());
    assert!(!root.join("out").exists());
    fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn json_symlinks_cannot_read_or_publish_an_asset_outside_the_owner_root() {
    let root = project("escape", true);
    let outside = root.with_extension("outside");
    fs::write(&outside, "{\"nested\":{\"value\":42}}").unwrap();
    std::os::unix::fs::symlink(&outside, root.join("src/data.json")).unwrap();
    let output = build(&root);
    assert!(!output.status.success());
    assert!(report(&output).contains("outside"), "{}", report(&output));
    assert!(!root.join("out").exists());
    fs::remove_dir_all(root).unwrap();
    fs::remove_file(outside).unwrap();
}
