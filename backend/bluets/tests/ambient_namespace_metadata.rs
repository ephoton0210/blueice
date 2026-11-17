// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Explicit ambient providers retain their own metadata without runtime grants.
use blueice_bluets::{compile, CompilerOptions, MapLoader, ModuleSource};

#[test]
fn owner_global_namespace_metadata_is_unique_and_stays_with_its_source() {
    let entry = "memory:///entry.ts";
    let globals = "memory:///globals.d.ts";
    let loader = MapLoader::from([ModuleSource::new(
        entry,
        "export const value: Shape.Item = { value: 1 };",
    )]);
    for prefix in ["", "/* 🧭 */\r\n"] {
        let source = format!("{prefix}export {{}}; declare global {{ namespace Shape {{ export interface Item {{ value: number; }} }} }}");
        for no_emit in [false, true] {
            let result = compile(
                entry,
                &loader,
                CompilerOptions {
                    ambient_declaration_modules: vec![ModuleSource::new(globals, &source)],
                    no_emit,
                    ..CompilerOptions::default()
                },
            );
            assert!(!result.has_errors(), "{:?}", result.diagnostics);
            assert_eq!(result.project.jsx_runtime_namespace_provider(entry), None);
            let debug = result.debug_info.as_ref().unwrap();
            let owned = debug
                .symbols
                .iter()
                .filter(|symbol| symbol.span.module == globals && symbol.name == "Shape.Item")
                .collect::<Vec<_>>();
            assert_eq!(owned.len(), 1, "{debug:?}");
            let symbol = owned[0];
            let identity = debug
                .sources
                .iter()
                .find(|item| item.id == symbol.source)
                .unwrap();
            assert_eq!(identity.module, globals);
            assert!(identity.content_hash.starts_with("bts-sha256:"));
            assert!(symbol.contract.is_none());
            for (offset, location) in [
                (symbol.span.start, symbol.location.start),
                (symbol.span.end, symbol.location.end),
            ] {
                let before = &source[..offset];
                assert_eq!(
                    location.line,
                    before.bytes().filter(|byte| *byte == b'\n').count()
                );
                assert_eq!(
                    location.column_utf16,
                    before.rsplit('\n').next().unwrap().encode_utf16().count()
                );
            }
            if no_emit {
                assert!(result.output.is_none());
            } else {
                let output = result.output.as_ref().unwrap();
                assert_eq!(output.artifacts.len(), 1);
                assert!(output.artifacts.contains_key(entry));
            }
        }
    }
}
