// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Nested private RHS edits retain module-relative operands and receiver roles.
#[path = "oracle_support/gd2_maps.rs"]
mod decoded;
use blueice_bluets::{compile, CompilerOptions, EcmaTarget, MapLoader, ModuleKind, ModuleSource};
use std::process::Command;

fn position(text: &str, offset: usize) -> (usize, usize) {
    let before = &text[..offset];
    (
        before.bytes().filter(|byte| *byte == b'\n').count(),
        before.rsplit('\n').next().unwrap().encode_utf16().count(),
    )
}

#[test]
fn private_assignments_keep_nested_reads_and_calls_when_rendering_fragments() {
    let body = "class Counter { #text:string='x'; #count:number=1; #step(n:number):number{return n+2;} #empty():number{return 3;} run():string{this.#text='z  '+this.#text;this.#count=this.#step(this.#empty());this.#count+=this.#step(2);this.#count*=this.#empty();this.#text=this.#text+this.#count;return this.#text;} } export const result=new Counter().run();\n";
    for prefix in ["", "/* 🧭 */\r\n"] {
        let source = format!("{prefix}{body}");
        for target in [EcmaTarget::Es2015, EcmaTarget::Es2020, EcmaTarget::Es2022] {
            for define in [false, true] {
                for source_map in [false, true] {
                    for module_kind in [ModuleKind::CommonJs, ModuleKind::Esm] {
                        let id = "memory:///main.ts";
                        let result = compile(
                            id,
                            &MapLoader::from([ModuleSource::new(id, &source)]),
                            CompilerOptions {
                                target,
                                module_kind,
                                use_define_for_class_fields: Some(define),
                                source_map,
                                inline_sources: source_map,
                                ..CompilerOptions::default()
                            },
                        );
                        assert!(!result.has_errors(), "{:?}", result.diagnostics);
                        let artifact = &result.output.unwrap().artifacts[id];
                        assert_eq!(artifact.source_map.is_some(), source_map);
                        let code = &artifact.javascript;
                        assert!(!code.contains('\0'));
                        if let Some(map) = &artifact.source_map {
                            let data = decoded::decoded(&map.to_json());
                            assert_eq!(
                                data["metadata"]["sourcesContent"],
                                serde_json::json!([source])
                            );
                            let widths = |text: &str| {
                                text.split('\n')
                                    .map(|line| line.encode_utf16().count())
                                    .collect::<Vec<_>>()
                            };
                            let generated = widths(code);
                            let original = widths(&source);
                            for segment in data["segments"].as_array().unwrap() {
                                let item = segment.as_array().unwrap();
                                let line = item[0].as_u64().unwrap() as usize;
                                let column = item[1].as_u64().unwrap() as usize;
                                assert!(line < generated.len() && column <= generated[line]);
                                if item.len() > 2 {
                                    assert_eq!(item[2], 0);
                                    let line = item[3].as_u64().unwrap() as usize;
                                    let column = item[4].as_u64().unwrap() as usize;
                                    assert!(line < original.len() && column <= original[line]);
                                }
                            }
                            let owner = |offset| {
                                let (line, column) = position(code, offset);
                                map.original_position_at(code, line, column, 100_000, 32_000_000)
                                    .unwrap()
                                    .map(|point| {
                                        (point.source_index, point.line, point.column_utf16)
                                    })
                            };
                            let tokens = blueice_bluets::lex("emitted.js", code).unwrap();
                            let literal =
                                tokens.iter().find(|token| token.text == "'z  '").unwrap();
                            let point = position(&source, source.find("'z  '").unwrap());
                            assert_eq!(
                                owner(literal.start),
                                Some((0, point.0, point.1)),
                                "copied RHS literal keeps its original start"
                            );
                            for (label, start) in [
                                (
                                    "private read",
                                    source.find("'z  '+this.#text").unwrap() + "'z  '+".len(),
                                ),
                                (
                                    "outer private call",
                                    source.find("this.#step(this.#empty())").unwrap(),
                                ),
                                ("inner private call", source.find("this.#empty())").unwrap()),
                            ] {
                                let point = position(&source, start);
                                let expected = Some((0, point.0, point.1));
                                let receivers = tokens
                                    .iter()
                                    .filter(|token| {
                                        token.is("this") && owner(token.start) == expected
                                    })
                                    .collect::<Vec<_>>();
                                assert!(!receivers.is_empty(), "{label}: exact original receiver survives {target:?}/{define}/{module_kind:?}");
                                for receiver in receivers {
                                    for offset in receiver.start..receiver.end {
                                        assert_eq!(
                                            owner(offset),
                                            expected,
                                            "{label}: entire receiver role"
                                        );
                                    }
                                    let end = position(&source, start + "this".len());
                                    assert_eq!(
                                        owner(receiver.end),
                                        Some((0, end.0, end.1)),
                                        "{label}: exact receiver end"
                                    );
                                }
                            }
                        }
                        let result = if module_kind == ModuleKind::CommonJs {
                            "exports.result"
                        } else {
                            "result"
                        };
                        let output = Command::new("node")
                            .arg("--input-type")
                            .arg(if module_kind == ModuleKind::CommonJs {
                                "commonjs"
                            } else {
                                "module"
                            })
                            .arg("-e")
                            .arg(format!("{code}\nprocess.stdout.write({result});"))
                            .output()
                            .unwrap();
                        assert!(
                            output.status.success(),
                            "{}",
                            String::from_utf8_lossy(&output.stderr)
                        );
                        assert_eq!(
                            output.stdout, b"z  x27",
                            "{target:?}/{define}/{source_map}/{module_kind:?}"
                        );
                    }
                }
            }
        }
    }
}
