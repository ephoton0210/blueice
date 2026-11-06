// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! CommonJS class prefixes compose with erased modifiers and ambient neighbours.
use blueice_bluets::{compile, CompilerOptions, EcmaTarget, MapLoader, ModuleKind, ModuleSource};
use std::process::Command;

#[test]
fn exported_class_prefixes_do_not_restore_erased_typescript_modifiers() {
    let cases = [
        ("frozen", include_str!("fixtures/declaration_inference/dtsinfer-commonjs-g1/main.ts"), "exports.result", "[42]"),
        ("abstract-default-alias", "export abstract class Base<T> { abstract get():T; } class Box extends Base<number> { get(){return 42;} } export default Box; export const result=new Box().get();", "exports.result", "42"),
        ("named-default-declare-field", "export default class Named { declare label:string; get():number{return 42;} } export const result=new Named().get();", "[exports.result,Object.prototype.hasOwnProperty.call(new exports.default(),'label')]", "[42,false]"),
        ("anonymous-default", "export default class { value:number=42; }", "new exports.default().value", "42"),
        ("ambient-neighbours", "export declare const absent:number; export declare function unused():number; export class Present { get():number{return 42;} } export const result=new Present().get();", "[exports.result,Object.prototype.hasOwnProperty.call(exports,'absent'),Object.prototype.hasOwnProperty.call(exports,'unused')]", "[42,false,false]"),
        ("abstract-default-derived", "export abstract class Base<T> { abstract get():T; } export default class Box extends Base<number> { get(){return 42;} }", "new exports.default().get()", "42"),
    ];
    for (label, body, value, expected) in cases {
        for prefix in ["", "/* 🧭 */\r\n"] {
            let source = format!("{prefix}{body}");
            for target in [EcmaTarget::Es2015, EcmaTarget::Es2022] {
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
                            declaration: true,
                            ..CompilerOptions::default()
                        },
                    );
                    assert!(!result.has_errors(), "{label}: {:?}", result.diagnostics);
                    let artifact = &result.output.unwrap().artifacts[id];
                    assert!(artifact.declaration.is_some());
                    assert_eq!(artifact.source_map.is_some(), source_map);
                    if let Some(map) = &artifact.source_map {
                        assert_eq!(map.sources_content, vec![source.clone()]);
                        let position = |text: &str, offset: usize| {
                            let before = &text[..offset];
                            (
                                before.bytes().filter(|byte| *byte == b'\n').count(),
                                before.rsplit('\n').next().unwrap().encode_utf16().count(),
                            )
                        };
                        let owner = |offset| {
                            let (line, column) = position(&artifact.javascript, offset);
                            map.original_position_at(
                                &artifact.javascript,
                                line,
                                column,
                                200_000,
                                16 * 1024 * 1024,
                            )
                            .unwrap()
                            .map(|point| (point.source_index, point.line, point.column_utf16))
                        };
                        // Lookup validates the complete decoded stream, including
                        // every generated/source coordinate and source index.
                        let _ = owner(artifact.javascript.len());
                        let original = blueice_bluets::lex(id, &source).unwrap();
                        let generated =
                            blueice_bluets::lex("generated.js", &artifact.javascript).unwrap();
                        for pair in original.windows(2).filter(|pair| {
                            pair[0].is("class")
                                && pair[1].kind == blueice_bluets::TokenKind::Identifier
                        }) {
                            let name = &pair[1];
                            let point = position(&source, name.start);
                            assert!(
                                generated.iter().any(|token| token.text == name.text
                                    && owner(token.start) == Some((0, point.0, point.1))),
                                "{label}: retained class name keeps its original source owner"
                            );
                        }
                    }
                    let output = Command::new("node")
                        .arg("--input-type=commonjs")
                        .arg("-e")
                        .arg(format!(
                            "{}\nprocess.stdout.write(JSON.stringify({value}));",
                            artifact.javascript
                        ))
                        .output()
                        .unwrap();
                    assert!(
                        output.status.success(),
                        "{label}/{target:?}/{source_map}: {}",
                        String::from_utf8_lossy(&output.stderr)
                    );
                    assert_eq!(
                        String::from_utf8(output.stdout).unwrap(),
                        expected,
                        "{label}"
                    );
                }
            }
        }
    }
}

#[test]
fn preserved_export_decorators_keep_their_runtime_header() {
    let source = "function dec(value:any,context:any):any{return value;} export @dec class C {}";
    for source_map in [false, true] {
        let id = "memory:///main.ts";
        let result = compile(
            id,
            &MapLoader::from([ModuleSource::new(id, source)]),
            CompilerOptions {
                target: EcmaTarget::EsNext,
                module_kind: ModuleKind::CommonJs,
                use_define_for_class_fields: Some(true),
                source_map,
                ..CompilerOptions::default()
            },
        );
        assert!(!result.has_errors(), "{:?}", result.diagnostics);
        let output = result.output.unwrap();
        let tokens = blueice_bluets::lex("generated.js", &output.artifacts[id].javascript).unwrap();
        assert!(
            tokens.windows(4).any(|part| part[0].is("@")
                && part[1].is("dec")
                && part[2].is("class")
                && part[3].is("C")),
            "the requested proposal syntax must retain its decorator"
        );
    }
}
