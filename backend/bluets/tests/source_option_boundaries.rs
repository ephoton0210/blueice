// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! JavaScript checking and JSON source selection preserve the public CLI boundary.

use serde_json::{json, Value};
use std::path::PathBuf;
use std::process::{Command, Output};
use std::{env, fs};

struct Tree(PathBuf);

impl Tree {
    fn new(label: &str) -> Self {
        let root = fs::canonicalize(env::temp_dir()).unwrap().join(format!(
            "bluets-source-boundary-{label}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("app")).unwrap();
        Self(root)
    }

    fn write(&self, name: &str, body: &str) {
        let path = self.0.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }

    fn config(&self, entry: &str, options: Value) {
        let mut compiler = json!({
            "target": "ES2022", "module": "CommonJS", "moduleResolution": "Node10",
            "strict": true, "declaration": true, "noEmitOnError": true, "outDir": "out"
        });
        compiler
            .as_object_mut()
            .unwrap()
            .extend(options.as_object().unwrap().clone());
        self.write(
            "app/tsconfig.json",
            &json!({
                "compilerOptions": compiler, "files": [entry]
            })
            .to_string(),
        );
    }

    fn run(&self) -> Output {
        Command::new(env!("CARGO_BIN_EXE_bluetsc"))
            .arg("--project")
            .arg(self.0.join("app/tsconfig.json"))
            .arg("--diagnostics-json")
            .output()
            .unwrap()
    }

    fn success(&self) {
        let output = self.run();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn refusal(&self, expected: &str) {
        let output = self.run();
        assert!(!output.status.success());
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(error.contains(expected), "{error}");
        assert!(!error.contains("OUTSIDE_BODY"), "{error}");
        assert!(!self.0.join("app/out").exists());
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn unchecked_javascript_does_not_disable_typescript_importer_checking() {
    let tree = Tree::new("importer");
    tree.write("app/value.js", "export const answer = 42;\n");
    tree.write(
        "app/main.ts",
        "import { answer } from './value.js'; export const result: string = answer;\n",
    );
    tree.config("main.ts", json!({"allowJs": true, "checkJs": false}));
    tree.refusal("2322");
}

#[test]
fn explicit_javascript_import_prefers_a_typescript_sibling() {
    let tree = Tree::new("sibling");
    tree.write("app/value.js", "export const answer = 'wrong';\n");
    tree.write("app/value.ts", "export const answer = 42;\n");
    tree.write(
        "app/main.ts",
        "import { answer } from './value.js'; export const result: number = answer;\n",
    );
    tree.config("main.ts", json!({"allowJs": true, "checkJs": true}));
    tree.success();
    let emitted = fs::read_to_string(tree.0.join("app/out/value.js")).unwrap();
    assert!(emitted.contains("42"), "{emitted}");
    assert!(!emitted.contains("wrong"), "{emitted}");
}

#[test]
fn changing_check_js_rechecks_a_previously_accepted_source() {
    let tree = Tree::new("recheck");
    tree.write(
        "app/main.js",
        "let value = 42; value = 'wrong'; export const result = [42];\n",
    );
    tree.config("main.js", json!({"allowJs": true, "checkJs": false}));
    tree.success();
    fs::remove_dir_all(tree.0.join("app/out")).unwrap();
    tree.config("main.js", json!({"allowJs": true, "checkJs": true}));
    tree.refusal("2322");
}

#[test]
fn source_policy_options_contribute_to_the_build_fingerprint() {
    let tree = Tree::new("fingerprint");
    tree.write("app/main.ts", "export const result = 42;\n");
    let mut fingerprints = Vec::new();
    for options in [
        json!({"allowJs": false, "checkJs": false}),
        json!({"allowJs": true, "checkJs": false}),
        json!({"allowJs": true, "checkJs": true}),
    ] {
        tree.config("main.ts", options);
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
    assert_ne!(fingerprints[1], fingerprints[2]);
}

#[test]
#[cfg(unix)]
fn javascript_symlink_escape_is_refused_before_reading_the_body() {
    let tree = Tree::new("javascript-link");
    tree.write(
        "app/main.ts",
        "import { answer } from './value.js'; export const result = answer;\n",
    );
    tree.write("outside.js", "OUTSIDE_BODY\n");
    std::os::unix::fs::symlink(tree.0.join("outside.js"), tree.0.join("app/value.js")).unwrap();
    tree.config("main.ts", json!({"allowJs": true, "checkJs": false}));
    tree.refusal("outside");
}

#[test]
#[cfg(unix)]
fn json_symlink_escape_is_refused_before_reading_the_body() {
    let tree = Tree::new("json-link");
    tree.write(
        "app/main.ts",
        "import data from './data.json'; export const result = data.answer;\n",
    );
    tree.write("outside.json", "OUTSIDE_BODY\n");
    std::os::unix::fs::symlink(tree.0.join("outside.json"), tree.0.join("app/data.json")).unwrap();
    tree.config(
        "main.ts",
        json!({"resolveJsonModule": true, "esModuleInterop": true}),
    );
    tree.refusal("outside");
}
