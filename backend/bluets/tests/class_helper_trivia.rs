// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A class that needs no helper cannot add a generated preamble before comments.
use blueice_bluets::{
    compile, CompilerOptions, EcmaTarget, JavaScriptLayout, MapLoader, ModuleKind, ModuleSource,
    NewLine,
};

#[test]
fn helper_free_native_class_factories_keep_leading_comments_at_file_start() {
    let id = "memory:///types.ts";
    let source = "/* exports 🧭 */\r\nconst state:any=globalThis;state.ge3Provided=(state.ge3Provided||0)+1;\r\nexport class Model { value:number=17; }\r\nexport { Model as Alias };\r\n";
    for (newline, ending) in [(NewLine::Lf, "\n"), (NewLine::CrLf, "\r\n")] {
        for (import_helpers, no_emit_helpers) in [(false, false), (true, false), (false, true)] {
            let mut modes = Vec::new();
            for source_map in [false, true] {
                let result = compile(
                    id,
                    &MapLoader::from([ModuleSource::new(id, source)]),
                    CompilerOptions {
                        target: EcmaTarget::Es5,
                        libraries: Some(vec![EcmaTarget::Es2022]),
                        module_kind: ModuleKind::Esm,
                        javascript_layout: JavaScriptLayout::TypeScript,
                        new_line: newline,
                        source_map,
                        import_helpers,
                        no_emit_helpers,
                        ..CompilerOptions::default()
                    },
                );
                assert!(!result.has_errors(), "{:?}", result.diagnostics);
                let artifact = &result.output.as_ref().unwrap().artifacts[id];
                // Pinned TypeScript5.9.3 starts with the surviving source
                // comment; the empty helper group contributes zero bytes.
                assert!(
                    artifact
                        .javascript
                        .starts_with(&format!("/* exports 🧭 */{ending}")),
                    "unexpected helper trivia: {:?}",
                    artifact.javascript
                );
                modes.push(artifact.javascript.clone());
            }
            assert_eq!(modes[0], modes[1], "map/noMap executable bytes");
        }
    }
}
