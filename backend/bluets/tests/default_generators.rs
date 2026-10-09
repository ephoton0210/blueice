// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Default generator naming preserves executable syntax in ESM and CommonJS.

use serde_json::json;
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

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
fn default_generators_match_pinned_execution_names_and_declarations() {
    let tsc = PathBuf::from(env::var_os("BLUEICE_BLUETSC_ORACLE").unwrap());
    let version = Command::new(&tsc).arg("--version").output().unwrap();
    assert!(version.status.success(), "{}", report(&version));
    assert_eq!(
        String::from_utf8_lossy(&version.stdout).trim(),
        "Version 5.9.3"
    );
    let root = fs::canonicalize(env::temp_dir())
        .unwrap()
        .join(format!("bluets-default-generators-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let mut references = Vec::new();
    for module in ["es2022", "commonjs"] {
        for asynchronous in [false, true] {
            for named in [false, true] {
                let name = format!("{module}-async-{asynchronous}-named-{named}");
                let directory = root.join(&name);
                fs::create_dir_all(&directory).unwrap();
                let (main, dependency) = if asynchronous {
                    (
                        include_str!("fixtures/default_generators/main-async.ts"),
                        include_str!("fixtures/default_generators/dep-async.ts"),
                    )
                } else {
                    (
                        include_str!("fixtures/default_generators/main-sync.ts"),
                        include_str!("fixtures/default_generators/dep-sync.ts"),
                    )
                };
                fs::write(directory.join("main.ts"), main).unwrap();
                fs::write(
                    directory.join("dep.ts"),
                    if named {
                        dependency.replace("function* (", "function* generate(")
                    } else {
                        dependency.to_string()
                    },
                )
                .unwrap();
                fs::write(
                    directory.join("package.json"),
                    json!({"type":if module == "commonjs" {"commonjs"} else {"module"}})
                        .to_string(),
                )
                .unwrap();
                let config = directory.join("tsconfig.json");
                fs::write(
                    &config,
                    json!({"compilerOptions":{"target":"ES2022","module":module,"strict":true,
                        "declaration":true,"allowImportingTsExtensions":true,
                        "rewriteRelativeImportExtensions":true,"outDir":directory.join("blue")},
                        "files":["main.ts"]})
                    .to_string(),
                )
                .unwrap();
                let reference = directory.join("reference");
                let built = Command::new(&tsc)
                    .arg("--project")
                    .arg(&config)
                    .arg("--outDir")
                    .arg(&reference)
                    .output()
                    .unwrap();
                assert!(built.status.success(), "{name}: {}", report(&built));
                let executed = Command::new("node")
                    .arg(reference.join("main.js"))
                    .output()
                    .unwrap();
                assert!(executed.status.success(), "{name}: {}", report(&executed));
                let expected_name = if named {
                    "generate"
                } else if module == "commonjs" {
                    "default_1"
                } else {
                    "default"
                };
                assert_eq!(
                    String::from_utf8_lossy(&executed.stdout),
                    format!("{expected_name} 41 42\n"),
                    "{name}"
                );
                references.push((
                    name,
                    directory,
                    config,
                    asynchronous,
                    executed.stdout,
                    declarations(&reference),
                ));
            }
        }
    }
    // Execute every pinned witness before collecting BlueTSC differences.
    assert_eq!(references.len(), 8);
    let mut failures = Vec::new();
    for (name, directory, config, asynchronous, expected_stdout, expected_declarations) in
        references
    {
        let built = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
            .arg("--project")
            .arg(config)
            .output()
            .unwrap();
        // Async generators retain the existing parser boundary. Their pinned
        // programs remain reference controls; no runtime output is promised.
        if asynchronous {
            let expected = "BTS1001: an async generator is not supported yet";
            if built.status.success()
                || !report(&built).contains(expected)
                || directory.join("blue").exists()
            {
                failures.push(format!(
                    "{name}: async generator refusal differs: {}",
                    report(&built)
                ));
            }
            continue;
        }
        if !built.status.success() {
            failures.push(format!("{name}: build: {}", report(&built)));
            continue;
        }
        let blue = directory.join("blue");
        let executed = Command::new("node")
            .arg(blue.join("main.js"))
            .output()
            .unwrap();
        if !executed.status.success() || executed.stdout != expected_stdout {
            failures.push(format!("{name}: execution differs: {}", report(&executed)));
        }
        if declarations(&blue) != expected_declarations {
            failures.push(format!("{name}: exact declarations differ"));
        }
    }
    fs::remove_dir_all(root).unwrap();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
