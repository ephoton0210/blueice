// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Public filesystem/process observations for the frozen G-M2 workflows.
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

pub fn input() -> Value {
    serde_json::from_str(include_str!("../fixtures/gm2_build_interop/cases.json")).unwrap()
}
pub fn reference() -> Value {
    serde_json::from_str(include_str!("../fixtures/gm2_build_interop/reference.json")).unwrap()
}
static NEXT: AtomicU64 = AtomicU64::new(0);
pub struct Temporary(pub PathBuf);
impl Temporary {
    pub fn new(name: &str) -> Self {
        let root = std::env::temp_dir().canonicalize().unwrap().join(format!(
            "bluets-gm2-{name}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        Self(root)
    }
}
impl Drop for Temporary {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
pub fn write(root: &Path, files: &Value) {
    for (name, source) in files.as_object().unwrap() {
        assert!(!Path::new(name).is_absolute());
        assert!(!Path::new(name)
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir)));
        let file = root.join(name);
        if source.is_null() {
            fs::remove_file(file).unwrap();
        } else {
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(file, source.as_str().unwrap()).unwrap();
        }
    }
}
pub fn apply(root: &Path, inputs: &mut Value, changes: &Value) {
    write(root, changes);
    for (name, source) in changes.as_object().unwrap() {
        if source.is_null() {
            inputs.as_object_mut().unwrap().remove(name);
        } else {
            inputs[name] = source.clone();
        }
    }
}
pub fn outputs(root: &Path, inputs: &Value) -> Value {
    fn visit(root: &Path, directory: &Path, inputs: &Value, result: &mut BTreeMap<String, String>) {
        for entry in fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            let metadata = fs::symlink_metadata(&path).unwrap();
            if metadata.file_type().is_symlink() {
                continue;
            }
            if metadata.is_dir() {
                visit(root, &path, inputs, result);
            } else {
                let name = path
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                if [
                    ".js", ".mjs", ".cjs", ".jsx", ".d.ts", ".d.mts", ".d.cts", ".map",
                ]
                .iter()
                .any(|suffix| name.ends_with(suffix))
                    && inputs.get(&name).is_none()
                {
                    result.insert(name, fs::read_to_string(path).unwrap());
                }
            }
        }
    }
    let mut result = BTreeMap::new();
    visit(root, root, inputs, &mut result);
    json!(result)
}
pub fn run(root: &Path, args: &[String]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_bluetsc"))
        .current_dir(root)
        .args(args)
        .output()
        .unwrap()
}
pub fn build_args() -> Vec<String> {
    [
        "--build",
        "tsconfig.json",
        "--verbose",
        "--diagnostics-json",
    ]
    .map(str::to_string)
    .to_vec()
}
pub fn relative(root: &Path, text: &str) -> String {
    let text = text.replace('\\', "/");
    let root = root.to_string_lossy().replace('\\', "/");
    text.strip_prefix(&(root + "/"))
        .unwrap_or(&text)
        .to_string()
}
pub fn events(root: &Path, text: &str) -> Value {
    let mut result = Vec::new();
    for line in text.lines() {
        if let Some(project) = line.strip_prefix("Building project: ") {
            result.push(json!({"kind":"build","project":relative(root,project)}));
        } else if let Some(project) = line
            .strip_prefix("Project ")
            .and_then(|s| s.strip_suffix(" is up to date with dependency declarations"))
        {
            result.push(json!({"kind":"signature-current","project":relative(root,project)}));
        } else if let Some(project) = line.strip_prefix("Updating output timestamps: ") {
            result.push(json!({"kind":"timestamps","project":relative(root,project)}));
        } else if let Some(project) = line
            .strip_prefix("Project ")
            .and_then(|s| s.strip_suffix(" is up to date"))
        {
            result.push(json!({"kind":"current","project":relative(root,project)}));
        } else if line.contains("can't be built because its dependency '")
            || line.contains("Skipping build of project '")
        {
            let project = line.split('\'').nth(1).unwrap();
            let dependency = line
                .split("dependency '")
                .nth(1)
                .unwrap()
                .split('\'')
                .next()
                .unwrap();
            result.push(
                json!({"kind":if line.starts_with("Skipping") {"skip"} else {"blocked"},
                "project":relative(root,project),"dependency":relative(root,dependency),
                "reason":if line.contains("was not built") {"not-built"} else {"errors"}}),
            );
        }
    }
    json!(result)
}
fn diagnostic(root: &Path, inputs: &Value, ts: &Value) -> Value {
    if ts.is_null() {
        return json!({"unmapped":true});
    }
    let position = &ts["position"];
    let (module, start, length) = if position.is_null() {
        (Value::Null, Value::Null, Value::Null)
    } else {
        let name = relative(root, ts["span"]["module"].as_str().unwrap());
        let source = inputs[&name]
            .as_str()
            .unwrap_or_else(|| panic!("unowned diagnostic {name}: {ts}"));
        let offset = ts["span"]["start"].as_u64().unwrap() as usize;
        (
            json!(name),
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
                    diagnostic(root, inputs, &item)
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    json!({"code":ts["code"],"message":ts["message"].as_str().unwrap().replace(root.to_str().unwrap(),"<root>"),
        "module":module,"start":start,"length":length,"related":related})
}
pub fn snapshot(root: &Path, inputs: &Value, output: &Output) -> Value {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let diagnostics = stderr
        .lines()
        .map(|line| {
            serde_json::from_str::<Value>(line)
                .map(|value| diagnostic(root, inputs, &value["typescript"]))
                .unwrap_or_else(|_| json!({"nonJson":line}))
        })
        .collect::<Vec<_>>();
    json!({"exit":output.status.code(),"events":events(root,&stdout),"diagnostics":diagnostics,
        "outputs":outputs(root,inputs)})
}
pub fn compare(actual: &Value, expected: &Value, failures: &mut Vec<String>) {
    for key in ["exit", "events", "diagnostics", "shown", "selectedInputs"] {
        if actual.get(key).is_some() && actual[key] != expected[key] {
            failures.push(format!(
                "{} {key}: expected {}; got {}",
                expected["id"], expected[key], actual[key]
            ));
        }
    }
    let outputs = &actual["outputs"];
    if outputs.as_object().unwrap().keys().collect::<Vec<_>>()
        != expected["outputs"]
            .as_object()
            .unwrap()
            .keys()
            .collect::<Vec<_>>()
    {
        failures.push(format!(
            "{} artifact keys: expected {:?}; got {:?}",
            expected["id"],
            expected["outputs"].as_object().unwrap().keys(),
            outputs.as_object().unwrap().keys()
        ));
    }
    for (name, text) in outputs.as_object().unwrap() {
        if name.ends_with(".d.ts") && text != &expected["outputs"][name] {
            failures.push(format!(
                "{} declaration {name}: expected {}; got {text}",
                expected["id"], expected["outputs"][name]
            ));
        }
    }
}
pub fn expected<'a>(reference: &'a Value, id: &Value) -> &'a Value {
    reference["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| &row["id"] == id)
        .unwrap()
}
pub fn finish(failures: &[String]) {
    assert!(
        failures.is_empty(),
        "{} G-M2 differences:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
pub fn recorder() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../development/browser_core/phase-18-bluets/tools/record_gm2_build_interop.cjs")
}
pub fn node(mode: &str, input: &Value) -> Value {
    let mut child = Command::new("node")
        .arg(recorder())
        .arg(mode)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut pipe = child.stdin.take().unwrap();
    let bytes = serde_json::to_vec(input).unwrap();
    let writer = std::thread::spawn(move || pipe.write_all(&bytes));
    let result = child.wait_with_output().unwrap();
    writer.join().unwrap().unwrap();
    assert!(
        result.status.success(),
        "{}{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    serde_json::from_slice(&result.stdout).unwrap()
}
pub fn attach(row: &Value, inputs: &Value, actual: &Value) -> Value {
    json!({"id":row["id"],"inputs":inputs,"outputs":actual["outputs"]})
}
