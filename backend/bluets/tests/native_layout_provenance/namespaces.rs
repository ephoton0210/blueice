// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Namespace publication preserves complete original callable boundaries.
use super::*;

#[test]
fn namespace_entries_and_member_publications_keep_every_native_segment() {
    let reference: Value = serde_json::from_str(
        &std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/ge5_source_maps/reference.json"),
        )
        .unwrap(),
    )
    .unwrap();
    for (base, form, module, module_kind) in [
        (7620, "namespace-nested", "commonjs", ModuleKind::CommonJs),
        (7628, "namespace-nested", "esnext", ModuleKind::Esm),
        (7636, "namespace-merging", "commonjs", ModuleKind::CommonJs),
        (7644, "namespace-merging", "esnext", ModuleKind::Esm),
        (
            7652,
            "function-namespace-merge",
            "commonjs",
            ModuleKind::CommonJs,
        ),
        (7660, "function-namespace-merge", "esnext", ModuleKind::Esm),
    ] {
        for (offset, variant) in ["original", "crlfAstral"].into_iter().enumerate() {
            let index = base + offset;
            let case = &reference["cases"][index];
            assert_eq!(case["id"], format!("gd2_declaration_maps/{form}-{module}-runtime-regular-accept/{variant}/external"));
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
                    "complete namespace {index}: {code}"
                );
            }
        }
    }
}
