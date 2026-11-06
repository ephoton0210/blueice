// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Whole-stream bounds and metadata checks; source-role parity is a separate gate.
use super::{cli, decoded, inputs};
use base64::Engine;
use blueice_bluets::SourceMap;
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::OnceLock;

/// Select the separately reproduced correction only for its exact stock case
/// and map identity. Every stock byte remains available through inputs::blob.
pub(super) fn native_map(case: &Value, name: &str) -> &'static str {
    let expected = &case["maps"][name];
    let hash = expected["raw"].as_str().unwrap();
    if expected["coordinateIssues"].as_array().unwrap().is_empty() {
        return inputs::blob(hash);
    }
    static CORRECTIONS: OnceLock<Value> = OnceLock::new();
    let corrections = CORRECTIONS.get_or_init(|| {
        let bytes = include_bytes!("../fixtures/ge5_source_maps/native-map-defects.json");
        assert_eq!(
            inputs::sha(bytes),
            "097a9b0df2857de39e2210e427c21f3dbcadebb4f8ed8d9dabbe5f0a87b04007"
        );
        serde_json::from_slice(bytes).unwrap()
    });
    let defect = corrections["defects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|defect| defect["caseId"] == case["id"] && defect["map"] == name)
        .expect("every Native coordinate issue requires a reproduced, exact correction");
    assert_eq!(defect["stockRawSha256"], hash);
    assert_eq!(
        expected["coordinateIssues"],
        serde_json::json!([defect["issue"]])
    );
    let fixed_hash = defect["patchedRawSha256"].as_str().unwrap();
    let text = corrections["patchedBlobs"][fixed_hash].as_str().unwrap();
    assert_eq!(inputs::sha(text.as_bytes()), fixed_hash);
    text
}

#[test]
fn corrected_native_comparison_changes_only_the_572_enumerated_maps() {
    let mut changed = 0;
    let mut unchanged = 0;
    for case in inputs::reference()["cases"].as_array().unwrap() {
        for (name, expected) in case["maps"].as_object().unwrap() {
            let original = inputs::blob(expected["raw"].as_str().unwrap());
            let corrected = native_map(case, name);
            if expected["coordinateIssues"].as_array().unwrap().is_empty() {
                assert_eq!(corrected, original);
                unchanged += 1;
            } else {
                assert_ne!(corrected, original);
                changed += 1;
                assert_eq!(
                    decoded::decoded(corrected)["metadata"],
                    decoded::decoded(original)["metadata"]
                );
            }
        }
    }
    assert_eq!(changed, 572);
    assert_eq!(unchanged, 20018);
}

fn source_key(specification: &Value) -> String {
    let mut key = specification.as_object().unwrap().clone();
    key.remove("id");
    key.remove("profile");
    serde_json::to_string(&key).unwrap()
}

type SourceContents = BTreeMap<String, BTreeMap<String, Vec<String>>>;
fn source_authorities() -> &'static SourceContents {
    static AUTHORITIES: OnceLock<SourceContents> = OnceLock::new();
    AUTHORITIES.get_or_init(|| {
        let native = inputs::reference()["cases"].as_array().unwrap();
        let mut result = BTreeMap::new();
        for (index, specification) in inputs::input()["cases"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
        {
            if specification["profile"] != "external" {
                continue;
            }
            let mut outputs = BTreeMap::new();
            for expected in native[index]["maps"].as_object().unwrap().values() {
                if expected["kind"] != "javascript" {
                    continue;
                }
                let map: Value =
                    serde_json::from_str(inputs::blob(expected["raw"].as_str().unwrap())).unwrap();
                let contents = map["sourcesContent"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|source| source.as_str().unwrap().to_owned())
                    .collect();
                outputs.insert(expected["output"].as_str().unwrap().to_owned(), contents);
            }
            assert!(result.insert(source_key(specification), outputs).is_none());
        }
        result
    })
}

pub fn compare(
    specification: &Value,
    row: &Value,
    native: &Value,
    actual: &cli::Observation,
) -> Vec<String> {
    let id = specification["id"].as_str().unwrap();
    let mut failures = Vec::new();
    for (name, expected) in native["maps"].as_object().unwrap() {
        if expected["kind"] != "javascript" {
            continue;
        }
        let output = expected["output"].as_str().unwrap();
        let Some(javascript) = actual.outputs.get(output) else {
            continue;
        };
        let text = if name.ends_with("#inline") {
            let encoded = javascript.lines().find_map(|line| {
                line.strip_prefix("//# sourceMappingURL=data:application/json;base64,")
            });
            let Some(text) = encoded
                .and_then(|encoded| {
                    base64::engine::general_purpose::STANDARD
                        .decode(encoded)
                        .ok()
                })
                .and_then(|bytes| String::from_utf8(bytes).ok())
            else {
                failures.push(format!("{id}: invalid inline map: {name}"));
                continue;
            };
            text
        } else {
            let Some(text) = actual.outputs.get(name) else {
                continue;
            };
            text.clone()
        };
        let Ok(raw) = serde_json::from_str::<Value>(&text) else {
            failures.push(format!("{id}: invalid map JSON: {name}"));
            continue;
        };
        let native_map = decoded::decoded(native_map(native, name));
        let actual_map = decoded::decoded(&text);
        if actual_map["metadata"] != native_map["metadata"] {
            failures.push(format!("{id}: complete JavaScript map metadata: {name}"));
        }
        // No-content publication gets bounds from the hash-bound external
        // sibling, never from a generated path or an arbitrary filesystem read.
        let contents = &source_authorities()[&source_key(specification)][output];
        for source in contents {
            assert!(
                row["files"]
                    .as_object()
                    .unwrap()
                    .values()
                    .any(|text| text.as_str() == Some(source.as_str())),
                "{id}: Native source content must belong to the resolved input graph"
            );
        }
        let map = SourceMap {
            file: raw["file"].as_str().unwrap().into(),
            source_root: raw["sourceRoot"].as_str().unwrap_or("").into(),
            sources: raw["sources"]
                .as_array()
                .unwrap()
                .iter()
                .map(|value| value.as_str().unwrap().into())
                .collect(),
            sources_content: contents.clone(),
            inline_sources: raw.get("sourcesContent").is_some(),
            names: raw["names"]
                .as_array()
                .unwrap()
                .iter()
                .map(|value| value.as_str().unwrap().into())
                .collect(),
            mappings: raw["mappings"].as_str().unwrap().into(),
        };
        if let Err(error) = map.original_position_at(javascript, 0, 0, 100_000, 32_000_000) {
            failures.push(format!(
                "{id}: complete JavaScript map bounds/order/tables: {name}: {error}"
            ));
        }
    }
    failures
}
