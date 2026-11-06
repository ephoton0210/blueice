// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Static JSX namespaces cross declaration barrels without granting runtime code.

use blueice_bluets::package_resolution::ImportMode;
use blueice_bluets::{
    compile, CompilerOptions, IncrementalCompiler, JsxMode, MapLoader, ModuleKind, ModuleLoader,
    ModuleSource,
};
use std::cell::Cell;

const ENTRY: &str = "memory:///main.tsx";
const BARREL: &str = "memory:///jsx-runtime.d.cts";
const NAMESPACE: &str = "memory:///namespace.d.cts";

struct Owner {
    value: &'static str,
    prop_type: Cell<&'static str>,
}

impl Owner {
    fn new(value: &'static str) -> Self {
        Self {
            value,
            prop_type: Cell::new("number"),
        }
    }
}

impl ModuleLoader for Owner {
    fn load(&self, id: &str) -> Result<ModuleSource, String> {
        let source = match id {
            ENTRY => format!("export const element = <item value={{{}}} />;", self.value),
            BARREL => "export { JSX, jsx, jsxs } from './namespace.cjs';".into(),
            NAMESPACE => concat!(
                "export declare namespace JSX { ",
                "interface Element { tag: number; } ",
                "interface IntrinsicElements { item: { value: number }; } } ",
                "export declare function jsx(tag: string, props: { value: number }): JSX.Element; ",
                "export { jsx as jsxs };"
            )
            .replace("value: number", &format!("value: {}", self.prop_type.get())),
            _ => return Err(format!("owner has no module {id}")),
        };
        Ok(ModuleSource::new(id, source))
    }

    fn resolve(&self, importer: &str, specifier: &str) -> Result<String, String> {
        if importer == BARREL && specifier == "./namespace.cjs" {
            Ok(NAMESPACE.into())
        } else {
            Err(format!("owner refuses {importer} -> {specifier}"))
        }
    }

    fn resolve_jsx_runtime_namespace(
        &self,
        importer: &str,
        specifier: &str,
        mode: Option<ImportMode>,
    ) -> Result<Option<String>, String> {
        assert_eq!(importer, ENTRY);
        assert_eq!(specifier, "typed-ui/jsx-runtime");
        assert_eq!(mode, Some(ImportMode::Require));
        Ok(Some(BARREL.into()))
    }
}

fn options() -> CompilerOptions {
    CompilerOptions {
        jsx: Some(JsxMode::ReactJsx),
        jsx_import_source: Some("typed-ui".into()),
        module_kind: ModuleKind::CommonJs,
        ..CompilerOptions::default()
    }
}

#[test]
fn declaration_provider_barrel_preserves_namespace_and_declared_function_aliases() {
    let compiled = compile(ENTRY, &Owner::new("42"), options());
    assert!(!compiled.has_errors(), "{:?}", compiled.diagnostics);
    let output = compiled.output.unwrap();
    assert_eq!(output.artifacts.len(), 1);
    assert!(output.artifacts.contains_key(ENTRY));
    assert!(!output.artifacts.contains_key(BARREL));
    assert!(!output.artifacts.contains_key(NAMESPACE));
}

#[test]
fn declaration_provider_barrel_retains_exact_intrinsic_prop_constraints() {
    let compiled = compile(ENTRY, &Owner::new("'wrong'"), options());
    assert!(compiled.has_errors());
    assert!(compiled.output.is_none());
    let codes = compiled
        .diagnostics
        .iter()
        .filter_map(|diagnostic| diagnostic.typescript.as_ref().map(|ts| ts.code))
        .collect::<Vec<_>>();
    assert_eq!(codes, [2322], "{:?}", compiled.diagnostics);
}

#[test]
fn inferred_provider_declaration_uses_the_public_runtime_module_type() {
    let mut options = options();
    options.declaration = true;
    let compiled = compile(ENTRY, &Owner::new("42"), options);
    assert!(!compiled.has_errors(), "{:?}", compiled.diagnostics);
    let output = compiled.output.unwrap();
    assert_eq!(
        output.artifacts[ENTRY].declaration.as_deref(),
        Some("export declare const element: import(\"typed-ui/jsx-runtime\").JSX.Element;\n")
    );
}

#[test]
fn changed_reexported_namespace_rechecks_the_unchanged_jsx_consumer() {
    let owner = Owner::new("42");
    let mut compiler = IncrementalCompiler::new();
    let first = compiler.compile(ENTRY, &owner, options());
    assert!(!first.compilation.has_errors());
    assert!(compiler.compile(ENTRY, &owner, options()).cache_hit);
    owner.prop_type.set("string");
    let fresh = compile(ENTRY, &owner, options());
    let changed = compiler.compile(ENTRY, &owner, options());
    assert!(!changed.cache_hit);
    assert!(changed.rechecked_modules.contains(ENTRY));
    assert!(changed.compilation.output.is_none());
    assert_eq!(changed.compilation.diagnostics, fresh.diagnostics);
    assert!(fresh.diagnostics.iter().any(|diagnostic| diagnostic
        .typescript
        .as_ref()
        .is_some_and(|ts| ts.code == 2322)));
}

#[test]
fn owner_supplied_global_jsx_types_do_not_install_a_runtime_provider() {
    let registry: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/gc3_compositions/registry.json")).unwrap();
    let form = registry["forms"]
        .as_array()
        .unwrap()
        .iter()
        .find(|form| form["id"] == "jsx-provider-missing-provider-global-fallback")
        .unwrap();
    let source = form["files"]["src/context/globals.d.ts"].as_str().unwrap();
    let owner = MapLoader::from([ModuleSource::new(
        ENTRY,
        "export const node = <badge value={42} />; export const answer: number = node.value;",
    )]);
    let mut options = options();
    options.jsx_import_source = Some("absent-global-ui".into());
    options.ambient_declaration_modules = vec![ModuleSource::new("memory:///globals.d.ts", source)];
    let compiled = compile(ENTRY, &owner, options);
    assert!(!compiled.has_errors(), "{:?}", compiled.diagnostics);
    assert_eq!(compiled.project.jsx_runtime_namespace_provider(ENTRY), None);
    let debug = compiled.debug_info.as_ref().unwrap();
    assert!(debug
        .symbols
        .iter()
        .any(|symbol| symbol.span.module == "memory:///globals.d.ts"));
    for symbol in &debug.symbols {
        let source = debug
            .sources
            .iter()
            .find(|source| source.id == symbol.source)
            .unwrap();
        assert_eq!(source.module, symbol.span.module);
        let owner = &compiled.project.modules[&symbol.span.module].source;
        assert_eq!(
            blueice_bluets::source_locations_for_spans(owner, [&symbol.span])[0],
            Some(symbol.location)
        );
        if symbol.span.module != ENTRY {
            assert!(symbol.contract.is_none());
        }
    }
    let output = compiled.output.unwrap();
    assert_eq!(output.artifacts.len(), 1);
    assert!(output.artifacts.contains_key(ENTRY));
}
