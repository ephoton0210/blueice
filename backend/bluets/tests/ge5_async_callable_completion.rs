// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A source name finishes before generated arity and helper identities begin.
use blueice_bluets::{
    compile, lex, CompilerOptions, EcmaTarget, MapLoader, ModuleKind, ModuleLoader, ModuleSource,
};

const PROVIDER: &str = "memory:///node_modules/tslib/tslib.d.ts";
struct Owner(MapLoader);
impl ModuleLoader for Owner {
    fn load(&self, id: &str) -> Result<ModuleSource, String> {
        self.0.load(id)
    }
    fn resolve(&self, _: &str, specifier: &str) -> Result<String, String> {
        if specifier == "tslib" {
            Ok(PROVIDER.into())
        } else {
            Err("only the frozen helper provider is exposed".into())
        }
    }
}

fn position(source: &str, offset: usize) -> (usize, usize) {
    let prefix = &source[..offset];
    (
        prefix.bytes().filter(|byte| *byte == b'\n').count(),
        prefix.rsplit('\n').next().unwrap().encode_utf16().count(),
    )
}

#[test]
fn original_async_name_completion_never_grants_generated_arity_or_helper_names() {
    let id = "memory:///main.ts";
    let helpers: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/ge2_module_helpers/cases.json")).unwrap();
    let provider = helpers["sharedFiles"]["node_modules/tslib/tslib.d.ts"]
        .as_str()
        .unwrap();
    let mut products = 0;
    for prefix in ["", "/* G-E5 🧭 */\r\n"] {
        let source = format!("{prefix}const events:number[]=[];\r\nexport async function compute(value:number):Promise<number>{{try{{return value?await Promise.resolve(value+1):await Promise.resolve(0);}}finally{{events.push(1);}}}}\r\nconst arrow=async(value:number):Promise<number>=>{{return await compute(value);}};\r\nexport const result=arrow(41);\r\n");
        let source_tokens = lex(id, &source).unwrap();
        let name = source_tokens
            .windows(2)
            .find(|part| part[0].is("function") && part[1].is("compute"))
            .unwrap()[1]
            .end;
        let expected = position(&source, name);
        for target in [EcmaTarget::Es5, EcmaTarget::Es2015] {
            for module_kind in [
                ModuleKind::CommonJs,
                ModuleKind::Esm,
                ModuleKind::Amd,
                ModuleKind::Umd,
                ModuleKind::System,
            ] {
                for (import_helpers, no_emit_helpers) in
                    [(false, false), (true, false), (false, true)]
                {
                    let mut outputs = Vec::new();
                    for maps in [false, true] {
                        let result = compile(
                            id,
                            &Owner(MapLoader::from([
                                ModuleSource::new(id, &source),
                                ModuleSource::new(PROVIDER, provider),
                            ])),
                            CompilerOptions {
                                target,
                                module_kind,
                                import_helpers,
                                no_emit_helpers,
                                source_map: maps,
                                inline_sources: true,
                                libraries: Some(vec![EcmaTarget::Es2022]),
                                ..CompilerOptions::default()
                            },
                        );
                        assert!(!result.has_errors(), "{:?}", result.diagnostics);
                        let artifact = &result.output.unwrap().artifacts[id];
                        let code = &artifact.javascript;
                        outputs.push(
                            code.lines()
                                .filter(|line| !line.starts_with("//# sourceMappingURL="))
                                .collect::<Vec<_>>()
                                .join("\n"),
                        );
                        if !maps {
                            continue;
                        }
                        let tokens = lex("emitted.js", code).unwrap();
                        let named = tokens
                            .windows(2)
                            .filter(|part| part[0].is("function") && part[1].is("compute"))
                            .collect::<Vec<_>>();
                        assert_eq!(named.len(), 1);
                        let name_end = named[0][1].end;
                        let map = artifact.source_map.as_ref().unwrap();
                        let owner = |offset| {
                            let (line, column) = position(code, offset);
                            map.original_position_at(code, line, column, 100_000, 32_000_000)
                                .unwrap()
                                .map(|point| (point.line, point.column_utf16))
                        };
                        assert_eq!(owner(name_end), Some(expected),
                            "{target:?} {module_kind:?} helpers={import_helpers}/{no_emit_helpers}: original name.end");
                        let arity = tokens.iter().find(|token| token.start >= name_end).unwrap();
                        assert!(arity.is("("));
                        assert_eq!(
                            owner(arity.start),
                            None,
                            "generated outer arity remains unmapped"
                        );
                        for pair in tokens.windows(2) {
                            if pair[0].is("function") && pair[1].text.starts_with("__blueice_") {
                                assert_eq!(owner(pair[1].start), None,
                                    "a generated helper/capture name cannot inherit source completion");
                                assert_eq!(
                                    owner(pair[1].end),
                                    None,
                                    "generated helper/capture name endings remain unmapped"
                                );
                            }
                        }
                        products += 1;
                    }
                    assert!(outputs[0] == outputs[1],
                        "{target:?} {module_kind:?} helpers={import_helpers}/{no_emit_helpers}: maps preserve executable JavaScript");
                }
            }
        }
    }
    assert_eq!(products, 60);
}

#[test]
fn async_generator_keeps_its_original_name_completion_before_the_generator_arity() {
    let id = "memory:///main.ts";
    for prefix in ["", "/* G-E5 🧭 */\r\n"] {
        let source = format!("{prefix}const log:string[]=[]; async function* sequence():AsyncGenerator<string,number,number>{{try{{yield `v${{await Promise.resolve(1)}}`;}}finally{{log.push(\"finally\");yield `f${{await Promise.resolve(2)}}`;}}return 3;}} async function observe():Promise<string>{{const iterator=sequence();const first=await iterator.next();const second=await iterator.return(9);const third=await iterator.next();return JSON.stringify([first,second,third,log]);}}\nexport const result: Promise<string> = observe();\n");
        let original = lex(id, &source).unwrap();
        let end = original
            .windows(3)
            .find(|part| part[0].is("function") && part[1].is("*") && part[2].is("sequence"))
            .unwrap()[2]
            .end;
        for module_kind in [ModuleKind::CommonJs, ModuleKind::Esm, ModuleKind::System] {
            let result = compile(
                id,
                &MapLoader::from([ModuleSource::new(id, &source)]),
                CompilerOptions {
                    target: EcmaTarget::Es5,
                    module_kind,
                    source_map: true,
                    inline_sources: true,
                    libraries: Some(vec![EcmaTarget::Es2022]),
                    ..CompilerOptions::default()
                },
            );
            assert!(!result.has_errors(), "{:?}", result.diagnostics);
            let artifact = &result.output.unwrap().artifacts[id];
            let tokens = lex("emitted.js", &artifact.javascript).unwrap();
            let name = tokens
                .windows(2)
                .find(|part| part[0].is("function") && part[1].is("sequence"))
                .unwrap()[1]
                .end;
            let (line, column) = position(&artifact.javascript, name);
            let actual = artifact
                .source_map
                .as_ref()
                .unwrap()
                .original_position_at(&artifact.javascript, line, column, 100_000, 32_000_000)
                .unwrap()
                .map(|point| (point.line, point.column_utf16));
            assert_eq!(
                actual,
                Some(position(&source, end)),
                "{module_kind:?}: the ordinary async fix preserves the async-generator name.end"
            );
        }
    }
}
