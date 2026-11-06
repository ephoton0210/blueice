// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Frozen direct compositions retain complete diagnostics and raw declarations.
use serde_json::{json, Value};
use std::{fs, path::Path};
#[path = "gc3_cases.rs"]
mod cases;
use cases::{compile_case_output, Temporary};

fn diagnostic(value: &Value, form: &Value, root: &Path, primary: bool) -> Value {
    let span = &value["span"];
    let physical = span["module"].as_str().unwrap().replace('\\', "/");
    let prefix = root.to_string_lossy().replace('\\', "/") + "/";
    let file = physical.strip_prefix(&prefix).unwrap_or(&physical);
    let source = form["files"][file]
        .as_str()
        .expect("authored diagnostic source");
    let start = span["start"].as_u64().unwrap() as usize;
    let end = span["end"].as_u64().unwrap() as usize;
    let before = &source[..start];
    let length = source[start..end].encode_utf16().count();
    assert_eq!(
        value["position"],
        json!({
            "line": before.bytes().filter(|byte| *byte == b'\n').count() + 1,
            "column": before.rsplit('\n').next().unwrap().encode_utf16().count() + 1,
            "length": length,
        })
    );
    let related = value["relatedInformation"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .map(|item| diagnostic(item, form, root, false))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let mut result = json!({
        "code": value["code"], "file": file, "start": before.encode_utf16().count(),
        "length": length, "message": value["message"].as_str().unwrap().replace(root.to_str().unwrap(), "<root>"),
        "related": related,
    });
    if primary {
        result["category"] = json!("Error");
    }
    result
}

fn expected_diagnostic(value: &Value, primary: bool) -> Value {
    let mut result = value.clone();
    if !primary {
        result.as_object_mut().unwrap().remove("category");
    }
    result["related"] = json!(value["related"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| expected_diagnostic(item, false))
        .collect::<Vec<_>>());
    result
}

pub(crate) fn family_matches_native(family: &str, count: usize) {
    let registry: Value =
        serde_json::from_str(include_str!("../fixtures/gc3_compositions/registry.json")).unwrap();
    let reference: Value =
        serde_json::from_str(include_str!("../fixtures/gc3_compositions/reference.json")).unwrap();
    // The full composition gate binds this policy to the unchanged phase
    // matrix. Keep this compiled input inside the measured backend closure.
    let policy: Value =
        serde_json::from_str(include_str!("../fixtures/gc3_compositions/policy.json")).unwrap();
    let selected = reference["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|case| {
            case["family"] == family
                && policy["caseRoles"][case["id"].as_str().unwrap()]["comparison"] == "direct"
        })
        .collect::<Vec<_>>();
    assert_eq!(selected.len(), count);
    let temporary = Temporary::new(family);
    let mut failures = Vec::new();
    for (index, expected) in selected.into_iter().enumerate() {
        let form = registry["forms"]
            .as_array()
            .unwrap()
            .iter()
            .find(|form| form["id"] == expected["form"])
            .unwrap();
        let (directory, output) = compile_case_output(&registry, expected, &temporary, index);
        let actual = String::from_utf8(output.stderr)
            .unwrap()
            .lines()
            .filter(|line| !line.is_empty())
            .map(|line| {
                let row: Value = serde_json::from_str(line).unwrap();
                assert_eq!(row["severity"], "error");
                diagnostic(&row["typescript"], form, &directory, true)
            })
            .collect::<Vec<_>>();
        let expected_diagnostics = expected["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| expected_diagnostic(item, true))
            .collect::<Vec<_>>();
        if output.status.success() != (expected["verdict"] == "accept")
            || actual != expected_diagnostics
        {
            failures.push(format!(
                "{}: accepted={}\nBlue: {}\nNative: {}",
                expected["id"],
                output.status.success(),
                json!(actual),
                json!(expected_diagnostics)
            ));
        }
        for (name, source) in expected["outputs"].as_object().unwrap() {
            if name.ends_with(".d.ts") || name.ends_with(".d.mts") || name.ends_with(".d.cts") {
                assert_eq!(
                    fs::read_to_string(directory.join(name)).unwrap(),
                    source.as_str().unwrap(),
                    "{}: {name}",
                    expected["id"]
                );
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
