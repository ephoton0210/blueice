// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! End-to-end coverage for the standalone `bluetsc` process boundary.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static TEST_DIRECTORY_COUNTER: AtomicU64 = AtomicU64::new(0);

#[test]
fn check_and_build_use_the_same_closed_project_and_preserve_output_on_error() {
    let temporary = unique_test_directory();
    let root = temporary.join("project");
    let source = root.join("src");
    let output = temporary.join("output");
    fs::create_dir_all(&source).unwrap();
    fs::write(
        source.join("model.ts"),
        "export interface User { id: string }\n",
    )
    .unwrap();
    let entry = source.join("main.ts");
    fs::write(
        &entry,
        "import type { User } from './model.ts';\nexport function label(_user: User): string { const name: string = 'Ada'; return name; }\n",
    )
    .unwrap();

    let checked = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
        .args([
            "check",
            entry.to_str().unwrap(),
            "--project-root",
            root.to_str().unwrap(),
        ])
        .status()
        .unwrap();
    assert!(checked.success());

    let built = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
        .args([
            "build",
            entry.to_str().unwrap(),
            "--project-root",
            root.to_str().unwrap(),
            "--out-dir",
            output.to_str().unwrap(),
            "--source-map",
            "--declaration",
        ])
        .status()
        .unwrap();
    assert!(built.success());
    let javascript = fs::read_to_string(output.join("src/main.js")).unwrap();
    assert!(javascript.contains("function label(_user)"));
    assert!(output.join("src/main.js.map").is_file());
    assert!(output.join("src/main.d.ts").is_file());

    fs::write(&entry, "export const broken: number = 'wrong';\n").unwrap();
    let failed = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
        .args([
            "build",
            entry.to_str().unwrap(),
            "--project-root",
            root.to_str().unwrap(),
            "--out-dir",
            output.to_str().unwrap(),
        ])
        .status()
        .unwrap();
    assert!(!failed.success());
    assert_eq!(
        fs::read_to_string(output.join("src/main.js")).unwrap(),
        javascript
    );
    fs::remove_dir_all(temporary).unwrap();
}

#[test]
fn config_builds_multiple_entries_and_resolves_only_declared_imports() {
    let temporary = unique_test_directory();
    let root = temporary.join("project");
    let source = root.join("src");
    fs::create_dir_all(source.join("entries")).unwrap();
    fs::create_dir_all(source.join("shared")).unwrap();
    fs::write(
        source.join("shared/model.ts"),
        "export interface User { id: string }\n",
    )
    .unwrap();
    fs::write(
        source.join("entries/main.ts"),
        "import type { User } from '@shared/model.ts';\nexport const mainUser: User = { id: 'main' };\n",
    )
    .unwrap();
    fs::write(
        source.join("entries/admin.ts"),
        "import type { User } from '@shared/model.ts';\nexport const adminUser: User = { id: 'admin' };\n",
    )
    .unwrap();
    let config = root.join("bluetsc.json");
    fs::write(
        &config,
        r#"{
  "entries": ["src/entries/main.ts", "src/entries/admin.ts"],
  "outDir": "dist",
  "sourceMap": true,
  "declaration": true,
  "imports": { "@shared/": "src/shared" }
}"#,
    )
    .unwrap();

    let checked = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
        .args(["check", "--config", config.to_str().unwrap()])
        .status()
        .unwrap();
    assert!(checked.success());
    let built = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
        .args(["build", "--config", config.to_str().unwrap()])
        .status()
        .unwrap();
    assert!(built.success());
    let output = root.join("dist");
    assert!(output.join("src/entries/main.js").is_file());
    assert!(output.join("src/entries/admin.js").is_file());
    assert!(output.join("src/shared/model.js").is_file());
    assert!(output.join("src/entries/main.d.ts").is_file());
    assert!(output.join("src/entries/main.js.map").is_file());
    let main_javascript = fs::read_to_string(output.join("src/entries/main.js")).unwrap();
    let manifest = fs::read_to_string(output.join("bluetsc.manifest.json")).unwrap();
    let import_map = fs::read_to_string(output.join("bluetsc.importmap.json")).unwrap();
    let source_map = fs::read_to_string(output.join("src/entries/main.js.map")).unwrap();
    assert!(manifest.contains("\"runtimePolicy\": \"checked\""));
    assert!(manifest.contains("\"src/entries/main.js\""));
    assert!(import_map.contains("\"@shared/\": \"./src/shared/\""));
    assert!(!manifest.contains(&root.to_string_lossy().into_owned()));
    assert!(!source_map.contains(&root.to_string_lossy().into_owned()));

    fs::write(
        source.join("entries/admin.ts"),
        "export const broken: number = 'wrong';\n",
    )
    .unwrap();
    let failed = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
        .args(["build", "--config", config.to_str().unwrap()])
        .status()
        .unwrap();
    assert!(!failed.success());
    assert_eq!(
        fs::read_to_string(output.join("src/entries/main.js")).unwrap(),
        main_javascript
    );
    assert_eq!(
        fs::read_to_string(output.join("bluetsc.manifest.json")).unwrap(),
        manifest
    );
    fs::remove_dir_all(temporary).unwrap();
}

