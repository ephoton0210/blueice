// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Reparsed decorator classes retain their original class identity and distinct phases.
use blueice_bluets::{compile, CompilerOptions, EcmaTarget, MapLoader, ModuleKind, ModuleSource};
use std::process::Command;

#[test]
fn legacy_reparsed_member_phases_complete_before_the_es5_outer_binding() {
    for prefix in ["", "/* G-E5 🧭 */ "] {
        let source=format!("{prefix}const events:any[]=[]; function dec(target:any,key:any):void {{events.push([typeof Box,target.constructor.x]);}} function replacement(value:any):any {{return class extends value {{static replacement=true;}};}} export class Box {{@dec value:number=2; static x:number=1;}} export const result:any=events;\n");
        for target in [EcmaTarget::Es5, EcmaTarget::Es2015, EcmaTarget::Es2022] {
            for source_map in [false, true] {
                let id = "memory:///main.ts";
                let result = compile(
                    id,
                    &MapLoader::from([ModuleSource::new(id, &source)]),
                    CompilerOptions {
                        target,
                        module_kind: ModuleKind::CommonJs,
                        source_map,
                        inline_sources: source_map,
                        experimental_decorators: true,
                        use_define_for_class_fields: Some(false),
                        libraries: Some(vec![EcmaTarget::Es2022]),
                        ..CompilerOptions::default()
                    },
                );
                assert!(!result.has_errors(), "{:?}", result.diagnostics);
                let code = &result.output.unwrap().artifacts[id].javascript;
                let execution = Command::new("node")
                    .args([
                        "-e",
                        &format!("{code}\nprocess.stdout.write(JSON.stringify(exports.result));"),
                    ])
                    .output()
                    .unwrap();
                assert!(
                    execution.status.success(),
                    "{}",
                    String::from_utf8_lossy(&execution.stderr)
                );
                let expected = if target == EcmaTarget::Es5 {
                    "[[\"undefined\",1]]"
                } else {
                    "[[\"function\",1]]"
                };
                assert_eq!(
                    String::from_utf8(execution.stdout).unwrap(),
                    expected,
                    "{target:?} maps={source_map} original member decoration phase"
                );
            }
        }
    }
}

