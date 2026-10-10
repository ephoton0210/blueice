// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Path and type-library options retain the CLI's owner-root boundary.

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::{env, fs};

struct Tree(PathBuf);

impl Tree {
    fn new(label: &str) -> Self {
        let root = fs::canonicalize(env::temp_dir()).unwrap().join(format!(
            "bluets-path-boundary-{label}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("app")).unwrap();
        Self(root)
    }

    fn write(&self, name: &str, text: &str) {
        let path = self.0.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn config(&self, options: Value) {
        let mut compiler = json!({
            "target": "ES2022", "module": "CommonJS", "moduleResolution": "Node10",
            "strict": true, "declaration": true, "outDir": "out"
        });
        compiler
            .as_object_mut()
            .unwrap()
            .extend(options.as_object().unwrap().clone());
        self.write(
            "app/tsconfig.json",
            &json!({
                "compilerOptions": compiler, "files": ["main.ts"]
            })
            .to_string(),
        );
    }

    fn run(&self, flags: &[&str]) -> Output {
        run(&self.0.join("app/tsconfig.json"), flags)
    }

    fn assert_confined_refusal(&self) {
        let output = self.run(&["--diagnostics-json"]);
        assert!(!output.status.success());
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(error.contains("outside"), "{error}");
        assert!(!error.contains("OUTSIDE_BODY"), "{error}");
        assert!(!self.0.join("app/out").exists());
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn run(config: &Path, flags: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_bluetsc"))
        .arg("--project")
        .arg(config)
        .args(flags)
        .output()
        .unwrap()
}

#[test]
fn mapped_target_escape_is_refused_before_publication() {
    let tree = Tree::new("escape");
    tree.write(
        "app/main.ts",
        "import type { Answer } from 'answer'; export const result: Answer = 42;\n",
    );
    tree.write("outside.d.ts", "OUTSIDE_BODY\n");
    tree.config(json!({"paths": {"answer": ["../outside.d.ts"]}}));
    tree.assert_confined_refusal();
}

#[test]
#[cfg(unix)]
fn wildcard_target_symlink_is_refused_before_publication() {
    let tree = Tree::new("wildcard-link");
    tree.write(
        "app/main.ts",
        "import type { Answer } from '@lib/answer'; export const result: Answer = 42;\n",
    );
    tree.write(
        "app/types/placeholder.d.ts",
        "export type Unused = number;\n",
    );
    tree.write("outside.d.ts", "OUTSIDE_BODY\n");
    std::os::unix::fs::symlink(
        tree.0.join("outside.d.ts"),
        tree.0.join("app/types/answer.d.ts"),
    )
    .unwrap();
    tree.config(json!({"paths": {"@lib/*": ["./types/*.d.ts"]}}));
    tree.assert_confined_refusal();
}

#[test]
#[cfg(unix)]
fn virtual_root_symlink_is_refused_before_publication() {
    let tree = Tree::new("virtual-link");
    tree.write(
        "app/src/main.ts",
        "import type { Answer } from './answer'; export const result: Answer = 42;\n",
    );
    tree.write("outside/answer.d.ts", "OUTSIDE_BODY\n");
    std::os::unix::fs::symlink(tree.0.join("outside"), tree.0.join("app/generated")).unwrap();
    tree.config(json!({"rootDirs": ["src", "generated"]}));
    tree.write("app/main.ts", "export const result: number = 42;\n");
    tree.assert_confined_refusal();
}

#[test]
#[cfg(unix)]
fn automatic_type_library_symlink_is_refused_before_publication() {
    let tree = Tree::new("types-link");
    tree.write("app/main.ts", "export const result: Answer = 42;\n");
    tree.write(
        "app/node_modules/@types/placeholder/index.d.ts",
        "declare type Unused = number;\n",
    );
    tree.write("outside/index.d.ts", "OUTSIDE_BODY\n");
    std::os::unix::fs::symlink(
        tree.0.join("outside"),
        tree.0.join("app/node_modules/@types/globals"),
    )
    .unwrap();
    tree.config(json!({}));
    tree.assert_confined_refusal();
}

#[test]
fn inherited_paths_without_base_url_retain_the_declaring_directory() {
    let tree = Tree::new("inherited");
    tree.write(
        "app/main.ts",
        "import type { Answer } from 'answer'; export const result: Answer = 42;\n",
    );
    tree.write(
        "app/config/types/answer.d.ts",
        "export type Answer = number;\n",
    );
    tree.write("app/types/answer.d.ts", "export type Answer = string;\n");
    tree.write(
        "app/config/base.json",
        &json!({"compilerOptions": {"paths": {"answer": ["./types/answer.d.ts"]}}}).to_string(),
    );
    tree.config(json!({}));
    let config = tree.0.join("app/tsconfig.json");
    let mut value: Value = serde_json::from_slice(&fs::read(&config).unwrap()).unwrap();
    value["extends"] = json!("./config/base.json");
    fs::write(&config, value.to_string()).unwrap();
    let output = tree.run(&["--listFiles", "--noEmit"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let files = String::from_utf8_lossy(&output.stdout).replace('\\', "/");
    assert!(files.contains("config/types/answer.d.ts"), "{files}");
    assert!(!files
        .lines()
        .any(|file| file.ends_with("/app/types/answer.d.ts")));
}

#[test]
fn changed_automatic_library_is_checked_and_empty_types_suppresses_it() {
    let tree = Tree::new("changed-types");
    tree.write("app/main.ts", "export const result: Answer = 42;\n");
    tree.write(
        "app/node_modules/@types/globals/index.d.ts",
        "declare type Answer = number;\n",
    );
    tree.config(json!({}));
    let output = tree.run(&["--noEmit"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    tree.write(
        "app/node_modules/@types/globals/index.d.ts",
        "declare type Answer = string;\n",
    );
    let output = tree.run(&["--noEmit", "--diagnostics-json"]);
    assert!(!output.status.success());
    let diagnostics: Vec<Value> = String::from_utf8_lossy(&output.stderr)
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(diagnostics[0]["typescript"]["code"], 2322);
    tree.config(json!({"types": []}));
    let output = tree.run(&["--noEmit", "--diagnostics-json"]);
    assert!(!output.status.success());
    let diagnostics: Vec<Value> = String::from_utf8_lossy(&output.stderr)
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(diagnostics[0]["typescript"]["code"], 2304);
}

#[test]
fn explicit_owner_roots_allow_external_type_libraries_with_stable_identities() {
    let tree = Tree::new("owner-types");
    tree.write("app/main.ts", "export const result: Answer = 42;\n");
    tree.write(
        "shared/globals/index.d.ts",
        "declare type Answer = number;\n",
    );
    tree.write(
        "app/bluetsc.json",
        &json!({"packageRoots": ["../shared"]}).to_string(),
    );
    tree.config(json!({"typeRoots": ["../shared"], "types": ["globals"]}));
    let output = tree.run(&["--noEmit", "--listFiles"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout)
        .replace('\\', "/")
        .contains("shared/globals/index.d.ts"));
    tree.write(
        "shared/globals/index.d.ts",
        "declare type Answer = string;\n",
    );
    let output = tree.run(&["--noEmit", "--diagnostics-json"]);
    assert!(!output.status.success());
    let diagnostic: Value = serde_json::from_str(
        String::from_utf8_lossy(&output.stderr)
            .lines()
            .next()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(diagnostic["typescript"]["code"], 2322);
}

#[test]
fn changing_a_type_library_manifest_changes_the_build_fingerprint() {
    let tree = Tree::new("type-manifest");
    tree.write("app/main.ts", "export const result: Answer = 42;\n");
    tree.write(
        "app/node_modules/@types/globals/index.d.ts",
        "declare type Answer = number;\n",
    );
    tree.config(json!({}));
    let mut fingerprints = Vec::new();
    for version in ["1", "2"] {
        tree.write(
            "app/node_modules/@types/globals/package.json",
            &json!({"types": "index.d.ts", "version": version}).to_string(),
        );
        let output = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
            .args(["build", "--config"])
            .arg(tree.0.join("app/tsconfig.json"))
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let manifest: Value = serde_json::from_slice(
            &fs::read(tree.0.join("app/out/bluetsc.manifest.json")).unwrap(),
        )
        .unwrap();
        fingerprints.push(manifest["fingerprint"].clone());
    }
    assert_ne!(fingerprints[0], fingerprints[1]);
}
