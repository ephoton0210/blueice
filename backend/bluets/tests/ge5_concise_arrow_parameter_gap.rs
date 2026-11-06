// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! An erased arrow leaves its original closing parameter phase across surviving trivia.
use blueice_bluets::{compile, CompilerOptions, EcmaTarget, MapLoader, ModuleKind, ModuleSource};
#[test]
fn parenthesized_plain_concise_arrow_retains_the_close_role_before_its_body() {
    for prefix in ["", "/* G-E5 🧭 */\r\n"] {
        for separator in [" ", "\t "] {
            let source = format!("{prefix}export const fn=(value:number){separator}=>value+1;\r\n");
            for module_kind in [ModuleKind::CommonJs, ModuleKind::Esm] {
                let id = "memory:///main.ts";
                let result = compile(
                    id,
                    &MapLoader::from([ModuleSource::new(id, &source)]),
                    CompilerOptions {
                        target: EcmaTarget::Es5,
                        module_kind,
                        source_map: true,
                        inline_sources: true,
                        libraries: Some(vec![EcmaTarget::Es2022]),
                        ..CompilerOptions::default()
                    },
                );
                assert!(!result.has_errors(), "{:?}", result.diagnostics);
                let artifact = &result.output.unwrap().artifacts[id];
                let code = &artifact.javascript;
                let map = artifact.source_map.as_ref().unwrap();
                let tokens = blueice_bluets::lex("generated.js", code).unwrap();
                let function = tokens
                    .iter()
                    .position(|token| token.is("function"))
                    .unwrap();
                let close = function
                    + tokens[function..]
                        .iter()
                        .position(|token| token.is(")"))
                        .unwrap();
                assert!(tokens[close + 1].is("{"));
                let original = source.find(')').unwrap();
                let before = &source[..original];
                let expected = (
                    before.bytes().filter(|byte| *byte == b'\n').count(),
                    before.rsplit('\n').next().unwrap().encode_utf16().count(),
                );
                for offset in tokens[close].end..tokens[close + 1].start {
                    let before = &code[..offset];
                    let line = before.bytes().filter(|byte| *byte == b'\n').count();
                    let column = before.rsplit('\n').next().unwrap().encode_utf16().count();
                    let owner = map
                        .original_position_at(code, line, column, 100_000, 32_000_000)
                        .unwrap()
                        .map(|owner| (owner.line, owner.column_utf16));
                    assert_eq!(
                        owner,
                        Some(expected),
                        "complete parenthesized arrow parameter-close gap"
                    );
                }
            }
        }
    }
}
