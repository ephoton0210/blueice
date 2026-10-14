// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Interface overload inheritance through the public compiler boundary.

use blueice_bluets::{
    compile, CheckingOptions, CompilerLimits, CompilerOptions, DiagnosticCode, MapLoader,
    ModuleSource, Severity,
};
use serde_json::Value;

#[test]
fn interface_overload_heritage_matches_typescript_5_9_3_verdicts() {
    let reference: Value = serde_json::from_str(include_str!(
        "fixtures/interface-overload-heritage-reference.json"
    ))
    .unwrap();
    assert_eq!(reference["version"], "5.9.3");
    for case in reference["cases"].as_array().unwrap() {
        let result = compile(
            "memory:///main.ts",
            &MapLoader::from([ModuleSource::new(
                "memory:///main.ts",
                case["source"].as_str().unwrap(),
            )]),
            CompilerOptions {
                checking: Some(CheckingOptions::default()),
                ..CompilerOptions::default()
            },
        );
        assert_eq!(
            !result.has_errors(),
            case["accepts"].as_bool().unwrap(),
            "{}: {:#?}",
            case["name"],
            result.diagnostics
        );
        let actual: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.severity == Severity::Error)
            .map(|diagnostic| diagnostic.typescript.as_ref().unwrap().code)
            .collect();
        let expected: Vec<_> = case["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .map(|diagnostic| diagnostic["code"].as_u64().unwrap() as u32)
            .collect();
        assert_eq!(actual, expected, "{}", case["name"]);
    }
}

#[test]
fn inherited_property_lookup_exhaustion_reports_a_resource_limit() {
    let mut source = "interface Base0 { value: number; }".to_string();
    for index in 1..=20 {
        source.push_str(&format!(
            "interface Base{index} extends Base{} {{}}",
            index - 1
        ));
    }
    source.push_str("interface Child extends Base20 { value: string; }");
    let loader = MapLoader::from([ModuleSource::new("memory:///main.ts", source)]);
    let result = compile(
        "memory:///main.ts",
        &loader,
        CompilerOptions {
            limits: CompilerLimits {
                max_type_expansions: 4,
                ..CompilerLimits::default()
            },
            ..CompilerOptions::default()
        },
    );
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == DiagnosticCode::ResourceLimit),
        "{:#?}",
        result.diagnostics
    );
    let full = compile("memory:///main.ts", &loader, CompilerOptions::default());
    assert!(
        full.diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == DiagnosticCode::TypeMismatch),
        "the complete lookup must still reject the incompatible property"
    );
}

#[test]
fn weak_inherited_records_match_typescript_5_9_3_verdicts() {
    let reference: Value = serde_json::from_str(include_str!(
        "fixtures/weak-interface-heritage-reference.json"
    ))
    .unwrap();
    assert_eq!(reference["version"], "5.9.3");
    for case in reference["cases"].as_array().unwrap() {
        let result = compile(
            "memory:///main.ts",
            &MapLoader::from([ModuleSource::new(
                "memory:///main.ts",
                case["source"].as_str().unwrap(),
            )]),
            CompilerOptions {
                checking: Some(CheckingOptions::default()),
                ..CompilerOptions::default()
            },
        );
        assert_eq!(
            !result.has_errors(),
            case["accepts"].as_bool().unwrap(),
            "{}: {:#?}",
            case["name"],
            result.diagnostics
        );
    }
}
