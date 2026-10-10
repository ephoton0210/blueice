// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! End-to-end coverage for the standalone `bluetsc` process boundary.

use std::collections::BTreeMap;
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::symlink;
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
        "import type { User } from './model.ts';\nconst label: User = { id: 'Ada' };\nexport default label;\n",
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
    assert!(javascript.contains("const label"));
    assert!(javascript.contains("export default label"));
    assert!(output.join("src/main.js.map").is_file());
    let declaration = fs::read_to_string(output.join("src/main.d.ts")).unwrap();
    assert_eq!(
        declaration,
        "import type { User } from './model.ts';\ndeclare const label: User;\nexport default label;\n"
    );

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
fn class_fields_build_for_es2020_under_either_semantics_and_the_manifest_records_which() {
    let temporary = unique_test_directory();
    let root = temporary.join("project");
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("src/main.ts"),
        "class A { x: number = 1; y?: number; static s: number = 2; }\n\
         const a = new A();\n\
         console.log(Object.keys(a).join(','), 'y' in a, A.s);\n",
    )
    .unwrap();
    let node = Command::new("node").arg("--version").output().is_ok();
    // (arguments, whether `y` exists on the instance, manifest value)
    let flag_cases: [(&[&str], &str, bool); 3] = [
        (&["--target", "es2020"], "x false 2", false),
        (
            &[
                "--target",
                "es2020",
                "--use-define-for-class-fields",
                "true",
            ],
            "x,y true 2",
            true,
        ),
        (
            &[
                "--target",
                "es2022",
                "--use-define-for-class-fields",
                "false",
            ],
            "x false 2",
            false,
        ),
    ];
    for (index, (flags, expected_stdout, defines)) in flag_cases.iter().enumerate() {
        let output = temporary.join(format!("flags-{index}"));
        let built = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
            .args([
                "build",
                root.join("src/main.ts").to_str().unwrap(),
                "--project-root",
                root.to_str().unwrap(),
                "--out-dir",
                output.to_str().unwrap(),
            ])
            .args(*flags)
            .output()
            .unwrap();
        assert!(
            built.status.success(),
            "{}",
            String::from_utf8_lossy(&built.stderr)
        );
        let manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(output.join("bluetsc.manifest.json")).unwrap())
                .unwrap();
        assert_eq!(manifest["useDefineForClassFields"], *defines, "{flags:?}");
        if node {
            fs::write(output.join("package.json"), r#"{"type":"module"}"#).unwrap();
            let executed = Command::new("node")
                .arg(output.join("src/main.js"))
                .output()
                .unwrap();
            assert_eq!(
                String::from_utf8_lossy(&executed.stdout).trim(),
                *expected_stdout,
                "{flags:?}: {}",
                String::from_utf8_lossy(&executed.stderr)
            );
        }
    }

    // The same choice through a configuration file.
    let config = root.join("bluetsc.json");
    for (value, defines) in [(true, true), (false, false)] {
        let out_dir = format!("dist-config-{value}");
        fs::write(
            &config,
            serde_json::to_vec_pretty(&serde_json::json!({
                "entries": ["src/main.ts"],
                "outDir": out_dir,
                "target": "es2020",
                "useDefineForClassFields": value,
            }))
            .unwrap(),
        )
        .unwrap();
        let built = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
            .args(["build", "--config", config.to_str().unwrap()])
            .output()
            .unwrap();
        assert!(
            built.status.success(),
            "{}",
            String::from_utf8_lossy(&built.stderr)
        );
        let manifest: serde_json::Value = serde_json::from_slice(
            &fs::read(root.join(&out_dir).join("bluetsc.manifest.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(manifest["useDefineForClassFields"], defines);
        assert_eq!(manifest["target"], "es2020");
    }
    fs::remove_dir_all(temporary).unwrap();
}

#[test]
fn lowered_private_names_build_for_es2020_run_under_node_and_the_manifest_names_the_helper_version()
{
    let temporary = unique_test_directory();
    let root = temporary.join("project");
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("src/main.ts"),
        "class Counter {\n    #n: number = 0;\n    static #made: number = 0;\n    \
         static make(): Counter { Counter.#made += 1; return new Counter(); }\n    \
         bump(by: number): number { this.#n += by; return this.#n; }\n    \
         static get made(): number { return Counter.#made; }\n    \
         static has(o: any): boolean { return #n in o; }\n}\n\
         const c = Counter.make();\n\
         console.log(c.bump(2), c.bump(3), Counter.made, Counter.has(c), Counter.has({}));\n",
    )
    .unwrap();
    for target in ["es2020", "es2022"] {
        let output = temporary.join(format!("out-{target}"));
        let built = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
            .args([
                "build",
                root.join("src/main.ts").to_str().unwrap(),
                "--project-root",
                root.to_str().unwrap(),
                "--out-dir",
                output.to_str().unwrap(),
                "--target",
                target,
            ])
            .output()
            .unwrap();
        assert!(
            built.status.success(),
            "{}",
            String::from_utf8_lossy(&built.stderr)
        );
        let javascript = fs::read_to_string(output.join("src/main.js")).unwrap();
        assert_eq!(
            javascript.contains("#n"),
            target == "es2022",
            "{target}: {javascript}"
        );
        let manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(output.join("bluetsc.manifest.json")).unwrap())
                .unwrap();
        assert_eq!(manifest["classHelperVersion"], "bluets-class-helper-v1");
        if Command::new("node").arg("--version").output().is_ok() {
            fs::write(output.join("package.json"), r#"{"type":"module"}"#).unwrap();
            let executed = Command::new("node")
                .arg(output.join("src/main.js"))
                .output()
                .unwrap();
            assert_eq!(
                String::from_utf8_lossy(&executed.stdout).trim(),
                "2 5 1 true false",
                "{target}: {}",
                String::from_utf8_lossy(&executed.stderr)
            );
        }
    }
    fs::remove_dir_all(temporary).unwrap();
}

#[test]
fn const_enums_build_inlined_preserved_or_isolated_and_the_manifest_says_which() {
    let temporary = unique_test_directory();
    let root = temporary.join("project");
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("src/main.ts"),
        "const enum E { A = 1, B }\nconsole.log(E.A + E.B, E['B']);\n",
    )
    .unwrap();
    let cases: [(&[&str], bool, bool, bool); 3] = [
        (&[], false, true, false),
        (&["--preserve-const-enums"], true, true, true),
        (&["--isolated-modules"], false, false, true),
    ];
    for (index, (flags, preserve, inline, object)) in cases.iter().enumerate() {
        let output = temporary.join(format!("out-{index}"));
        let built = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
            .args([
                "build",
                root.join("src/main.ts").to_str().unwrap(),
                "--project-root",
                root.to_str().unwrap(),
                "--out-dir",
                output.to_str().unwrap(),
            ])
            .args(*flags)
            .output()
            .unwrap();
        assert!(
            built.status.success(),
            "{}",
            String::from_utf8_lossy(&built.stderr)
        );
        let javascript = fs::read_to_string(output.join("src/main.js")).unwrap();
        assert_eq!(
            javascript.contains("var E;"),
            *object,
            "{flags:?}: {javascript}"
        );
        assert_eq!(
            javascript.contains("/* E.A */"),
            *inline,
            "{flags:?}: {javascript}"
        );
        let manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(output.join("bluetsc.manifest.json")).unwrap())
                .unwrap();
        assert_eq!(manifest["preserveConstEnums"], *preserve, "{flags:?}");
        assert_eq!(manifest["inlineConstEnums"], *inline, "{flags:?}");
        if Command::new("node").arg("--version").output().is_ok() {
            fs::write(output.join("package.json"), r#"{"type":"module"}"#).unwrap();
            let executed = Command::new("node")
                .arg(output.join("src/main.js"))
                .output()
                .unwrap();
            assert_eq!(
                String::from_utf8_lossy(&executed.stdout).trim(),
                "3 2",
                "{flags:?}"
            );
        }
    }
    // The same choices through a configuration file.
    let config = root.join("bluetsc.json");
    fs::write(
        &config,
        serde_json::to_vec_pretty(&serde_json::json!({
            "entries": ["src/main.ts"],
            "outDir": "dist-config",
            "isolatedModules": true,
            "preserveConstEnums": true,
        }))
        .unwrap(),
    )
    .unwrap();
    let built = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
        .args(["build", "--config", config.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        built.status.success(),
        "{}",
        String::from_utf8_lossy(&built.stderr)
    );
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("dist-config/bluetsc.manifest.json")).unwrap())
            .unwrap();
    assert_eq!(manifest["inlineConstEnums"], false);
    assert_eq!(manifest["preserveConstEnums"], true);
    fs::remove_dir_all(temporary).unwrap();
}

