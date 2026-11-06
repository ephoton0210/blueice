// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Public CLI observations keep complete diagnostics and original output bytes.
use super::{inputs, project_paths};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static TEMPORARY_SEQUENCE: AtomicUsize = AtomicUsize::new(0);

pub struct Temporary(PathBuf);
impl Temporary {
    pub fn new(name: &str) -> Self {
        let temporary = std::env::temp_dir().canonicalize().unwrap();
        loop {
            let sequence = TEMPORARY_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let root = temporary.join(format!(
                "bluets-ge5-{name}-{}-{sequence}",
                std::process::id()
            ));
            match fs::create_dir(&root) {
                Ok(()) => return Self(root),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("create G-E5 test directory: {error}"),
            }
        }
    }
}
impl Drop for Temporary {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
#[derive(Debug, PartialEq, Eq)]
pub struct Observation {
    pub success: bool,
    pub diagnostics: Value,
    pub outputs: BTreeMap<String, String>,
    pub selected_inputs: BTreeSet<String>,
}
fn diagnostic(ts: &Value, root: &Path, row: &Value) -> Value {
    if ts.is_null() {
        return json!({"unmapped":true});
    }
    let (module, start, length) = if ts["position"].is_null() {
        (Value::Null, Value::Null, Value::Null)
    } else {
        let module = project_paths::relative_to(root, ts["span"]["module"].as_str().unwrap());
        let source = if module == "tsconfig.json" {
            fs::read_to_string(root.join(&module)).unwrap()
        } else {
            row["files"][&module].as_str().unwrap_or("").to_string()
        };
        let offset = ts["span"]["start"].as_u64().unwrap() as usize;
        let Some(prefix) = source.get(..offset) else {
            return json!({"unowned":true});
        };
        (
            json!(module),
            json!(prefix.encode_utf16().count()),
            ts["position"]["length"].clone(),
        )
    };
    let related = ts["relatedInformation"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .map(|item| {
                    let mut item = item.clone();
                    item["relatedInformation"] = json!([]);
                    diagnostic(&item, root, row)
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    json!({"code":ts["code"],"message":ts["message"].as_str().unwrap().replace(root.to_str().unwrap(),"<root>"),
        "module":module,"start":start,"length":length,"related":related})
}
fn outputs(root: &Path, dir: &Path, result: &mut BTreeMap<String, String>) {
    if !dir.is_dir() {
        return;
    }
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            outputs(root, &path, result);
        } else {
            result.insert(
                path.strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/"),
                fs::read_to_string(path).unwrap(),
            );
        }
    }
}
fn command(root: &Path, row: &Value) -> Observation {
    let result = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
        .arg("--project")
        .arg(root.join("tsconfig.json"))
        .args(["--diagnostics-json", "--listFiles"])
        .output()
        .unwrap();
    let diagnostics = String::from_utf8_lossy(&result.stderr)
        .lines()
        .map(|line| {
            let value: Value = serde_json::from_str(line).unwrap_or_else(|error| {
                panic!(
                    "{}: non-JSON diagnostic: {error}: {line:?}; status {}\nstdout: {}\nstderr: {}",
                    row["id"],
                    result.status,
                    String::from_utf8_lossy(&result.stdout),
                    String::from_utf8_lossy(&result.stderr)
                )
            });
            diagnostic(&value["typescript"], root, row)
        })
        .collect::<Vec<_>>();
    let selected_inputs = String::from_utf8_lossy(&result.stdout)
        .lines()
        .map(|line| project_paths::relative_to(root, line))
        .filter(|name| row["files"].as_object().unwrap().contains_key(name))
        .collect();
    let mut emitted = BTreeMap::new();
    for name in ["out", "types"] {
        outputs(root, &root.join(name), &mut emitted);
    }
    Observation {
        success: result.status.success(),
        diagnostics: json!(diagnostics),
        outputs: emitted,
        selected_inputs,
    }
}

pub fn observe(temporary: &Temporary, index: usize, row: &Value) -> Observation {
    observe_builds(temporary, index, row, 1)
}

pub fn observe_builds(
    temporary: &Temporary,
    index: usize,
    row: &Value,
    builds: usize,
) -> Observation {
    let root = temporary.0.join(index.to_string());
    for (name, text) in row["files"].as_object().unwrap() {
        assert!(
            !Path::new(name).is_absolute()
                && !Path::new(name)
                    .components()
                    .any(|part| matches!(part, std::path::Component::ParentDir))
        );
        let file = root.join(name);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, text.as_str().unwrap()).unwrap();
    }
    let configuration = json!({"compilerOptions":row["options"],"files":[row["entry"]]});
    let native = &inputs::reference()["cases"][index];
    let text = if let Some(hash) = native["configuration"].as_str() {
        let text = inputs::blob(hash);
        assert_eq!(serde_json::from_str::<Value>(text).unwrap(), configuration);
        text.to_string()
    } else {
        serde_json::to_string_pretty(&configuration).unwrap() + "\n"
    };
    fs::write(root.join("tsconfig.json"), text).unwrap();
    let observation = command(&root, row);
    for _ in 1..builds {
        assert_eq!(
            command(&root, row),
            observation,
            "{}: repeated build",
            row["id"]
        );
    }
    fs::remove_dir_all(&root).unwrap();
    observation
}
