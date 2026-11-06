// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Moved public static stores keep Native field and initializer source roles.
use blueice_bluets::{compile, CompilerOptions, EcmaTarget, MapLoader, ModuleKind, ModuleSource};

fn check_public_static_store(instance_initializer: bool) {
    for prefix in ["", "/* G-E5 🧭 */ "] {
        let instance = if instance_initializer {
            "instance: number = 1; "
        } else {
            ""
        };
        let source = format!("{prefix}export class Box {{ {instance}static value: number = 40; }} export const result: number = Box.value;\n");
        let position = |text: &str, offset| {
            let before = &text[..offset];
            (
                before.bytes().filter(|byte| *byte == b'\n').count(),
                before.rsplit('\n').next().unwrap().encode_utf16().count(),
            )
        };
        let start = source.find("value: number").unwrap();
        let operand = source.find("40").unwrap();
        let end = operand + 3;
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
                let at = tokens.iter().position(|token| token.text == "40").unwrap();
                let lowered_define = define && target < EcmaTarget::Es2022;
                let begin = if lowered_define {
                    tokens[..at]
                        .iter()
                        .rposition(|token| token.is("Object"))
                        .unwrap()
                } else if define {
                    at - 2
                } else {
                    at - 4
                };
                let prefix_end = if lowered_define {
                    tokens[begin..at]
                        .iter()
                        .position(|token| token.is("{"))
                        .unwrap()
                        + begin
                        + 1
                } else {
                    at - 1
                };
                for token in &tokens[begin..prefix_end] {
                    for offset in token.start..token.end {
                        assert_eq!(
                            owner(offset),
                            Some(position(&source, start)),
                            "{target:?} define={define} static receiver/key {}",
                            token.text
                        );
                    }
                }
                if lowered_define {
                    for token in &tokens[prefix_end..at] {
                        for offset in token.start..token.end {
                            assert_eq!(
                                owner(offset),
                                None,
                                "generated descriptor data {}",
                                token.text
                            );
                        }
                    }
                } else {
                    let equals = &tokens[at - 1];
                    for offset in equals.start..equals.end {
                        assert_eq!(
                            owner(offset),
                            Some(position(&source, start + 5)),
                            "static assignment operator"
                        );
                    }
                }
                for offset in tokens[at].start..tokens[at].end {
                    assert_eq!(
                        owner(offset),
                        Some(position(&source, operand)),
                        "static initializer UTF-16 interior"
                    );
                }
                assert_eq!(
                    owner(tokens[at].end),
                    Some(position(
                        &source,
                        if instance_initializer && !define {
                            start + 5
                        } else {
                            operand + 2
                        }
                    )),
                    "static initializer completion"
                );
                let semicolon = at + if lowered_define { 3 } else { 1 };
                assert!(tokens[semicolon].is(";"));
                assert_eq!(
                    owner(tokens[semicolon].end),
                    Some(position(&source, end)),
                    "{target:?} define={define} complete static field endpoint"
                );
                if target == EcmaTarget::Es2022 && !define {
                    for token in &tokens[begin - 2..begin] {
                        for offset in token.start..token.end {
                            assert_eq!(
                                owner(offset),
                                None,
                                "generated static block prefix {}",
                                token.text
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn public_static_store_distinguishes_generated_transport_and_source_operands() {
    check_public_static_store(false);
}

#[test]
fn public_static_store_after_instance_initializers_retains_its_native_completion() {
    check_public_static_store(true);
}
