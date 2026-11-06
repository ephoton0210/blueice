// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Retained bindings and ES5 method values preserve separate Native logical ends.
use blueice_bluets::{compile, CompilerOptions, EcmaTarget, MapLoader, ModuleKind, ModuleSource};

fn position(source: &str, offset: usize) -> (usize, usize) {
    let before = &source[..offset];
    (
        before.bytes().filter(|byte| *byte == b'\n').count(),
        before.rsplit('\n').next().unwrap().encode_utf16().count(),
    )
}

fn inspect(targets: &[EcmaTarget], method: bool) {
    for prefix in ["", "/* G-E5 🧭 */\r\n"] {
        let body="function dec(...args:any[]):any{}\r\n@dec export class C { @dec property!:number; @dec method(value:number):number{return value;} constructor(@dec value:number){this.property=value;} }\r\nexport const result=C;\r\n";
        let source = format!("{prefix}{body}");
        for &target in targets {
            for module_kind in [ModuleKind::CommonJs, ModuleKind::Esm] {
                for define in [false, true] {
                    let id = "memory:///main.ts";
                    let result = compile(
                        id,
                        &MapLoader::from([ModuleSource::new(id, &source)]),
                        CompilerOptions {
                            target,
                            module_kind,
                            experimental_decorators: true,
                            emit_decorator_metadata: true,
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
                    let tokens = blueice_bluets::lex("generated.js", code).unwrap();
                    let start = if method && !define {
                        tokens
                            .windows(7)
                            .position(|part| {
                                part.iter().map(|token| token.text.as_str()).eq([
                                    "C",
                                    ".",
                                    "prototype",
                                    ".",
                                    "method",
                                    "=",
                                    "function",
                                ])
                            })
                            .unwrap()
                            + 6
                    } else if method {
                        tokens
                            .windows(3)
                            .position(|part| {
                                part[0].is("value") && part[1].is(":") && part[2].is("function")
                            })
                            .unwrap()
                            + 2
                    } else {
                        tokens
                            .windows(3)
                            .position(|part| {
                                part[0].is("C") && part[1].is("=") && part[2].is("class")
                            })
                            .unwrap()
                            + 2
                    };
                    let open = start
                        + tokens[start..]
                            .iter()
                            .position(|token| token.is("{"))
                            .unwrap();
                    let mut depth = 0;
                    let mut end = None;
                    for (index, token) in tokens.iter().enumerate().skip(open) {
                        if token.is("{") {
                            depth += 1;
                        } else if token.is("}") {
                            depth -= 1;
                            if depth == 0 {
                                end = Some(if method {
                                    token.end
                                } else {
                                    assert!(tokens[index + 1].is(";"));
                                    tokens[index + 1].end
                                });
                                break;
                            }
                        }
                    }
                    let offset = end.unwrap();
                    let (line, column) = position(code, offset);
                    let owner = artifact
                        .source_map
                        .as_ref()
                        .unwrap()
                        .original_position_at(code, line, column, 100_000, 32_000_000)
                        .unwrap()
                        .map(|value| (value.line, value.column_utf16));
                    let expected = if method {
                        let value = "method(value:number):number{return value;}";
                        source.find(value).unwrap() + value.len()
                    } else {
                        source.find(" }\r\nexport").unwrap() + 2
                    };
                    assert_eq!(owner,Some(position(&source,expected)),"{target:?} {module_kind:?} define={define} method={method} complete logical endpoint");
                }
            }
        }
    }
}

#[test]
fn retained_class_binding_finishes_after_its_synthetic_semicolon() {
    inspect(
        &[EcmaTarget::Es2015, EcmaTarget::Es2022, EcmaTarget::EsNext],
        false,
    );
}

#[test]
fn es5_method_body_finishes_before_generated_assignment_or_descriptor_transport() {
    inspect(&[EcmaTarget::Es5], true);
}
