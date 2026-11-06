// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Reparsing generated classes keeps decorated static-block identity through original endpoints.
use blueice_bluets::{compile, CompilerOptions, EcmaTarget, MapLoader, ModuleKind, ModuleSource};

#[test]
fn decorated_static_store_headers_keep_the_original_member_origin() {
    for prefix in ["", "/* G-E5 🧭 */ "] {
        let source=format!("{prefix}function dec(value:any,context:any):any {{}} export class Box {{ @dec a:number=1; @dec static 'static x':number=3; }} export const result:number=Box['static x'];\n");
        let expected = source.find("static 'static x'").unwrap();
        let position = |text: &str, offset| {
            let before = &text[..offset];
            (
                before.bytes().filter(|byte| *byte == b'\n').count(),
                before.rsplit('\n').next().unwrap().encode_utf16().count(),
            )
        };
        let id = "memory:///main.ts";
        let build = |source_map| {
            let result = compile(
                id,
                &MapLoader::from([ModuleSource::new(id, &source)]),
                CompilerOptions {
                    target: EcmaTarget::Es2022,
                    module_kind: ModuleKind::CommonJs,
                    source_map,
                    inline_sources: source_map,
                    use_define_for_class_fields: Some(false),
                    libraries: Some(vec![EcmaTarget::Es2022]),
                    ..CompilerOptions::default()
                },
            );
            assert!(!result.has_errors(), "{:?}", result.diagnostics);
            result.output.unwrap().artifacts[id].clone()
        };
        let artifact = build(true);
        let without = build(false);
        assert_eq!(
            artifact.javascript, without.javascript,
            "map-only field identity does not alter emitted JavaScript bytes"
        );
        let code = &artifact.javascript;
        let map = artifact.source_map.as_ref().unwrap();
        let owner = |offset| {
            let (line, column) = position(code, offset);
            map.original_position_at(code, line, column, 100_000, 32_000_000)
                .unwrap()
                .map(|owner| (owner.line, owner.column_utf16))
        };
        let tokens = blueice_bluets::lex("generated.js", code).unwrap();
        let at = tokens
            .iter()
            .position(|t| {
                t.is("3")
                    && owner(t.start) == Some(position(&source, source.find("=3;").unwrap() + 1))
            })
            .expect("original static initializer");
        let start = tokens[..at].iter().rposition(|t| t.is("static")).unwrap();
        assert!(tokens[start + 1].is("{"));
        for token in &tokens[start..=start + 1] {
            for offset in token.start..token.end {
                assert_eq!(
                    owner(offset),
                    Some(position(&source, expected)),
                    "decorated static block uses its exact original logical member header"
                );
            }
        }
    }
}
