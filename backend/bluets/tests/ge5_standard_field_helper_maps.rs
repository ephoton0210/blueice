// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Native source roles at decorator field initializer helper boundaries.
use blueice_bluets::{compile, CompilerOptions, EcmaTarget, MapLoader, ModuleKind, ModuleSource};

fn check_field(static_modifier: &str) {
    let body = "function dec(value: any, context: any): any { if (context.kind === 'field') return function(initial: any): any { return initial + 1; }; } export class Box { @dec STATIC value: number = 40; } export const result: number = new Box().value;\n";
    let body = body.replace("STATIC ", static_modifier);
    let body = if static_modifier.is_empty() {
        body
    } else {
        body.replace("new Box().value", "Box.value")
    };
    for prefix in ["", "/* G-E5 🧭 */ "] {
        let source = format!("{prefix}{body}");
        let name_end = source.find("value: number").unwrap() + "value".len();
        let operand = source.find("40").unwrap();
        let position = |text: &str, offset| {
            let before = &text[..offset];
            (
                before.bytes().filter(|byte| *byte == b'\n').count(),
                before.rsplit('\n').next().unwrap().encode_utf16().count(),
            )
        };
        for target in [EcmaTarget::Es5, EcmaTarget::Es2015, EcmaTarget::Es2022] {
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
                let index = tokens.iter().position(|token| token.text == "40").unwrap();
                let helper = tokens[..index]
                    .iter()
                    .rfind(|token| token.text.ends_with("RunInitializers"))
                    .unwrap();
                for offset in helper.start..helper.end {
                    assert_eq!(
                        owner(offset),
                        if define && target < EcmaTarget::Es2022 {
                            None
                        } else {
                            Some(position(&source, name_end))
                        },
                        "{target:?} define={define} helper prefix"
                    );
                }
                for offset in tokens[index].start..tokens[index].end {
                    assert_eq!(
                        owner(offset),
                        Some(position(&source, operand)),
                        "{target:?} operand interior"
                    );
                }
                assert_eq!(
                    owner(tokens[index].end),
                    Some(position(&source, operand + 2)),
                    "{target:?} helper operand completion"
                );
            }
        }
    }
}

#[test]
fn field_helpers_inherit_name_end_and_retain_operand_completion() {
    check_field("");
}

#[test]
fn static_field_helpers_keep_their_complete_initializer_origins() {
    check_field("static ");
}
