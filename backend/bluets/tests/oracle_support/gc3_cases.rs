// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Materialize unchanged frozen G-C3 programs at the public CLI boundary.

use serde_json::Value;
use std::path::PathBuf;
use std::process::Command;
use std::{env, fs};

pub(crate) struct Temporary(pub(crate) PathBuf);

impl Temporary {
    pub(crate) fn new(label: &str) -> Self {
        let root = fs::canonicalize(env::temp_dir()).unwrap().join(format!(
            "bluets-gc3-provider-{label}-{}",
            std::process::id()
        ));
        if root.exists() {
            fs::remove_dir_all(&root).unwrap();
        }
        fs::create_dir_all(&root).unwrap();
        Self(root)
    }
}

impl Drop for Temporary {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub(crate) fn compile_case_output(
    registry: &Value,
    expected: &Value,
    root: &Temporary,
    index: usize,
) -> (PathBuf, std::process::Output) {
    let id = expected["id"].as_str().unwrap();
    let form = registry["forms"]
        .as_array()
        .unwrap()
        .iter()
        .find(|form| form["id"] == expected["form"])
        .unwrap();
    let configuration = form["configurations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|config| config["id"] == expected["configuration"])
        .unwrap();
    assert_eq!(configuration["options"], expected["options"]);
    let directory = root.0.join(index.to_string());
    fs::create_dir_all(&directory).unwrap();
    for (name, source) in form["files"].as_object().unwrap() {
        let destination = directory.join(name);
        fs::create_dir_all(destination.parent().unwrap()).unwrap();
        fs::write(destination, source.as_str().unwrap()).unwrap();
    }
    let config = directory.join("tsconfig.json");
    fs::write(
        &config,
        serde_json::to_vec(&serde_json::json!({
            "compilerOptions": configuration["options"],
            "files": configuration["roots"],
        }))
        .unwrap(),
    )
    .unwrap();
    let mut compiler = Command::new(env!("CARGO_BIN_EXE_bluetsc"));
    for (key, _) in env::vars_os() {
        let name = key.to_string_lossy();
        if name.starts_with("BLUETS_") || name.starts_with("BLUEICE_GE5_") {
            compiler.env_remove(key);
        }
    }
    let compiled = compiler
        .arg("--project")
        .arg(&config)
        .arg("--diagnostics-json")
        .output()
        .unwrap();
    assert!(compiled.status.code().is_some(), "{id}: compiler signal");
    assert!(
        !String::from_utf8_lossy(&compiled.stderr).contains("panicked at"),
        "{id}: {}",
        String::from_utf8_lossy(&compiled.stderr)
    );
    for (name, source) in form["files"].as_object().unwrap() {
        assert_eq!(
            fs::read_to_string(directory.join(name)).unwrap(),
            source.as_str().unwrap(),
            "{id}: authored input {name} changed"
        );
    }
    fn emitted_files(root: &std::path::Path, directory: &std::path::Path, files: &mut Vec<String>) {
        for entry in fs::read_dir(directory).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_dir() {
                emitted_files(root, &entry.path(), files);
            } else {
                files.push(
                    entry
                        .path()
                        .strip_prefix(root)
                        .unwrap()
                        .to_str()
                        .unwrap()
                        .replace('\\', "/"),
                );
            }
        }
    }
    let mut files = Vec::new();
    emitted_files(&directory, &directory, &mut files);
    files.retain(|name| {
        name != "tsconfig.json" && !form["files"].as_object().unwrap().contains_key(name)
    });
    files.sort();
    assert_eq!(
        serde_json::json!(files),
        expected["files"],
        "{id}: emitted inventory; compiler diagnostics: {}",
        String::from_utf8_lossy(&compiled.stderr)
    );
    (directory, compiled)
}
