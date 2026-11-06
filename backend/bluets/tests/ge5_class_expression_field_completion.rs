// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Class value static stores have a comma-expression completion in Native output.
use blueice_bluets::{compile, CompilerOptions, EcmaTarget, MapLoader, ModuleKind, ModuleSource};

#[test]
fn class_value_static_initializers_keep_the_original_field_completion() {
    for prefix in ["", "/* G-E5 🧭 */ "] {
        let source = format!("{prefix}export function make(value:any):any {{ return class extends value {{ static replacement = true; }}; }} export const result:any = make(class {{}}).replacement;\n");
        let initializer = source.find("true;").unwrap();
        let position = |text: &str, offset| {
            let before = &text[..offset];
            (
                before.bytes().filter(|byte| *byte == b'\n').count(),
                before.rsplit('\n').next().unwrap().encode_utf16().count(),
            )
        };
        for target in [EcmaTarget::Es5, EcmaTarget::Es2015] {
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
                let at = tokens
                    .iter()
                    .position(|t| {
                        t.text == "true" && owner(t.start) == Some(position(&source, initializer))
                    })
                    .expect("original static initializer survives");
                assert_eq!(
                    owner(tokens[at].end),
                    Some(position(&source, initializer + if define { 4 } else { 5 })),
                    "{target:?} define={define} initializer completes its Native expression role"
                );
                if define {
                    assert!(tokens[at + 2].is(")"));
                    assert_eq!(
                        owner(tokens[at + 2].end),
                        Some(position(&source, initializer + 5)),
                        "descriptor call completes the original field"
                    );
                }
            }
        }
    }
}