#[test]
fn standard_field_only_es5_factory_prefix_has_no_source_owner() {
    for prefix in ["", "/* G-E5 🧭 */ "] {
        let source=format!("{prefix}function dec(value:any,context:any):any {{}} export class Box {{@dec value:number=2;}} export const result:number=new Box().value;\n");
        let id = "memory:///main.ts";
        let result = compile(
            id,
            &MapLoader::from([ModuleSource::new(id, &source)]),
            CompilerOptions {
                target: EcmaTarget::Es5,
                module_kind: ModuleKind::CommonJs,
                source_map: true,
                inline_sources: true,
                use_define_for_class_fields: Some(false),
                libraries: Some(vec![EcmaTarget::Es2022]),
                ..CompilerOptions::default()
            },
        );
        assert!(!result.has_errors(), "{:?}", result.diagnostics);
        let artifact = &result.output.unwrap().artifacts[id];
        let code = &artifact.javascript;
        let map = artifact.source_map.as_ref().unwrap();
        let tokens = blueice_bluets::lex("emitted.js", code).unwrap();
        let constructor = tokens
            .windows(2)
            .position(|t| t[0].is("function") && t[1].is("Box"))
            .unwrap();
        let factory = tokens[..constructor]
            .iter()
            .rposition(|t| t.is("function"))
            .unwrap();
        assert!(
            tokens[factory + 1].is("(")
                && tokens[factory + 2].is(")")
                && tokens[factory + 3].is("{")
        );
        for token in &tokens[factory..factory + 4] {
            for offset in token.start..token.end {
                let before = &code[..offset];
                let line = before.bytes().filter(|b| *b == b'\n').count();
                let column = before.rsplit('\n').next().unwrap().encode_utf16().count();
                assert_eq!(
                    map.original_position_at(code, line, column, 100_000, 32_000_000)
                        .unwrap(),
                    None,
                    "generated standard field-class factory prefix"
                );
            }
        }
        let mut depth = 0;
        let mut close = factory + 3;
        for (index, token) in tokens.iter().enumerate().skip(factory + 3) {
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
        let end = source.find("} export const result").unwrap() + 1;
        let expected = (0, source[..end].encode_utf16().count());
        for token in tokens[close + 1..]
            .iter()
            .take_while(|token| !token.is(";"))
        {
            for offset in token.start..token.end {
                let before = &code[..offset];
                let line = before.bytes().filter(|b| *b == b'\n').count();
                let column = before.rsplit('\n').next().unwrap().encode_utf16().count();
                assert_eq!(map.original_position_at(code,line,column,100_000,32_000_000).unwrap().map(|point|(point.line,point.column_utf16)),Some(expected),"standard factory invocation token {} at {} retains its original class completion", token.text, token.start);
            }
        }
    }
}

#[test]
fn standard_field_class_publication_retains_the_original_binding() {
    for prefix in ["", "/* G-E5 🧭 */ "] {
        let source=format!("{prefix}function dec(value:any,context:any):any {{}} export class Box {{@dec value:number=2;}} export const result:number=new Box().value;\n");
        let expected = source.find("class Box").unwrap() + 6;
        let source_prefix = &source[..expected];
        let expected = (0, source_prefix.encode_utf16().count());
        for target in [EcmaTarget::Es5, EcmaTarget::Es2015, EcmaTarget::Es2022] {
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
                        use_define_for_class_fields: Some(false),
                        libraries: Some(vec![EcmaTarget::Es2022]),
                        ..CompilerOptions::default()
                    },
                );
                assert!(!result.has_errors(), "{:?}", result.diagnostics);
                let artifact = &result.output.unwrap().artifacts[id];
                let code = &artifact.javascript;
                let map = artifact.source_map.as_ref().unwrap();
                let tokens = blueice_bluets::lex("emitted.js", code).unwrap();
                let published = if module_kind == ModuleKind::CommonJs {
                    tokens
                        .windows(6)
                        .rfind(|t| {
                            t[0].is("exports")
                                && t[1].is(".")
                                && t[2].is("Box")
                                && t[3].is("=")
                                && t[4].is("Box")
                                && t[5].is(";")
                        })
                        .unwrap()[0]
                        .clone()
                } else {
                    tokens
                        .windows(3)
                        .find(|t| t[0].is("export") && t[1].is("{") && t[2].is("Box"))
                        .unwrap()[2]
                        .clone()
                };
                if target == EcmaTarget::Es5 {
                    let publication = tokens
                        .iter()
                        .position(|token| token.start == published.start)
                        .unwrap();
                    let prefix = if module_kind == ModuleKind::CommonJs {
                        publication
                    } else {
                        publication - 2
                    };
                    let end = tokens[prefix - 1].end;
                    assert!(tokens[prefix - 1].is(";"));
                    let before = &code[..end];
                    let line = before.bytes().filter(|byte| *byte == b'\n').count();
                    let column = before.rsplit('\n').next().unwrap().encode_utf16().count();
                    let class_end = source.find("} export const result").unwrap() + 1;
                    assert_eq!(
                        map.original_position_at(code, line, column, 100_000, 32_000_000)
                            .unwrap()
                            .map(|point| (point.line, point.column_utf16)),
                        Some((0, source[..class_end].encode_utf16().count())),
                        "ES5 class binding logical endpoint"
                    );
                }
                for offset in published.start..published.end {
                    let before = &code[..offset];
                    let line = before.bytes().filter(|b| *b == b'\n').count();
                    let column = before.rsplit('\n').next().unwrap().encode_utf16().count();
                    let owner = map
                        .original_position_at(code, line, column, 100_000, 32_000_000)
                        .unwrap()
                        .map(|p| (p.line, p.column_utf16));
                    assert_eq!(
                        owner,
                        Some(expected),
                        "{target:?}/{module_kind:?} complete source class binding publication"
                    );
                }
            }
        }
    }
}

#[test]
fn legacy_static_publication_observations_follow_the_native_target_phase() {
    for prefix in ["", "/* G-E5 🧭 */ "] {
        let source=format!("{prefix}declare const exports:any; const events:any[]=[]; function observe():number {{events.push(['static',typeof exports.Box]); return 1;}} function dec(target:any,key:any):void {{events.push(['decorate',typeof exports.Box,target.constructor.value]);}} function replacement(value:any):any {{return class extends value {{static replacement=true;}};}} export class Box {{@dec field:number=2; static value:number=observe();}} export const result:any=events;\n");
        for target in [EcmaTarget::Es5, EcmaTarget::Es2015, EcmaTarget::Es2022] {
            for source_map in [false, true] {
                let id = "memory:///main.ts";
                let result = compile(
                    id,
                    &MapLoader::from([ModuleSource::new(id, &source)]),
                    CompilerOptions {
                        target,
                        module_kind: ModuleKind::CommonJs,
                        source_map,
                        inline_sources: source_map,
                        experimental_decorators: true,
                        use_define_for_class_fields: Some(false),
                        libraries: Some(vec![EcmaTarget::Es2022]),
                        ..CompilerOptions::default()
                    },
                );
                assert!(!result.has_errors(), "{:?}", result.diagnostics);
                let code = &result.output.unwrap().artifacts[id].javascript;
                let execution = Command::new("node")
                    .args([
                        "-e",
                        &format!("{code}\nprocess.stdout.write(JSON.stringify(exports.result));"),
                    ])
                    .output()
                    .unwrap();
                assert!(
                    execution.status.success(),
                    "{}",
                    String::from_utf8_lossy(&execution.stderr)
                );
                let expected = if target == EcmaTarget::Es5 {
                    "[[\"static\",\"undefined\"],[\"decorate\",\"undefined\",1]]"
                } else if target == EcmaTarget::Es2015 {
                    "[[\"static\",\"function\"],[\"decorate\",\"function\",1]]"
                } else {
                    "[[\"static\",\"undefined\"],[\"decorate\",\"function\",1]]"
                };
                assert_eq!(String::from_utf8(execution.stdout).unwrap(),expected,"{target:?} maps={source_map}: static initialization precedes publication and member decoration");
            }
        }
    }
}

