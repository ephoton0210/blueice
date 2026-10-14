// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Mutation diagnostics through the public compiler boundary (K.1.2).

use blueice_bluets::{
    compile, CompilerLimits, CompilerOptions, DiagnosticCode, MapLoader, ModuleSource,
    RuntimePolicy,
};

fn checked(source: &str, options: CompilerOptions) -> blueice_bluets::Compilation {
    compile(
        "memory:///main.ts",
        &MapLoader::from([
            ModuleSource::new("memory:///main.ts", source),
            ModuleSource::new("memory:///dep.ts", "export let value: number = 1;"),
        ]),
        options,
    )
}

#[test]
fn immutable_binding_diagnostics_select_the_written_identifier() {
    for (source, name, ts) in [
        ("const fixed: number = 1; fixed += 2;", "fixed", "TS2588"),
        ("const fixed: number = 1; [fixed] = [2];", "fixed", "TS2588"),
        (
            "const fixed: number = 1; ({value: fixed} = {value: 2});",
            "fixed",
            "TS2588",
        ),
        (
            "const fixed: number = 1; for (fixed of [2]) {}",
            "fixed",
            "TS2588",
        ),
        ("const of: number = 1; for (of of [2]) {}", "of", "TS2588"),
        (
            "for (const [fixed] of [[1]]) { fixed++; }",
            "fixed",
            "TS2588",
        ),
        (
            "const fixed: number = 1; const run = (): number => ++fixed;",
            "fixed",
            "TS2588",
        ),
        (
            "import {value as fixed} from './dep.ts'; fixed = 2;",
            "fixed",
            "TS2632",
        ),
    ] {
        let result = checked(source, CompilerOptions::default());
        assert!(result.output.is_none(), "{source}");
        let errors: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::ImmutableAssignment)
            .collect();
        assert_eq!(errors.len(), 1, "{source}: {:#?}", result.diagnostics);
        let diagnostic = errors[0];
        assert_eq!(&source[diagnostic.span.start..diagnostic.span.end], name);
        assert!(diagnostic.message.starts_with(ts));
        assert_eq!(
            diagnostic.span.start,
            if name == "of" {
                source.find("for (").unwrap() + 5
            } else {
                source.rfind(name).unwrap()
            },
            "{source}"
        );
    }
}

#[test]
fn writes_follow_shadowed_bindings_and_preserve_mutable_members() {
    for source in [
        "const fixed: number = 1; function run(fixed: number): number { fixed++; return fixed; }",
        "const fixed: number = 1; { let fixed: number = 2; fixed += 1; }",
        "const fixed: {value: number} = {value: 1}; fixed.value++;",
        "const fixed: number[] = [1]; fixed[0]++;",
        "for (let [value] of [[1]]) { value++; }",
        "for (const {value} of [{value: 1}]) { console.log(value); }",
        "for (const value of [1]) {} let value: number = 0; value++;",
        "import * as dep from './dep.ts'; function run(dep: {value: number}): void { dep.value++; }",
    ] {
        let result = checked(source, CompilerOptions::default());
        assert!(!result.has_errors(), "{source}: {:#?}", result.diagnostics);
    }
}

#[test]
fn readonly_destructuring_and_iteration_report_the_property() {
    for write in [
        "[holder.value] = [2];",
        "({value: holder.value} = {value: 2});",
        "for (holder.value of [2]) {}",
    ] {
        let source = format!("const holder: {{readonly value: number}} = {{value: 1}}; {write}");
        let result = checked(&source, CompilerOptions::default());
        assert!(result.output.is_none());
        let diagnostic = result
            .diagnostics
            .iter()
            .find(|d| d.code == DiagnosticCode::TypeMismatch && d.message.contains("readonly"))
            .unwrap();
        assert_eq!(&source[diagnostic.span.start..diagnostic.span.end], "value");
    }
    let source =
        "class Holder { readonly #value: number = 1; run(): void { [this.#value] = [2]; } }";
    let result = checked(source, CompilerOptions::default());
    let diagnostic = result
        .diagnostics
        .iter()
        .find(|d| d.code == DiagnosticCode::TypeMismatch && d.message.contains("readonly"))
        .unwrap();
    assert_eq!(
        &source[diagnostic.span.start..diagnostic.span.end],
        "#value"
    );
    assert!(result.output.is_none());
}

