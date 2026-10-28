// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Native-first G-M1 projects: selected graph, diagnostics and exact declarations.
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[path = "oracle_support/gm1_boundaries.rs"]
mod boundaries;
#[path = "oracle_support/project_paths.rs"]
mod project_paths;

fn input() -> Value {
    serde_json::from_str(include_str!(
        "fixtures/gm1_resolver_compositions/cases.json"
    ))
    .unwrap()
}
fn reference() -> Value {
    serde_json::from_str(include_str!(
        "fixtures/gm1_resolver_compositions/reference.json"
    ))
    .unwrap()
}
struct Temporary(PathBuf);
impl Temporary {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir()
            .canonicalize()
            .unwrap()
            .join(format!("bluets-gm1-{name}-{}", std::process::id()));
        if root.exists() {
            fs::remove_dir_all(&root).unwrap();
        }
        fs::create_dir_all(&root).unwrap();
        Self(root)
    }
    fn project(&self, row: &Value) -> PathBuf {
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
fn write_files<'a>(root: &Path, files: impl IntoIterator<Item = (&'a str, &'a str)>) {
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

/// Match the native recorder's configuration bytes, including property order.
/// This is needed for diagnostics related to an explicit `types` entry.
fn config_text(row: &Value) -> String {
    use serde::ser::{Serialize, SerializeMap, Serializer};
    let source = include_str!("fixtures/gm1_resolver_compositions/cases.json");
    let tokens = blueice_bluets::lex("cases.json", source).unwrap();
    let id = serde_json::to_string(row["id"].as_str().unwrap()).unwrap();
    let start = tokens.iter().position(|token| token.text == id).unwrap();
    let start = start
        + tokens[start..]
            .iter()
            .position(|token| token.is("\"options\""))
            .unwrap()
        + 2;
    let mut keys = Vec::new();
    let mut depth = 0;
    for pair in tokens[start..].windows(2) {
        if pair[0].is("{") || pair[0].is("[") {
            depth += 1;
        }
        if pair[0].is("}") || pair[0].is("]") {
            depth -= 1;
            if depth == 0 {
                break;
            }
        }
        if depth == 1 && pair[1].is(":") {
            keys.push(serde_json::from_str::<String>(&pair[0].text).unwrap());
        }
    }
    struct Ordered<'a>(&'a Value, &'a [String]);
    impl Serialize for Ordered<'_> {
        fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            let mut map = serializer.serialize_map(None)?;
            for key in self.1 {
                map.serialize_entry(key, &self.0[key])?;
            }
            for (key, value) in self.0.as_object().unwrap() {
                if !self.1.contains(key) {
                    map.serialize_entry(key, value)?;
                }
            }
            map.end()
        }
    }
    #[derive(serde::Serialize)]
    struct Config<'a> {
        #[serde(rename = "compilerOptions")]
        options: Ordered<'a>,
        files: [&'a Value; 1],
    }
    serde_json::to_string_pretty(&Config {
        options: Ordered(&row["options"], &keys),
        files: [&row["entry"]],
    })
    .unwrap()
        + "\n"
}
fn command(root: &Path) -> std::process::Output {
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
fn outputs(root: &Path) -> BTreeMap<String, String> {
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
    result
}
fn observations(name: &str) -> Vec<Value> {
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
                if file.contains(".d.") && Some(text.as_str()) != native["outputs"][file].as_str() {
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
        "{} G-M1 mismatches:\n{}",
        failures.len(),
        failures.join("\n")
    );
    accepted
}
#[test]
fn manifest_retains_every_frozen_pair_and_owner_control() {
    let input = input();
    let reference = reference();
    assert_eq!(reference["typescript"], "5.9.3");
    let rows = input["cases"].as_array().unwrap();
    assert_eq!(rows.len(), 134);
    assert_eq!(rows.iter().filter(|r| r["accepts"] == true).count(), 67);
    assert_eq!(
        rows.iter()
            .map(|r| r["id"].as_str().unwrap())
            .collect::<BTreeSet<_>>()
            .len(),
        134
    );
    assert_eq!(
        rows.iter()
            .map(|r| r["form"].as_str().unwrap())
            .collect::<BTreeSet<_>>()
            .len(),
        37
    );
    assert_eq!(reference["cases"].as_array().unwrap().len(), 134);
    assert_eq!(input["ownerControls"], reference["ownerControls"]);
    assert_eq!(input["ownerControls"].as_array().unwrap().len(), 5);
    assert_eq!(
        reference["cases"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["outputs"]
                .as_object()
                .unwrap()
                .keys()
                .filter(|n| n.contains(".d."))
                .count())
            .sum::<usize>(),
        118
    );
}
#[test]
fn project_graph_diagnostics_and_declarations_match_frozen_typescript() {
    observations("matrix");
}
#[test]
#[ignore = "requires pinned TypeScript 5.9.3 and Acorn 8.15.0"]
fn native_matrix_is_reproducible() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join(
        "../../development/browser_core/phase-18-bluets/tools/record_gm1_resolver_compositions.cjs",
    );
    let result = Command::new("node").arg(root).output().unwrap();
    assert!(
        result.status.success(),
        "{}{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
}
#[test]
#[ignore = "requires pinned TypeScript 5.9.3, Acorn 8.15.0 and Node"]
fn exact_declarations_imports_consumers_syntax_and_execution_match_native() {
    let data = observations("outputs");
    assert_eq!(data.len(), 67);
    let recorder = Path::new(env!("CARGO_MANIFEST_DIR")).join(
        "../../development/browser_core/phase-18-bluets/tools/record_gm1_resolver_compositions.cjs",
    );
    let mut child = Command::new("node")
        .arg(recorder)
        .arg("--outputs")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut pipe = child.stdin.take().unwrap();
    let bytes = serde_json::to_vec(&data).unwrap();
    let writer = std::thread::spawn(move || pipe.write_all(&bytes));
    let result = child.wait_with_output().unwrap();
    writer.join().unwrap().unwrap();
    assert!(
        result.status.success(),
        "{}{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
}
