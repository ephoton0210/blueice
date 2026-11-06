// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A source literal name retains its logical end inside a generated descriptor.
use blueice_bluets::{compile, CompilerOptions, EcmaTarget, MapLoader, ModuleKind, ModuleSource};

#[test]
fn literal_descriptor_keys_retain_their_source_name_completion() {
    for prefix in ["", "/* G-E5 🧭 */ "] {
        let source=format!("{prefix}export class Box {{ 'a-b':number=40; 1.5:number=2; static 'static x':number=3; }} export const result:any=new Box();\n");
        let position = |text: &str, offset| {
            let before = &text[..offset];
            (
                before.bytes().filter(|byte| *byte == b'\n').count(),
                before.rsplit('\n').next().unwrap().encode_utf16().count(),
            )
        };
        for target in [EcmaTarget::Es5, EcmaTarget::Es2015] {
            let id = "memory:///main.ts";
            let result = compile(
                id,
                &MapLoader::from([ModuleSource::new(id, &source)]),
                CompilerOptions {
                    target,
                    module_kind: ModuleKind::CommonJs,
                    source_map: true,
                    inline_sources: true,
                    use_define_for_class_fields: Some(true),
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
            for (original, generated) in [
                ("'a-b'", "\"a-b\""),
                ("1.5", "\"1.5\""),
                ("'static x'", "\"static x\""),
            ] {
                let start = source.find(&format!("{original}:")).unwrap();
                let end = start + original.len();
                let at = tokens
                    .iter()
                    .position(|token| token.text == generated)
                    .unwrap();
                for offset in tokens[at].start..tokens[at].end {
                    assert_eq!(
                        owner(offset),
                        Some(position(&source, start)),
                        "complete literal-key UTF-16 interior"
                    );
                }
                assert!(tokens[at + 1].is(","));
                assert!(tokens[at + 2].is("{"));
                for token in &tokens[at + 1..=at + 2] {
                    for offset in token.start..token.end {
                        assert_eq!(
                            owner(offset),
                            Some(position(&source, end)),
                            "descriptor prefix retains the exact literal-name end"
                        );
                    }
                }
            }
        }
    }
}