#[test]
fn readonly_constructor_permission_is_limited_to_own_immediate_fields() {
    for write in ["this.value += 1;", "this.value++;", "[this.value] = [2];"] {
        let source =
            format!("class Holder {{ readonly value: number = 1; constructor() {{ {write} }} }}");
        let result = checked(&source, CompilerOptions::default());
        assert!(!result.has_errors(), "{source}: {:#?}", result.diagnostics);
    }
    for source in [
        "class Holder { readonly value: number = 1; constructor() { const run = (): void => { this.value++; }; } }",
        "class Base { readonly value: number = 1; } class Holder extends Base { constructor() { super(); this.value++; } }",
    ] {
        let result = checked(source, CompilerOptions::default());
        assert!(result.diagnostics.iter().any(|d| d.code == DiagnosticCode::TypeMismatch && d.message.contains("readonly")), "{source}: {:#?}", result.diagnostics);
        assert!(result.output.is_none());
    }
}

#[test]
fn owner_ambient_mutability_and_transpile_policy_are_explicit() {
    let options = CompilerOptions {
        ambient_declaration_modules: vec![ModuleSource::new(
            "blueice:///owner.d.ts",
            "declare const fixed: number; declare let mutable: number;",
        )],
        ..CompilerOptions::default()
    };
    let result = checked("fixed = 2;", options.clone());
    assert!(result
        .diagnostics
        .iter()
        .any(|d| d.code == DiagnosticCode::ImmutableAssignment));
    assert!(result.output.is_none());
    assert!(!checked("mutable++;", options).has_errors());
    let result = checked(
        "const fixed: number = 1; fixed++;",
        CompilerOptions {
            runtime_policy: RuntimePolicy::TranspileOnly,
            ..CompilerOptions::default()
        },
    );
    assert!(!result.has_errors());
    assert!(result.output.is_some());
}

#[test]
fn namespace_exports_keep_binding_mutability() {
    let source = "namespace Area { export const value: number = 1; } Area.value = 2;";
    let result = checked(source, CompilerOptions::default());
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::TypeMismatch && d.message.contains("readonly")),
        "{:#?}",
        result.diagnostics
    );
    assert!(result.output.is_none());
    assert!(!checked(
        "namespace Area { export let value: number = 1; } Area.value = 2;",
        CompilerOptions::default()
    )
    .has_errors());
}

#[test]
fn mutation_budget_exhaustion_prevents_unchecked_output() {
    let source = "type Holder = {readonly value: number}; const holder: Holder = {value: 1}; [holder.value] = [2];";
    let result = checked(
        source,
        CompilerOptions {
            limits: CompilerLimits {
                max_type_expansions: 0,
                ..CompilerLimits::default()
            },
            ..CompilerOptions::default()
        },
    );
    assert!(result
        .diagnostics
        .iter()
        .any(|d| d.code == DiagnosticCode::ResourceLimit));
    assert!(result.output.is_none());
    let source = format!(
        "const fixed: number = 1; {}fixed{} = 2;",
        "(".repeat(70),
        ")".repeat(70)
    );
    let result = checked(
        &source,
        CompilerOptions {
            limits: CompilerLimits {
                max_type_expansions: 1,
                ..CompilerLimits::default()
            },
            ..CompilerOptions::default()
        },
    );
    assert!(result
        .diagnostics
        .iter()
        .any(|d| d.code == DiagnosticCode::ResourceLimit && d.message.contains("mutation")));
    assert!(result.output.is_none());
}
