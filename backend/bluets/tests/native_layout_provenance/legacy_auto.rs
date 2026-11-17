// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The original class publication ends before its separate decorator phase.
use super::*;

#[test]
fn legacy_auto_accessor_publications_keep_the_native_endpoint_and_gap() {
    let reference: Value = serde_json::from_str(
        &std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/ge5_source_maps/reference.json"),
        )
        .unwrap(),
    )
    .unwrap();
    for (base, edition, target, define) in [
        (6310, "es2022", EcmaTarget::Es2022, false),
        (6312, "es2022", EcmaTarget::Es2022, true),
        (6318, "esnext", EcmaTarget::EsNext, false),
        (6320, "esnext", EcmaTarget::EsNext, true),
    ] {
        for (offset, variant) in ["original", "crlfAstral"].into_iter().enumerate() {
            let index = base + offset;
            let case = &reference["cases"][index];
            let mode = if define { "define" } else { "assign" };
            assert_eq!(case["id"], format!("ge3_decorator_compositions/ge3-decorators-literal-auto-accessors-legacy-{edition}-commonjs-{mode}/{variant}/external"));
            let expected = frozen_native_map(&reference, case, "out/main.js.map");
            let source = expected["metadata"]["sourcesContent"][0].as_str().unwrap();
            let native = reference["blobs"][case["outputs"]["out/main.js"].as_str().unwrap()]
                .as_str()
                .unwrap();
            let publication = "exports.C = C;";
            let native_start = native.find(publication).unwrap();
            for newline in [NewLine::Lf, NewLine::CrLf] {
                let id = "memory:///main.ts";
                let result = compile(
                    id,
                    &MapLoader::from([ModuleSource::new(id, source)]),
                    CompilerOptions {
                        target,
                        module_kind: ModuleKind::CommonJs,
                        source_map: true,
                        experimental_decorators: true,
                        use_define_for_class_fields: Some(define),
                        javascript_layout: JavaScriptLayout::TypeScript,
                        new_line: newline,
                        ..CompilerOptions::default()
                    },
                );
                assert!(!result.has_errors(), "{:?}", result.diagnostics);
                let artifact = &result.output.as_ref().unwrap().artifacts[id];
                let code = &artifact.javascript;
                let actual = maps::decoded(&artifact.source_map.as_ref().unwrap().to_json());
                let start = code.find(publication).unwrap();
                for offset in 0..=publication.len() {
                    assert_eq!(
                        owner(code, &actual, start + offset),
                        owner(native, &expected, native_start + offset),
                        "whole legacy publication {index} + {offset}: {code}"
                    );
                }
                let end = start + publication.len();
                let call = end + code[end..].find("__bluetsDecorate").unwrap();
                for (offset, ch) in code[end..call].char_indices() {
                    assert!(ch.is_whitespace());
                    assert_eq!(
                        owner(code, &actual, end + offset),
                        owner(native, &expected, native_start + publication.len()),
                        "legacy publication separator {index}: {code}"
                    );
                }
            }
        }
    }
}
