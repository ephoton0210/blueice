// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! State cross-use includes native parsing and real native consumption of BlueTS state.
use super::support::*;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub fn selected(root: &Path) -> Value {
    fn visit(root: &Path, directory: &Path, result: &mut BTreeSet<String>) {
        for entry in fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                visit(root, &path, result);
            } else if path.to_string_lossy().ends_with(".bluetsc.tsbuildinfo") {
                let value: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
                for path in value["resolution_inputs"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                {
                    let path = Path::new(path);
                    if path.is_file()
                        && path.starts_with(root)
                        && path.to_string_lossy().ends_with(".ts")
                    {
                        result.insert(relative(root, path.to_str().unwrap()));
                    }
                }
            }
        }
    }
    let mut result = BTreeSet::new();
    visit(root, root, &mut result);
    json!(result)
}
pub fn builds() -> Vec<Value> {
    let input = input();
    let reference = reference();
    let mut failures = Vec::new();
    let mut observed = Vec::new();
    for workflow in input["build"].as_array().unwrap() {
        let temp = Temporary::new("build");
        let root = &temp.0;
        let mut inputs = workflow["files"].clone();
        write(root, &inputs);
        for step in workflow["steps"].as_array().unwrap() {
            apply(root, &mut inputs, &step["changes"]);
            let mut args = build_args();
            if workflow["stop"] == true {
                args.push("--stopBuildOnErrors".into());
            }
            let output = run(root, &args);
            let mut actual = snapshot(root, &inputs, &output);
            actual["selectedInputs"] = selected(root);
            let expected = expected(&reference, &step["id"]);
            compare(&actual, expected, &mut failures);
            observed.push(attach(step, &inputs, &actual));
        }
    }
    finish(&failures);
    observed
}
fn state_path(root: &Path, workflow: &Value) -> PathBuf {
    root.join(workflow["stateFile"].as_str().unwrap())
}
pub fn program_state(root: &Path, workflow: &Value) -> Value {
    let path = state_path(root, workflow);
    let Ok(bytes) = fs::read(&path) else {
        return json!({"missingState":true});
    };
    let Ok(info) = serde_json::from_slice::<Value>(&bytes) else {
        return json!({"invalidState":true});
    };
    let mut result = serde_json::Map::new();
    for (index, name) in info["fileNames"]
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
    {
        let name = name.as_str().unwrap();
        if name.starts_with("@typescript/") {
            continue;
        }
        let Ok(source) = path.parent().unwrap().join(name).canonicalize() else {
            continue;
        };
        if !source.starts_with(root) {
            continue;
        }
        let entry = &info["fileInfos"][index];
        let version = if entry.is_string() {
            entry.clone()
        } else {
            entry["version"].clone()
        };
        let signature = if entry.is_string() {
            entry.clone()
        } else {
            entry["signature"].clone()
        };
        result.insert(
            relative(root, source.to_str().unwrap()),
            json!({"version":version,"signature":signature}),
        );
    }
    Value::Object(result)
}
pub fn seed(root: &Path, workflow: &Value, reference: &Value, updated: Option<&Value>) {
    let seed = &reference["seeds"][workflow["id"].as_str().unwrap()];
    let mut info = seed["info"].clone();
    if let Some(updated) = updated {
        write(root, &updated["outputs"]);
        let path = state_path(root, workflow);
        let names = info["fileNames"].as_array().unwrap().clone();
        for (index, name) in names.iter().enumerate() {
            let name = name.as_str().unwrap();
            if name.starts_with("@typescript/") {
                continue;
            }
            let Ok(path) = path.parent().unwrap().join(name).canonicalize() else {
                continue;
            };
            if !path.starts_with(root) {
                continue;
            }
            let name = relative(root, path.to_str().unwrap());
            let entry = &updated["programState"][&name];
            info["fileInfos"][index]["version"] = entry["version"].clone();
            info["fileInfos"][index]["signature"] = entry["signature"].clone();
        }
    } else {
        write(root, &seed["outputs"]);
    }
    let file = state_path(root, workflow);
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(file, serde_json::to_vec(&info).unwrap()).unwrap();
}
pub fn states(live: bool) -> Vec<Value> {
    let input = input();
    let reference = reference();
    let mut failures = Vec::new();
    let mut observed = Vec::new();
    for workflow in input["state"].as_array().unwrap() {
        let temp = Temporary::new("state");
        let root = &temp.0;
        let mut inputs = workflow["files"].clone();
        write(root, &inputs);
        seed(root, workflow, &reference, None);
        let mut previous: Option<Value> = None;
        for step in workflow["steps"].as_array().unwrap() {
            std::thread::sleep(Duration::from_millis(25));
            apply(root, &mut inputs, &step["changes"]);
            let expected = expected(&reference, &step["id"]);
            if step["actor"] == "native" && !live {
                seed(root, workflow, &reference, Some(expected));
                previous = Some(expected["programState"].clone());
                continue;
            }
            let mut actual = if step["actor"] == "native" {
                let native = node(
                    "--native-step",
                    &json!({"root":root,"workflow":workflow["id"]}),
                );
                assert_eq!(
                    native["consumedState"],
                    previous.as_ref().unwrap().clone(),
                    "{}: native must consume the BlueTS-produced version/signature map",
                    step["id"]
                );
                json!({"exit":native["exit"],"events":native["events"],"diagnostics":native["diagnostics"],"outputs":outputs(root,&inputs)})
            } else {
                snapshot(root, &inputs, &run(root, &build_args()))
            };
            actual["programState"] = program_state(root, workflow);
            actual["selectedInputs"] = actual["programState"]
                .as_object()
                .unwrap()
                .keys()
                .cloned()
                .collect::<Vec<_>>()
                .into();
            compare(&actual, expected, &mut failures);
            if actual["programState"] != expected["programState"] {
                failures.push(format!(
                    "{} state versions/signatures: expected {}; got {}",
                    step["id"], expected["programState"], actual["programState"]
                ));
            }
            previous = Some(actual["programState"].clone());
            observed.push(attach(step, &inputs, &actual));
        }
    }
    finish(&failures);
    observed
}
pub fn cli(raw: bool) -> Vec<Value> {
    let input = input();
    let reference = reference();
    let mut failures = Vec::new();
    let mut observed = Vec::new();
    for row in input["cli"].as_array().unwrap() {
        let temp = Temporary::new("cli");
        let root = &temp.0;
        let inputs = &row["files"];
        write(root, inputs);
        let mut args = if raw {
            Vec::new()
        } else {
            vec!["--diagnostics-json".into(), "--listFiles".into()]
        };
        args.extend(
            row["args"]
                .as_array()
                .unwrap()
                .iter()
                .map(|s| s.as_str().unwrap().to_string()),
        );
        let output = run(root, &args);
        let expected = expected(&reference, &row["id"]);
        if raw {
            let actual = json!({"exit":output.status.code(),"outputs":outputs(root,inputs)});
            compare(&actual, expected, &mut failures);
            continue;
        }
        let mut actual = snapshot(root, inputs, &output);
        actual.as_object_mut().unwrap().remove("events");
        let shown = row["args"]
            .as_array()
            .unwrap()
            .iter()
            .any(|arg| arg == "--showConfig");
        actual["shown"] = if shown {
            serde_json::from_slice(&output.stdout).unwrap_or(Value::Null)
        } else {
            Value::Null
        };
        actual["selectedInputs"] = if shown {
            json!([])
        } else {
            json!(String::from_utf8_lossy(&output.stdout)
                .lines()
                .filter(|line| Path::new(line).is_absolute() && line.ends_with(".ts"))
                .map(|line| relative(root, line))
                .collect::<BTreeSet<_>>())
        };
        compare(&actual, expected, &mut failures);
        observed.push(attach(row, inputs, &actual));
    }
    finish(&failures);
    observed
}