#[test]
fn build_uses_root_confined_declaration_modules_without_emitting_runtime_artifacts() {
    let temporary = unique_test_directory();
    let root = temporary.join("project");
    let source = root.join("src");
    let types = root.join("types");
    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(&types).unwrap();
    fs::write(
        types.join("account.d.ts"),
        "export interface Account<T extends string = string> { id: T }\n",
    )
    .unwrap();
    fs::write(
        source.join("main.ts"),
        "import type { Account } from '@local/account';\nexport const account: Account = { id: 'ada' };\n",
    )
    .unwrap();
    let config = root.join("bluetsc.json");
    fs::write(
        &config,
        r#"{
  "entries": ["src/main.ts"],
  "outDir": "dist",
  "declaration": true,
  "imports": { "@local/account": "types/account.d.ts" }
}"#,
    )
    .unwrap();

    let checked = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
        .args(["check", "--config", config.to_str().unwrap()])
        .status()
        .unwrap();
    assert!(checked.success());
    let built = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
        .args(["build", "--config", config.to_str().unwrap()])
        .status()
        .unwrap();
    assert!(built.success());

    let output = root.join("dist");
    assert!(output.join("src/main.js").is_file());
    assert!(output.join("src/main.d.ts").is_file());
    assert!(!output.join("types/account.d.js").exists());
    assert_eq!(
        fs::read_to_string(output.join("types/account.d.ts")).unwrap(),
        "export interface Account<T extends string = string> { id: T }\n"
    );
    let main_declaration = fs::read_to_string(output.join("src/main.d.ts")).unwrap();
    assert!(main_declaration.contains("import type { Account } from '@local/account';"));
    assert!(main_declaration.contains("Account;"));
    let manifest = fs::read_to_string(output.join("bluetsc.manifest.json")).unwrap();
    assert!(manifest.contains("\"types/account.d.ts\""));
    let import_map = fs::read_to_string(output.join("bluetsc.importmap.json")).unwrap();
    assert!(!import_map.contains("@local/account"));
    fs::remove_dir_all(temporary).unwrap();
}

#[test]
fn repeated_config_builds_publish_byte_identical_artifacts() {
    let temporary = unique_test_directory();
    let root = temporary.join("project");
    let source = root.join("src");
    let types = root.join("types");
    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(&types).unwrap();
    fs::write(
        types.join("model.d.ts"),
        "export interface Model<T extends string = string> { id: T }\n",
    )
    .unwrap();
    fs::write(
        source.join("main.ts"),
        "import type { Model } from '@local/model';\nexport const model: Model = { id: 'stable' };\n",
    )
    .unwrap();
    let config = root.join("bluetsc.json");
    fs::write(
        &config,
        r#"{
  "entries": ["src/main.ts"],
  "outDir": "dist",
  "sourceMap": true,
  "declaration": true,
  "imports": { "@local/model": "types/model.d.ts" }
}"#,
    )
    .unwrap();

    for snapshot in 0..2 {
        let built = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
            .args(["build", "--config", config.to_str().unwrap()])
            .status()
            .unwrap();
        assert!(built.success());
        let current = build_snapshot(&root.join("dist"));
        if snapshot == 0 {
            fs::write(
                temporary.join("first-build.json"),
                serde_json::to_string(&current).unwrap(),
            )
            .unwrap();
        } else {
            let previous: BTreeMap<String, String> = serde_json::from_str(
                &fs::read_to_string(temporary.join("first-build.json")).unwrap(),
            )
            .unwrap();
            assert_eq!(current, previous);
        }
    }
    fs::remove_dir_all(temporary).unwrap();
}

fn build_snapshot(output: &Path) -> BTreeMap<String, String> {
    [
        "src/main.js",
        "src/main.js.map",
        "src/main.d.ts",
        "types/model.d.ts",
        "bluetsc.manifest.json",
        "bluetsc.importmap.json",
    ]
    .into_iter()
    .map(|path| {
        (
            path.to_string(),
            fs::read_to_string(output.join(path)).unwrap(),
        )
    })
    .collect()
}

fn unique_test_directory() -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let sequence = TEST_DIRECTORY_COUNTER.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "blueice-bluets-cli-{}-{nonce}-{sequence}",
        std::process::id()
    ));
    fs::create_dir(&path).unwrap();
    path
}
