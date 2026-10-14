// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Declaration publishing preserves the CLI's canonical owner-root boundary.

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::{env, fs};

struct Temporary(PathBuf);

impl Temporary {
    fn new(label: &str) -> Self {
        let root = fs::canonicalize(env::temp_dir()).unwrap().join(format!(
            "bluets-declaration-boundary-{label}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        Self(root)
    }

    fn project(&self, declaration_dir: &str) -> PathBuf {
        let project = self.0.join("project");
        fs::create_dir_all(&project).unwrap();
        fs::write(
            project.join("main.ts"),
            "export const answer: number = 42;\n",
        )
        .unwrap();
        fs::write(
            project.join("tsconfig.json"),
            serde_json::to_vec_pretty(&json!({
                "compilerOptions": {
                    "target": "ES2022", "module": "ESNext", "strict": true,
                    "declaration": true, "declarationDir": declaration_dir,
                    "sourceMap": true, "outDir": "out"
                },
                "files": ["main.ts"]
            }))
            .unwrap(),
        )
        .unwrap();
        project
    }
}

impl Drop for Temporary {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn build(project: &Path, flags: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_bluetsc"))
        .arg("--project")
        .arg(project.join("tsconfig.json"))
        .args(flags)
        .output()
        .unwrap()
}

#[test]
fn declaration_only_cli_overrides_publish_maps_without_javascript() {
    let root = Temporary::new("only");
    let project = root.project("types");
    let output = build(&project, &["--emitDeclarationOnly", "--declarationMap"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!project.join("out").exists());
    let declaration = fs::read_to_string(project.join("types/main.d.ts")).unwrap();
    assert_eq!(
        declaration,
        "export declare const answer: number;\n//# sourceMappingURL=main.d.ts.map"
    );
    let map: Value =
        serde_json::from_slice(&fs::read(project.join("types/main.d.ts.map")).unwrap()).unwrap();
    assert_eq!(map["file"], "main.d.ts");
    assert_eq!(map["sources"], json!(["../main.ts"]));
    assert!(map.get("sourcesContent").is_none());
}

#[test]
fn declaration_directory_escape_refuses_before_any_publication() {
    let root = Temporary::new("escape");
    let outside = root.0.join("outside");
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("owner.txt"), "preserve").unwrap();
    let project = root.project("../outside");
    let output = build(&project, &[]);
    assert!(!output.status.success());
    assert!(!project.join("out").exists());
    assert_eq!(
        fs::read_to_string(outside.join("owner.txt")).unwrap(),
        "preserve"
    );
    assert_eq!(fs::read_dir(&outside).unwrap().count(), 1);
}

#[test]
#[cfg(unix)]
fn declaration_directory_symlink_refuses_before_any_publication() {
    use std::os::unix::fs::symlink;

    let root = Temporary::new("symlink");
    let outside = root.0.join("outside");
    fs::create_dir_all(&outside).unwrap();
    let project = root.project("types");
    symlink(&outside, project.join("types")).unwrap();
    let output = build(&project, &[]);
    assert!(!output.status.success());
    assert!(!project.join("out").exists());
    assert_eq!(fs::read_dir(&outside).unwrap().count(), 0);
}