#[test]
fn strict_config_builds_only_complete_string_boundaries_for_both_targets() {
    let temporary = unique_test_directory();
    let root = temporary.join("project");
    fs::create_dir_all(root.join("src")).unwrap();
    let source_file = root.join("src/main.ts");
    let config_file = root.join("bluetsc.json");
    let good_source = "export function echo(value: string): string { return value; }";
    for target in ["es2020", "es2022"] {
        fs::write(&source_file, good_source).unwrap();
        let out_dir = format!("dist-{target}");
        let mut config = serde_json::json!({
            "entries": ["src/main.ts"],
            "outDir": out_dir,
            "target": target,
            "runtimePolicy": "strict-runtime",
            "sourceMap": true,
            "declaration": true,
            "strictBoundaries": [{
                "contractId": "echo-string-v1",
                "module": "src/main.ts",
                "function": "echo",
                "sourceStart": 0,
                "sourceEnd": good_source.len(),
                "maxStringBytes": 64,
                "helperVersion": "bluets-runtime-helper-v1"
            }]
        });
        fs::write(&config_file, serde_json::to_vec_pretty(&config).unwrap()).unwrap();
        let built = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
            .args(["build", "--config", config_file.to_str().unwrap()])
            .output()
            .unwrap();
        assert!(
            built.status.success(),
            "{}",
            String::from_utf8_lossy(&built.stderr)
        );
        let output = root.join(&out_dir);
        let javascript = fs::read_to_string(output.join("src/main.js")).unwrap();
        let manifest_bytes = fs::read(output.join("bluetsc.manifest.json")).unwrap();
        let manifest: serde_json::Value = serde_json::from_slice(&manifest_bytes).unwrap();
        assert_eq!(manifest["runtimePolicy"], "strict-runtime");
        assert_eq!(manifest["target"], target);
        assert_eq!(
            manifest["strictBoundaries"][0]["contractId"],
            "echo-string-v1"
        );
        assert_eq!(manifest["strictArtifacts"][0]["module"], "src/main.ts");
        assert!(javascript.contains("from '../bluets.runtime-helper.v1.mjs'"));
        assert_eq!(javascript.matches("__bluetsValidateStringV1(").count(), 2);
        assert!(output.join("bluets.runtime-helper.v1.mjs").is_file());
        assert!(output.join("src/main.js.map").is_file());
        assert!(output.join("src/main.d.ts").is_file());
        if Command::new("node").arg("--version").output().is_ok() {
            fs::write(output.join("package.json"), r#"{"type":"module"}"#).unwrap();
            let executed = Command::new("node")
                .args([
                    "--input-type=module",
                    "-e",
                    r#"import { echo } from './src/main.js';
if (echo('é') !== 'é') process.exit(1);
for (const value of [42, 'x'.repeat(65), '\uD800']) {
  let refused = false;
  try { echo(value); } catch (error) {
    refused = error === 'BlueTS runtime contract rejected the value';
  }
  if (!refused) process.exit(2);
}"#,
                ])
                .current_dir(&output)
                .output()
                .unwrap();
            assert!(
                executed.status.success(),
                "{}",
                String::from_utf8_lossy(&executed.stderr)
            );
        }

        let bad_source = "export function echo(value: string): string { return globalText(); }";
        fs::write(&source_file, bad_source).unwrap();
        config["strictBoundaries"][0]["sourceEnd"] = bad_source.len().into();
        fs::write(&config_file, serde_json::to_vec_pretty(&config).unwrap()).unwrap();
        let refused = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
            .args(["build", "--config", config_file.to_str().unwrap()])
            .output()
            .unwrap();
        assert!(!refused.status.success());
        assert!(String::from_utf8_lossy(&refused.stderr).contains("BTS4000"));
        assert_eq!(
            fs::read_to_string(output.join("src/main.js")).unwrap(),
            javascript
        );
        assert_eq!(
            fs::read(output.join("bluetsc.manifest.json")).unwrap(),
            manifest_bytes
        );
    }
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

#[cfg(unix)]
#[test]
fn config_build_rejects_an_output_directory_with_a_symlinked_parent_outside_the_root() {
    let temporary = unique_test_directory();
    let root = temporary.join("project");
    let source = root.join("src");
    let outside = temporary.join("outside");
    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(&outside).unwrap();
    fs::write(source.join("main.ts"), "export const answer: number = 1;\n").unwrap();
    symlink(&outside, root.join("linked")).unwrap();
    let config = root.join("bluetsc.json");
    fs::write(
        &config,
        r#"{
  "entries": ["src/main.ts"],
  "outDir": "linked/dist"
}"#,
    )
    .unwrap();

    let status = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
        .args(["build", "--config", config.to_str().unwrap()])
        .status()
        .unwrap();
    assert!(!status.success());
    assert!(!outside.join("dist").exists());
    fs::remove_dir_all(temporary).unwrap();
}

#[test]
fn config_build_rejects_an_output_directory_that_contains_source_modules() {
    let temporary = unique_test_directory();
    let root = temporary.join("project");
    let source = root.join("src");
    fs::create_dir_all(&source).unwrap();
    let entry = source.join("main.ts");
    let original = "export const answer: number = 1;\n";
    fs::write(&entry, original).unwrap();
    let config = root.join("bluetsc.json");
    fs::write(
        &config,
        r#"{
  "entries": ["src/main.ts"],
  "outDir": "src"
}"#,
    )
    .unwrap();

    let status = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
        .args(["build", "--config", config.to_str().unwrap()])
        .status()
        .unwrap();
    assert!(!status.success());
    assert_eq!(fs::read_to_string(entry).unwrap(), original);
    assert!(!source.join("main.js").exists());
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

#[test]
fn builds_commonjs_output_runs_it_under_node_and_records_the_module_system() {
    let temporary = std::env::temp_dir().join(format!("bluetsc-commonjs-{}", std::process::id()));
    let _ = fs::remove_dir_all(&temporary);
    fs::create_dir_all(&temporary).unwrap();
    fs::write(
        temporary.join("main.ts"),
        "import { two } from \"./lib.ts\";\nexport const answer = two() * 21;\nconsole.log(answer);\n",
    )
    .unwrap();
    fs::write(
        temporary.join("lib.ts"),
        "export function two(): number { return 2; }\n",
    )
    .unwrap();
    for (flags, module) in [
        (&["--module", "commonjs"][..], "commonjs"),
        (&["--module", "amd"][..], "amd"),
        (&["--module", "umd"][..], "umd"),
        (&[][..], "esm"),
    ] {
        let output = temporary.join(format!("out-{module}"));
        let built = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
            .current_dir(&temporary)
            .args(["build", "main.ts", "--out-dir", output.to_str().unwrap()])
            .args(flags)
            .output()
            .unwrap();
        assert!(
            built.status.success(),
            "{}",
            String::from_utf8_lossy(&built.stderr)
        );
        let manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(output.join("bluetsc.manifest.json")).unwrap())
                .unwrap();
        assert_eq!(manifest["module"], module);
        let javascript = fs::read_to_string(output.join("main.js")).unwrap();
        assert_eq!(
            javascript.contains("require("),
            module != "esm",
            "{javascript}"
        );
        if module == "commonjs" && Command::new("node").arg("--version").output().is_ok() {
            fs::write(output.join("package.json"), r#"{"type":"commonjs"}"#).unwrap();
            let executed = Command::new("node")
                .arg(output.join("main.js"))
                .output()
                .unwrap();
            assert_eq!(String::from_utf8_lossy(&executed.stdout).trim(), "42");
        }
    }
    let rejected = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
        .current_dir(&temporary)
        .args(["check", "main.ts", "--module", "unsupported-kind"])
        .output()
        .unwrap();
    assert!(!rejected.status.success());
    assert!(
        String::from_utf8_lossy(&rejected.stderr).contains("unsupported module `unsupported-kind`")
    );
    fs::remove_dir_all(temporary).unwrap();
}
