// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! K.1.4: library declarations remain static, bounded and owner replaceable.

use blueice_bluets::{compile, CompilerOptions, EcmaTarget, MapLoader, ModuleSource};

fn checked(source: &str, options: CompilerOptions) -> blueice_bluets::Compilation {
    compile(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new("memory:///main.ts", source)]),
        options,
    )
}

#[test]
fn library_types_never_enter_the_source_graph_or_outputs() {
    let source = "export const root: number = Math.sqrt(9);";
    let mut options = CompilerOptions::default();
    options.limits.max_modules = 1;
    options.limits.max_total_source_bytes = source.len();
    let result = checked(source, options);
    assert!(!result.has_errors(), "{:#?}", result.diagnostics);
    assert_eq!(result.project.modules.len(), 1);
    assert_eq!(result.output.unwrap().artifacts.len(), 1);
    assert!(result
        .checked
        .unwrap()
        .modules
        .values()
        .flat_map(|module| &module.symbols)
        .all(|symbol| symbol.span.module == "memory:///main.ts"));
    assert!(checked("Math.sqrt('bad');", CompilerOptions::default()).has_errors());
}

#[test]
fn owner_and_local_declarations_replace_standard_names() {
    let options = CompilerOptions {
        ambient_declaration_modules: vec![ModuleSource::new(
            "memory:///owner.d.ts",
            "interface Map<K, V> { owner(key: K): V; } declare const Math: { sqrt(value: string): string };",
        )],
        ..CompilerOptions::default()
    };
    let result = checked("declare const entries: Map<number, string>; const text: string = entries.owner(1); const root: string = Math.sqrt('four');", options.clone());
    assert!(!result.has_errors(), "{:#?}", result.diagnostics);
    assert!(checked(
        "declare const entries: Map<number, string>; entries.get(1);",
        options
    )
    .has_errors());
    let local = checked("declare const Math: { sqrt(value: string): string }; const text: string = Math.sqrt('four');", CompilerOptions::default());
    assert!(!local.has_errors(), "{:#?}", local.diagnostics);
}

#[test]
fn library_typings_do_not_supply_page_runtime_bindings() {
    let mut options = CompilerOptions {
        require_declared_global_calls: true,
        ..CompilerOptions::default()
    };
    assert!(checked("Math.sqrt(9);", options.clone()).has_errors());
    let local = checked(
        "declare const values: Array<number>; values.push(1);",
        options.clone(),
    );
    assert!(!local.has_errors(), "{:#?}", local.diagnostics);
    options.ambient_declaration_modules.push(ModuleSource::new(
        "memory:///owner.d.ts",
        "declare const Math: { sqrt(value: number): number };",
    ));
    let declared = checked("Math.sqrt(9);", options);
    assert!(!declared.has_errors(), "{:#?}", declared.diagnostics);
}

#[test]
fn target_controls_new_members_and_fingerprints() {
    let source = "export const present: boolean = Object.hasOwn({x: 1}, 'x');";
    let old = checked(
        source,
        CompilerOptions {
            target: EcmaTarget::Es2020,
            ..CompilerOptions::default()
        },
    );
    let new = checked(
        source,
        CompilerOptions {
            target: EcmaTarget::Es2022,
            ..CompilerOptions::default()
        },
    );
    assert!(old.has_errors());
    assert!(old.output.is_none());
    assert!(!new.has_errors(), "{:#?}", new.diagnostics);
    assert_ne!(old.project_fingerprint, new.project_fingerprint);
}

#[test]
fn explicit_libraries_preserve_owner_types_and_page_authority() {
    let mut options = CompilerOptions {
        target: EcmaTarget::Es5,
        libraries: Some(vec![EcmaTarget::Es2020]),
        ambient_declaration_modules: vec![ModuleSource::new(
            "memory:///owner.d.ts",
            "interface Map<K, V> { owner(key: K): V; }",
        )],
        ..CompilerOptions::default()
    };
    let source =
        "declare const entries: Map<number, string>; const value: string = entries.owner(1);";
    assert!(!checked(source, options.clone()).has_errors());
    assert!(checked(
        "declare const entries: Map<number, string>; entries.get(1);",
        options.clone()
    )
    .has_errors());
    options.require_declared_global_calls = true;
    assert!(checked("Math.sqrt(9);", options).has_errors());
}

#[test]
fn empty_library_selection_accepts_owner_supplied_intrinsic_types() {
    let options = CompilerOptions {
        libraries: Some(Vec::new()),
        ambient_declaration_modules: vec![ModuleSource::new(
            "memory:///intrinsics.d.ts",
            "interface Array<T> { length: number; } interface Boolean {} interface Function {} interface IArguments {} interface Number {} interface Object {} interface RegExp {} interface String {}",
        )],
        ..CompilerOptions::default()
    };
    let result = checked("export function answer(): number { return 42; }", options);
    assert!(!result.has_errors(), "{:#?}", result.diagnostics);
}

#[test]
fn implicit_library_constants_do_not_pollute_opaque_readonly_checks() {
    let pick = "declare function pick(value: unknown): unknown;";
    for source in [
        format!("{pick} function write(count: number): void {{ pick(count).total = 1; }}"),
        format!("{pick} function write(Math: {{ PI: number }}): void {{ pick(1).total = 1; }}"),
    ] {
        let result = checked(&source, CompilerOptions::default());
        assert!(!result.has_errors(), "{source}\n{:#?}", result.diagnostics);
    }
    // Explicit library flow, aliases, source bindings and owner bindings retain
    // the existing conservative protection for an unmodeled receiver.
    for source in [
        format!("{pick} pick(Math).PI = 1;"),
        format!("{pick} const chosen = pick(Math); chosen.PI = 1;"),
        format!("{pick} declare const Math: {{ readonly PI: number }}; pick(1).total = 1;"),
        format!("{pick} function write(Math: {{ readonly PI: number }}): void {{ pick(1).total = 1; }}"),
        format!("{pick} namespace Area {{ declare const Math: {{ readonly PI: number }}; function write(): void {{ pick(1).total = 1; }} }}"),
    ] {
        let result = checked(&source, CompilerOptions::default());
        assert!(result.diagnostics.iter().any(|diagnostic| diagnostic.message.contains("cannot prove")), "{source}\n{:#?}", result.diagnostics);
    }
    let owner = checked(
        &format!("{pick} pick(1).total = 1;"),
        CompilerOptions {
            ambient_declaration_modules: vec![ModuleSource::new(
                "memory:///owner.d.ts",
                "declare const Math: { readonly PI: number };",
            )],
            ..CompilerOptions::default()
        },
    );
    assert!(
        owner
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("cannot prove")),
        "{:#?}",
        owner.diagnostics
    );
}
