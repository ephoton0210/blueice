// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Parser regression tests.

use super::*;

#[test]
fn parses_interface_method_signatures_as_static_function_members() {
    let module = parse_module(
        "memory:///lib.blueice.d.ts",
        "interface Document { getElementById(id: string): Element | null; }",
    )
    .unwrap();
    let Declaration::Interface(document) = &module.declarations[0] else {
        panic!("expected an interface");
    };
    assert_eq!(document.fields.len(), 1);
    assert_eq!(document.fields[0].name, "getElementById");
    let Type::Function { parameters, result } = &document.fields[0].value else {
        panic!("expected a function-valued interface member");
    };
    assert_eq!(parameters.len(), 1);
    assert_eq!(parameters[0].name, "id");
    assert_eq!(parameters[0].annotation, Some(Type::String));
    assert_eq!(
        **result,
        Type::Union(vec![
            Type::Named {
                name: "Element".into(),
                arguments: vec![],
            },
            Type::Null,
        ])
    );
}

#[test]
fn parses_keyword_named_event_field_and_bounded_function_type() {
    let module = parse_module(
        "memory:///events.d.ts",
        "interface Event { type: 'click'; }\n\
         interface Node { addEventListener(eventType: 'click', listener: (event: Event) => void): void; }",
    )
    .unwrap();
    let Declaration::Interface(event) = &module.declarations[0] else {
        panic!("expected event interface");
    };
    assert_eq!(event.fields[0].name, "type");
    let Declaration::Interface(node) = &module.declarations[1] else {
        panic!("expected node interface");
    };
    let Type::Function { parameters, .. } = &node.fields[0].value else {
        panic!("expected addEventListener method");
    };
    assert!(matches!(
        parameters[1].annotation,
        Some(Type::Function { .. })
    ));
}

#[test]
fn preserves_readonly_interface_and_record_fields() {
    let module = parse_module(
        "memory:///events.d.ts",
        "interface Event { readonly type: 'click'; target: string; }\n\
         type Detail = { readonly currentTarget: string; mutable?: string };",
    )
    .unwrap();
    let Declaration::Interface(event) = &module.declarations[0] else {
        panic!("expected event interface");
    };
    assert!(event.fields[0].readonly);
    assert!(!event.fields[1].readonly);
    let Declaration::TypeAlias(detail) = &module.declarations[1] else {
        panic!("expected detail alias");
    };
    let Type::Record(fields) = &detail.value else {
        panic!("expected record type");
    };
    assert!(fields[0].readonly);
    assert!(!fields[1].readonly);
}

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
fn retains_named_class_headers_and_bodies_without_claiming_class_semantics() {
    let source = "class Base { value(): number { return 1; } }\nexport class Child extends Base { constructor() { super(); } }";
    let module = parse_module("memory:///classes.ts", source).unwrap();
    assert_eq!(module.declarations.len(), 2);
    let Declaration::Class(base) = &module.declarations[0] else {
        panic!("expected the base class");
    };
    assert_eq!(base.name, "Base");
    assert_eq!(&source[base.name_span.start..base.name_span.end], "Base");
    assert_eq!(base.extends_name, None);
    assert_eq!(
        &source[base.body_span.start..base.body_span.end],
        "{ value(): number { return 1; } }"
    );
    assert!(!base.exported);
    let Declaration::Class(child) = &module.declarations[1] else {
        panic!("expected the child class");
    };
    assert_eq!(child.name, "Child");
    assert_eq!(child.extends_name.as_deref(), Some("Base"));
    let heritage = child.extends_span.as_ref().unwrap();
    assert_eq!(&source[heritage.start..heritage.end], "Base");
    assert_eq!(
        child
            .body
            .iter()
            .map(|token| token.text.as_str())
            .collect::<Vec<_>>(),
        vec!["constructor", "(", ")", "{", "super", "(", ")", ";", "}"]
    );
    assert!(child.exported);
    assert_eq!(
        &source[child.span.start..child.span.end],
        "export class Child extends Base { constructor() { super(); } }"
    );
}

