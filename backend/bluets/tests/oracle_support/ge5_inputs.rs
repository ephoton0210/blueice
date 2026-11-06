// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Hash-bound parent sources; coordinate changes never rewrite old fixtures.
use ring::digest::{digest, SHA256};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::OnceLock;

pub fn input() -> &'static Value {
    static INPUT: OnceLock<Value> = OnceLock::new();
    INPUT.get_or_init(|| {
        serde_json::from_str(include_str!("../fixtures/ge5_source_maps/cases.json")).unwrap()
    })
}
pub fn reference() -> &'static Value {
    static REFERENCE: OnceLock<Value> = OnceLock::new();
    REFERENCE.get_or_init(|| {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/ge5_source_maps/reference.json");
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    })
}
pub fn sha(bytes: &[u8]) -> String {
    digest(&SHA256, bytes)
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
fn fixture(suite: &str, name: &str) -> Vec<u8> {
    std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(suite)
            .join(name),
    )
    .unwrap()
}
fn parents() -> &'static BTreeMap<String, Value> {
    static PARENTS: OnceLock<BTreeMap<String, Value>> = OnceLock::new();
    PARENTS.get_or_init(|| {
        input()["parentAuthorities"]
            .as_object()
            .unwrap()
            .keys()
            .map(|suite| {
                (
                    suite.clone(),
                    serde_json::from_slice(&fixture(suite, "cases.json")).unwrap(),
                )
            })
            .collect()
    })
}
pub fn verify_authorities() {
    for (suite, expected) in input()["parentAuthorities"].as_object().unwrap() {
        assert_eq!(sha(&fixture(suite, "cases.json")), expected["casesSha256"]);
        assert_eq!(
            sha(&fixture(suite, "reference.json")),
            expected["referenceSha256"]
        );
    }
}
pub fn blob(hash: &str) -> &'static str {
    let value = reference()["blobs"][hash].as_str().unwrap();
    assert_eq!(sha(value.as_bytes()), hash);
    value
}
fn coordinate(source: &str, variant: &str) -> String {
    if variant == "original" {
        return source.to_string();
    }
    assert!(["lf", "crlf", "lfAstral", "crlfAstral"].contains(&variant));
    let source = source.replace("\r\n", "\n").replace('\r', "\n");
    let source = if variant.starts_with("crlf") {
        source.replace('\n', "\r\n")
    } else {
        source
    };
    format!(
        "{}{source}",
        if variant.ends_with("Astral") {
            "/* G-E5 🧭 */ "
        } else {
            ""
        }
    )
}
pub fn resolve(specification: &Value) -> Value {
    let variant = specification["variant"].as_str().unwrap();
    let (mut files, entry, mut options) = if let Some(origin) = specification.get("origin") {
        let parent = &parents()[origin["suite"].as_str().unwrap()];
        let original = parent["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"] == origin["id"])
            .unwrap();
        assert_eq!(original["accepts"], true);
        assert_ne!(original["nativeRuntimeDefect"], true);
        let mut files = if original["provider"] == "tslib-2.8.1" {
            parent["sharedFiles"].as_object().unwrap().clone()
        } else {
            serde_json::Map::new()
        };
        for (name, source) in original["files"].as_object().unwrap() {
            let text = source.as_str().unwrap();
            files.insert(
                name.clone(),
                json!(if [".ts", ".mts", ".cts"]
                    .iter()
                    .any(|suffix| name.ends_with(suffix))
                {
                    coordinate(text, variant)
                } else {
                    text.to_string()
                }),
            );
        }
        (
            files,
            original["entry"].clone(),
            original["options"].as_object().unwrap().clone(),
        )
    } else {
        let source = input()["regressions"][specification["regression"].as_str().unwrap()]
            .as_str()
            .unwrap();
        (json!({"main.ts":coordinate(source, variant)}).as_object().unwrap().clone(), json!("main.ts"),
            json!({"strict":true,"target":"ES2022","module":specification["module"],"moduleResolution":"Node10",
                "lib":["ES2022"],"types":[],"declaration":true,"noEmitOnError":true,"rootDir":".","outDir":"out"})
                .as_object().unwrap().clone())
    };
    options.extend(
        input()["profiles"][specification["profile"].as_str().unwrap()]
            .as_object()
            .unwrap()
            .clone(),
    );
    json!({"id":specification["id"],"files":std::mem::take(&mut files),"entry":entry,"options":options})
}
