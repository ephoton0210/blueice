// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! K.1.5 regression coverage at the public compiler boundary.

use blueice_bluets::{compile, CompilerOptions, MapLoader, ModuleSource};

#[test]
fn inferred_results_check_callers_and_emit_declarations() {
    let options = CompilerOptions {
        declaration: true,
        ..CompilerOptions::default()
    };
    let good = compile(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new(
            "memory:///main.ts",
            "export function count() { return 1; } const value: number = count();",
        )]),
        options.clone(),
    );
    assert!(!good.has_errors(), "{:#?}", good.diagnostics);
    assert!(good.checked.as_ref().unwrap().modules["memory:///main.ts"]
        .module
        .declarations
        .iter()
        .any(|declaration| {
            matches!(declaration, blueice_bluets::Declaration::Function(function)
                if function.name == "count" && function.return_type.is_none())
        }));
    assert_eq!(
        good.output.unwrap().artifacts["memory:///main.ts"]
            .declaration
            .as_deref(),
        Some("export declare function count(): number;\n")
    );
    let bad = compile(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new(
            "memory:///main.ts",
            "function count() { return 1; } const value: string = count();",
        )]),
        options,
    );
    assert!(bad.has_errors());
    assert!(bad.output.is_none());
}

#[test]
fn importers_receive_the_inferred_return_signature() {
    let loader = MapLoader::from([
        ModuleSource::new(
            "memory:///main.ts",
            "import { count } from './value.ts'; const value: string = count();",
        ),
        ModuleSource::new(
            "memory:///value.ts",
            "export function count() { return 1; }",
        ),
    ]);
    let result = compile("memory:///main.ts", &loader, CompilerOptions::default());
    assert!(result.has_errors());
    assert!(result.output.is_none());
}

#[test]
fn recursive_inference_requires_an_annotation() {
    let result = compile(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new(
            "memory:///main.ts",
            "export function loop() { return { value: loop() }; }",
        )]),
        CompilerOptions::default(),
    );
    assert!(result.has_errors());
    assert!(result.output.is_none());
    assert!(result.diagnostics.iter().any(|diagnostic| {
        diagnostic.message.contains("return") && diagnostic.message.contains("annotation")
    }));
}

#[test]
fn return_inference_respects_the_configured_expansion_budget() {
    let mut options = CompilerOptions::default();
    options.limits.max_type_expansions = 1;
    let result = compile(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new(
            "memory:///main.ts",
            "function first() { return second(); } function second() { return 1; }",
        )]),
        options,
    );
    assert!(result.has_errors());
    assert!(result.output.is_none());
    assert!(result.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == blueice_bluets::DiagnosticCode::ResourceLimit
            && diagnostic.message.contains("return type inference")
    }));
}