#[test]
fn class_header_errors_and_unimplemented_forms_fail_closed_and_a_plain_class_compiles() {
    for source in [
        "class Missing",
        "class { }",
        "class Box<T> {}",
        "class C extends Base.Member {}",
        "class C extends Base[0] {}",
        "class C implements Shape {}",
    ] {
        assert!(
            parse_module("memory:///classes.ts", source).is_err(),
            "{source}"
        );
    }
    let compilation = crate::compile(
        "memory:///classes.ts",
        &crate::MapLoader::from([crate::ModuleSource::new(
            "memory:///classes.ts",
            "class Ready {}",
        )]),
        crate::CompilerOptions::default(),
    );
    assert!(!compilation.has_errors(), "{:#?}", compilation.diagnostics);
    assert!(compilation.output.is_some());
}

#[test]
fn partitions_constructor_method_and_opaque_class_members_at_source_spans() {
    let source = "class Counter { constructor(value: number) { this.value = value; } read(): number { return this.value; } }";
    let module = parse_module("memory:///counter.ts", source).unwrap();
    let Declaration::Class(class) = &module.declarations[0] else {
        panic!("expected a class");
    };
    assert_eq!(class.members.len(), 2);
    assert_eq!(class.members[0].kind, ClassMemberKind::Constructor);
    assert_eq!(class.members[0].name.as_deref(), Some("constructor"));
    assert_eq!(class.members[1].kind, ClassMemberKind::Method);
    assert_eq!(class.members[1].name.as_deref(), Some("read"));
    for member in &class.members {
        let tokens = &class.body[member.token_start..member.token_end];
        assert_eq!(member.span.start, tokens[0].start);
        assert_eq!(member.span.end, tokens.last().unwrap().end);
    }
    assert_eq!(
        &source[class.members[1].span.start..class.members[1].span.end],
        "read(): number { return this.value; }"
    );

    let module = parse_module(
        "memory:///other.ts",
        "class Other { field = 1; method() {} }",
    )
    .unwrap();
    let Declaration::Class(class) = &module.declarations[0] else {
        panic!("expected a class");
    };
    assert_eq!(class.members.len(), 2);
    assert_eq!(class.members[0].kind, ClassMemberKind::Field);
    let field = class.members[0].field.as_ref().unwrap();
    assert_eq!(field.name, "field");
    assert!(!field.is_static && !field.readonly && !field.optional && !field.definite);
    assert_eq!(field.declared_type(), Some(Type::Number));
    assert_eq!(class.members[1].kind, ClassMemberKind::Method);

    // An accessor is structured, with its accessibility, placement and body.
    let module = parse_module(
        "memory:///accessor.ts",
        "class Other { private static get field(): number { return 1; } set field(value: number) {} method() {} }",
    )
    .unwrap();
    let Declaration::Class(class) = &module.declarations[0] else {
        panic!("expected a class");
    };
    assert_eq!(class.members.len(), 3);
    assert_eq!(class.members[0].kind, ClassMemberKind::Accessor);
    let getter = class.members[0].accessor.as_ref().unwrap();
    assert!(getter.getter && getter.is_static);
    assert_eq!(getter.visibility, crate::Visibility::Private);
    assert_eq!(getter.name, "field");
    assert_eq!(getter.return_type, Some(Type::Number));
    assert_eq!(class.members[1].kind, ClassMemberKind::Accessor);
    let setter = class.members[1].accessor.as_ref().unwrap();
    assert!(!setter.getter && !setter.is_static);
    assert_eq!(setter.parameters.len(), 1);
    assert_eq!(class.members[2].kind, ClassMemberKind::Method);

    // An ECMAScript private name is not a member this parser structures.
    let module = parse_module("memory:///private-name.ts", "class Other { #field = 1; }").unwrap();
    let Declaration::Class(class) = &module.declarations[0] else {
        panic!("expected a class");
    };
    assert_eq!(class.members.len(), 1);
    assert_eq!(class.members[0].kind, ClassMemberKind::Opaque);
}

