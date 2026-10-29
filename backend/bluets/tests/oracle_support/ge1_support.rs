// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Shared public CLI observations for the frozen G-E1 configurations.
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::project_paths;

pub(super) fn input() -> Value {
    serde_json::from_str(include_str!("../fixtures/ge1_downlevel/cases.json")).unwrap()
}
pub(super) fn reference() -> Value {
    serde_json::from_str(include_str!("../fixtures/ge1_downlevel/reference.json")).unwrap()
}
pub(super) struct Temporary(pub(super) PathBuf);
impl Temporary {
    pub(super) fn new(name: &str) -> Self {
        let root = std::env::temp_dir()
            .canonicalize()
            .unwrap()
            .join(format!("bluets-ge1-{name}-{}", std::process::id()));
        if root.exists() {
            fs::remove_dir_all(&root).unwrap();
        }
        fs::create_dir_all(&root).unwrap();
        Self(root)
    }
    pub(super) fn project(&self, row: &Value) -> PathBuf {
        let root = self.0.join(row["id"].as_str().unwrap());
        write_files(
            &root,
            row["files"]
                .as_object()
                .unwrap()
                .iter()
                .map(|(n, s)| (n.as_str(), s.as_str().unwrap())),
        );
        fs::write(root.join("tsconfig.json"), config_text(row)).unwrap();
        root
    }
}
impl Drop for Temporary {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
pub(super) fn write_files<'a>(root: &Path, files: impl IntoIterator<Item = (&'a str, &'a str)>) {
    for (name, source) in files {
        assert!(
            !Path::new(name).is_absolute()
                && !Path::new(name)
                    .components()
                    .any(|p| matches!(p, std::path::Component::ParentDir))
        );
        let file = root.join(name);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, source).unwrap();
    }
}

fn config_text(row: &Value) -> String {
    serde_json::to_string_pretty(&json!({"compilerOptions":row["options"],"files":[row["entry"]]}))
        .unwrap()
        + "\n"
}
pub(super) fn command(root: &Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_bluetsc"))
        .arg("--project")
        .arg(root.join("tsconfig.json"))
        .args(["--diagnostics-json", "--listFiles"])
        .output()
        .unwrap()
}
fn actual_diagnostic(ts: &Value, root: &Path, row: &Value) -> Value {
    if ts.is_null() {
        return json!({"unmapped":true});
    }
    let position = &ts["position"];
    let (module, start, length) = if position.is_null() {
        (Value::Null, Value::Null, Value::Null)
    } else {
        let module = project_paths::relative_to(root, ts["span"]["module"].as_str().unwrap());
        let source = if module == "tsconfig.json" {
            fs::read_to_string(root.join(&module)).unwrap()
        } else {
            row["files"][&module]
                .as_str()
                .unwrap_or_else(|| panic!("unowned diagnostic module {module}: {ts}"))
                .to_string()
        };
        let offset = ts["span"]["start"].as_u64().unwrap() as usize;
        (
            json!(module),
            json!(source[..offset].encode_utf16().count()),
            position["length"].clone(),
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
                    actual_diagnostic(&item, root, row)
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    json!({"code":ts["code"],"message":ts["message"].as_str().unwrap().replace(&root.to_string_lossy().to_string(),"<root>"),"module":module,"start":start,"length":length,"related":related})
}
pub(super) fn outputs(root: &Path) -> BTreeMap<String, String> {
    fn visit(root: &Path, path: &Path, outputs: &mut BTreeMap<String, String>) {
        if !path.is_dir() {
            return;
        }
        for entry in fs::read_dir(path).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                visit(root, &path, outputs);
            } else {
                let name = path
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                outputs.insert(name, fs::read_to_string(path).unwrap());
            }
        }
    }
    let mut result = BTreeMap::new();
    visit(root, &root.join("out"), &mut result);
    visit(root, &root.join("types"), &mut result);
    result
}
pub(super) fn observations(name: &str) -> Vec<Value> {
    let input = input();
    let reference = reference();
    let temporary = Temporary::new(name);
    let mut failures = Vec::new();
    let mut accepted = Vec::new();
    for (row, native) in input["cases"]
        .as_array()
        .unwrap()
        .iter()
        .zip(reference["cases"].as_array().unwrap())
    {
        assert_eq!(row["id"], native["id"]);
        let root = temporary.project(row);
        let result = command(&root);
        let parsed = String::from_utf8_lossy(&result.stderr)
            .lines()
            .map(serde_json::from_str::<Value>)
            .collect::<Result<Vec<_>, _>>();
        let Ok(parsed) = parsed else {
            failures.push(format!(
                "{}: non-JSON diagnostics: {}",
                row["id"],
                String::from_utf8_lossy(&result.stderr)
            ));
            continue;
        };
        let diagnostics = json!(parsed
            .iter()
            .map(|d| actual_diagnostic(&d["typescript"], &root, row))
            .collect::<Vec<_>>());
        if diagnostics != native["diagnostics"]
            || result.status.success() != row["accepts"].as_bool().unwrap()
        {
            failures.push(format!(
                "{}: expected {}; got {}; exit={:?}",
                row["id"],
                native["diagnostics"],
                diagnostics,
                result.status.code()
            ));
            continue;
        }
        if result.status.success() {
            let selected = String::from_utf8_lossy(&result.stdout)
                .lines()
                .map(|line| project_paths::relative_to(&root, line))
                .filter(|name| row["files"].get(name).is_some())
                .collect::<BTreeSet<_>>();
            let expected = native["selectedInputs"]
                .as_array()
                .unwrap()
                .iter()
                .map(|s| s.as_str().unwrap().to_owned())
                .collect::<BTreeSet<_>>();
            if selected != expected {
                failures.push(format!(
                    "{}: selected graph expected {expected:?}; got {selected:?}",
                    row["id"]
                ));
            }
            let actual = outputs(&root);
            if actual.keys().map(String::as_str).collect::<BTreeSet<_>>()
                != native["outputs"]
                    .as_object()
                    .unwrap()
                    .keys()
                    .map(String::as_str)
                    .collect::<BTreeSet<_>>()
            {
                failures.push(format!(
                    "{}: emitted inventory differs: {:?}",
                    row["id"],
                    actual.keys()
                ));
            }
            for (file, text) in &actual {
                if file.ends_with(".d.ts")
                    && Some(text.as_str()) != native["outputs"][file].as_str()
                {
                    failures.push(format!(
                        "{}: declaration {file}: expected {}; got {}",
                        row["id"], native["outputs"][file], text
                    ));
                }
            }
            accepted.push(json!({"id":row["id"],"outputs":actual}));
        } else if !outputs(&root).is_empty() {
            failures.push(format!("{}: failed project published output", row["id"]));
        }
    }
    assert!(
        failures.is_empty(),
        "{} G-E1 mismatches:\n{}",
        failures.len(),
        failures.join("\n")
    );
    accepted
}
