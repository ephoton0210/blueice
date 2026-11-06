// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Legacy constructor decorators and retained fields keep independent source phases.
use blueice_bluets::{compile, CompilerOptions, EcmaTarget, MapLoader, ModuleKind, ModuleSource};

fn source_position(source: &str, offset: usize) -> (usize, usize) {
    let before = &source[..offset];
    (
        before.bytes().filter(|byte| *byte == b'\n').count(),
        before.rsplit('\n').next().unwrap().encode_utf16().count(),
    )
}
fn inspect(
    target: EcmaTarget,
    check: impl Fn(&str, &str, &dyn Fn(usize) -> Option<(usize, usize)>),
) {
    for prefix in ["", "/* G-E5 🧭 */\r\n"] {
        let source=format!("{prefix}function dec(...args:any[]):any{{}}\r\n@dec export class C {{ @dec property!: number; constructor(@dec value:number){{ this.property=value; }} }}\r\n");
        for module_kind in [ModuleKind::CommonJs, ModuleKind::Esm] {
            let id = "memory:///main.ts";
            let result = compile(
                id,
                &MapLoader::from([ModuleSource::new(id, &source)]),
                CompilerOptions {
                    target,
                    module_kind,
                    experimental_decorators: true,
                    emit_decorator_metadata: true,
                    use_define_for_class_fields: Some(true),
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
            let owner = |offset| {
                let (line, column) = source_position(code, offset);
                map.original_position_at(code, line, column, 100_000, 32_000_000)
                    .unwrap()
                    .map(|owner| (owner.line, owner.column_utf16))
            };
            check(&source, code, &owner);
        }
    }
}
#[test]
fn constructor_decorator_factory_invocation_retains_the_whole_original_class_start() {
    inspect(EcmaTarget::Es5, |source, code, owner| {
        let returned = code.rfind("return C;").unwrap();
        let tokens = blueice_bluets::lex("generated.js", &code[returned..]).unwrap();
        let close = tokens.iter().position(|token| token.is("}")).unwrap();
        assert_eq!(
            owner(returned + tokens[close].start),
            None,
            "decorated factory close remains generated"
        );
        let expected = Some(source_position(source, source.find("@dec export").unwrap()));
        for token in tokens
            .iter()
            .skip(close + 1)
            .take_while(|token| !token.is(";"))
        {
            assert!(matches!(token.text.as_str(), "(" | ")"));
            for offset in returned + token.start..returned + token.end {
                assert_eq!(
                    owner(offset),
                    expected,
                    "complete generated invocation delimiter"
                );
            }
        }
    });
}
#[test]
fn uninitialized_retained_field_semicolon_inherits_the_original_name_endpoint() {
    for target in [EcmaTarget::Es2022, EcmaTarget::EsNext] {
        inspect(target, |source, code, owner| {
            let tokens = blueice_bluets::lex("generated.js", code).unwrap();
            let pair = tokens
                .windows(2)
                .find(|pair| pair[0].is("property") && pair[1].is(";"))
                .unwrap();
            assert_eq!(
                owner(pair[1].start),
                Some(source_position(
                    source,
                    source.find("property!").unwrap() + "property".len()
                )),
                "retained field terminator"
            );
        });
    }
}
#[test]
fn lowered_field_entry_gap_keeps_the_constructor_signature_phase() {
    inspect(EcmaTarget::Es2015, |source, code, owner| {
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
        let first = &tokens[open + 1];
        assert!(first.is("Object"));
        let expected = Some(source_position(
            source,
            source.find("value:number)").unwrap() + "value:number".len(),
        ));
        for offset in tokens[open].end..first.start {
            assert_eq!(
                owner(offset),
                expected,
                "constructor entry before relocated fields"
            );
        }
    });
}