#[test]
fn parses_constructor_parameters_and_body_and_compiles_the_class() {
    let source = "class Counter { constructor(value: number) { const next: number = value; return; } read() {} }";
    let module = parse_module("memory:///counter.ts", source).unwrap();
    let Declaration::Class(class) = &module.declarations[0] else {
        panic!("expected a class");
    };
    let constructor = class.members[0].constructor.as_ref().unwrap();
    assert_eq!(constructor.parameters.len(), 1);
    assert_eq!(constructor.parameters[0].name, "value");
    assert_eq!(constructor.parameters[0].annotation, Some(Type::Number));
    assert_eq!(
        &source[constructor.parameters[0].span.start..constructor.parameters[0].span.end],
        "value: number"
    );
    assert_eq!(constructor.body.as_ref().unwrap().len(), 2);
    assert!(matches!(
        constructor.body.as_ref().unwrap()[0],
        FunctionBodyItem::Variable(_)
    ));
    assert!(matches!(
        constructor.body.as_ref().unwrap()[1],
        FunctionBodyItem::Return { .. }
    ));
    let FunctionBodyItem::Variable(local) = &constructor.body.as_ref().unwrap()[0] else {
        unreachable!();
    };
    assert_eq!(
        &source[local.span.start..local.span.end],
        "const next: number = value;"
    );
    assert_eq!(
        &source[constructor.span.start..constructor.span.end],
        "constructor(value: number) { const next: number = value; return; }"
    );
    assert_eq!(module.edits.len(), 2);
    assert!(class.members[1].constructor.is_none());
    let compilation = crate::compile(
        "memory:///counter.ts",
        &crate::MapLoader::from([crate::ModuleSource::new("memory:///counter.ts", source)]),
        crate::CompilerOptions::default(),
    );
    assert!(!compilation.has_errors(), "{:#?}", compilation.diagnostics);
    assert!(compilation.output.is_some());
}

#[test]
fn constructor_signatures_and_incomplete_forms_keep_the_parser_closed() {
    let module = parse_module(
        "memory:///constructors.ts",
        "class Box { constructor(value: number); constructor(value: number) {} }",
    )
    .unwrap();
    let Declaration::Class(class) = &module.declarations[0] else {
        panic!("expected a class");
    };
    assert!(class.members[0]
        .constructor
        .as_ref()
        .unwrap()
        .body
        .is_none());
    assert_eq!(
        class.members[1].constructor.as_ref().unwrap().body,
        Some(vec![])
    );
    for source in [
        "class Bad { constructor(value: ) {} }",
        "class Bad { constructor(value: number): number {} }",
        "class Bad { constructor(value: number) }",
        "class Bad { constructor(value: number {} }",
    ] {
        assert!(
            parse_module("memory:///bad.ts", source).is_err(),
            "{source}"
        );
    }
}

#[test]
fn parses_simple_named_method_signatures_and_bodies_at_original_spans() {
    let source = "class Counter { read(value: number): number; read(value: number): number { return value; } }";
    let module = parse_module("memory:///methods.ts", source).unwrap();
    let Declaration::Class(class) = &module.declarations[0] else {
        panic!("expected a class");
    };
    assert_eq!(class.members.len(), 2);
    let signature = class.members[0].method.as_ref().unwrap();
    assert!(signature.body.is_none());
    assert_eq!(signature.parameters[0].annotation, Some(Type::Number));
    assert_eq!(signature.return_type, Some(Type::Number));
    let implementation = class.members[1].method.as_ref().unwrap();
    assert_eq!(implementation.name, "read");
    assert_eq!(implementation.return_type, Some(Type::Number));
    assert!(matches!(
        implementation.body.as_ref().unwrap()[0],
        FunctionBodyItem::Return { .. }
    ));
    assert_eq!(
        &source[implementation.span.start..implementation.span.end],
        "read(value: number): number { return value; }"
    );
    assert_eq!(module.edits.len(), 4);
    let compilation = crate::compile(
        "memory:///methods.ts",
        &crate::MapLoader::from([crate::ModuleSource::new("memory:///methods.ts", source)]),
        crate::CompilerOptions::default(),
    );
    assert!(!compilation.has_errors(), "{:#?}", compilation.diagnostics);
    assert!(compilation.output.is_some());
}

#[test]
fn incomplete_methods_fail_and_ecmascript_private_names_remain_opaque() {
    for source in [
        "class Bad { read(value: ) {} }",
        "class Bad { read(value: number) }",
        "class Bad { read(value: number {} }",
    ] {
        assert!(
            parse_module("memory:///bad.ts", source).is_err(),
            "{source}"
        );
    }
    let module = parse_module("memory:///opaque.ts", "class C { #value = 1; }").unwrap();
    let Declaration::Class(class) = &module.declarations[0] else {
        panic!("expected a class");
    };
    assert_eq!(class.members[0].kind, ClassMemberKind::Opaque);
}

