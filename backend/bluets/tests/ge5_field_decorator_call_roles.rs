// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A generated field-decoration call inherits the complete original member header.
use blueice_bluets::{compile, CompilerOptions, EcmaTarget, MapLoader, ModuleKind, ModuleSource};

#[test]
fn static_field_decorator_calls_keep_header_origin_and_field_completion() {
    for prefix in ["", "/* G-E5 🧭 */ "] {
        let source=format!("{prefix}function dec(value:any,context:any):any {{}} export class Box {{ @dec static value:number=40; }} export const result:number=Box.value;\n");
        let start = source.find("static value").unwrap();
        let end = source.find("40;").unwrap() + 3;
        let position = |text: &str, offset| {
            let before = &text[..offset];
            (
                before.bytes().filter(|byte| *byte == b'\n').count(),
                before.rsplit('\n').next().unwrap().encode_utf16().count(),
            )
        };
        for target in [EcmaTarget::Es2022, EcmaTarget::Es2015, EcmaTarget::Es5] {
            for define in [false, true] {
                let id = "memory:///main.ts";
                let result = compile(
                    id,
                    &MapLoader::from([ModuleSource::new(id, &source)]),
                    CompilerOptions {
                        target,
                        module_kind: ModuleKind::CommonJs,
                        source_map: true,
                        inline_sources: true,
                        use_define_for_class_fields: Some(define),
                        libraries: Some(vec![EcmaTarget::Es2022]),
                        ..CompilerOptions::default()
                    },
                );
                assert!(!result.has_errors(), "{:?}", result.diagnostics);
                let artifact = &result.output.unwrap().artifacts[id];
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
                    .windows(2)
                    .position(|tokens| tokens[0].is("__bluetsEsDecorate") && tokens[1].is("("))
                    .unwrap();
                let mut depth = 0;
                let mut close = at + 1;
                for (index, token) in tokens.iter().enumerate().skip(at + 1) {
                    if token.is("(") {
                        depth += 1;
                    } else if token.is(")") {
                        depth -= 1;
                        if depth == 0 {
                            close = index;
                            break;
                        }
                    }
                }
                let accesses = tokens[at..=close]
                    .windows(3)
                    .filter(|tokens| {
                        tokens[0].is("obj") && tokens[1].is(".") && tokens[2].is("value")
                    })
                    .collect::<Vec<_>>();
                assert_eq!(
                    accesses.len(),
                    2,
                    "one getter and one setter original identifier use"
                );
                let name_start = source.find("value:number").unwrap();
                let name_end = name_start + 5;
                for offset in tokens[at].start..tokens[close].end {
                    let expected = if offset < accesses[0][2].end {
                        start
                    } else if offset < accesses[1][2].start {
                        name_end
                    } else if offset < accesses[1][2].end {
                        name_start
                    } else {
                        name_end
                    };
                    assert_eq!(owner(offset),Some(position(&source,expected)),"{target:?} define={define} complete generated field decoration source role at {offset}");
                }
                assert!(tokens[close + 1].is(";"));
                assert_eq!(
                    owner(tokens[close + 1].end),
                    Some(position(&source, end)),
                    "complete original field endpoint after generated decoration"
                );
            }
        }
    }
}
