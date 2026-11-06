// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Original import binding clauses own alias keywords without changing assertion syntax.
use blueice_bluets::{compile, CompilerOptions, EcmaTarget, MapLoader, ModuleKind, ModuleSource};
#[test]
fn retained_named_and_namespace_import_aliases_keep_their_original_binding_phase() {
    for prefix in ["", "/* G-E5 🧭 */ "] {
        for (clause, expression, namespace) in [
            ("{ Model as Local }", "Local", false),
            ("* as NS", "NS.Model", true),
        ] {
            let source=format!("{prefix}import {clause} from './types.ts'; export const value={expression} as any;\r\n");
            let id = "memory:///main.ts";
            for target in [
                EcmaTarget::Es5,
                EcmaTarget::Es2015,
                EcmaTarget::Es2022,
                EcmaTarget::EsNext,
            ] {
                let result = compile(
                    id,
                    &MapLoader::from([
                        ModuleSource::new(id, &source),
                        ModuleSource::new("memory:///types.ts", "export class Model{}\n"),
                    ]),
                    CompilerOptions {
                        target,
                        module_kind: ModuleKind::Esm,
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
                let keyword = tokens.iter().position(|token| token.is("as")).unwrap();
                let origin = if namespace {
                    source.find("* as NS").unwrap()
                } else {
                    source.find("Model as").unwrap() + "Model".len()
                };
                let expected = source[..origin].encode_utf16().count();
                for token in &tokens[if namespace { keyword - 1 } else { keyword }..=keyword] {
                    for offset in token.start..token.end {
                        let before = &code[..offset];
                        let line = before.bytes().filter(|byte| *byte == b'\n').count();
                        let column = before.rsplit('\n').next().unwrap().encode_utf16().count();
                        let owner = map
                            .original_position_at(code, line, column, 100_000, 32_000_000)
                            .unwrap()
                            .map(|owner| (owner.line, owner.column_utf16));
                        assert_eq!(
                            owner,
                            Some((0, expected)),
                            "{target:?} complete binding token {}",
                            token.text
                        );
                    }
                }
                for offset in tokens[keyword].end..tokens[keyword + 1].start {
                    let before = &code[..offset];
                    let line = before.bytes().filter(|byte| *byte == b'\n').count();
                    let column = before.rsplit('\n').next().unwrap().encode_utf16().count();
                    let owner = map
                        .original_position_at(code, line, column, 100_000, 32_000_000)
                        .unwrap()
                        .map(|owner| (owner.line, owner.column_utf16));
                    assert_eq!(owner, Some((0, expected)), "complete alias-to-local gap");
                }
                assert_eq!(
                    tokens.iter().filter(|token| token.is("as")).count(),
                    1,
                    "source assertion still erases independently of its import alias"
                );
            }
        }
    }
}
