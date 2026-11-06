// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Relocation endpoints survive factory moves without granting execute gaps.
use blueice_bluets::{compile, CompilerOptions, EcmaTarget, MapLoader, ModuleKind, ModuleSource};

#[test]
fn system_function_completion_moves_once_and_execute_cut_is_unmapped() {
    const ID: &str = "memory:///main.ts";
    const FUNCTION: &str = "function moved(): number { return 1; }";
    for newline in ["\n", "\r\n"] {
        for target in [EcmaTarget::Es5, EcmaTarget::Es2015, EcmaTarget::Es2022] {
            let source = [
                "export const before = 0;",
                FUNCTION,
                "export const after = 2;",
                "",
            ]
            .join(newline);
            let loader = MapLoader::from([ModuleSource::new(ID, source)]);
            let compilation = compile(
                ID,
                &loader,
                CompilerOptions {
                    module_kind: ModuleKind::System,
                    target,
                    source_map: true,
                    ..CompilerOptions::default()
                },
            );
            assert!(!compilation.has_errors(), "{:?}", compilation.diagnostics);
            let artifact = &compilation.output.as_ref().unwrap().artifacts[ID];
            let js = &artifact.javascript;
            let map = artifact.source_map.as_ref().unwrap();
            let lookup = |offset: usize| {
                let prefix = &js[..offset];
                map.original_position_at(
                    js,
                    prefix.bytes().filter(|byte| *byte == b'\n').count(),
                    prefix.rsplit('\n').next().unwrap().encode_utf16().count(),
                    100_000,
                    1_000_000,
                )
                .unwrap()
            };
            let function = js.find("function moved(").unwrap();
            let close = function + js[function..].find('}').unwrap() + 1;
            let endpoint = lookup(close).unwrap();
            assert_eq!(
                (endpoint.line, endpoint.column_utf16),
                (1, FUNCTION.encode_utf16().count())
            );
            let before = js.find("exports.before").unwrap();
            let end = before + js[before..].find(';').unwrap() + 1;
            let after = js[end..].find("exports.after").unwrap() + end;
            let mut blank = false;
            for offset in end..after {
                assert!(js.as_bytes()[offset].is_ascii_whitespace());
                let owner = lookup(offset);
                assert_ne!(
                    owner.map(|owner| owner.line),
                    Some(1),
                    "moved function endpoint leaked into execute gap"
                );
                blank |= owner.is_none();
            }
            assert!(blank);
            assert_eq!(lookup(after).unwrap().line, 2);
        }
    }
}

#[test]
fn adjacent_hoisted_functions_keep_a_valid_ordered_map() {
    const ID: &str = "memory:///main.ts";
    for source in [
        "function first(){}function second(){}export const result = 3;",
        "export const result = 3;function first(){}function second(){}",
    ] {
        let loader = MapLoader::from([ModuleSource::new(ID, source)]);
        let compilation = compile(
            ID,
            &loader,
            CompilerOptions {
                module_kind: ModuleKind::System,
                source_map: true,
                ..CompilerOptions::default()
            },
        );
        assert!(!compilation.has_errors(), "{:?}", compilation.diagnostics);
        let artifact = &compilation.output.as_ref().unwrap().artifacts[ID];
        let js = &artifact.javascript;
        let offset = js.rfind("exports.result").unwrap();
        let prefix = &js[..offset];
        let owner = artifact
            .source_map
            .as_ref()
            .unwrap()
            .original_position_at(
                js,
                prefix.bytes().filter(|byte| *byte == b'\n').count(),
                prefix.rsplit('\n').next().unwrap().encode_utf16().count(),
                100_000,
                1_000_000,
            )
            .unwrap()
            .expect("a following real statement keeps its start mapping");
        assert_eq!(
            (owner.line, owner.column_utf16),
            (0, source.find("export").unwrap())
        );
    }
}
