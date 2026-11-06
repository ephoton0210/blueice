// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Class construction completes before its independent generated publication phase.
use blueice_bluets::{compile, CompilerOptions, EcmaTarget, MapLoader, ModuleKind, ModuleSource};
#[test]
fn exported_class_construction_retains_its_exact_original_logical_endpoint() {
    for prefix in ["", "/* G-E5 🧭 */\r\n"] {
        for default in [false, true] {
            let body = format!(
                "export {}class Model {{ value:number=1; }}\r\n",
                if default { "default " } else { "" }
            );
            let source = format!("{prefix}{body}");
            for target in [
                EcmaTarget::Es5,
                EcmaTarget::Es2015,
                EcmaTarget::Es2022,
                EcmaTarget::EsNext,
            ] {
                for module_kind in [ModuleKind::CommonJs, ModuleKind::Esm] {
                    let id = "memory:///main.ts";
                    let result = compile(
                        id,
                        &MapLoader::from([ModuleSource::new(id, &source)]),
                        CompilerOptions {
                            target,
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
                    let before = if module_kind == ModuleKind::CommonJs {
                        tokens
                            .windows(3)
                            .rposition(|part| {
                                part[0].is("exports")
                                    && part[1].is(".")
                                    && part[2].is(if default { "default" } else { "Model" })
                            })
                            .unwrap()
                    } else if target == EcmaTarget::Es5 {
                        tokens.iter().rposition(|token| token.is("export")).unwrap()
                    } else {
                        tokens.len()
                    };
                    let end = tokens[..before]
                        .iter()
                        .rfind(|token| token.is(if target == EcmaTarget::Es5 { ";" } else { "}" }))
                        .unwrap()
                        .end;
                    let before = &code[..end];
                    let line = before.bytes().filter(|byte| *byte == b'\n').count();
                    let column = before.rsplit('\n').next().unwrap().encode_utf16().count();
                    let owner = map
                        .original_position_at(code, line, column, 100_000, 32_000_000)
                        .unwrap()
                        .map(|owner| (owner.line, owner.column_utf16));
                    assert_eq!(owner,Some((usize::from(!prefix.is_empty()),body.trim_end().encode_utf16().count())),"{target:?} {module_kind:?} default={default} complete class construction endpoint");
                }
            }
        }
    }
}

#[test]
fn retained_static_initializer_has_a_distinct_generated_class_syntax_endpoint() {
    for prefix in ["", "/* G-E5 🧭 */\r\n"] {
        for exported in [false, true] {
            for (body, generated) in [
                ("static value:number=1;", true),
                ("static value:number;", false),
                ("static method(){return 1;}", false),
                ("static {}", false),
                ("value:number=1;", false),
            ] {
                let declaration = format!(
                    "{}class Base {{ {body} }}",
                    if exported { "export " } else { "" }
                );
                let source = format!("{prefix}{declaration}\r\nexport const result=Base;\r\n");
                for target in [EcmaTarget::Es2015, EcmaTarget::Es2022, EcmaTarget::EsNext] {
                    for module_kind in [ModuleKind::CommonJs, ModuleKind::Esm] {
                        let id = "memory:///main.ts";
                        let result = compile(
                            id,
                            &MapLoader::from([ModuleSource::new(id, &source)]),
                            CompilerOptions {
                                target,
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
                        let start = tokens.iter().position(|token| token.is("class")).unwrap();
                        let open = start
                            + tokens[start..]
                                .iter()
                                .position(|token| token.is("{"))
                                .unwrap();
                        let mut depth = 0;
                        let mut end = None;
                        for token in &tokens[open..] {
                            if token.is("{") {
                                depth += 1;
                            } else if token.is("}") {
                                depth -= 1;
                                if depth == 0 {
                                    end = Some(token.end);
                                    break;
                                }
                            }
                        }
                        let before = &code[..end.unwrap()];
                        let line = before.bytes().filter(|byte| *byte == b'\n').count();
                        let column = before.rsplit('\n').next().unwrap().encode_utf16().count();
                        let owner = map
                            .original_position_at(code, line, column, 100_000, 32_000_000)
                            .unwrap()
                            .map(|owner| (owner.line, owner.column_utf16));
                        let expected = (!generated).then_some((
                            usize::from(!prefix.is_empty()),
                            declaration.encode_utf16().count(),
                        ));
                        assert_eq!(owner, expected, "{target:?} {module_kind:?} exported={exported} {body} exact static-initialization phase");
                    }
                }
            }
        }
    }
}
