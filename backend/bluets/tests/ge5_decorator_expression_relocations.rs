// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Generated static metadata transport keeps the exact original decorator expression.
use blueice_bluets::{compile, CompilerOptions, EcmaTarget, MapLoader, ModuleKind, ModuleSource};

#[test]
fn lowered_static_metadata_preserves_every_decorator_expression_unit() {
    for prefix in ["", "/* G-E5 🧭 */ "] {
        let source=format!("{prefix}function dec(value:any,context:any):any {{}} export class Box {{ @dec value:number=40; }} export const result:number=new Box().value;\n");
        let start = source.find("@dec").unwrap() + 1;
        let end = start + 3;
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
                let uses = tokens
                    .windows(3)
                    .filter(|t| t[0].is("[") && t[1].is("dec") && t[2].is("]"))
                    .collect::<Vec<_>>();
                assert_eq!(uses.len(), 1, "one source decorator evaluation");
                let expression = &uses[0][1];
                for offset in expression.start..expression.end {
                    assert_eq!(owner(offset),Some(position(&source,start)),"{target:?} define={define} complete original decorator expression interior");
                }
                assert_eq!(
                    owner(expression.end),
                    Some(position(&source, end)),
                    "original decorator logical completion"
                );
            }
        }
    }
}