#[test]
fn preserves_union_and_record_method_return_types_without_losing_body_boundaries() {
    let source = "class Reader { union(): string | number { return 1; } record(): { value: number } { return { value: 1 }; } }";
    let module = parse_module("memory:///reader.ts", source).unwrap();
    let Declaration::Class(class) = &module.declarations[0] else {
        panic!("expected a class");
    };
    assert_eq!(class.members.len(), 2);
    assert_eq!(
        class.members[0].method.as_ref().unwrap().return_type,
        Some(Type::Union(vec![Type::String, Type::Number]))
    );
    assert!(matches!(
        class.members[1].method.as_ref().unwrap().return_type,
        Some(Type::Record(_))
    ));
    assert!(class
        .members
        .iter()
        .all(|member| member.method.as_ref().unwrap().body.is_some()));
    assert_eq!(module.edits.len(), 2);
    assert_eq!(
        &source[class.members[1].span.start..class.members[1].span.end],
        "record(): { value: number } { return { value: 1 }; }"
    );
}

#[test]
fn groups_contiguous_method_overloads_at_original_member_boundaries() {
    let source = "class Reader { read(value: number): number; read(value: string): string; read(value: number | string): number | string { return value; } other() {} read(value: number): number; }";
    let module = parse_module("memory:///reader.ts", source).unwrap();
    let Declaration::Class(class) = &module.declarations[0] else {
        panic!("expected a class");
    };
    assert_eq!(class.method_groups.len(), 3);
    let overloads = &class.method_groups[0];
    assert_eq!(overloads.name, "read");
    assert_eq!(overloads.signature_member_indices, vec![0, 1]);
    assert_eq!(overloads.implementation_member_index, Some(2));
    assert_eq!(overloads.span.start, class.members[0].span.start);
    assert_eq!(overloads.span.end, class.members[2].span.end);
    assert_eq!(
        &source[overloads.span.start..overloads.span.end],
        "read(value: number): number; read(value: string): string; read(value: number | string): number | string { return value; }"
    );
    assert_eq!(class.method_groups[1].name, "other");
    assert!(class.method_groups[1].signature_member_indices.is_empty());
    assert_eq!(class.method_groups[1].implementation_member_index, Some(3));
    assert_eq!(class.method_groups[2].name, "read");
    assert_eq!(class.method_groups[2].signature_member_indices, vec![4]);
    assert_eq!(class.method_groups[2].implementation_member_index, None);
    assert!(crate::compile(
        "memory:///reader.ts",
        &crate::MapLoader::from([crate::ModuleSource::new("memory:///reader.ts", source)]),
        crate::CompilerOptions::default(),
    )
    .output
    .is_none());
}

#[test]
fn opaque_class_member_interrupts_method_overload_group() {
    let module = parse_module(
        "memory:///reader.ts",
        "class Reader { read(value: number): number; field = 1; read(value: number): number { return value; } }",
    )
    .unwrap();
    let Declaration::Class(class) = &module.declarations[0] else {
        panic!("expected a class");
    };
    assert_eq!(class.method_groups.len(), 2);
    assert_eq!(class.method_groups[0].signature_member_indices, vec![0]);
    assert_eq!(class.method_groups[0].implementation_member_index, None);
    assert!(class.method_groups[1].signature_member_indices.is_empty());
    assert_eq!(class.method_groups[1].implementation_member_index, Some(2));
}

#[test]
fn routes_unimplemented_class_member_shapes_to_opaque_shells() {
    for source in [
        "class C { #field = 1; }",
        "class C { static private read() {} }",
        "class C { async read() {} }",
        "class C { [key]() {} }",
        "class C { #secret() {} }",
        "class C { generic<T>() {} }",
    ] {
        let module = parse_module("memory:///opaque.ts", source).unwrap();
        let Declaration::Class(class) = &module.declarations[0] else {
            panic!("expected a class");
        };
        assert_eq!(class.members.len(), 1, "{source}");
        assert_eq!(class.members[0].kind, ClassMemberKind::Opaque, "{source}");
    }
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
