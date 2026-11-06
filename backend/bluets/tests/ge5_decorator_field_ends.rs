// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A field completion survives the following generated decorator phase boundary.
use blueice_bluets::{compile, CompilerOptions, EcmaTarget, MapLoader, ModuleKind, ModuleSource};

#[test]
fn decorator_field_completions_precede_the_following_generated_initialization() {
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/ge3_decorator_compositions/cases.json");
    let cases: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fixture).unwrap()).unwrap();
    for legacy in [false, true] {
        let case = format!(
            "ge3-decorators-literal-fields-{}-es5-commonjs-assign",
            if legacy { "legacy" } else { "standard" }
        );
        let original = cases["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"] == case)
            .unwrap()["files"]["main.ts"]
            .as_str()
            .unwrap();
        for astral in [false, true] {
            let source = if astral {
                format!("/* G-E5 🧭 */ {}", original.replace('\n', "\r\n"))
            } else {
                original.to_string()
            };
            let position = |text: &str, offset| {
                let before = &text[..offset];
                (
                    before.bytes().filter(|byte| *byte == b'\n').count(),
                    before.rsplit('\n').next().unwrap().encode_utf16().count(),
                )
            };
            for target in [EcmaTarget::Es5, EcmaTarget::Es2015, EcmaTarget::Es2022] {
                for define in [false, true] {
                    let id = "memory:///main.ts";
                    let result = compile(
                        id,
                        &MapLoader::from([ModuleSource::new(id, &source)]),
                        CompilerOptions {
                            target,
                            module_kind: ModuleKind::CommonJs,
                            source_map: true,
                            inline_sources: true,
                            experimental_decorators: legacy,
                            use_define_for_class_fields: Some(define),
                            libraries: Some(vec![EcmaTarget::Es2022]),
                            ..CompilerOptions::default()
                        },
                    );
                    assert!(!result.has_errors(), "{:?}", result.diagnostics);
                    let artifact = &result.output.unwrap().artifacts[id];
                    let code = &artifact.javascript;
                    let map = artifact.source_map.as_ref().unwrap();
                    let owner = |offset| {
                        let (line, column) = position(code, offset);
                        map.original_position_at(code, line, column, 100_000, 32_000_000)
                            .unwrap()
                            .map(|owner| (owner.line, owner.column_utf16))
                    };
                    let tokens = blueice_bluets::lex("generated.js", code).unwrap();
                    for (key, value) in [("1.5", "2"), ("'static x'", "3")] {
                        if !legacy && value == "3" {
                            continue;
                        }
                        let name = source.find(&format!("{key}:")).unwrap();
                        let end = name + source[name..].find(';').unwrap() + 1;
                        let initializer = end - 2;
                        let at = tokens
                            .iter()
                            .position(|token| {
                                token.text == value
                                    && owner(token.start) == Some(position(&source, initializer))
                            })
                            .expect("complete source initializer identity");
                        let semicolon = tokens[at..].iter().find(|token| token.is(";")).unwrap();
                        assert_eq!(owner(semicolon.end),Some(position(&source,end)),"legacy={legacy} {target:?} define={define} field {key} logical completion");
                    }
                }
            }
        }
    }
}
