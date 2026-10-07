// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Additional compiler locations outside the existing language matrix.

use blueice_bluets::{Diagnostic, DiagnosticCode, SourceSpan};
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
