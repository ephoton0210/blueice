// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A lowered field descriptor retains the original initializer's logical end.
use blueice_bluets::{compile, CompilerOptions, EcmaTarget, MapLoader, ModuleKind, ModuleSource};

#[test]
fn define_field_value_ends_before_its_generated_descriptor_transport() {
    let body = "export class Box{value:number=40;read=async(extra:number):Promise<number>=>this.value+await Promise.resolve(extra);}\nexport const result: Promise<number> = new Box().read(2);\n";
    for prefix in ["", "/* G-E5 🧭 */ "] {
        let source = format!("{prefix}{body}");
        let end = source.find(";}\n").unwrap();
        let prefix = &source[..end];
        let expected = (
            prefix.bytes().filter(|byte| *byte == b'\n').count(),
            prefix.rsplit('\n').next().unwrap().encode_utf16().count(),
        );
        for target in [EcmaTarget::Es5, EcmaTarget::Es2015, EcmaTarget::Es2017] {
            let id = "memory:///main.ts";
            let result = compile(
                id,
                &MapLoader::from([ModuleSource::new(id, &source)]),
                CompilerOptions {
                    target,
                    module_kind: ModuleKind::CommonJs,
                    source_map: true,
                    inline_sources: true,
                    use_define_for_class_fields: Some(true),
                    libraries: Some(vec![EcmaTarget::Es2022]),
                    ..CompilerOptions::default()
                },
            );
            assert!(!result.has_errors(), "{:?}", result.diagnostics);
            let artifact = &result.output.unwrap().artifacts[id];
            let code = &artifact.javascript;
            let tokens = blueice_bluets::lex("generated.js", code).unwrap();
            let key = tokens
                .iter()
                .position(|token| token.text == "\"read\"")
                .unwrap();
            let open = key
                + tokens[key..]
                    .iter()
                    .position(|token| token.is("{"))
                    .unwrap();
            let mut depth = 1;
            let mut close = open + 1;
            while depth > 0 {
                if tokens[close].is("{") {
                    depth += 1;
                } else if tokens[close].is("}") {
                    depth -= 1;
                }
                close += 1;
            }
            let value_end = tokens[close - 2].end;
            let before = &code[..value_end];
            let (line, column) = (
                before.bytes().filter(|byte| *byte == b'\n').count(),
                before.rsplit('\n').next().unwrap().encode_utf16().count(),
            );
            let map = artifact.source_map.as_ref().unwrap();
            let owner = map
                .original_position_at(code, line, column, 100_000, 32_000_000)
                .unwrap()
                .map(|owner| (owner.line, owner.column_utf16));
            assert_eq!(
                owner,
                Some(expected),
                "{target:?}: original arrow end before descriptor close"
            );
        }
    }
}
