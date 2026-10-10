// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Additional compiler locations outside the existing language matrix.

use blueice_bluets::{
    compile, CompilerOptions, Diagnostic, DiagnosticCode, MapLoader, ModuleSource, SourceSpan,
};
use serde_json::Value;
use std::path::Path;
use std::process::Command;

#[test]
fn lexical_error_families_use_the_recorded_typescript_template() {
    let reference: Value =
        serde_json::from_str(include_str!("fixtures/diagnostics/source-families.json")).unwrap();
    for id in ["comment", "string", "template"] {
        let case = reference["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|case| case["id"] == format!("diagnostics-other-{id}"))
            .unwrap();
        let diagnostic = Diagnostic::error(
            DiagnosticCode::ParseError,
            SourceSpan::new("memory:///source.ts", 0, 1),
            case["blueMessage"].as_str().unwrap(),
        );
        let json = diagnostic.to_json();
        assert_eq!(
            json["typescript"]["code"], case["diagnostics"][0]["code"],
            "{id}: {json}"
        );
        assert_eq!(json["btsCode"], "BTS1000");
    }
}

#[test]
fn source_witnesses_have_precise_counterparts_or_recorded_subset_reasons() {
    let reference: Value =
        serde_json::from_str(include_str!("fixtures/diagnostics/source-families.json")).unwrap();
    let known_checker_gaps = ["tuple-index", "throw-newline", "throw-empty"];
    let subset_refusals = [
        "rest-alias",
        "catch-type",
        "name-type",
        "name-area",
        "compatible-function-field",
        "setter-destructure",
    ];
    for case in reference["cases"].as_array().unwrap() {
        let id = case["id"].as_str().unwrap();
        let suffix = id.strip_prefix("diagnostics-other-").unwrap();
        let source = case["source"].as_str().unwrap();
        let mut options = CompilerOptions::default();
        let flags = case["flags"].as_array().unwrap();
        if !flags.is_empty() {
            options.checking = Some(blueice_bluets::CheckingOptions {
                no_unused_locals: flags.iter().any(|flag| flag == "--noUnusedLocals"),
                ..Default::default()
            });
        }
        let result = compile(
            "memory:///witness.ts",
            &MapLoader::from([ModuleSource::new("memory:///witness.ts", source)]),
            options,
        );
        if case["accepts"] == true && matches!(suffix, "apply-array" | "argument-spread-rest") {
            assert!(
                result.diagnostics.is_empty(),
                "{id}: {:?}",
                result.diagnostics
            );
            continue;
        }
        if known_checker_gaps.contains(&suffix) {
            assert!(
                result.diagnostics.is_empty(),
                "recorded checker gap changed: {id}: {:?}",
                result.diagnostics
            );
            continue;
        }
        let primary = result
            .diagnostics
            .first()
            .unwrap_or_else(|| panic!("missing diagnostic: {id}"));
        let json = primary.to_json();
        if subset_refusals.contains(&suffix) {
            assert!(json["typescript"].is_null(), "{id}: {json}");
            assert!(
                json["noTypeScriptCounterpart"].as_str().is_some(),
                "{id}: {json}"
            );
        } else {
            let counterpart = primary
                .typescript
                .as_ref()
                .unwrap_or_else(|| panic!("unmapped {id}: {json}"));
            assert!(
                case["diagnostics"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|recorded| {
                        recorded["code"].as_u64() == Some(u64::from(counterpart.code))
                    }),
                "unwitnessed counterpart {id}: {json}"
            );
            if matches!(suffix, "call-this-missing" | "call-this-type") {
                let recorded = &case["diagnostics"][0];
                assert_eq!(
                    counterpart.span.start as u64,
                    recorded["start"].as_u64().unwrap(),
                    "{id}: {json}"
                );
                assert_eq!(
                    (counterpart.span.end - counterpart.span.start) as u64,
                    recorded["length"].as_u64().unwrap(),
                    "{id}: {json}"
                );
                assert_eq!(
                    counterpart.message,
                    recorded["message"].as_str().unwrap(),
                    "{id}: {json}"
                );
            }
            assert!(json["noTypeScriptCounterpart"].is_null(), "{id}: {json}");
        }
        assert!(result.output.is_none(), "{id}");
        for diagnostic in &result.diagnostics {
            let json = diagnostic.to_json();
            assert!(
                diagnostic.typescript.is_some()
                    || json["noTypeScriptCounterpart"].as_str().is_some(),
                "unclassified {id}: {json}"
            );
        }
    }
}

#[test]
#[ignore = "requires BLUEICE_BLUETSC_ORACLE to point to the pinned TypeScript compiler"]
fn source_witnesses_match_pinned_typescript() {
    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../development/browser_core/phase-18-bluets/tools/record_diagnostic_source_families.cjs");
    let output = Command::new("node").arg(script).output().unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
