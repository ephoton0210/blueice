// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Parenthesized typed atomic operands erase with their complete source endpoint.
use blueice_bluets::{compile, CompilerOptions, EcmaTarget, MapLoader, ModuleKind, ModuleSource};
#[test]
fn parenthesized_atomic_assertion_keeps_the_surviving_operand_and_complete_endpoint() {
    for prefix in ["", "/* G-E5 🧭 */\r\n"] {
        let source = format!("{prefix}export const result=(globalThis as any).marker;\r\n");
        for target in [
            EcmaTarget::Es5,
            EcmaTarget::Es2015,
            EcmaTarget::Es2022,
            EcmaTarget::EsNext,
        ] {
            for module_kind in [ModuleKind::CommonJs, ModuleKind::Esm] {
                for maps in [false, true] {
                    let id = "memory:///main.ts";
                    let result = compile(
                        id,
                        &MapLoader::from([ModuleSource::new(id, &source)]),
                        CompilerOptions {
                            target,
                            module_kind,
                            source_map: maps,
                            inline_sources: true,
                            libraries: Some(vec![EcmaTarget::Es2022]),
                            ..CompilerOptions::default()
                        },
                    );
                    assert!(!result.has_errors(), "{:?}", result.diagnostics);
                    let artifact = &result.output.unwrap().artifacts[id];
                    let code = &artifact.javascript;
                    assert!(
                        code.contains("globalThis.marker"),
                        "transparent typed grouping is erased: {code}"
                    );
                    if !maps {
                        continue;
                    }
                    let map = artifact.source_map.as_ref().unwrap();
                    let start = code.find("globalThis").unwrap();
                    let original = source.find("globalThis").unwrap();
                    let endpoint = source.find(").marker").unwrap() + 1;
                    for offset in start..=start + "globalThis".len() {
                        let before = &code[..offset];
                        let line = before.bytes().filter(|byte| *byte == b'\n').count();
                        let column = before.rsplit('\n').next().unwrap().encode_utf16().count();
                        let owner = map
                            .original_position_at(code, line, column, 100_000, 32_000_000)
                            .unwrap()
                            .map(|owner| (owner.line, owner.column_utf16));
                        let source_offset = if offset == start + "globalThis".len() {
                            endpoint
                        } else {
                            original
                        };
                        let before = &source[..source_offset];
                        let expected = (
                            before.bytes().filter(|byte| *byte == b'\n').count(),
                            before.rsplit('\n').next().unwrap().encode_utf16().count(),
                        );
                        assert_eq!(
                            owner,
                            Some(expected),
                            "complete atomic assertion operand/end {target:?}"
                        );
                    }
                }
            }
        }
    }
}
#[test]
fn non_atomic_assertion_retains_its_precedence_grouping() {
    let id = "memory:///main.ts";
    let result = compile(
        id,
        &MapLoader::from([ModuleSource::new(
            id,
            "export const result=(1+2 as number)*3;\n",
        )]),
        CompilerOptions::default(),
    );
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    let code = &result.output.unwrap().artifacts[id].javascript;
    assert!(
        code.contains("(1+2 )*3"),
        "compound expression precedence is retained: {code}"
    );
}
