// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Named exports preserve the complete Native map, including trivia events.
use blueice_bluets::{
    compile, CompilerOptions, EcmaTarget, JavaScriptLayout, MapLoader, ModuleKind, ModuleSource,
    NewLine,
};
use serde_json::Value;

#[allow(dead_code)]
#[path = "oracle_support/gd2_maps.rs"]
mod maps;

#[test]
fn named_exports_keep_every_native_segment_without_redundant_keyword_events() {
    let fixture: Value = serde_json::from_str(include_str!(
        "fixtures/named_export_boundaries/native.v1.json"
    ))
    .unwrap();
    assert_eq!(fixture["typescript"], "5.9.3");
    assert_eq!(fixture["nativeRepeatPrograms"], 2);
    let cases = fixture["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 48);
    for row in cases {
        let id = "memory:///types.ts";
        let target = match row["target"].as_str().unwrap() {
            "ES5" => EcmaTarget::Es5,
            "ES2022" => EcmaTarget::Es2022,
            _ => panic!("unexpected target"),
        };
        let newline = match row["newLine"].as_str().unwrap() {
            "LF" => NewLine::Lf,
            "CRLF" => NewLine::CrLf,
            _ => panic!("unexpected newline"),
        };
        let result = compile(
            id,
            &MapLoader::from([ModuleSource::new(id, row["source"].as_str().unwrap())]),
            CompilerOptions {
                target,
                libraries: Some(vec![EcmaTarget::Es2022]),
                module_kind: ModuleKind::Esm,
                javascript_layout: JavaScriptLayout::TypeScript,
                source_map: true,
                new_line: newline,
                use_define_for_class_fields: Some(false),
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
