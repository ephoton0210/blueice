// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Pinned object-method receiver contexts through the public compiler.

use blueice_bluets::{compile, CheckingOptions, CompilerOptions, MapLoader, ModuleSource};
use serde_json::Value;

#[test]
fn object_receivers_and_nested_functions_match_pinned_this_diagnostics() {
    let reference: Value = serde_json::from_str(include_str!(
        "fixtures/object-method-context-reference.json"
    ))
    .unwrap();
    assert_eq!(reference["version"], "5.9.3");
    assert_eq!(reference["cases"].as_array().unwrap().len(), 10);
    let mut failures = Vec::new();
    for case in reference["cases"].as_array().unwrap() {
        let source = case["source"].as_str().unwrap();
        let result = compile(
            "memory:///main.ts",
            &MapLoader::from([ModuleSource::new("memory:///main.ts", source)]),
            CompilerOptions {
                checking: Some(CheckingOptions {
                    no_implicit_any: false,
                    ..CheckingOptions::default()
                }),
                ..CompilerOptions::default()
            },
        );
        let diagnostics: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.severity == blueice_bluets::Severity::Error)
            .map(|diagnostic| {
                (
                    diagnostic
                        .typescript
                        .as_ref()
                        .map(|counterpart| counterpart.code),
                    diagnostic.span.start,
                    diagnostic.span.end - diagnostic.span.start,
                )
            })
            .collect();
        let expected: Vec<_> = case["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .map(|diagnostic| {
                (
                    Some(diagnostic["code"].as_u64().unwrap() as u32),
                    diagnostic["start"].as_u64().unwrap() as usize,
                    diagnostic["length"].as_u64().unwrap() as usize,
                )
            })
            .collect();
        if diagnostics != expected {
            failures.push(format!(
                "{}: expected {expected:?}, got {diagnostics:?}",
                case["name"]
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
