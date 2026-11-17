// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Nested class factories complete at their own original source boundary.
use super::*;

#[test]
fn nested_class_returns_keep_every_native_token_character_and_endpoint() {
    let reference: Value = serde_json::from_str(
        &std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/ge5_source_maps/reference.json"),
        )
        .unwrap(),
    )
    .unwrap();
    for (index, variant) in [(6734, "original"), (6735, "crlfAstral")] {
        let case = &reference["cases"][index];
        assert_eq!(case["id"], format!("ge3_decorator_compositions/ge3-decorators-static-super-arrows-nested-homeobjects-standard-es5-commonjs-assign/{variant}/external"));
        let expected = frozen_native_map(&reference, case, "out/main.js.map");
        let source = expected["metadata"]["sourcesContent"][0].as_str().unwrap();
        let native = reference["blobs"][case["outputs"]["out/main.js"].as_str().unwrap()]
            .as_str()
            .unwrap();
        let text = "return Nested;";
        let native_start = native.find(text).unwrap();
        for newline in [NewLine::Lf, NewLine::CrLf] {
            let (code, actual) = artifact(source, EcmaTarget::Es5, newline);
            let start = code.find(text).unwrap();
            for offset in 0..=text.len() {
                assert_eq!(
                    owner(&code, &actual, start + offset),
                    owner(native, &expected, native_start + offset),
                    "nested class return {index} + {offset}: {code}"
                );
            }
        }
    }
}