#[test]
fn legacy_esm_publication_does_not_erase_the_following_original_statement() {
    for prefix in ["", "/* G-E5 🧭 */ "] {
        let source=format!("{prefix}function dec(...args:any[]):any {{}} function replacement(value:any):any {{return class extends value {{static replacement=true;}};}} export class Box {{@dec 'a-b':number=1; @dec static 'static x':number=3;}} const value=new Box(); export const result:number=value['a-b'];\n");
        let expected = source.find("const value=").unwrap();
        let id = "memory:///main.ts";
        let result = compile(
            id,
            &MapLoader::from([ModuleSource::new(id, &source)]),
            CompilerOptions {
                target: EcmaTarget::Es5,
                module_kind: ModuleKind::Esm,
                source_map: true,
                inline_sources: true,
                experimental_decorators: true,
                use_define_for_class_fields: Some(false),
                libraries: Some(vec![EcmaTarget::Es2022]),
                ..CompilerOptions::default()
            },
        );
        assert!(!result.has_errors(), "{:?}", result.diagnostics);
        let artifact = &result.output.unwrap().artifacts[id];
        let code = &artifact.javascript;
        let map = artifact.source_map.as_ref().unwrap();
        let tokens = blueice_bluets::lex("emitted.js", code).unwrap();
        let token = &tokens
            .windows(2)
            .find(|tokens| tokens[0].is("var") && tokens[1].is("value"))
            .unwrap()[0];
        for offset in token.start..token.end {
            let before = &code[..offset];
            let line = before.bytes().filter(|byte| *byte == b'\n').count();
            let column = before.rsplit('\n').next().unwrap().encode_utf16().count();
            assert_eq!(
                map.original_position_at(code, line, column, 100_000, 32_000_000)
                    .unwrap()
                    .map(|point| (point.line, point.column_utf16)),
                Some((0, source[..expected].encode_utf16().count())),
                "the post-publication source statement retains every keyword UTF-16 unit"
            );
        }
    }
}

#[test]
fn standard_class_capture_keeps_its_native_completion_before_metadata() {
    for prefix in ["", "/* G-E5 🧭 */ "] {
        let source=format!("{prefix}function dec(value:any,context:any):any {{}} export class Box {{@dec 'a-b':number=1; @dec static 'static x':number=3;}} export const result:number=new Box()['a-b'];\n");
        let expected = source.find("} export const result").unwrap() + 1;
        for target in [EcmaTarget::Es5, EcmaTarget::Es2015] {
            let id = "memory:///main.ts";
            let result = compile(
                id,
                &MapLoader::from([ModuleSource::new(id, &source)]),
                CompilerOptions {
                    target,
                    module_kind: ModuleKind::CommonJs,
                    source_map: true,
                    inline_sources: true,
                    use_define_for_class_fields: Some(false),
                    libraries: Some(vec![EcmaTarget::Es2022]),
                    ..CompilerOptions::default()
                },
            );
            assert!(!result.has_errors(), "{:?}", result.diagnostics);
            let artifact = &result.output.unwrap().artifacts[id];
            let code = &artifact.javascript;
            let map = artifact.source_map.as_ref().unwrap();
            let tokens = blueice_bluets::lex("emitted.js", code).unwrap();
            let capture = tokens
                .windows(3)
                .enumerate()
                .rfind(|(_, tokens)| {
                    matches!(tokens[0].text.as_str(), "var" | "const")
                        && tokens[1].is("Box")
                        && tokens[2].is("=")
                })
                .unwrap()
                .0;
            let mut depth = 0;
            let mut end = 0;
            for token in &tokens[capture + 3..] {
                match token.text.as_str() {
                    "(" | "[" | "{" => depth += 1,
                    ")" | "]" | "}" => depth -= 1,
                    ";" if depth == 0 => {
                        end = token.end;
                        break;
                    }
                    _ => {}
                }
            }
            assert_ne!(end, 0);
            assert!(
                code[end..].starts_with(' '),
                "existing generated phase separator"
            );
            let before = &code[..end];
            let line = before.bytes().filter(|byte| *byte == b'\n').count();
            let column = before.rsplit('\n').next().unwrap().encode_utf16().count();
            assert_eq!(
                map.original_position_at(code, line, column, 100_000, 32_000_000)
                    .unwrap()
                    .map(|point| (point.line, point.column_utf16)),
                (target == EcmaTarget::Es5)
                    .then_some((0, source[..expected].encode_utf16().count())),
                "{target:?}: the original target phase owns its class capture completion"
            );
        }
    }
}
