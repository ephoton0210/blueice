// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Field relocation retains the effective constructor signature's entry phase.
use blueice_bluets::{compile, CompilerOptions, EcmaTarget, MapLoader, ModuleKind, ModuleSource};

fn position(source: &str, offset: usize) -> (usize, usize) {
    let before = &source[..offset];
    (
        before.bytes().filter(|byte| *byte == b'\n').count(),
        before.rsplit('\n').next().unwrap().encode_utf16().count(),
    )
}

#[test]
fn empty_ordinary_and_rest_constructors_keep_their_native_field_entry_phase() {
    for prefix in ["", "/* G-E5 🧭 */\r\n"] {
        for parameters in ["", "value:number", "...args:any[]"] {
            let source = format!(
                "{prefix}export class C {{ field:number=1; constructor({parameters}){{}} }}\r\n"
            );
            for target in [EcmaTarget::Es2015, EcmaTarget::Es5, EcmaTarget::Es2022] {
                for module_kind in [ModuleKind::CommonJs, ModuleKind::Esm] {
                    for define in [false, true] {
                        let id = "memory:///main.ts";
                        let result = compile(
                            id,
                            &MapLoader::from([ModuleSource::new(id, &source)]),
                            CompilerOptions {
                                target,
                                module_kind,
                                use_define_for_class_fields: Some(define),
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
                        let constructor = if target == EcmaTarget::Es5 {
                            tokens
                                .windows(2)
                                .position(|part| part[0].is("function") && part[1].is("C"))
                                .unwrap()
                        } else {
                            tokens
                                .iter()
                                .position(|token| token.is("constructor"))
                                .unwrap()
                        };
                        let open = constructor
                            + tokens[constructor..]
                                .iter()
                                .position(|token| token.is("{"))
                                .unwrap();
                        if tokens[open].end == tokens[open + 1].start {
                            continue;
                        }
                        let offset = tokens[open].end;
                        let (line, column) = position(code, offset);
                        let owner = map
                            .original_position_at(code, line, column, 100_000, 32_000_000)
                            .unwrap()
                            .map(|value| (value.line, value.column_utf16));
                        let keyword = source.find("constructor").unwrap();
                        let expected = if parameters.is_empty()
                            || target == EcmaTarget::Es5 && parameters.starts_with("...")
                        {
                            keyword
                        } else {
                            keyword + source[keyword..].find(')').unwrap()
                        };
                        assert_eq!(owner,Some(position(&source,expected)),"{target:?} {module_kind:?} define={define} parameters={parameters:?} exact constructor entry");
                    }
                }
            }
        }
    }
}

#[test]
fn an_empty_constructor_preserves_its_original_accessibility_header_phase() {
    for prefix in ["", "/* G-E5 🧭 */\r\n"] {
        let source =
            format!("{prefix}export class C {{ field:number=1; public constructor(){{}} }}\r\n");
        for target in [EcmaTarget::Es2015, EcmaTarget::Es2022] {
            for module_kind in [ModuleKind::CommonJs, ModuleKind::Esm] {
                let id = "memory:///main.ts";
                let result = compile(
                    id,
                    &MapLoader::from([ModuleSource::new(id, &source)]),
                    CompilerOptions {
                        target,
                        module_kind,
                        use_define_for_class_fields: Some(false),
                        source_map: true,
                        inline_sources: true,
                        libraries: Some(vec![EcmaTarget::Es2022]),
                        ..CompilerOptions::default()
                    },
                );
                assert!(!result.has_errors(), "{:?}", result.diagnostics);
                let artifact = &result.output.unwrap().artifacts[id];
                let code = &artifact.javascript;
                let tokens = blueice_bluets::lex("generated.js", code).unwrap();
                let constructor = tokens
                    .iter()
                    .position(|token| token.is("constructor"))
                    .unwrap();
                let open = constructor
                    + tokens[constructor..]
                        .iter()
                        .position(|token| token.is("{"))
                        .unwrap();
                let (line, column) = position(code, tokens[open].end);
                let owner = artifact
                    .source_map
                    .as_ref()
                    .unwrap()
                    .original_position_at(code, line, column, 100_000, 32_000_000)
                    .unwrap()
                    .map(|value| (value.line, value.column_utf16));
                assert_eq!(
                    owner,
                    Some(position(
                        &source,
                        source.find("public constructor").unwrap()
                    )),
                    "{target:?} {module_kind:?} original constructor header"
                );
            }
        }
    }
}
