// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Shared static providers do not create foreign symbols in a consumer file.

use blueice_bluets::{compile_program, CompilerOptions, MapLoader, ModuleSource};

#[test]
fn global_class_and_namespace_metadata_retain_their_real_source_owners() {
    let roots = vec![
        "program:///provider.ts".to_string(),
        "program:///consumer.ts".to_string(),
    ];
    let loader = MapLoader::from([
        ModuleSource::new(&roots[0],"class Box<T> { constructor(public value: T) {} } namespace Host { export const count: number = 1; }"),
        ModuleSource::new(&roots[1],"const box = new Box<string>('x'); const count: number = Host.count;"),
    ]);
    let result = compile_program(
        &roots,
        &loader,
        CompilerOptions {
            declaration: true,
            ..CompilerOptions::default()
        },
    );
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    let info = result.debug_info.unwrap();
    for symbol in &info.symbols {
        let owner = info
            .sources
            .iter()
            .find(|source| source.id == symbol.source)
            .unwrap();
        assert_eq!(owner.module, symbol.span.module, "{}", symbol.name);
    }
    let namespace_members = info
        .symbols
        .iter()
        .filter(|symbol| symbol.name == "Host.count")
        .collect::<Vec<_>>();
    assert_eq!(
        namespace_members.len(),
        1,
        "foreign bindings manufactured extra declaration handles"
    );
    assert_eq!(namespace_members[0].span.module, roots[0]);
}
