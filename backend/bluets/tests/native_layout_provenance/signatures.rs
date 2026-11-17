// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Complete callable maps retain parameter and statement endpoint inventories.
use super::*;

#[test]
fn adjacent_generic_return_arrows_keep_one_native_entry_in_both_layouts() {
    for prefix in ["", "/* 🧭 */ "] {
        let source = format!("{prefix}export const calculate=async(value:number=40):Promise<number>=>{{return value;}};\n");
        let original = source.find("=>").unwrap();
        let expected = json!([0, 0, source[..original].encode_utf16().count()]);
        for layout in [
            JavaScriptLayout::PreserveSource,
            JavaScriptLayout::TypeScript,
        ] {
            for newline in [NewLine::Lf, NewLine::CrLf] {
                let id = "memory:///main.ts";
                let result = compile(
                    id,
                    &MapLoader::from([ModuleSource::new(id, &source)]),
                    CompilerOptions {
                        target: EcmaTarget::EsNext,
                        module_kind: ModuleKind::Esm,
                        libraries: Some(vec![EcmaTarget::Es2022]),
                        source_map: true,
                        javascript_layout: layout,
                        new_line: newline,
                        ..CompilerOptions::default()
                    },
                );
                assert!(!result.has_errors(), "{:?}", result.diagnostics);
                let artifact = &result.output.as_ref().unwrap().artifacts[id];
                let decoded = maps::decoded(&artifact.source_map.as_ref().unwrap().to_json());
                let arrow = artifact.javascript.find("=>").unwrap();
                for offset in arrow..arrow + 2 {
                    assert_eq!(
                        owner(&artifact.javascript, &decoded, offset),
                        expected,
                        "complete original arrow: {}",
                        artifact.javascript
                    );
                }
            }
        }
    }
}

#[test]
fn default_rest_signatures_keep_every_native_segment_after_type_erasure() {
    let reference: Value = serde_json::from_str(
        &std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/ge5_source_maps/reference.json"),
        )
        .unwrap(),
    )
    .unwrap();
    for (index, module, variant, module_kind) in [
        (7604, "commonjs", "original", ModuleKind::CommonJs),
        (7605, "commonjs", "crlfAstral", ModuleKind::CommonJs),
        (7612, "esnext", "original", ModuleKind::Esm),
        (7613, "esnext", "crlfAstral", ModuleKind::Esm),
    ] {
        let case = &reference["cases"][index];
        assert_eq!(case["id"], format!("gd2_declaration_maps/parameter-default-rest-{module}-runtime-regular-accept/{variant}/external"));
        let expected = frozen_native_map(&reference, case, "out/main.js.map");
        let source = expected["metadata"]["sourcesContent"][0].as_str().unwrap();
        let native = reference["blobs"][case["outputs"]["out/main.js"].as_str().unwrap()]
            .as_str()
            .unwrap()
            .split("//# sourceMappingURL=")
            .next()
            .unwrap()
            .trim_end();
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
                "complete default-rest signature {index}: {code}"
            );
        }
    }
}

#[test]
fn a_spaced_default_parameter_keeps_its_complete_native_inventory() {
    // Frozen from a full TypeScript 5.9.3 program, with zero diagnostics.
    let source = "export function read(value:number =42, ...rest:number[]):number {return value;}\nexport const result:number = read();\n";
    let native = "\"use strict\";\nObject.defineProperty(exports, \"__esModule\", { value: true });\nexports.result = void 0;\nexports.read = read;\nfunction read(value = 42, ...rest) { return value; }\nexports.result = read();";
    let expected = json!([
        [3, 0, 0, 0, 0],
        [3, 20, 0, 0, 79],
        [4, 0, 0, 0, 0],
        [4, 9, 0, 0, 16],
        [4, 13, 0, 0, 20],
        [4, 14, 0, 0, 21],
        [4, 22, 0, 0, 35],
        [4, 24, 0, 0, 37],
        [4, 26, 0, 0, 39],
        [4, 29, 0, 0, 42],
        [4, 33, 0, 0, 55],
        [4, 37, 0, 0, 65],
        [4, 44, 0, 0, 72],
        [4, 49, 0, 0, 77],
        [4, 50, 0, 0, 78],
        [4, 51, 0, 0, 78],
        [4, 52, 0, 0, 79],
        [5, 0, 0, 1, 13],
        [5, 8, 0, 1, 13],
        [5, 14, 0, 1, 19],
        [5, 17, 0, 1, 29],
        [5, 21, 0, 1, 33],
        [5, 23, 0, 1, 35],
        [5, 24, 0, 1, 36]
    ]);
    for newline in [NewLine::Lf, NewLine::CrLf] {
        let (code, actual) = artifact(source, EcmaTarget::Es2022, newline);
        let expected_code = if newline == NewLine::CrLf {
            native.replace('\n', "\r\n")
        } else {
            native.to_owned()
        };
        assert_eq!(code.trim_end(), expected_code);
        assert_eq!(
            actual["segments"], expected,
            "complete spaced default: {code}"
        );
    }
}
