// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Type-only attributes choose each package condition through the public CLI.

use serde_json::json;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::{env, fs};

fn report(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn declarations(directory: &Path) -> BTreeMap<String, String> {
    fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.to_string_lossy().ends_with(".d.ts"))
        .map(|path| {
            (
                path.file_name().unwrap().to_string_lossy().into_owned(),
                fs::read_to_string(path).unwrap().replace("\r\n", "\n"),
            )
        })
        .collect()
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3 and Node"]
fn type_only_imports_and_reexports_select_distinct_conditions_for_the_same_specifier() {
    let tsc = PathBuf::from(env::var_os("BLUEICE_BLUETSC_ORACLE").unwrap());
    let version = Command::new(&tsc).arg("--version").output().unwrap();
    assert!(version.status.success(), "{}", report(&version));
    assert_eq!(
        String::from_utf8_lossy(&version.stdout).trim(),
        "Version 5.9.3"
    );
    let root = fs::canonicalize(env::temp_dir()).unwrap().join(format!(
        "bluets-type-resolution-conditions-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    let mut references = Vec::new();
    for (name, main) in [
        (
            "imports",
            include_str!("fixtures/module_type_resolution/imports.ts"),
        ),
        (
            "reexports",
            include_str!("fixtures/module_type_resolution/reexports.ts"),
        ),
    ] {
        let directory = root.join(name);
        let package = directory.join("node_modules/conditional-types");
        fs::create_dir_all(&package).unwrap();
        fs::write(directory.join("main.ts"), main).unwrap();
        fs::write(directory.join("package.json"), "{\"type\":\"module\"}").unwrap();
        if name == "reexports" {
            fs::write(
                directory.join("barrel.ts"),
                include_str!("fixtures/module_type_resolution/barrel.ts"),
            )
            .unwrap();
        }
        fs::write(
            package.join("package.json"),
            json!({
                "name":"conditional-types", "type":"module", "exports":{
                    ".":{"import":{"types":"./import.d.ts"},"require":{"types":"./require.d.ts"}}
                }
            })
            .to_string(),
        )
        .unwrap();
        fs::write(
            package.join("import.d.ts"),
            include_str!("fixtures/module_type_resolution/import.d.ts"),
        )
        .unwrap();
        fs::write(
            package.join("require.d.ts"),
            include_str!("fixtures/module_type_resolution/require.d.ts"),
        )
        .unwrap();
        let config = directory.join("tsconfig.json");
        let blue = directory.join("blue");
        let reference = directory.join("reference");
        fs::write(
            &config,
            json!({"compilerOptions":{
            "target":"ES2022", "module":"ESNext", "moduleResolution":"bundler",
            "strict":true, "declaration":true, "outDir":blue
        }, "files":["main.ts"]})
            .to_string(),
        )
        .unwrap();
        let built = Command::new(&tsc)
            .arg("--project")
            .arg(&config)
            .arg("--outDir")
            .arg(&reference)
            .output()
            .unwrap();
        assert!(built.status.success(), "{name}: {}", report(&built));
        let run = Command::new("node")
            .arg(reference.join("main.js"))
            .output()
            .unwrap();
        assert!(run.status.success(), "{name}: {}", report(&run));
        assert_eq!(run.stdout, b"3\n", "{name}");
        references.push((name, config, blue, run.stdout, declarations(&reference)));
    }
    let mut failures = Vec::new();
    for (name, config, blue, stdout, expected) in references {
        let built = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
            .arg("--project")
            .arg(config)
            .output()
            .unwrap();
        if !built.status.success() {
            failures.push(format!("{name}: {}", report(&built)));
            continue;
        }
        let run = Command::new("node")
            .arg(blue.join("main.js"))
            .output()
            .unwrap();
        if !run.status.success() || run.stdout != stdout {
            failures.push(format!("{name}: execution differs: {}", report(&run)));
        }
        let actual = declarations(&blue);
        if actual != expected {
            failures.push(format!(
                "{name}: declarations differ:\nBlue: {actual:#?}\nTypeScript: {expected:#?}"
            ));
        }
    }
    fs::remove_dir_all(root).unwrap();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
