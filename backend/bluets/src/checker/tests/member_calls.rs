// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn ambient_interface_methods_check_member_call_arguments_and_return_types() {
    let ambient = ModuleSource::new(
        "memory:///lib.blueice.d.ts",
        "interface Element { textContent: string; }\n\
         interface Document { getElementById(id: string): Element | null; }\n\
         declare const document: Document;\n\
         declare function snapshot(): string;\n\
         declare function accept(value: Element | null): void;",
    );
    let check = |source| {
        crate::compile(
            "memory:///main.ts",
            &MapLoader::from([ModuleSource::new("memory:///main.ts", source)]),
            CompilerOptions {
                ambient_declaration_modules: vec![ambient.clone()],
                require_declared_global_calls: true,
                ..CompilerOptions::default()
            },
        )
    };
    let valid = check("const found: Element | null = document.getElementById('target');");
    assert!(!valid.has_errors(), "{:#?}", valid.diagnostics);
    let nested = check("const found: Element | null = document.getElementById(snapshot());");
    assert!(!nested.has_errors(), "{:#?}", nested.diagnostics);
    for source in [
        "document.getElementById(123);",
        "document.getElementById();",
        "document.missing('target');",
        "document.getElementById(snapshot(1));",
        "accept(document.getElementById(42));",
    ] {
        let invalid = check(source);
        assert!(
            invalid
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == DiagnosticCode::TypeMismatch),
            "{source}: {:#?}",
            invalid.diagnostics
        );
    }
}

#[test]
fn inferred_ambient_method_result_checks_live_member_assignment() {
    let ambient = ModuleSource::new(
        "memory:///lib.blueice.d.ts",
        "interface Element { textContent: string; }\n\
         interface Document { getElementById(id: string): Element | null; }\n\
         declare const document: Document;",
    );
    let check = |source| {
        crate::compile(
            "memory:///main.ts",
            &MapLoader::from([ModuleSource::new("memory:///main.ts", source)]),
            CompilerOptions {
                ambient_declaration_modules: vec![ambient.clone()],
                require_declared_global_calls: true,
                ..CompilerOptions::default()
            },
        )
    };
    let valid =
        check("const node = document.getElementById('target')!; node.textContent = 'rendered';");
    assert!(!valid.has_errors(), "{:#?}", valid.diagnostics);
    for source in [
        "const node = document.getElementById('target')!; node.textContent = 42;",
        "const node = document.getElementById('target')!; node.missing = 'rendered';",
    ] {
        let invalid = check(source);
        assert!(
            invalid
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == DiagnosticCode::TypeMismatch),
            "{source}: {:#?}",
            invalid.diagnostics
        );
    }
}

#[test]
fn chained_ambient_member_calls_check_the_returned_receiver_and_arguments() {
    let ambient = ModuleSource::new(
        "memory:///lib.blueice.d.ts",
        "interface Node { appendChild(child: Node): Node; }\n\
         interface Document { getElementById(id: string): Node | null; createElement(tag: string): Node; createTextNode(data: string): Node; }\n\
         declare const document: Document;",
    );
    let check = |source| {
        crate::compile(
            "memory:///main.ts",
            &MapLoader::from([ModuleSource::new("memory:///main.ts", source)]),
            CompilerOptions {
                ambient_declaration_modules: vec![ambient.clone()],
                require_declared_global_calls: true,
                ..CompilerOptions::default()
            },
        )
    };
    let valid =
        check("document.getElementById('target')!.appendChild(document.createTextNode('ok'));");
    assert!(!valid.has_errors(), "{:#?}", valid.diagnostics);
    for source in [
        "document.getElementById('target')!.appendChild('wrong');",
        "document.createElement('span').appendChild('wrong');",
        "document.getElementById('target')!.missing('wrong');",
    ] {
        let invalid = check(source);
        assert!(
            invalid
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == DiagnosticCode::TypeMismatch),
            "{source}: {:#?}",
            invalid.diagnostics
        );
    }
}

#[test]
fn chained_member_inference_respects_the_type_expansion_limit() {
    let ambient = ModuleSource::new(
        "memory:///lib.blueice.d.ts",
        "interface Node { appendChild(child: Node): Node; }\n\
         interface Document { createElement(tag: string): Node; }\n\
         declare const document: Document;",
    );
    let result = crate::compile(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new(
            "memory:///main.ts",
            "document.createElement('a').appendChild(document.createElement('b'));",
        )]),
        CompilerOptions {
            ambient_declaration_modules: vec![ambient],
            require_declared_global_calls: true,
            limits: crate::compiler::CompilerLimits {
                max_type_expansions: 2,
                ..crate::compiler::CompilerLimits::default()
            },
            ..CompilerOptions::default()
        },
    );
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == DiagnosticCode::ResourceLimit),
        "{:#?}",
        result.diagnostics
    );
}

#[test]
fn member_calls_need_a_declared_receiver_under_the_page_profile_policy() {
    let result = crate::compile(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new(
            "memory:///main.ts",
            "document.getElementById('target');",
        )]),
        CompilerOptions {
            require_declared_global_calls: true,
            ..CompilerOptions::default()
        },
    );
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == DiagnosticCode::UnknownName),
        "{:#?}",
        result.diagnostics
    );
}
