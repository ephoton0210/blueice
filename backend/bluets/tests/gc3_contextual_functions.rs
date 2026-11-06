// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Nested generic receivers retain static contracts and lexical identities.
use blueice_bluets::{compile, CheckingOptions, CompilerOptions, MapLoader, ModuleSource};

fn checked(source: &str) -> blueice_bluets::Compilation {
    compile(
        "main.ts",
        &MapLoader::from([ModuleSource::new("main.ts", source)]),
        CompilerOptions {
            checking: Some(CheckingOptions::default()),
            ..CompilerOptions::default()
        },
    )
}

#[test]
fn receiver_type_syntax_is_not_an_implicit_runtime_this_read() {
    let result = checked(
        "export function wrap(value: (this: { value: number }) => number): void { void value; }",
    );
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    let rejected = checked("export function invalid(): number { return this.value; }");
    assert!(rejected
        .diagnostics
        .iter()
        .any(|d| d.typescript.as_ref().is_some_and(|ts| ts.code == 2683)));
}

#[test]
fn nested_functions_can_forward_outer_constrained_tuple_parameters() {
    let result = checked("export function forward<Args extends [number, string]>(invoke: (...args: Args) => number) {
        return function (...args: Args): number { return invoke(...args); };
    }");
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    for source in [
        "export function invalid<Args extends [number, string], Other extends [number, string]>(invoke: (...args: Args) => number) {
            return function (...args: Other): number { return invoke(...args); };
        }",
        "export function invalid<Args extends [number, string]>(invoke: (prefix: number, ...args: Args) => number) {
            return function (...args: Args): number { return invoke('wrong', ...args); };
        }",
    ] {
        assert!(checked(source).has_errors(), "{source}");
    }
}

#[test]
fn generic_callback_receivers_keep_writable_constraints() {
    let result = checked(
        "interface Context<This> { add(initializer: (this: This) => void): void; }
        export function change<This extends { offset: number }>(context: Context<This>): void {
            context.add(function () { this.offset += 1; });
        }",
    );
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
}

#[test]
fn imported_context_aliases_keep_generic_callback_receivers() {
    let result = compile("memory:///main.ts", &MapLoader::from([
        ModuleSource::new("memory:///main.ts", "import type { FieldContext as Context } from './contexts.ts';
            export function change<This extends { offset: number }>(context: Context<This, number>): void {
                context.addInitializer(function () { this.offset += 1; });
            }"),
        ModuleSource::new("memory:///contexts.ts", "export type FieldContext<This, Value> = ClassFieldDecoratorContext<This, Value>;"),
    ]), CompilerOptions { checking: Some(CheckingOptions::default()), ..CompilerOptions::default() });
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
}

#[test]
fn nested_body_ownership_preserves_call_and_readonly_failures() {
    for source in [
        "declare const value: { readonly offset: number };
            export function invalid() { return function () { value.offset += 1; }; }",
        "declare function invoke(value: number): void;
            export function invalid() { return function () { invoke('wrong'); }; }",
        "interface Context<This> { add(initializer: (this: This) => void): void; }
            export function invalid(context: Context<{ readonly offset: number }>): void {
                context.add(function () { this.offset += 1; });
            }",
    ] {
        let result = checked(source);
        assert!(result.has_errors(), "{source}");
        assert!(result.output.is_none());
    }
}
