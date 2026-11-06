// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Default class lowering separates source completion from generated publication.
use blueice_bluets::{compile, CompilerOptions, EcmaTarget, MapLoader, ModuleKind, ModuleSource};

#[test]
fn default_es5_publications_are_generated_and_retained_publications_keep_the_class_origin() {
    for prefix in ["", "/* G-E5 🧭 */\r\n"] {
        let body = "export default class Default { value:number=2; }\n";
        let source = format!("{prefix}{body}");
        for target in [EcmaTarget::Es5, EcmaTarget::Es2015, EcmaTarget::Es2022] {
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
                    if !maps {
                        continue;
                    }
                    let code = &artifact.javascript;
                    let tokens = blueice_bluets::lex("generated.js", code).unwrap();
                    let start = if module_kind == ModuleKind::CommonJs {
                        tokens
                            .windows(3)
                            .rfind(|part| {
                                part[0].is("exports") && part[1].is(".") && part[2].is("default")
                            })
                            .unwrap()[0]
                            .start
                    } else if target == EcmaTarget::Es5 {
                        tokens
                            .windows(2)
                            .rfind(|part| part[0].is("export") && part[1].is("default"))
                            .unwrap()[0]
                            .start
                    } else {
                        continue;
                    };
                    let publication = tokens
                        .iter()
                        .filter(|token| token.start >= start)
                        .collect::<Vec<_>>();
                    let end = publication.iter().find(|token| token.is(";")).unwrap().end;
                    let map = artifact.source_map.as_ref().unwrap();
                    for offset in start..=end {
                        let before = &code[..offset];
                        let line = before.bytes().filter(|byte| *byte == b'\n').count();
                        let column = before.rsplit('\n').next().unwrap().encode_utf16().count();
                        let owner = map
                            .original_position_at(code, line, column, 100_000, 32_000_000)
                            .unwrap()
                            .map(|owner| (owner.line, owner.column_utf16));
                        let expected = (target != EcmaTarget::Es5).then_some((
                            usize::from(!prefix.is_empty()),
                            if offset == end {
                                body.trim_end().encode_utf16().count()
                            } else {
                                0
                            },
                        ));
                        assert_eq!(
                            owner, expected,
                            "{target:?} {module_kind:?} complete publication at {offset}"
                        );
                    }
                }
            }
        }
    }
}
