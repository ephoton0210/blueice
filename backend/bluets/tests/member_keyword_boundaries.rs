// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Property keywords retain call/index semantics across restricted line breaks.
use blueice_bluets::{
    compile, CompilerOptions, EcmaTarget, JavaScriptLayout, MapLoader, ModuleKind, ModuleSource,
    NewLine,
};
use serde_json::Value;

#[allow(dead_code)]
#[path = "oracle_support/gd2_maps.rs"]
mod maps;

#[test]
fn direct_member_keywords_keep_complete_native_javascript_and_every_map_segment() {
    let fixture: Value = serde_json::from_str(include_str!(
        "fixtures/member_keyword_boundaries/direct.v1.json"
    ))
    .unwrap();
    assert_eq!(fixture["typescript"], "5.9.3");
    assert_eq!(fixture["nativeRepeatPrograms"], 2);
    let cases = fixture["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 12);
    for row in cases {
        let id = "memory:///main.ts";
        let result = compile(
            id,
            &MapLoader::from([ModuleSource::new(id, row["source"].as_str().unwrap())]),
            CompilerOptions {
                target: EcmaTarget::Es2022,
                module_kind: ModuleKind::Esm,
                javascript_layout: JavaScriptLayout::TypeScript,
                source_map: true,
                new_line: match row["newLine"].as_str().unwrap() {
                    "LineFeed" => NewLine::Lf,
                    "CarriageReturnLineFeed" => NewLine::CrLf,
                    _ => panic!("unexpected Native newline"),
                },
                ..CompilerOptions::default()
            },
        );
        assert!(!result.has_errors(), "{:?}", result.diagnostics);
        let artifact = &result.output.as_ref().unwrap().artifacts[id];
        assert_eq!(
            artifact.javascript.trim_end(),
            row["javascript"].as_str().unwrap().trim_end(),
            "complete JavaScript {}",
            row["id"]
        );
        let decoded = maps::decoded(&artifact.source_map.as_ref().unwrap().to_json());
        assert_eq!(
            decoded["segments"], row["segments"],
            "complete map {}",
            row["id"]
        );
    }
}
