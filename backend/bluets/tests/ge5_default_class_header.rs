// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A retained default class keeps both original export modifier boundaries.
use blueice_bluets::{compile, CompilerOptions, EcmaTarget, MapLoader, ModuleKind, ModuleSource};

fn check(body: &str, default: bool, callable: bool) {
    for prefix in ["", "/* G-E5 🧭 */\r\n"] {
        let source = format!("{prefix}{body}");
        for target in [EcmaTarget::Es2015, EcmaTarget::Es2022, EcmaTarget::EsNext] {
            for module_kind in [ModuleKind::Esm, ModuleKind::CommonJs] {
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
                    if !maps || module_kind != ModuleKind::Esm {
                        continue;
                    }
                    let code = &artifact.javascript;
                    let map = artifact.source_map.as_ref().unwrap();
                    let tokens = blueice_bluets::lex("generated.js", code).unwrap();
                    let export = tokens.iter().position(|token| token.is("export")).unwrap();
                    let header = &tokens[export..export + if default { 3 } else { 2 }];
                    let line = usize::from(!prefix.is_empty());
                    let owners = if default { vec![0, 7, 14] } else { vec![0, 6] };
                    for (token, column) in header.iter().zip(owners) {
                        for offset in token.start..token.end {
                            let before = &code[..offset];
                            let generated_line =
                                before.bytes().filter(|byte| *byte == b'\n').count();
                            let generated_column =
                                before.rsplit('\n').next().unwrap().encode_utf16().count();
                            let owner = map
                                .original_position_at(
                                    code,
                                    generated_line,
                                    generated_column,
                                    100_000,
                                    32_000_000,
                                )
                                .unwrap()
                                .map(|owner| (owner.line, owner.column_utf16));
                            assert_eq!(
                                owner,
                                Some((line, column)),
                                "{target:?} {callable} {} complete token",
                                token.text
                            );
                        }
                    }
                    for (index, token) in header.iter().enumerate().take(header.len() - 1) {
                        let expected = if index == 0 { 6 } else { 14 };
                        for offset in token.end..header[index + 1].start {
                            let before = &code[..offset];
                            let generated_line =
                                before.bytes().filter(|byte| *byte == b'\n').count();
                            let generated_column =
                                before.rsplit('\n').next().unwrap().encode_utf16().count();
                            let owner = map
                                .original_position_at(
                                    code,
                                    generated_line,
                                    generated_column,
                                    100_000,
                                    32_000_000,
                                )
                                .unwrap()
                                .map(|owner| (owner.line, owner.column_utf16));
                            assert_eq!(
                                owner,
                                Some((line, expected)),
                                "{target:?} {callable} modifier boundary"
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn default_class_header_retains_export_and_default_endpoints() {
    check(
        "export default class Default { value:number=2; }\n",
        true,
        false,
    );
}

#[test]
fn ordinary_export_class_and_function_keep_their_header_roles() {
    check("export class Model { value:number=1; }\n", false, false);
    check("export function ordinary(){return 2;}\n", false, true);
}
