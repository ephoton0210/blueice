// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Compiler-synthesized field-extra calls do not introduce user breakpoints.
use blueice_bluets::{compile, CompilerOptions, EcmaTarget, MapLoader, ModuleKind, ModuleSource};

#[test]
fn standalone_field_extra_initialization_is_generated() {
    for prefix in ["", "/* G-E5 🧭 */ "] {
        let source=format!("{prefix}function dec(value:any,context:any):any {{}} export class Box {{ @dec a:number=1; @dec b:number=2; @dec static c:number=3; }} export const result:number=new Box().a;\n");
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
                let calls = tokens
                    .windows(7)
                    .filter(|t| {
                        t[0].is("__bluetsRunInitializers")
                            && t[1].is("(")
                            && t[2].is("this")
                            && t[3].is(",")
                            && t[5].is(")")
                            && t[6].is(";")
                    })
                    .collect::<Vec<_>>();
                assert_eq!(
                    calls.len(),
                    2,
                    "one last-instance and one last-static field extra phase"
                );
                for call in calls {
                    for offset in call[0].start..call[6].end {
                        assert_eq!(
                            owner(offset),
                            None,
                            "{target:?} define={define} compiler-generated extra field phase"
                        );
                    }
                }
            }
        }
    }
}
