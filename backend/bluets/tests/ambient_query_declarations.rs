// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_bluets::{compile, parse_module, CompilerOptions, MapLoader, ModuleSource};

#[test]
fn inferred_ambient_callable_queries_publish_valid_declarations() {
    let result = compile(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new("memory:///main.ts", "export const callback = host.schedule;")]),
        CompilerOptions {
            declaration: true,
            ambient_declaration_modules: vec![ModuleSource::new("memory:///host.d.ts", "declare function schedule(value: number): string; interface Host { schedule: typeof schedule; } declare const host: Host;")],
            ..Default::default()
        },
    );
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    let output = result.output.unwrap();
    let declaration = output.artifacts["memory:///main.ts"]
        .declaration
        .as_deref()
        .unwrap();
    assert!(!declaration.contains('\0'), "{declaration:?}");
    assert!(
        parse_module("memory:///published.d.ts", declaration).is_ok(),
        "{declaration}"
    );
    assert!(
        declaration.contains("number") && declaration.contains("string"),
        "{declaration}"
    );
}

#[test]
fn global_augmentation_queries_keep_imported_overloads_and_generics() {
    let declarations = [
        ModuleSource::new("memory:///host.d.ts", "import type { convert, identity } from './functions.d.ts'; export {}; declare global { interface Host { convert: typeof convert; identity: typeof identity; } const host: Host; }"),
        ModuleSource::new("memory:///functions.d.ts", "export declare function convert(value: string): string; export declare function convert(value: number): number; export declare function identity<T>(value: T): T;"),
    ];
    let mut failures = Vec::new();
    for (source, accepted) in [
        (
            "const count: number = host.convert(1); const text: string = host.convert('text');",
            true,
        ),
        (
            "const count: number = host.identity(1); const text: string = host.identity('text');",
            true,
        ),
        ("host.convert(true);", false),
        ("const text: string = host.identity(1);", false),
    ] {
        let result = compile(
            "memory:///main.ts",
            &MapLoader::from([
                ModuleSource::new(
                    "memory:///main.ts",
                    format!("/// <reference path=\"./host.d.ts\" />\n{source}"),
                ),
                declarations[0].clone(),
                declarations[1].clone(),
            ]),
            CompilerOptions::default(),
        );
        if result.has_errors() == accepted {
            failures.push((source, accepted, result.diagnostics));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
fn ambient_runtime_imports_do_not_gain_resolution_authority_from_queries() {
    let result = compile("memory:///main.ts", &MapLoader::from([ModuleSource::new("memory:///main.ts", "host.call();")]), CompilerOptions {
        ambient_declaration_modules: vec![ModuleSource::new("memory:///host.d.ts", "import { call } from './private'; declare global { interface Host { call: typeof call; } const host: Host; }")],
        ..Default::default()
    });
    assert!(result.output.is_none());
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code
                == blueice_bluets::DiagnosticCode::InvalidDeclarationFile),
        "{:?}",
        result.diagnostics
    );
}

#[test]
fn recursive_public_callable_queries_emit_a_named_cycle() {
    let result = compile("memory:///main.ts", &MapLoader::from([ModuleSource::new("memory:///main.ts", "export const callback = host.schedule;")]), CompilerOptions {
        declaration: true,
        ambient_declaration_modules: vec![ModuleSource::new("memory:///host.d.ts", "declare function schedule(value: number): typeof schedule; interface Host { schedule: typeof schedule; } declare const host: Host;")],
        ..Default::default()
    });
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    let output = result.output.unwrap();
    let declaration = output.artifacts["memory:///main.ts"]
        .declaration
        .as_deref()
        .unwrap();
    assert!(!declaration.contains('\0'), "{declaration:?}");
    assert!(declaration.contains("typeof schedule"), "{declaration}");
    assert!(
        parse_module("memory:///published.d.ts", declaration).is_ok(),
        "{declaration}"
    );
}
