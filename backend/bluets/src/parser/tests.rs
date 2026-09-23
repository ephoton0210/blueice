// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Parser regression tests.

use super::*;

#[test]
fn parses_typed_exports_and_marks_only_type_syntax_for_erasure() {
    let module = parse_module(
            "memory:///app.ts",
            "export interface User { name: string; age?: number }\nexport const user: User = { name: 'Ada' };",
        )
        .unwrap();
    assert!(matches!(module.declarations[0], Declaration::Interface(_)));
    assert!(matches!(module.declarations[1], Declaration::Variable(_)));
    assert_eq!(module.edits.len(), 2);
}

#[test]
fn retains_array_holes_in_variable_initializer_tokens() {
    let module = parse_module("memory:///app.ts", "const values = [1,,3];").unwrap();
    let Declaration::Variable(values) = &module.declarations[0] else {
        panic!("expected a variable declaration");
    };
    assert_eq!(
        values
            .initializer
            .iter()
            .map(|token| token.text.as_str())
            .collect::<Vec<_>>(),
        vec!["[", "1", ",", ",", "3", "]"]
    );
}

#[test]
fn rejects_runtime_enums_explicitly() {
    let diagnostics = parse_module("memory:///app.ts", "enum Colour { Red }").unwrap_err();
    assert_eq!(diagnostics[0].code, DiagnosticCode::UnsupportedSyntax);
}

#[test]
fn rejects_tsx_modules_even_when_they_contain_no_tag_tokens() {
    let diagnostics =
        parse_module("memory:///view.tsx", "const label: string = 'BlueIce';").unwrap_err();
    assert_eq!(diagnostics[0].code, DiagnosticCode::UnsupportedSyntax);
    assert!(diagnostics[0].message.contains("TSX/JSX"));
}

#[test]
fn rejects_legacy_commonjs_module_assignment_forms_explicitly() {
    for source in [
        "import Legacy = require('./legacy.ts');",
        "export = Legacy;",
    ] {
        let diagnostics = parse_module("memory:///app.ts", source).unwrap_err();
        assert_eq!(diagnostics[0].code, DiagnosticCode::UnsupportedSyntax);
    }
}

#[test]
fn rejects_unparenthesized_nullish_and_logical_mixing() {
    for source in [
        "const value = false || null ?? 42;",
        "const value = null ?? false || true;",
        "function choose() { return null ?? false || true; }",
        "null ?? false || true;",
    ] {
        let diagnostics = parse_module("memory:///app.ts", source).unwrap_err();
        assert_eq!(diagnostics.len(), 1, "{source}: {diagnostics:#?}");
        assert_eq!(diagnostics[0].code, DiagnosticCode::ParseError, "{source}");
        assert!(diagnostics[0].message.contains("parentheses are required"));
    }

    parse_module(
        "memory:///app.ts",
        "const left = (false || null) ?? 42; const right = null ?? (false || true);",
    )
    .unwrap();
}

#[test]
fn rejects_unparenthesized_unary_exponentiation_bases() {
    for source in [
        "const invalid: number = -2 ** 2;",
        "const invalid: number = ~(2) ** 2;",
        "function value() { return -value() ** 2; }",
    ] {
        let diagnostics = parse_module("memory:///app.ts", source).unwrap_err();
        assert_eq!(diagnostics.len(), 1, "{source}: {diagnostics:#?}");
        assert_eq!(diagnostics[0].code, DiagnosticCode::ParseError, "{source}");
        assert!(diagnostics[0].message.contains("unparenthesized base"));
    }

    parse_module(
        "memory:///app.ts",
        "const reciprocal: number = 2 ** -3; const squared: number = (-2) ** 2;",
    )
    .unwrap();
}

#[test]
fn bounds_deeply_nested_type_expressions() {
    let diagnostics = parse_module_with_limits(
        "memory:///deep.ts",
        "type Deep = { value: { value: { value: string } } };",
        ParserLimits {
            max_type_depth: 2,
            ..ParserLimits::default()
        },
    )
    .unwrap_err();
    assert!(diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == DiagnosticCode::ResourceLimit));
}

#[test]
fn retains_generic_constraints_and_defaults_for_checker_and_declarations() {
    let module = parse_module(
        "memory:///generic.ts",
        "export interface Box<T extends string = string> { value: T }",
    )
    .unwrap();
    let Declaration::Interface(interface) = &module.declarations[0] else {
        panic!("expected interface declaration");
    };
    assert_eq!(interface.type_parameters.len(), 1);
    assert_eq!(interface.type_parameters[0].name, "T");
    assert_eq!(interface.type_parameters[0].constraint, Some(Type::String));
    assert_eq!(interface.type_parameters[0].default, Some(Type::String));
}

#[test]
fn retains_named_generic_interface_heritage() {
    let module = parse_module(
        "memory:///inheritance.ts",
        "interface Envelope<T> { payload: T }\n\
             interface Tagged { tag: string }\n\
             interface Labeled<T extends string = string> extends Envelope<T>, Tagged { label: T }",
    )
    .unwrap();
    let Declaration::Interface(interface) = &module.declarations[2] else {
        panic!("expected inherited interface declaration");
    };
    assert_eq!(
        interface.heritage,
        vec![
            Type::Named {
                name: "Envelope".to_string(),
                arguments: vec![Type::Named {
                    name: "T".to_string(),
                    arguments: Vec::new(),
                }],
            },
            Type::Named {
                name: "Tagged".to_string(),
                arguments: Vec::new(),
            },
        ]
    );
}

#[test]
fn rejects_non_named_interface_heritage() {
    let diagnostics = parse_module(
        "memory:///invalid.ts",
        "interface Invalid extends string {}",
    )
    .unwrap_err();
    assert!(diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == DiagnosticCode::UnsupportedSyntax));
}

#[test]
fn parses_and_erases_signature_only_function_overloads() {
    let source = "function describe(value: string): string;\n\
                      function describe(value: number): number;\n\
                      function describe(value: string | number): string | number { return value; }";
    let module = parse_module("memory:///overload.ts", source).unwrap();
    let Declaration::Function(first) = &module.declarations[0] else {
        panic!("expected first overload declaration");
    };
    let Declaration::Function(implementation) = &module.declarations[2] else {
        panic!("expected implementation declaration");
    };
    assert!(first.overload);
    assert!(!implementation.overload);
    assert!(module.edits.iter().any(|edit| {
        &source[edit.start..edit.end] == "function describe(value: string): string;"
    }));
}

#[test]
fn records_and_erases_explicit_direct_call_type_arguments() {
    let source = "function identity<T>(value: T): T { return value; }\n\
                      const result: string = identity<string>('Ada');";
    let module = parse_module("memory:///generic-call.ts", source).unwrap();
    let call_start = source.rfind("identity<string>").unwrap();
    assert_eq!(
        module.generic_call_type_arguments.get(&call_start),
        Some(&vec![Type::String])
    );
    assert!(module.edits.iter().any(|edit| {
        &source[edit.start..edit.end] == "<string>" && edit.replacement.is_empty()
    }));
}
