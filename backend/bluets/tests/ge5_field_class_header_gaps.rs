// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A generated field-only class cannot revive its erased original body entry.
use blueice_bluets::{
    compile, CompilerOptions, Declaration, EcmaTarget, MapLoader, ModuleKind, ModuleSource,
};

#[test]
fn generated_field_class_body_gap_does_not_revive_the_original_body_entry() {
    for prefix in ["", "/* G-E5 🧭 */ "] {
        let source=format!("{prefix}function dec(value:any,context:any):any {{}} export class Box {{ @dec first:number=1; @dec second:number=2; }} export const result:number=new Box().first;\n");
        let parsed = blueice_bluets::parse_module("memory:///main.ts", &source).unwrap();
        let original = parsed
            .declarations
            .iter()
            .find_map(|d| match d {
                Declaration::Class(c) => Some(c),
                _ => None,
            })
            .unwrap();
        let position = |text: &str, offset| {
            let before = &text[..offset];
            (
                before.bytes().filter(|byte| *byte == b'\n').count(),
                before.rsplit('\n').next().unwrap().encode_utf16().count(),
            )
        };
        let erased_entry = position(&source, original.body_span.start + 1);
        for target in [EcmaTarget::Es2015, EcmaTarget::Es2022] {
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
                let class = tokens
                    .windows(2)
                    .position(|t| t[0].is("class") && t[1].is("Box"))
                    .unwrap();
                let open = class + tokens[class..].iter().position(|t| t.is("{")).unwrap();
                let boundary = if target == EcmaTarget::Es2022 {
                    let block = open
                        + 1
                        + tokens[open + 1..]
                            .iter()
                            .position(|t| t.is("static"))
                            .unwrap();
                    let mut depth = 0;
                    let mut close = block + 1;
                    for (index, token) in tokens.iter().enumerate().skip(block + 1) {
                        if token.is("{") {
                            depth += 1;
                        } else if token.is("}") {
                            depth -= 1;
                            if depth == 0 {
                                close = index;
                                break;
                            }
                        }
                    }
                    close
                } else {
                    open
                };
                let first = &tokens[boundary + 1];
                assert!(
                    tokens[boundary].end < first.start,
                    "generated class boundary has physical trivia"
                );
                for offset in tokens[boundary].end..first.start {
                    assert_ne!(owner(offset),Some(erased_entry),"{target:?} define={define} generated body trivia cannot claim an erased original class-entry breakpoint");
                    if target < EcmaTarget::Es2022 || !define {
                        if let Some(point) = owner(offset) {
                            assert_eq!(point, position(&source, original.name_span.end), "{target:?} define={define} generated body trivia only retains the Native header completion before entering the generated region");
                        }
                    }
                }
            }
        }
    }
}
