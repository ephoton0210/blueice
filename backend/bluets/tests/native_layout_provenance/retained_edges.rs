// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Consecutive comments and default-export modifiers keep exact Native events.
use super::*;

#[test]
fn retained_private_generator_headers_keep_native_adjacency_and_name_owners() {
    let reference: Value = serde_json::from_str(
        &std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/ge5_source_maps/reference.json"),
        )
        .unwrap(),
    )
    .unwrap();
    for (index, kind) in [
        (6794, ModuleKind::CommonJs),
        (6795, ModuleKind::CommonJs),
        (6796, ModuleKind::Esm),
        (6797, ModuleKind::Esm),
    ] {
        let case = &reference["cases"][index];
        assert!(case["id"]
            .as_str()
            .unwrap()
            .contains("private-super-async-generator-standard-esnext"));
        let expected = frozen_native_map(&reference, case, "out/main.js.map");
        let source = expected["metadata"]["sourcesContent"][0].as_str().unwrap();
        let native = reference["blobs"][case["outputs"]["out/main.js"].as_str().unwrap()]
            .as_str()
            .unwrap();
        let name = "#iterate";
        let native_name = native.find(name).unwrap();
        assert_eq!(&native[native_name - 1..native_name], "*");
        for newline in [NewLine::Lf, NewLine::CrLf] {
            let id = "memory:///main.ts";
            let result = compile(
                id,
                &MapLoader::from([ModuleSource::new(id, source)]),
                CompilerOptions {
                    target: EcmaTarget::EsNext,
                    module_kind: kind,
                    libraries: Some(vec![EcmaTarget::Es2022]),
                    use_define_for_class_fields: Some(true),
                    source_map: true,
                    javascript_layout: JavaScriptLayout::TypeScript,
                    new_line: newline,
                    ..CompilerOptions::default()
                },
            );
            assert!(!result.has_errors(), "{:?}", result.diagnostics);
            let artifact = &result.output.as_ref().unwrap().artifacts[id];
            let decoded = maps::decoded(&artifact.source_map.as_ref().unwrap().to_json());
            let actual_name = artifact.javascript.find(name).unwrap();
            assert_eq!(
                &artifact.javascript[actual_name - 1..actual_name],
                "*",
                "Native generator-name adjacency: {}",
                artifact.javascript
            );
            for offset in 0..name.len() {
                assert_eq!(
                    owner(&artifact.javascript, &decoded, actual_name + offset),
                    owner(native, &expected, native_name + offset),
                    "private name {index}"
                );
            }
        }
    }
}

fn complete(index: usize, expected_id: &str, module_kind: ModuleKind) {
    let reference: Value = serde_json::from_str(
        &std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/ge5_source_maps/reference.json"),
        )
        .unwrap(),
    )
    .unwrap();
    let case = &reference["cases"][index];
    assert_eq!(case["id"], expected_id);
    let expected = frozen_native_map(&reference, case, "out/main.js.map");
    let source = expected["metadata"]["sourcesContent"][0].as_str().unwrap();
    let native = reference["blobs"][case["outputs"]["out/main.js"].as_str().unwrap()]
        .as_str()
        .unwrap()
        .split("//# sourceMappingURL=")
        .next()
        .unwrap()
        .trim_end()
        .replace("\r\n", "\n");
    for newline in [NewLine::Lf, NewLine::CrLf] {
        let id = "memory:///main.ts";
        let result = compile(
            id,
            &MapLoader::from([ModuleSource::new(id, source)]),
            CompilerOptions {
                target: EcmaTarget::Es2022,
                module_kind,
                source_map: true,
                javascript_layout: JavaScriptLayout::TypeScript,
                new_line: newline,
                ..CompilerOptions::default()
            },
        );
        assert!(!result.has_errors(), "{:?}", result.diagnostics);
        let artifact = &result.output.as_ref().unwrap().artifacts[id];
        let code = &artifact.javascript;
        let expected_code = if newline == NewLine::CrLf {
            native.replace('\n', "\r\n")
        } else {
            native.to_owned()
        };
        assert_eq!(code.trim_end(), expected_code);
        let actual = maps::decoded(&artifact.source_map.as_ref().unwrap().to_json());
        assert_eq!(
            actual["segments"], expected["segments"],
            "complete retained boundary {index}: {code}"
        );
    }
}

#[test]
fn consecutive_unicode_comments_keep_every_native_start_and_end() {
    for (index, module, mode, kind) in [
        (7781, "commonjs", "regular", ModuleKind::CommonJs),
        (7783, "commonjs", "isolated", ModuleKind::CommonJs),
        (7789, "esnext", "regular", ModuleKind::Esm),
        (7791, "esnext", "isolated", ModuleKind::Esm),
    ] {
        complete(index, &format!("gd2_declaration_maps/unicode-comment-metadata-{module}-runtime-{mode}-accept/crlfAstral/external"), kind);
    }
}

#[test]
fn default_export_expressions_do_not_duplicate_the_modifier_owner() {
    for (index, variant) in [(7880, "original"), (7881, "crlfAstral")] {
        complete(index, &format!("gd2_declaration_maps/isolated-default-expression-esnext-runtime-regular-accept/{variant}/external"), ModuleKind::Esm);
    }
}

#[test]
fn exported_legacy_static_fields_keep_the_native_publication_completion() {
    let reference: Value = serde_json::from_str(
        &std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/ge5_source_maps/reference.json"),
        )
        .unwrap(),
    )
    .unwrap();
    for (index, define, variant) in [
        (6254, false, "original"),
        (6255, false, "crlfAstral"),
        (6256, true, "original"),
        (6257, true, "crlfAstral"),
    ] {
        let case = &reference["cases"][index];
        let mode = if define { "define" } else { "assign" };
        assert_eq!(case["id"], format!("ge3_decorator_compositions/ge3-decorators-literal-fields-legacy-es2015-commonjs-{mode}/{variant}/external"));
        let expected = frozen_native_map(&reference, case, "out/main.js.map");
        let source = expected["metadata"]["sourcesContent"][0].as_str().unwrap();
        let native = reference["blobs"][case["outputs"]["out/main.js"].as_str().unwrap()]
            .as_str()
            .unwrap();
        let publication = "exports.C = C;";
        let expected_owner = owner(
            native,
            &expected,
            native.find(publication).unwrap() + publication.len(),
        );
        assert_eq!(expected_owner, json!([0, 3, 100]));
        for newline in [NewLine::Lf, NewLine::CrLf] {
            let id = "memory:///main.ts";
            let result = compile(
                id,
                &MapLoader::from([ModuleSource::new(id, source)]),
                CompilerOptions {
                    target: EcmaTarget::Es2015,
                    module_kind: ModuleKind::CommonJs,
                    libraries: Some(vec![EcmaTarget::Es2022]),
                    experimental_decorators: true,
                    use_define_for_class_fields: Some(define),
                    source_map: true,
                    javascript_layout: JavaScriptLayout::TypeScript,
                    new_line: newline,
                    ..CompilerOptions::default()
                },
            );
            assert!(!result.has_errors(), "{:?}", result.diagnostics);
            let artifact = &result.output.as_ref().unwrap().artifacts[id];
            let decoded = maps::decoded(&artifact.source_map.as_ref().unwrap().to_json());
            let boundary = artifact.javascript.find(publication).unwrap() + publication.len();
            assert_eq!(
                owner(&artifact.javascript, &decoded, boundary),
                expected_owner,
                "publication ending before static field {index}: {}",
                artifact.javascript
            );
        }
    }
}
