// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Constructor metadata changes the source range of the ES5 class invocation.
use blueice_bluets::{compile, CompilerOptions, EcmaTarget, MapLoader, ModuleKind, ModuleSource};

fn position(source: &str, offset: usize) -> (usize, usize) {
    let before = &source[..offset];
    (
        before.bytes().filter(|byte| *byte == b'\n').count(),
        before.rsplit('\n').next().unwrap().encode_utf16().count(),
    )
}

#[test]
fn constructor_metadata_preserves_its_invocation_range_without_granting_plain_decorators() {
    for prefix in ["", "/* G-E5 🧭 */\r\n"] {
        for newline in [false, true] {
            for metadata in [false, true] {
                for property in [false, true] {
                    for module_kind in [ModuleKind::CommonJs, ModuleKind::Esm] {
                        let source=format!("{prefix}function dec(...args:any[]):any{{}}\r\n@dec{}export class C {{ constructor(@dec {}value:number){{}} }}\r\n",if newline {"\r\n"} else {" "},if property {"public "} else {""});
                        let id = "memory:///main.ts";
                        let result = compile(
                            id,
                            &MapLoader::from([ModuleSource::new(id, &source)]),
                            CompilerOptions {
                                target: EcmaTarget::Es5,
                                module_kind,
                                experimental_decorators: true,
                                emit_decorator_metadata: metadata,
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
                        let expected = Some(position(
                            &source,
                            source
                                .find(if metadata { "@dec" } else { "export class" })
                                .unwrap(),
                        ));
                        let returned = code.rfind("return C;").unwrap();
                        let tokens =
                            blueice_bluets::lex("generated.js", &code[returned..]).unwrap();
                        let close = tokens.iter().position(|token| token.is("}")).unwrap();
                        for token in tokens
                            .iter()
                            .skip(close + 1)
                            .take_while(|token| !token.is(";"))
                        {
                            assert!(matches!(token.text.as_str(), "(" | ")"));
                            for offset in returned + token.start..returned + token.end {
                                let (line, column) = position(code, offset);
                                let owner = map
                                    .original_position_at(code, line, column, 100_000, 32_000_000)
                                    .unwrap()
                                    .map(|value| (value.line, value.column_utf16));
                                assert_eq!(owner,expected,"metadata={metadata} newline={newline} property={property} {module_kind:?} invocation at {offset}");
                            }
                        }
                    }
                }
            }
        }
    }
}
