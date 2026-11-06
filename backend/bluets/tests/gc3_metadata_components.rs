// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Optional decorator metadata remains static and owner replaceable.
use blueice_bluets::{
    compile, standard_library, CompilerOptions, EcmaTarget, MapLoader, ModuleSource,
};

fn compile_source(source: &str, options: CompilerOptions) -> blueice_bluets::Compilation {
    compile(
        "main.ts",
        &MapLoader::from([ModuleSource::new("main.ts", source)]),
        options,
    )
}

#[test]
fn explicit_component_changes_exact_library_and_project_identity() {
    let base = standard_library::identity_with_components(EcmaTarget::Es2022, None, false);
    let selected = standard_library::identity_with_components(EcmaTarget::Es2022, None, true);
    assert_ne!(base.source_fingerprint, selected.source_fingerprint);
    assert!(!base.sources.contains(&"symbol-metadata.v1.d.ts"));
    assert!(selected.sources.contains(&"symbol-metadata.v1.d.ts"));
    let source =
        "export function read(context: ClassDecoratorContext) { return context.metadata; }";
    let base = compile_source(source, CompilerOptions::default());
    let selected = compile_source(
        source,
        CompilerOptions {
            symbol_metadata: true,
            ..CompilerOptions::default()
        },
    );
    assert!(!base.has_errors(), "{:?}", base.diagnostics);
    assert!(!selected.has_errors(), "{:?}", selected.diagnostics);
    assert_ne!(base.project_fingerprint, selected.project_fingerprint);
    assert_eq!(selected.project.modules.len(), 1);
    assert_eq!(selected.output.unwrap().artifacts.len(), 1);
}

#[test]
fn selected_component_does_not_replace_an_owner_symbol_constructor() {
    let source = "interface SymbolConstructor { readonly owner: string; }
        export function read(context: ClassDecoratorContext) { return context.metadata; }
        declare const symbols: SymbolConstructor;
        const value: symbol = symbols.metadata;";
    let result = compile_source(
        source,
        CompilerOptions {
            symbol_metadata: true,
            ..CompilerOptions::default()
        },
    );
    assert!(result.has_errors());
    assert!(
        result.diagnostics.iter().any(|diagnostic| diagnostic
            .typescript
            .as_ref()
            .is_some_and(|ts| ts.code == 2339)),
        "{:?}",
        result.diagnostics
    );
    assert!(result.output.is_none());
}

#[test]
fn component_typings_do_not_grant_symbol_runtime_authority() {
    let result = compile_source(
        "Symbol('metadata');",
        CompilerOptions {
            symbol_metadata: true,
            require_declared_global_calls: true,
            ..CompilerOptions::default()
        },
    );
    assert!(result.has_errors());
    assert!(result.output.is_none());
    let empty = standard_library::identity_with_components(EcmaTarget::Es2022, Some(&[]), true);
    assert!(empty.sources.is_empty());
}
