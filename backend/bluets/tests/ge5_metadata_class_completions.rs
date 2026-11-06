// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A decorator replacement completes before its independent export publication.
use blueice_bluets::{compile, CompilerOptions, EcmaTarget, MapLoader, ModuleKind, ModuleSource};

fn position(source: &str, offset: usize) -> (usize, usize) {
    let before = &source[..offset];
    (
        before.bytes().filter(|byte| *byte == b'\n').count(),
        before.rsplit('\n').next().unwrap().encode_utf16().count(),
    )
}

#[test]
fn decorated_replacement_and_esm_publication_have_distinct_native_endpoints() {
    for prefix in ["", "/* G-E5 🧭 */\r\n"] {
        let body="function dec(...args:any[]):any{}\r\n@dec export class C { @dec property!:number; constructor(@dec value:number){this.property=value;} }\r\nexport const result=C;\r\n";
        let source = format!("{prefix}{body}");
        let expected = Some(position(&source, source.find(" }\r\nexport").unwrap() + 2));
        for target in [
            EcmaTarget::Es5,
            EcmaTarget::Es2015,
            EcmaTarget::Es2022,
            EcmaTarget::EsNext,
        ] {
            for module_kind in [ModuleKind::CommonJs, ModuleKind::Esm] {
                for define in [false, true] {
                    let id = "memory:///main.ts";
                    let mut outputs = Vec::new();
                    for maps in [false, true] {
                        let result = compile(
                            id,
                            &MapLoader::from([ModuleSource::new(id, &source)]),
                            CompilerOptions {
                                target,
                                module_kind,
                                experimental_decorators: true,
                                emit_decorator_metadata: true,
                                use_define_for_class_fields: Some(define),
                                source_map: maps,
                                inline_sources: true,
                                libraries: Some(vec![EcmaTarget::Es2022]),
                                ..CompilerOptions::default()
                            },
                        );
                        assert!(!result.has_errors(), "{:?}", result.diagnostics);
                        let artifact = &result.output.unwrap().artifacts[id];
                        let code = &artifact.javascript;
                        outputs.push(
                            code.lines()
                                .filter(|line| !line.starts_with("//# sourceMappingURL="))
                                .collect::<Vec<_>>()
                                .join("\n"),
                        );
                        if !maps {
                            continue;
                        }
                        let map = artifact.source_map.as_ref().unwrap();
                        let owner = |offset| {
                            let (line, column) = position(code, offset);
                            map.original_position_at(code, line, column, 100_000, 32_000_000)
                                .unwrap()
                                .map(|value| (value.line, value.column_utf16))
                        };
                        let tokens = blueice_bluets::lex("generated.js", code).unwrap();
                        let start = tokens
                            .windows(3)
                            .rposition(|part| {
                                part[0].is("C") && part[1].is("=") && part[2].is("__bluetsDecorate")
                            })
                            .unwrap();
                        let end = tokens[start..]
                            .iter()
                            .find(|token| token.is(";"))
                            .unwrap()
                            .end;
                        assert_eq!(
                            owner(end),
                            expected,
                            "{target:?} {module_kind:?} define={define} replacement statement.end"
                        );
                        if target == EcmaTarget::Es5 && module_kind == ModuleKind::Esm {
                            let publication = tokens
                                .windows(2)
                                .rfind(|part| part[0].is("export") && part[1].is("{"))
                                .unwrap();
                            for offset in publication[0].start..publication[1].end {
                                assert_eq!(
                                    owner(offset),
                                    None,
                                    "generated ES5 export prefix at {offset}"
                                );
                            }
                        }
                    }
                    assert_eq!(
                        outputs[0], outputs[1],
                        "source maps preserve executable output"
                    );
                }
            }
        }
    }
}
