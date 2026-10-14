// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn property_lookup_keeps_both_callback_method_overloads_without_first_wins() {
    let module = crate::parse_module(
        "memory:///main.ts",
        "interface Visitor { visit(kind: 'text', callback: (value: string) => void): void; visit(kind: 'count', callback: (value: number) => void): void; }",
    )
    .unwrap();
    let [Declaration::Interface(interface)] = module.declarations.as_slice() else {
        panic!("expected one interface");
    };
    let PropertyType::Found {
        value: Type::Intersection(overloads),
        ..
    } = property_type(
        &Type::Record(interface.fields.clone()),
        "visit",
        &BTreeMap::new(),
        &mut HashSet::new(),
        &mut TypeExpansionBudget::new(8),
    )
    else {
        panic!("property lookup must retain both overload signatures");
    };
    assert_eq!(overloads.len(), 2);
    assert_eq!(overloads[0], interface.fields[0].value);
    assert_eq!(overloads[1], interface.fields[1].value);
    assert!(matches!(
        property_type(
            &Type::Record(interface.fields.clone()),
            "visit",
            &BTreeMap::new(),
            &mut HashSet::new(),
            &mut TypeExpansionBudget::new(0),
        ),
        PropertyType::Exhausted
    ));
}

#[test]
fn callback_method_overload_sets_select_compatible_signatures() {
    for (index, signatures) in [
        "visit(kind: 'text', callback: (value: string) => void): void; visit(kind: 'text', callback: (value: number) => void): void;",
        "visit(kind: 'text', callback: (value: string) => void): void; visit(kind: 'count', callback: (value: number) => void): void; visit(kind: 'other', callback: (value: boolean) => void): void;",
        "visit(kind: 'text', callback?: (value: string) => void): void; visit(kind: 'count', callback: (value: number) => void): void;",
        "visit(kind: 'text', callback: (value: string) => string): void; visit(kind: 'count', callback: (value: number) => void): void;",
    ].into_iter().enumerate() {
        let ambient = ModuleSource::new(
            "memory:///visitor.d.ts",
            format!("interface Visitor {{ {signatures} }} declare const visitor: Visitor;"),
        );
        let result = crate::compile(
            "memory:///main.ts",
            &MapLoader::from([ModuleSource::new(
                "memory:///main.ts",
                "function onText(value: string): void {} visitor.visit('text', onText);",
            )]),
            CompilerOptions {
                ambient_declaration_modules: vec![ambient],
                require_declared_global_calls: true,
                ..CompilerOptions::default()
            },
        );
        // Pinned TypeScript 5.9.3 accepts the first three former restrictions;
        // the fourth still rejects the callback's incompatible return type.
        assert_eq!(result.has_errors(), index == 3, "{signatures}: {:#?}", result.diagnostics);
        if index == 3 {
            assert_eq!(result.diagnostics[0].typescript.as_ref().unwrap().code, 2769);
        }
    }
}

#[test]
fn incompatible_inherited_callback_methods_remain_rejected() {
    let result = crate::compile(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new(
            "memory:///main.ts",
            "interface Base { visit(kind: 'text', callback: (value: string) => void): void; } interface Visitor extends Base { visit(kind: 'count', callback: (value: number) => void): void; } declare const visitor: Visitor; function onText(value: string): void {} visitor.visit('text', onText);",
        )]),
        CompilerOptions::default(),
    );
    assert!(
        result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == DiagnosticCode::TypeMismatch
                && diagnostic
                    .typescript
                    .as_ref()
                    .is_some_and(|counterpart| counterpart.code == 2430)
        }),
        "{:#?}",
        result.diagnostics
    );
}

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
