// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Table-driven coverage for the BlueTS parser, checker and emitter through
//! the public `compile` entry point: the diagnostic code and wording for each
//! rejected construct, and the erased JavaScript for accepted ones.

use blueice_bluets::{
    compile, parse_module, ClassMemberKind, CompilerLimits, CompilerOptions, Declaration,
    Diagnostic, DiagnosticCode, FunctionBodyItem, MapLoader, ModuleSource, RuntimePolicy,
};

const ENTRY: &str = "memory:///main.ts";
const HELPER: &str = "export const a: number = 1;\nexport const b: number = 2;\nexport interface Shape { x: number }\nexport type Box<T> = { value: T };\nexport type Id = number | string;\n";

fn compile_with_helper(source: &str) -> blueice_bluets::Compilation {
    let loader = MapLoader::from([
        ModuleSource::new(ENTRY, source),
        ModuleSource::new("memory:///a.ts", HELPER),
    ]);
    compile(ENTRY, &loader, CompilerOptions::default())
}

fn compile_class_module_pair(main: &str, box_module: &str) -> blueice_bluets::Compilation {
    compile(
        ENTRY,
        &MapLoader::from([
            ModuleSource::new(ENTRY, main),
            ModuleSource::new("memory:///box.ts", box_module),
        ]),
        CompilerOptions::default(),
    )
}

fn compile_class_module_triplet(
    main: &str,
    box_module: &str,
    base_module: &str,
) -> blueice_bluets::Compilation {
    compile(
        ENTRY,
        &MapLoader::from([
            ModuleSource::new(ENTRY, main),
            ModuleSource::new("memory:///box.ts", box_module),
            ModuleSource::new("memory:///base.ts", base_module),
        ]),
        CompilerOptions::default(),
    )
}

fn compile_imported_base_type_reexport(main: &str) -> blueice_bluets::Compilation {
    compile(
        ENTRY,
        &MapLoader::from([
            ModuleSource::new(ENTRY, main),
            ModuleSource::new(
                "memory:///second.ts",
                include_str!(
                    "fixtures/typescript_oracle/class-imported-base-type-reexport/second.ts"
                ),
            ),
            ModuleSource::new(
                "memory:///first.ts",
                include_str!(
                    "fixtures/typescript_oracle/class-imported-base-type-reexport/first.ts"
                ),
            ),
            ModuleSource::new(
                "memory:///box.ts",
                include_str!("fixtures/typescript_oracle/class-imported-base-type-reexport/box.ts"),
            ),
            ModuleSource::new(
                "memory:///base.ts",
                include_str!(
                    "fixtures/typescript_oracle/class-imported-base-type-reexport/base.ts"
                ),
            ),
        ]),
        CompilerOptions::default(),
    )
}

fn compile_imported_base_override(main: &str) -> blueice_bluets::Compilation {
    compile(
        ENTRY,
        &MapLoader::from([
            ModuleSource::new(ENTRY, main),
            ModuleSource::new(
                "memory:///middle.ts",
                include_str!("fixtures/typescript_oracle/class-override-imported/middle.ts"),
            ),
            ModuleSource::new(
                "memory:///base.ts",
                include_str!("fixtures/typescript_oracle/class-override-imported/base.ts"),
            ),
        ]),
        CompilerOptions::default(),
    )
}

fn diagnostics(source: &str) -> Vec<Diagnostic> {
    compile_with_helper(source).diagnostics
}

#[track_caller]
fn assert_rejected(source: &str, code: &str, message: &str) {
    let found = diagnostics(source);
    let Some(first) = found.first() else {
        panic!("`{source}` should be rejected with {code}: {message}");
    };
    assert_eq!(first.code.to_string(), code, "`{source}`: {found:?}");
    assert!(
        first.message.contains(message),
        "`{source}`: expected `{message}` in `{}`",
        first.message
    );
    assert!(first.span.start <= first.span.end, "`{source}`");
    assert_eq!(first.span.module, ENTRY, "`{source}`");
}

#[track_caller]
fn assert_accepted(source: &str) {
    let found = diagnostics(source);
    assert!(found.is_empty(), "`{source}` should compile: {found:?}");
}

fn emitted(source: &str) -> String {
    let result = compile_with_helper(source);
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    result.output.unwrap().artifacts[ENTRY].javascript.clone()
}

#[test]
fn class_method_oracle_fixtures_remain_source_bound_and_emit_nothing() {
    let cases = [
        (
            "overloads",
            include_str!("fixtures/typescript_oracle/class-method-overloads/main.ts"),
        ),
        (
            "record-return",
            include_str!("fixtures/typescript_oracle/class-method-record-return/main.ts"),
        ),
        (
            "deferred-private",
            include_str!("fixtures/typescript_oracle/class-method-deferred-private/main.ts"),
        ),
        (
            "orphan-signature",
            include_str!("fixtures/typescript_oracle/class-method-orphan-signature/main.ts"),
        ),
        (
            "interrupted-signature",
            include_str!("fixtures/typescript_oracle/class-method-interrupted-signature/main.ts"),
        ),
        (
            "incompatible-overload",
            include_str!("fixtures/typescript_oracle/class-method-incompatible-overload/main.ts"),
        ),
    ];
    for (name, source) in cases {
        let module = parse_module(ENTRY, source)
            .unwrap_or_else(|errors| panic!("{name} failed bounded class parsing: {errors:#?}"));
        let [Declaration::Class(class)] = module.declarations.as_slice() else {
            panic!("{name} did not retain exactly one class");
        };
        assert_eq!(class.name, "Reader", "{name}");
        assert_eq!(
            &source[class.span.start..class.span.end],
            source[source.find("class Reader").unwrap()..].trim_end(),
            "{name}"
        );
        match name {
            "overloads" => {
                assert_eq!(class.method_groups.len(), 1);
                assert_eq!(class.method_groups[0].signature_member_indices, [0, 1]);
                assert_eq!(class.method_groups[0].implementation_member_index, Some(2));
            }
            "record-return" => {
                assert_eq!(class.method_groups.len(), 1);
                assert_eq!(class.method_groups[0].implementation_member_index, Some(0));
            }
            "deferred-private" => {
                assert_eq!(class.method_groups.len(), 0);
                assert_eq!(class.members[0].kind, ClassMemberKind::Opaque);
            }
            "orphan-signature" => {
                assert_eq!(class.method_groups[0].signature_member_indices, [0]);
                assert_eq!(class.method_groups[0].implementation_member_index, None);
            }
            "interrupted-signature" => {
                assert_eq!(class.method_groups.len(), 2);
                assert_eq!(class.method_groups[0].implementation_member_index, None);
                assert_eq!(class.members[1].kind, ClassMemberKind::Opaque);
                assert_eq!(class.method_groups[1].implementation_member_index, Some(2));
            }
            "incompatible-overload" => {
                assert_eq!(class.method_groups.len(), 1);
                assert_eq!(class.method_groups[0].signature_member_indices, [0]);
                assert_eq!(class.method_groups[0].implementation_member_index, Some(1));
            }
            _ => unreachable!(),
        }
        let compilation = compile_with_helper(source);
        let unsupported = compilation
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == DiagnosticCode::UnsupportedSyntax)
            .count();
        let has_unstructured_member = class
            .members
            .iter()
            .any(|member| member.kind == ClassMemberKind::Opaque);
        if has_unstructured_member {
            // A member other than a constructor or method (here a private
            // member or a field between signatures) has no structured form yet
            // (J.3.2), so the class is refused.
            assert_eq!(unsupported, 1, "{name}: {:#?}", compilation.diagnostics);
            assert!(compilation.output.is_none(), "{name}");
        } else {
            // The class itself is admitted: no refusal diagnostic, and an
            // artifact exactly when nothing else is wrong.
            assert_eq!(unsupported, 0, "{name}: {:#?}", compilation.diagnostics);
            assert_eq!(
                compilation.output.is_some(),
                !compilation.has_errors(),
                "{name}"
            );
        }
    }
}

#[test]
fn class_method_groups_report_original_source_checker_failures() {
    type Case = (
        &'static str,
        &'static str,
        &'static [(DiagnosticCode, usize)],
    );
    let cases: [Case; 7] = [
        (
            "overloads",
            include_str!("fixtures/typescript_oracle/class-method-overloads/main.ts"),
            &[],
        ),
        (
            "record-return",
            include_str!("fixtures/typescript_oracle/class-method-record-return/main.ts"),
            &[],
        ),
        (
            "deferred-private",
            include_str!("fixtures/typescript_oracle/class-method-deferred-private/main.ts"),
            &[],
        ),
        (
            "orphan-signature",
            include_str!("fixtures/typescript_oracle/class-method-orphan-signature/main.ts"),
            &[(DiagnosticCode::TypeMismatch, 6)],
        ),
        (
            "interrupted-signature",
            include_str!("fixtures/typescript_oracle/class-method-interrupted-signature/main.ts"),
            &[(DiagnosticCode::TypeMismatch, 6)],
        ),
        (
            "incompatible-overload",
            include_str!("fixtures/typescript_oracle/class-method-incompatible-overload/main.ts"),
            &[(DiagnosticCode::TypeMismatch, 6)],
        ),
        (
            "duplicate-implementations",
            include_str!(
                "fixtures/typescript_oracle/class-method-duplicate-implementations/main.ts"
            ),
            &[
                (DiagnosticCode::DuplicateDeclaration, 6),
                (DiagnosticCode::DuplicateDeclaration, 7),
            ],
        ),
    ];
    for (name, source, expected) in cases {
        let compilation = compile_with_helper(source);
        assert_eq!(
            compilation.output.is_some(),
            !compilation.has_errors(),
            "{name}: an artifact exists exactly when nothing failed"
        );
        let found = compilation
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code != DiagnosticCode::UnsupportedSyntax)
            .map(|diagnostic| {
                (
                    diagnostic.code,
                    source[..diagnostic.span.start]
                        .bytes()
                        .filter(|byte| *byte == b'\n')
                        .count()
                        + 1,
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(found, expected, "{name}: {:#?}", compilation.diagnostics);
    }
}

#[test]
fn transpile_only_emits_structured_classes_and_still_refuses_unstructured_ones() {
    let compile_transpile = |source: &str| {
        compile(
            ENTRY,
            &MapLoader::from([ModuleSource::new(ENTRY, source)]),
            CompilerOptions {
                runtime_policy: RuntimePolicy::TranspileOnly,
                ..CompilerOptions::default()
            },
        )
    };
    // Erasing a constructor-and-method class needs no checking.
    let structured = compile_transpile(include_str!(
        "fixtures/typescript_oracle/class-method-overloads/main.ts"
    ));
    assert!(structured.output.is_some(), "{:#?}", structured.diagnostics);
    // A member the parser cannot erase is refused even without checking.
    let unstructured = compile_transpile("class A { x: number = 1; }");
    assert!(unstructured.output.is_none());
    assert!(unstructured
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == DiagnosticCode::UnsupportedSyntax));
}

#[test]
fn class_type_and_constructor_value_bind_separately_at_original_spans() {
    type Case = (
        &'static str,
        &'static str,
        &'static [(DiagnosticCode, usize)],
    );
    let cases: [Case; 5] = [
        (
            "dual-binding",
            include_str!("fixtures/typescript_oracle/class-dual-binding/main.ts"),
            &[],
        ),
        (
            "wrong-side",
            include_str!("fixtures/typescript_oracle/class-dual-binding-wrong-side/main.ts"),
            &[(DiagnosticCode::TypeMismatch, 8)],
        ),
        (
            "duplicate-name",
            include_str!("fixtures/typescript_oracle/class-duplicate-name/main.ts"),
            &[(DiagnosticCode::DuplicateDeclaration, 6)],
        ),
        (
            "type-alias-collision",
            include_str!("fixtures/typescript_oracle/class-type-alias-collision/main.ts"),
            &[(DiagnosticCode::DuplicateDeclaration, 6)],
        ),
        (
            "value-collision",
            include_str!("fixtures/typescript_oracle/class-value-collision/main.ts"),
            &[(DiagnosticCode::DuplicateDeclaration, 6)],
        ),
    ];
    for (name, source, expected) in cases {
        let compilation = compile_with_helper(source);
        assert_eq!(
            compilation.output.is_some(),
            !compilation.has_errors(),
            "{name}: an artifact exists exactly when nothing failed"
        );
        for diagnostic in &compilation.diagnostics {
            if diagnostic.code == DiagnosticCode::DuplicateDeclaration {
                assert_eq!(
                    &source[diagnostic.span.start..diagnostic.span.end],
                    "Reader",
                    "{name} must identify the second declaration's name"
                );
            } else if name == "wrong-side" && diagnostic.code == DiagnosticCode::TypeMismatch {
                assert!(
                    source[diagnostic.span.start..diagnostic.span.end]
                        .starts_with("const invalid: Reader = Reader"),
                    "{name} must identify the incorrect assignment"
                );
            }
        }
        let found = compilation
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code != DiagnosticCode::UnsupportedSyntax)
            .map(|diagnostic| {
                (
                    diagnostic.code,
                    source[..diagnostic.span.start]
                        .bytes()
                        .filter(|byte| *byte == b'\n')
                        .count()
                        + 1,
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(found, expected, "{name}: {:#?}", compilation.diagnostics);
    }

    for source in [
        "interface Reader {} class Reader {}",
        "class Reader {} interface Reader {}",
    ] {
        let compilation = compile_with_helper(source);
        // TypeScript merges the two declarations, which BlueTS does not model
        // yet, so it refuses the pair rather than typing only the class.
        assert!(compilation.output.is_none());
        assert!(
            compilation
                .diagnostics
                .iter()
                .all(|diagnostic| { diagnostic.code != DiagnosticCode::DuplicateDeclaration }),
            "class/interface merging is not an illegal name collision: {:#?}",
            compilation.diagnostics
        );
        assert!(
            compilation.diagnostics.iter().any(|diagnostic| {
                diagnostic.code == DiagnosticCode::UnsupportedSyntax
                    && diagnostic.message.contains("merge")
            }),
            "{:#?}",
            compilation.diagnostics
        );
    }
}

#[test]
fn class_construction_checks_arguments_and_infers_instance_shape() {
    for source in [
        include_str!("fixtures/typescript_oracle/class-construction/main.ts"),
        include_str!("fixtures/typescript_oracle/class-construction-overloads/main.ts"),
        include_str!("fixtures/typescript_oracle/class-construction-inherited-deferred/main.ts"),
    ] {
        let compilation = compile_with_helper(source);
        assert!(compilation.output.is_some());
        assert!(
            compilation.diagnostics.is_empty(),
            "accepted constructor shapes have no additional diagnostic: {:#?}",
            compilation.diagnostics
        );
    }

    for (source, expected_line, expected_span) in [
        (
            include_str!("fixtures/typescript_oracle/class-construction-argument-error/main.ts"),
            6,
            "new Box('wrong')",
        ),
        (
            include_str!("fixtures/typescript_oracle/class-construction-arity-error/main.ts"),
            6,
            "new Box()",
        ),
        (
            include_str!(
                "fixtures/typescript_oracle/class-construction-default-arity-error/main.ts"
            ),
            6,
            "new Empty(1)",
        ),
        (
            include_str!(
                "fixtures/typescript_oracle/class-construction-inferred-shape-error/main.ts"
            ),
            7,
            "const invalid: { missing(): number } = inferred;",
        ),
        (
            include_str!(
                "fixtures/typescript_oracle/class-construction-nested-argument-error/main.ts"
            ),
            7,
            "new Box('wrong')",
        ),
    ] {
        let compilation = compile_with_helper(source);
        assert!(compilation.output.is_none());
        let failures = compilation
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code != DiagnosticCode::UnsupportedSyntax)
            .collect::<Vec<_>>();
        assert_eq!(failures.len(), 1, "{:#?}", compilation.diagnostics);
        let failure = failures[0];
        assert_eq!(failure.code, DiagnosticCode::TypeMismatch);
        assert_eq!(
            source[..failure.span.start]
                .bytes()
                .filter(|byte| *byte == b'\n')
                .count()
                + 1,
            expected_line
        );
        assert_eq!(&source[failure.span.start..failure.span.end], expected_span);
    }
}

#[test]
fn class_constructor_groups_and_parameters_are_checked_at_source_spans() {
    let cases = [
        (
            "missing implementation",
            include_str!(
                "fixtures/typescript_oracle/class-constructor-missing-implementation/main.ts"
            ),
            DiagnosticCode::TypeMismatch,
            "constructor(value: number);",
        ),
        (
            "interrupted overload",
            include_str!(
                "fixtures/typescript_oracle/class-constructor-interrupted-overload/main.ts"
            ),
            DiagnosticCode::TypeMismatch,
            "constructor(value: number);",
        ),
        (
            "duplicate implementation",
            include_str!(
                "fixtures/typescript_oracle/class-constructor-duplicate-implementation/main.ts"
            ),
            DiagnosticCode::DuplicateDeclaration,
            "constructor(value: number) {}",
        ),
        (
            "incompatible overload",
            include_str!(
                "fixtures/typescript_oracle/class-constructor-incompatible-overload/main.ts"
            ),
            DiagnosticCode::TypeMismatch,
            "constructor(value: string);",
        ),
        (
            "unknown parameter type",
            include_str!("fixtures/typescript_oracle/class-constructor-unknown-type/main.ts"),
            DiagnosticCode::UnknownType,
            "value: Missing",
        ),
        (
            "invalid default",
            include_str!("fixtures/typescript_oracle/class-constructor-invalid-default/main.ts"),
            DiagnosticCode::TypeMismatch,
            "value: number = 'bad'",
        ),
        (
            "default in signature",
            include_str!("fixtures/typescript_oracle/class-constructor-overload-default/main.ts"),
            DiagnosticCode::TypeMismatch,
            "value: number = 1",
        ),
    ];
    for (name, source, code, expected_span) in cases {
        let compilation = compile_with_helper(source);
        assert_eq!(
            compilation.output.is_some(),
            !compilation.has_errors(),
            "{name}: an artifact exists exactly when nothing failed"
        );
        let failures = compilation
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code != DiagnosticCode::UnsupportedSyntax)
            .collect::<Vec<_>>();
        assert!(!failures.is_empty(), "{name} was accepted");
        assert_eq!(failures[0].code, code, "{name}: {failures:#?}");
        assert_eq!(
            &source[failures[0].span.start..failures[0].span.end],
            expected_span,
            "{name}: {failures:#?}"
        );
    }
    let accepted = compile_with_helper(include_str!(
        "fixtures/typescript_oracle/class-construction-overloads/main.ts"
    ));
    assert!(accepted.output.is_some());
    assert!(
        accepted.diagnostics.is_empty(),
        "{:#?}",
        accepted.diagnostics
    );
    let accepted = compile_with_helper(include_str!(
        "fixtures/typescript_oracle/class-constructor-valid-default/main.ts"
    ));
    assert!(accepted.output.is_some());
    assert!(
        accepted.diagnostics.is_empty(),
        "{:#?}",
        accepted.diagnostics
    );

    let source = include_str!(
        "fixtures/typescript_oracle/class-constructor-duplicate-implementation/main.ts"
    );
    let compilation = compile_with_helper(source);
    let duplicates = compilation
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == DiagnosticCode::DuplicateDeclaration)
        .map(|diagnostic| &source[diagnostic.span.start..diagnostic.span.end])
        .collect::<Vec<_>>();
    assert_eq!(
        duplicates,
        [
            "constructor(value: number) {}",
            "constructor(value: string) {}"
        ]
    );
}

#[test]
fn constructor_bodies_use_typed_scopes_and_check_object_returns() {
    for source in [
        include_str!("fixtures/typescript_oracle/class-constructor-body-valid/main.ts"),
        include_str!("fixtures/typescript_oracle/class-constructor-body-primitive-return/main.ts"),
        include_str!(
            "fixtures/typescript_oracle/class-constructor-body-aliased-primitive-return/main.ts"
        ),
    ] {
        let compilation = compile_with_helper(source);
        assert!(compilation.output.is_some());
        assert!(
            compilation.diagnostics.is_empty(),
            "{:#?}",
            compilation.diagnostics
        );
    }

    for (source, code, expected_span) in [
        (
            include_str!("fixtures/typescript_oracle/class-constructor-body-invalid-local/main.ts"),
            DiagnosticCode::TypeMismatch,
            "const wrong: string = value;",
        ),
        (
            include_str!("fixtures/typescript_oracle/class-constructor-body-invalid-call/main.ts"),
            DiagnosticCode::TypeMismatch,
            "take(value);",
        ),
        (
            include_str!(
                "fixtures/typescript_oracle/class-constructor-body-invalid-return/main.ts"
            ),
            DiagnosticCode::ReturnTypeMismatch,
            "return other;",
        ),
        (
            include_str!("fixtures/typescript_oracle/class-constructor-body-nested-return/main.ts"),
            DiagnosticCode::ReturnTypeMismatch,
            "return other;",
        ),
    ] {
        let compilation = compile_with_helper(source);
        assert!(compilation.output.is_none());
        let failures = compilation
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code != DiagnosticCode::UnsupportedSyntax)
            .collect::<Vec<_>>();
        assert!(!failures.is_empty(), "{source}");
        assert_eq!(failures[0].code, code, "{failures:#?}");
        assert_eq!(
            &source[failures[0].span.start..failures[0].span.end],
            expected_span,
            "{failures:#?}"
        );
    }
}

#[test]
fn class_method_parameters_and_bodies_use_typed_scopes_on_both_sides() {
    let accepted = compile_with_helper(include_str!(
        "fixtures/typescript_oracle/class-method-body-scopes/main.ts"
    ));
    assert!(accepted.output.is_some());
    assert!(
        accepted.diagnostics.is_empty(),
        "{:#?}",
        accepted.diagnostics
    );

    for (source, code, expected_span) in [
        (
            include_str!(
                "fixtures/typescript_oracle/class-method-body-invalid-instance-local/main.ts"
            ),
            DiagnosticCode::TypeMismatch,
            "const wrong: string = value;",
        ),
        (
            include_str!(
                "fixtures/typescript_oracle/class-method-body-invalid-static-local/main.ts"
            ),
            DiagnosticCode::TypeMismatch,
            "const wrong: string = value;",
        ),
        (
            include_str!("fixtures/typescript_oracle/class-method-body-invalid-call/main.ts"),
            DiagnosticCode::TypeMismatch,
            "take(value);",
        ),
        (
            include_str!("fixtures/typescript_oracle/class-method-body-unknown-parameter/main.ts"),
            DiagnosticCode::UnknownType,
            "value: Missing",
        ),
        (
            include_str!("fixtures/typescript_oracle/class-method-body-invalid-default/main.ts"),
            DiagnosticCode::TypeMismatch,
            "value: number = 'bad'",
        ),
        (
            include_str!("fixtures/typescript_oracle/class-method-body-overload-default/main.ts"),
            DiagnosticCode::TypeMismatch,
            "value: number = 1",
        ),
    ] {
        let compilation = compile_with_helper(source);
        assert!(compilation.output.is_none());
        let failures = compilation
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code != DiagnosticCode::UnsupportedSyntax)
            .collect::<Vec<_>>();
        assert!(!failures.is_empty(), "{source}");
        assert_eq!(failures[0].code, code, "{failures:#?}");
        assert_eq!(
            &source[failures[0].span.start..failures[0].span.end],
            expected_span,
            "{failures:#?}"
        );
    }
}

#[test]
fn class_method_declared_returns_use_original_spans() {
    let accepted = compile_with_helper(include_str!(
        "fixtures/typescript_oracle/class-method-returns-valid/main.ts"
    ));
    assert!(accepted.output.is_some());
    assert!(
        accepted.diagnostics.is_empty(),
        "{:#?}",
        accepted.diagnostics
    );

    for (source, code, expected_span) in [
        (
            include_str!(
                "fixtures/typescript_oracle/class-method-returns-invalid-instance/main.ts"
            ),
            DiagnosticCode::ReturnTypeMismatch,
            "return 'bad';",
        ),
        (
            include_str!("fixtures/typescript_oracle/class-method-returns-invalid-static/main.ts"),
            DiagnosticCode::ReturnTypeMismatch,
            "return 'bad';",
        ),
        (
            include_str!("fixtures/typescript_oracle/class-method-returns-invalid-void/main.ts"),
            DiagnosticCode::ReturnTypeMismatch,
            "return 1;",
        ),
        (
            include_str!("fixtures/typescript_oracle/class-method-returns-bare/main.ts"),
            DiagnosticCode::ReturnTypeMismatch,
            "return;",
        ),
        (
            include_str!("fixtures/typescript_oracle/class-method-returns-fallthrough/main.ts"),
            DiagnosticCode::ReturnTypeMismatch,
            "read(flag: boolean): number {\n        if (flag) { return 1; }\n    }",
        ),
        (
            include_str!("fixtures/typescript_oracle/class-method-returns-unknown-type/main.ts"),
            DiagnosticCode::UnknownType,
            "Missing",
        ),
        (
            include_str!("fixtures/typescript_oracle/class-method-returns-overload-body/main.ts"),
            DiagnosticCode::ReturnTypeMismatch,
            "return 'bad';",
        ),
    ] {
        let compilation = compile_with_helper(source);
        assert!(compilation.output.is_none());
        let failures = compilation
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code != DiagnosticCode::UnsupportedSyntax)
            .collect::<Vec<_>>();
        assert!(!failures.is_empty(), "{source}");
        assert_eq!(failures[0].code, code, "{failures:#?}");
        assert_eq!(
            &source[failures[0].span.start..failures[0].span.end],
            expected_span,
            "{failures:#?}"
        );
    }
}

#[test]
fn instance_this_in_class_bodies_uses_instance_members_at_source_spans() {
    let accepted = compile_with_helper(include_str!(
        "fixtures/typescript_oracle/class-instance-this-valid/main.ts"
    ));
    assert!(accepted.output.is_some());
    assert!(
        accepted.diagnostics.is_empty(),
        "{:#?}",
        accepted.diagnostics
    );
    for (source, expected_span) in [
        (
            include_str!("fixtures/typescript_oracle/class-instance-this-argument-error/main.ts"),
            "this.read('bad')",
        ),
        (
            include_str!("fixtures/typescript_oracle/class-instance-this-wrong-side-call/main.ts"),
            "this.make()",
        ),
        (
            include_str!("fixtures/typescript_oracle/class-instance-this-wrong-side-read/main.ts"),
            "this.make",
        ),
        (
            include_str!(
                "fixtures/typescript_oracle/class-instance-this-inferred-return-error/main.ts"
            ),
            "const wrong: string = this.read(1);",
        ),
        (
            include_str!(
                "fixtures/typescript_oracle/class-instance-this-self-return-error/main.ts"
            ),
            "return this;",
        ),
    ] {
        let compilation = compile_with_helper(source);
        assert!(compilation.output.is_none());
        let failures = compilation
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code != DiagnosticCode::UnsupportedSyntax)
            .collect::<Vec<_>>();
        assert_eq!(failures.len(), 1, "{failures:#?}");
        assert!(
            matches!(
                failures[0].code,
                DiagnosticCode::TypeMismatch | DiagnosticCode::ReturnTypeMismatch
            ),
            "{failures:#?}"
        );
        assert_eq!(
            &source[failures[0].span.start..failures[0].span.end],
            expected_span,
            "{failures:#?}"
        );
    }
}

#[test]
fn static_this_in_class_bodies_uses_constructor_side_members_at_source_spans() {
    let accepted = compile_with_helper(include_str!(
        "fixtures/typescript_oracle/class-static-this-valid/main.ts"
    ));
    assert!(accepted.output.is_some());
    assert!(
        accepted.diagnostics.is_empty(),
        "{:#?}",
        accepted.diagnostics
    );
    for (source, code, expected_span) in [
        (
            include_str!("fixtures/typescript_oracle/class-static-this-argument-error/main.ts"),
            DiagnosticCode::TypeMismatch,
            "this.answer('bad')",
        ),
        (
            include_str!("fixtures/typescript_oracle/class-static-this-wrong-side-call/main.ts"),
            DiagnosticCode::TypeMismatch,
            "this.read()",
        ),
        (
            include_str!("fixtures/typescript_oracle/class-static-this-wrong-side-read/main.ts"),
            DiagnosticCode::TypeMismatch,
            "this.read",
        ),
        (
            include_str!(
                "fixtures/typescript_oracle/class-static-this-inferred-return-error/main.ts"
            ),
            DiagnosticCode::TypeMismatch,
            "const wrong: string = this.answer();",
        ),
        (
            include_str!("fixtures/typescript_oracle/class-static-this-self-return-error/main.ts"),
            DiagnosticCode::ReturnTypeMismatch,
            "return this;",
        ),
    ] {
        let compilation = compile_with_helper(source);
        assert!(compilation.output.is_none());
        let failures = compilation
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code != DiagnosticCode::UnsupportedSyntax)
            .collect::<Vec<_>>();
        assert_eq!(failures.len(), 1, "{failures:#?}");
        assert_eq!(failures[0].code, code, "{failures:#?}");
        assert_eq!(
            &source[failures[0].span.start..failures[0].span.end],
            expected_span,
            "{failures:#?}"
        );
    }
}

#[test]
fn instance_method_overload_calls_select_returns_at_original_spans() {
    let accepted = compile_with_helper(include_str!(
        "fixtures/typescript_oracle/class-instance-overload-valid/main.ts"
    ));
    assert!(accepted.output.is_some());
    assert!(
        accepted.diagnostics.is_empty(),
        "{:#?}",
        accepted.diagnostics
    );
    for (source, code, expected_span) in [
        (
            include_str!(
                "fixtures/typescript_oracle/class-instance-overload-argument-error/main.ts"
            ),
            DiagnosticCode::TypeMismatch,
            "reader.read(true)",
        ),
        (
            include_str!(
                "fixtures/typescript_oracle/class-instance-overload-inferred-error/main.ts"
            ),
            DiagnosticCode::TypeMismatch,
            "const wrong: string = reader.read(1);",
        ),
        (
            include_str!(
                "fixtures/typescript_oracle/class-instance-overload-this-return-error/main.ts"
            ),
            DiagnosticCode::ReturnTypeMismatch,
            "return this.read(1);",
        ),
        (
            include_str!(
                "fixtures/typescript_oracle/class-instance-overload-this-argument-error/main.ts"
            ),
            DiagnosticCode::TypeMismatch,
            "this.read(true)",
        ),
    ] {
        let compilation = compile_with_helper(source);
        assert!(compilation.output.is_none());
        let failures = compilation
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code != DiagnosticCode::UnsupportedSyntax)
            .collect::<Vec<_>>();
        assert_eq!(failures.len(), 1, "{failures:#?}");
        assert_eq!(failures[0].code, code, "{failures:#?}");
        assert_eq!(
            &source[failures[0].span.start..failures[0].span.end],
            expected_span,
            "{failures:#?}"
        );
    }
}

#[test]
fn static_method_overload_calls_select_returns_on_values_and_this() {
    let accepted = compile_with_helper(include_str!(
        "fixtures/typescript_oracle/class-static-overload-this-valid/main.ts"
    ));
    assert!(accepted.output.is_some());
    assert!(
        accepted.diagnostics.is_empty(),
        "{:#?}",
        accepted.diagnostics
    );
    for (source, code, expected_span) in [
        (
            include_str!("fixtures/typescript_oracle/class-static-overload-error/main.ts"),
            DiagnosticCode::TypeMismatch,
            "Converter.parse(true)",
        ),
        (
            include_str!(
                "fixtures/typescript_oracle/class-static-overload-value-inferred-error/main.ts"
            ),
            DiagnosticCode::TypeMismatch,
            "const wrong: string = Converter.parse(1);",
        ),
        (
            include_str!(
                "fixtures/typescript_oracle/class-static-overload-this-argument-error/main.ts"
            ),
            DiagnosticCode::TypeMismatch,
            "this.parse(true)",
        ),
        (
            include_str!(
                "fixtures/typescript_oracle/class-static-overload-this-return-error/main.ts"
            ),
            DiagnosticCode::ReturnTypeMismatch,
            "return this.parse(1);",
        ),
    ] {
        let compilation = compile_with_helper(source);
        assert!(compilation.output.is_none());
        let failures = compilation
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code != DiagnosticCode::UnsupportedSyntax)
            .collect::<Vec<_>>();
        assert_eq!(failures.len(), 1, "{failures:#?}");
        assert_eq!(failures[0].code, code, "{failures:#?}");
        assert_eq!(
            &source[failures[0].span.start..failures[0].span.end],
            expected_span,
            "{failures:#?}"
        );
    }
}

#[test]
fn named_class_heritage_validates_base_names_and_declaration_order() {
    let accepted = compile_with_helper(include_str!(
        "fixtures/typescript_oracle/class-heritage-local-valid/main.ts"
    ));
    assert!(accepted.output.is_some());
    assert!(
        accepted.diagnostics.is_empty(),
        "{:#?}",
        accepted.diagnostics
    );
    let imported = compile_class_module_pair(
        include_str!("fixtures/typescript_oracle/class-heritage-imported-valid/main.ts"),
        include_str!("fixtures/typescript_oracle/class-heritage-imported-valid/box.ts"),
    );
    assert!(imported.output.is_some());
    assert!(
        imported.diagnostics.is_empty(),
        "{:#?}",
        imported.diagnostics
    );
    for (source, code, expected_span) in [
        (
            include_str!("fixtures/typescript_oracle/class-heritage-unknown-base/main.ts"),
            DiagnosticCode::UnknownName,
            "Missing",
        ),
        (
            include_str!("fixtures/typescript_oracle/class-heritage-nonconstructor-base/main.ts"),
            DiagnosticCode::TypeMismatch,
            "NotClass",
        ),
        (
            include_str!("fixtures/typescript_oracle/class-heritage-forward-base/main.ts"),
            DiagnosticCode::TypeMismatch,
            "Base",
        ),
    ] {
        let compilation = compile_with_helper(source);
        assert!(compilation.output.is_none());
        let failures = compilation
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code != DiagnosticCode::UnsupportedSyntax)
            .collect::<Vec<_>>();
        assert_eq!(failures.len(), 1, "{failures:#?}");
        assert_eq!(failures[0].code, code, "{failures:#?}");
        assert_eq!(
            &source[failures[0].span.start..failures[0].span.end],
            expected_span,
            "{failures:#?}"
        );
    }
}

#[test]
fn local_class_heritage_cycles_diagnose_cycle_members_only() {
    let source = include_str!("fixtures/typescript_oracle/class-heritage-self-cycle/main.ts");
    let compilation = compile_with_helper(source);
    assert!(compilation.output.is_none());
    let failures = compilation
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code != DiagnosticCode::UnsupportedSyntax)
        .collect::<Vec<_>>();
    assert_eq!(failures.len(), 1, "{failures:#?}");
    assert_eq!(failures[0].code, DiagnosticCode::TypeMismatch);
    assert_eq!(
        &source[failures[0].span.start..failures[0].span.end],
        "Self"
    );

    let source = include_str!("fixtures/typescript_oracle/class-heritage-mutual-cycle/main.ts");
    let compilation = compile_with_helper(source);
    assert!(compilation.output.is_none());
    let mut failures = compilation
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code != DiagnosticCode::UnsupportedSyntax)
        .collect::<Vec<_>>();
    failures.sort_by_key(|diagnostic| diagnostic.span.start);
    assert_eq!(failures.len(), 3, "{failures:#?}");
    assert!(failures
        .iter()
        .all(|diagnostic| diagnostic.code == DiagnosticCode::TypeMismatch));
    assert_eq!(
        failures
            .iter()
            .map(|diagnostic| diagnostic.span.start)
            .collect::<Vec<_>>(),
        vec![
            source.find("class A").unwrap() + 6,
            source.find("extends B").unwrap() + 8,
            source.find("class B").unwrap() + 6,
        ]
    );
    assert_eq!(
        failures
            .iter()
            .map(|diagnostic| &source[diagnostic.span.start..diagnostic.span.end])
            .collect::<Vec<_>>(),
        vec!["A", "B", "B"]
    );
}

#[test]
fn local_class_heritage_cycle_scan_obeys_the_type_expansion_budget() {
    let source = "class C {} class B extends C {} class A extends B {}";
    let compilation = compile(
        ENTRY,
        &MapLoader::from([ModuleSource::new(ENTRY, source)]),
        CompilerOptions {
            limits: CompilerLimits {
                max_type_expansions: 1,
                ..CompilerLimits::default()
            },
            ..CompilerOptions::default()
        },
    );
    assert!(compilation.output.is_none());
    let failures = compilation
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == DiagnosticCode::ResourceLimit)
        .collect::<Vec<_>>();
    assert_eq!(failures.len(), 1, "{:#?}", compilation.diagnostics);
    assert_eq!(&source[failures[0].span.start..failures[0].span.end], "A");
}

#[test]
fn inherited_instance_methods_work_through_local_and_imported_class_bases() {
    for accepted in [
        compile_with_helper(include_str!(
            "fixtures/typescript_oracle/class-inherited-instance-valid/main.ts"
        )),
        compile_class_module_pair(
            include_str!(
                "fixtures/typescript_oracle/class-inherited-imported-instance-valid/main.ts"
            ),
            include_str!(
                "fixtures/typescript_oracle/class-inherited-imported-instance-valid/box.ts"
            ),
        ),
    ] {
        assert!(accepted.output.is_some());
        assert!(
            accepted.diagnostics.is_empty(),
            "{:#?}",
            accepted.diagnostics
        );
    }
    for (source, expected_span) in [
        (
            include_str!(
                "fixtures/typescript_oracle/class-inherited-instance-argument-error/main.ts"
            ),
            "child.label('wrong')",
        ),
        (
            include_str!(
                "fixtures/typescript_oracle/class-inherited-instance-result-error/main.ts"
            ),
            "const wrong: number = child.label(1);",
        ),
    ] {
        let compilation = compile_with_helper(source);
        assert!(compilation.output.is_none());
        let failures = compilation
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code != DiagnosticCode::UnsupportedSyntax)
            .collect::<Vec<_>>();
        assert_eq!(failures.len(), 1, "{failures:#?}");
        assert_eq!(failures[0].code, DiagnosticCode::TypeMismatch);
        assert_eq!(
            &source[failures[0].span.start..failures[0].span.end],
            expected_span,
            "{failures:#?}"
        );
    }
}

#[test]
fn inherited_static_overloads_work_through_class_values_and_static_this() {
    for accepted in [
        compile_with_helper(include_str!(
            "fixtures/typescript_oracle/class-inherited-static-valid/main.ts"
        )),
        compile_class_module_pair(
            include_str!(
                "fixtures/typescript_oracle/class-inherited-imported-static-valid/main.ts"
            ),
            include_str!("fixtures/typescript_oracle/class-inherited-imported-static-valid/box.ts"),
        ),
    ] {
        assert!(accepted.output.is_some());
        assert!(
            accepted.diagnostics.is_empty(),
            "{:#?}",
            accepted.diagnostics
        );
    }
    for (source, expected_span) in [
        (
            include_str!(
                "fixtures/typescript_oracle/class-inherited-static-argument-error/main.ts"
            ),
            "Child.parse(true)",
        ),
        (
            include_str!("fixtures/typescript_oracle/class-inherited-static-result-error/main.ts"),
            "const wrong: string = Child.parse(1);",
        ),
    ] {
        let compilation = compile_with_helper(source);
        assert!(compilation.output.is_none());
        let failures = compilation
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code != DiagnosticCode::UnsupportedSyntax)
            .collect::<Vec<_>>();
        assert_eq!(failures.len(), 1, "{failures:#?}");
        assert_eq!(failures[0].code, DiagnosticCode::TypeMismatch);
        assert_eq!(
            &source[failures[0].span.start..failures[0].span.end],
            expected_span,
            "{failures:#?}"
        );
    }
}

#[test]
fn omitted_derived_constructor_reuses_local_and_imported_base_signatures() {
    for accepted in [
        compile_with_helper(include_str!(
            "fixtures/typescript_oracle/class-inherited-constructor-valid/main.ts"
        )),
        compile_class_module_pair(
            include_str!(
                "fixtures/typescript_oracle/class-inherited-imported-constructor-valid/main.ts"
            ),
            include_str!(
                "fixtures/typescript_oracle/class-inherited-imported-constructor-valid/box.ts"
            ),
        ),
    ] {
        assert!(accepted.output.is_some());
        assert!(
            accepted.diagnostics.is_empty(),
            "{:#?}",
            accepted.diagnostics
        );
    }
    for (source, expected_span) in [
        (
            include_str!(
                "fixtures/typescript_oracle/class-inherited-constructor-argument-error/main.ts"
            ),
            "new Child(true)",
        ),
        (
            include_str!(
                "fixtures/typescript_oracle/class-inherited-constructor-arity-error/main.ts"
            ),
            "new Child()",
        ),
        (
            include_str!(
                "fixtures/typescript_oracle/class-inherited-constructor-result-error/main.ts"
            ),
            "const wrong: { missing(): void } = new Child(1);",
        ),
        (
            include_str!(
                "fixtures/typescript_oracle/class-derived-own-constructor-argument-error/main.ts"
            ),
            "new Child(2)",
        ),
    ] {
        let compilation = compile_with_helper(source);
        assert!(compilation.output.is_none());
        let failures = compilation
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code != DiagnosticCode::UnsupportedSyntax)
            .collect::<Vec<_>>();
        assert_eq!(failures.len(), 1, "{failures:#?}");
        assert_eq!(failures[0].code, DiagnosticCode::TypeMismatch);
        assert_eq!(
            &source[failures[0].span.start..failures[0].span.end],
            expected_span,
            "{failures:#?}"
        );
    }
}

#[test]
fn exported_local_derived_classes_retain_inherited_surfaces_across_imports() {
    for (main, box_module) in [
        (
            include_str!("fixtures/typescript_oracle/class-export-inherited-direct/main.ts"),
            include_str!("fixtures/typescript_oracle/class-export-inherited-direct/box.ts"),
        ),
        (
            include_str!("fixtures/typescript_oracle/class-export-inherited-alias/main.ts"),
            include_str!("fixtures/typescript_oracle/class-export-inherited-alias/box.ts"),
        ),
    ] {
        let accepted = compile_class_module_pair(main, box_module);
        assert!(accepted.output.is_some());
        assert!(
            accepted.diagnostics.is_empty(),
            "{:#?}",
            accepted.diagnostics
        );
    }
    let box_module =
        include_str!("fixtures/typescript_oracle/class-export-inherited-direct/box.ts");
    for (main, expected_span) in [
        (
            include_str!(
                "fixtures/typescript_oracle/class-export-inherited-constructor-error/main.ts"
            ),
            "new Child('wrong')",
        ),
        (
            include_str!(
                "fixtures/typescript_oracle/class-export-inherited-instance-error/main.ts"
            ),
            "child.label('wrong')",
        ),
        (
            include_str!("fixtures/typescript_oracle/class-export-inherited-static-error/main.ts"),
            "Child.parse('wrong')",
        ),
        (
            include_str!("fixtures/typescript_oracle/class-export-inherited-result-error/main.ts"),
            "const wrong: string = Child.parse(1);",
        ),
    ] {
        let compilation = compile_class_module_pair(main, box_module);
        assert!(compilation.output.is_none());
        let failures = compilation
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code != DiagnosticCode::UnsupportedSyntax)
            .collect::<Vec<_>>();
        assert_eq!(failures.len(), 1, "{failures:#?}");
        assert_eq!(failures[0].code, DiagnosticCode::TypeMismatch);
        assert_eq!(failures[0].span.module, ENTRY);
        assert_eq!(
            &main[failures[0].span.start..failures[0].span.end],
            expected_span,
            "{failures:#?}"
        );
    }
}

#[test]
fn exported_imported_base_derived_classes_retain_inherited_surfaces() {
    let base = include_str!("fixtures/typescript_oracle/class-imported-base-derived-valid/base.ts");
    let direct_box =
        include_str!("fixtures/typescript_oracle/class-imported-base-derived-valid/box.ts");
    for (main, box_module) in [
        (
            include_str!("fixtures/typescript_oracle/class-imported-base-derived-valid/main.ts"),
            direct_box,
        ),
        (
            include_str!("fixtures/typescript_oracle/class-imported-base-derived-alias/main.ts"),
            include_str!("fixtures/typescript_oracle/class-imported-base-derived-alias/box.ts"),
        ),
    ] {
        let accepted = compile_class_module_triplet(main, box_module, base);
        assert!(accepted.output.is_some());
        assert!(
            accepted.diagnostics.is_empty(),
            "{:#?}",
            accepted.diagnostics
        );
    }
    let chained = compile(
        ENTRY,
        &MapLoader::from([
            ModuleSource::new(
                ENTRY,
                include_str!(
                    "fixtures/typescript_oracle/class-imported-base-derived-chain/main.ts"
                ),
            ),
            ModuleSource::new(
                "memory:///box.ts",
                include_str!("fixtures/typescript_oracle/class-imported-base-derived-chain/box.ts"),
            ),
            ModuleSource::new(
                "memory:///middle.ts",
                include_str!(
                    "fixtures/typescript_oracle/class-imported-base-derived-chain/middle.ts"
                ),
            ),
            ModuleSource::new("memory:///base.ts", base),
        ]),
        CompilerOptions::default(),
    );
    assert!(chained.output.is_some());
    assert!(chained.diagnostics.is_empty(), "{:#?}", chained.diagnostics);
    for (main, expected_span) in [
        (
            include_str!(
                "fixtures/typescript_oracle/class-imported-base-derived-constructor-error/main.ts"
            ),
            "new Child('wrong')",
        ),
        (
            include_str!(
                "fixtures/typescript_oracle/class-imported-base-derived-instance-error/main.ts"
            ),
            "child.label('wrong')",
        ),
        (
            include_str!(
                "fixtures/typescript_oracle/class-imported-base-derived-static-error/main.ts"
            ),
            "Child.parse('wrong')",
        ),
        (
            include_str!(
                "fixtures/typescript_oracle/class-imported-base-derived-result-error/main.ts"
            ),
            "const wrong: string = Child.parse(1);",
        ),
    ] {
        let compilation = compile_class_module_triplet(main, direct_box, base);
        assert!(compilation.output.is_none());
        let failures = compilation
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code != DiagnosticCode::UnsupportedSyntax)
            .collect::<Vec<_>>();
        assert_eq!(failures.len(), 1, "{failures:#?}");
        assert_eq!(failures[0].code, DiagnosticCode::TypeMismatch);
        assert_eq!(failures[0].span.module, ENTRY);
        assert_eq!(
            &main[failures[0].span.start..failures[0].span.end],
            expected_span,
            "{failures:#?}"
        );
    }
}

#[test]
fn imported_base_derived_instance_surfaces_survive_type_reexport_chains() {
    let accepted = compile_imported_base_type_reexport(include_str!(
        "fixtures/typescript_oracle/class-imported-base-type-reexport/valid.ts"
    ));
    assert!(accepted.output.is_some());
    assert!(
        accepted.diagnostics.is_empty(),
        "{:#?}",
        accepted.diagnostics
    );
    for (main, expected_span) in [
        (
            include_str!(
                "fixtures/typescript_oracle/class-imported-base-type-reexport/argument-error.ts"
            ),
            "child.label('wrong')",
        ),
        (
            include_str!(
                "fixtures/typescript_oracle/class-imported-base-type-reexport/result-error.ts"
            ),
            "const wrong: number = child.label(1);",
        ),
    ] {
        let compilation = compile_imported_base_type_reexport(main);
        assert!(compilation.output.is_none());
        let failures = compilation
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code != DiagnosticCode::UnsupportedSyntax)
            .collect::<Vec<_>>();
        assert_eq!(failures.len(), 1, "{failures:#?}");
        assert_eq!(failures[0].code, DiagnosticCode::TypeMismatch);
        assert_eq!(failures[0].span.module, ENTRY);
        assert_eq!(
            &main[failures[0].span.start..failures[0].span.end],
            expected_span,
            "{failures:#?}"
        );
    }
}

#[test]
fn local_class_method_overrides_check_instance_and_static_signatures() {
    let valid = include_str!("fixtures/typescript_oracle/class-override-local-valid/main.ts");
    let accepted = compile(
        ENTRY,
        &MapLoader::from([ModuleSource::new(ENTRY, valid)]),
        CompilerOptions::default(),
    );
    assert!(accepted.output.is_some());
    assert!(
        accepted.diagnostics.is_empty(),
        "{:#?}",
        accepted.diagnostics
    );
    for (source, member) in [
        (
            include_str!(
                "fixtures/typescript_oracle/class-override-local-instance-parameter-error/main.ts"
            ),
            "read(value: string): number { return 1; }",
        ),
        (
            include_str!(
                "fixtures/typescript_oracle/class-override-local-instance-result-error/main.ts"
            ),
            "read(value: number): string { return 'wrong'; }",
        ),
        (
            include_str!(
                "fixtures/typescript_oracle/class-override-local-static-parameter-error/main.ts"
            ),
            "static parse(value: string): number { return 1; }",
        ),
        (
            include_str!(
                "fixtures/typescript_oracle/class-override-local-static-result-error/main.ts"
            ),
            "static parse(value: number): string { return 'wrong'; }",
        ),
    ] {
        let compilation = compile(
            ENTRY,
            &MapLoader::from([ModuleSource::new(ENTRY, source)]),
            CompilerOptions::default(),
        );
        assert!(compilation.output.is_none());
        let failures = compilation
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code != DiagnosticCode::UnsupportedSyntax)
            .collect::<Vec<_>>();
        assert_eq!(failures.len(), 1, "{failures:#?}");
        assert_eq!(failures[0].code, DiagnosticCode::TypeMismatch);
        assert_eq!(failures[0].span.module, ENTRY);
        assert_eq!(
            &source[failures[0].span.start..failures[0].span.end],
            member,
            "{failures:#?}"
        );
    }
}

#[test]
fn ancestor_class_method_overrides_find_inherited_instance_and_static_signatures() {
    let valid = include_str!("fixtures/typescript_oracle/class-override-ancestor-valid/main.ts");
    let accepted = compile(
        ENTRY,
        &MapLoader::from([ModuleSource::new(ENTRY, valid)]),
        CompilerOptions::default(),
    );
    assert!(accepted.output.is_some());
    assert!(
        accepted.diagnostics.is_empty(),
        "{:#?}",
        accepted.diagnostics
    );
    for (source, member) in [
        (
            include_str!(
                "fixtures/typescript_oracle/class-override-ancestor-nearest-error/main.ts"
            ),
            "read(value: string): number { return 1; }",
        ),
        (
            include_str!(
                "fixtures/typescript_oracle/class-override-ancestor-instance-parameter-error/main.ts"
            ),
            "read(value: string): number { return 1; }",
        ),
        (
            include_str!(
                "fixtures/typescript_oracle/class-override-ancestor-instance-result-error/main.ts"
            ),
            "read(value: number): string { return 'wrong'; }",
        ),
        (
            include_str!(
                "fixtures/typescript_oracle/class-override-ancestor-static-parameter-error/main.ts"
            ),
            "static parse(value: string): number { return 1; }",
        ),
        (
            include_str!(
                "fixtures/typescript_oracle/class-override-ancestor-static-result-error/main.ts"
            ),
            "static parse(value: number): string { return 'wrong'; }",
        ),
    ] {
        let compilation = compile(
            ENTRY,
            &MapLoader::from([ModuleSource::new(ENTRY, source)]),
            CompilerOptions::default(),
        );
        assert!(compilation.output.is_none());
        let failures = compilation
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code != DiagnosticCode::UnsupportedSyntax)
            .collect::<Vec<_>>();
        assert_eq!(failures.len(), 1, "{failures:#?}");
        assert_eq!(failures[0].code, DiagnosticCode::TypeMismatch);
        assert_eq!(failures[0].span.module, ENTRY);
        assert_eq!(
            &source[failures[0].span.start..failures[0].span.end],
            member,
            "{failures:#?}"
        );
    }
}

#[test]
fn imported_class_method_overrides_check_direct_and_transitive_base_surfaces() {
    for main in [
        include_str!("fixtures/typescript_oracle/class-override-imported/direct-valid.ts"),
        include_str!("fixtures/typescript_oracle/class-override-imported/transitive-valid.ts"),
    ] {
        let accepted = compile_imported_base_override(main);
        assert!(accepted.output.is_some());
        assert!(
            accepted.diagnostics.is_empty(),
            "{:#?}",
            accepted.diagnostics
        );
    }
    for (main, member) in [
        (
            include_str!("fixtures/typescript_oracle/class-override-imported/required-extra-error.ts"),
            "read(value: number, label: string): number { return value; }",
        ),
        (
            include_str!("fixtures/typescript_oracle/class-override-imported/alias-static-error.ts"),
            "static parse(value: string): number { return 1; }",
        ),
        (
            include_str!("fixtures/typescript_oracle/class-override-imported/direct-instance-error.ts"),
            "read(value: string): number { return 1; }",
        ),
        (
            include_str!("fixtures/typescript_oracle/class-override-imported/direct-static-error.ts"),
            "static parse(value: string): number { return 1; }",
        ),
        (
            include_str!("fixtures/typescript_oracle/class-override-imported/transitive-instance-error.ts"),
            "read(value: string): number { return 1; }",
        ),
        (
            include_str!("fixtures/typescript_oracle/class-override-imported/transitive-static-error.ts"),
            "static parse(value: string): number { return 1; }",
        ),
        (
            include_str!("fixtures/typescript_oracle/class-override-imported/transitive-instance-result-error.ts"),
            "read(value: number): string { return 'wrong'; }",
        ),
        (
            include_str!("fixtures/typescript_oracle/class-override-imported/direct-static-result-error.ts"),
            "static parse(value: number): string { return 'wrong'; }",
        ),
    ] {
        let compilation = compile_imported_base_override(main);
        assert!(compilation.output.is_none());
        let failures = compilation
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code != DiagnosticCode::UnsupportedSyntax)
            .collect::<Vec<_>>();
        assert_eq!(failures.len(), 1, "{failures:#?}");
        assert_eq!(failures[0].code, DiagnosticCode::TypeMismatch);
        assert_eq!(failures[0].span.module, ENTRY);
        assert_eq!(
            &main[failures[0].span.start..failures[0].span.end],
            member,
            "{failures:#?}"
        );
    }
}

#[test]
fn class_method_overrides_check_optional_and_differing_required_arities() {
    for valid in [
        include_str!("fixtures/typescript_oracle/class-override-arity-valid/main.ts"),
        include_str!(
            "fixtures/typescript_oracle/class-override-arity-instance-optional-required/main.ts"
        ),
        include_str!(
            "fixtures/typescript_oracle/class-override-arity-static-optional-required/main.ts"
        ),
    ] {
        let accepted = compile(
            ENTRY,
            &MapLoader::from([ModuleSource::new(ENTRY, valid)]),
            CompilerOptions::default(),
        );
        assert!(accepted.output.is_some());
        assert!(
            accepted.diagnostics.is_empty(),
            "{:#?}",
            accepted.diagnostics
        );
    }
    for (source, member) in [
        (
            include_str!(
                "fixtures/typescript_oracle/class-override-arity-instance-extra-required/main.ts"
            ),
            "read(value: number, label: string): number { return value; }",
        ),
        (
            include_str!(
                "fixtures/typescript_oracle/class-override-arity-instance-optional-type-error/main.ts"
            ),
            "read(value: number, label?: number): number { return value; }",
        ),
        (
            include_str!(
                "fixtures/typescript_oracle/class-override-arity-static-extra-required/main.ts"
            ),
            "static parse(value: number, label: string): number { return value; }",
        ),
        (
            include_str!(
                "fixtures/typescript_oracle/class-override-arity-static-optional-type-error/main.ts"
            ),
            "static parse(value: number, label?: number): number { return value; }",
        ),
    ] {
        let compilation = compile(
            ENTRY,
            &MapLoader::from([ModuleSource::new(ENTRY, source)]),
            CompilerOptions::default(),
        );
        assert!(compilation.output.is_none());
        let failures = compilation
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code != DiagnosticCode::UnsupportedSyntax)
            .collect::<Vec<_>>();
        assert_eq!(failures.len(), 1, "{failures:#?}");
        assert_eq!(failures[0].code, DiagnosticCode::TypeMismatch);
        assert_eq!(failures[0].span.module, ENTRY);
        assert_eq!(
            &source[failures[0].span.start..failures[0].span.end],
            member,
            "{failures:#?}"
        );
    }
}

#[test]
fn class_method_overrides_check_matching_prefix_array_rest_signatures() {
    let valid = include_str!("fixtures/typescript_oracle/class-override-array-rest-valid/main.ts");
    let accepted = compile(
        ENTRY,
        &MapLoader::from([ModuleSource::new(ENTRY, valid)]),
        CompilerOptions::default(),
    );
    assert!(accepted.output.is_some());
    assert!(
        accepted.diagnostics.is_empty(),
        "{:#?}",
        accepted.diagnostics
    );
    for (source, member) in [
        (
            include_str!(
                "fixtures/typescript_oracle/class-override-array-rest-instance-element-error/main.ts"
            ),
            "read(prefix: number, ...values: string[]): number { return prefix; }",
        ),
        (
            include_str!(
                "fixtures/typescript_oracle/class-override-array-rest-instance-result-error/main.ts"
            ),
            "read(prefix: number, ...values: number[]): string { return 'wrong'; }",
        ),
        (
            include_str!(
                "fixtures/typescript_oracle/class-override-array-rest-static-element-error/main.ts"
            ),
            "static parse(prefix: number, ...values: string[]): number { return prefix; }",
        ),
        (
            include_str!(
                "fixtures/typescript_oracle/class-override-array-rest-static-result-error/main.ts"
            ),
            "static parse(prefix: number, ...values: number[]): string { return 'wrong'; }",
        ),
        (
            include_str!(
                "fixtures/typescript_oracle/class-override-array-rest-prefix-error/main.ts"
            ),
            "read(prefix: string, ...values: string[]): number { return 1; }",
        ),
    ] {
        let compilation = compile(
            ENTRY,
            &MapLoader::from([ModuleSource::new(ENTRY, source)]),
            CompilerOptions::default(),
        );
        assert!(compilation.output.is_none());
        let failures = compilation
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code != DiagnosticCode::UnsupportedSyntax)
            .collect::<Vec<_>>();
        assert_eq!(failures.len(), 1, "{failures:#?}");
        assert_eq!(failures[0].code, DiagnosticCode::TypeMismatch);
        assert_eq!(failures[0].span.module, ENTRY);
        assert_eq!(
            &source[failures[0].span.start..failures[0].span.end],
            member,
            "{failures:#?}"
        );
    }
}

#[test]
fn derived_array_rest_overrides_cover_remaining_fixed_base_parameters() {
    let valid =
        include_str!("fixtures/typescript_oracle/class-override-derived-rest-valid/main.ts");
    let accepted = compile(
        ENTRY,
        &MapLoader::from([ModuleSource::new(ENTRY, valid)]),
        CompilerOptions::default(),
    );
    assert!(accepted.output.is_some());
    assert!(
        accepted.diagnostics.is_empty(),
        "{:#?}",
        accepted.diagnostics
    );
    for (source, member) in [
        (
            include_str!(
                "fixtures/typescript_oracle/class-override-derived-rest-instance-type-error/main.ts"
            ),
            "read(value: number, ...labels: number[]): number { return value; }",
        ),
        (
            include_str!(
                "fixtures/typescript_oracle/class-override-derived-rest-static-type-error/main.ts"
            ),
            "static parse(value: number, ...labels: number[]): number { return value; }",
        ),
        (
            include_str!(
                "fixtures/typescript_oracle/class-override-derived-rest-later-type-error/main.ts"
            ),
            "read(value: number, ...labels: string[]): number { return value; }",
        ),
    ] {
        let compilation = compile(
            ENTRY,
            &MapLoader::from([ModuleSource::new(ENTRY, source)]),
            CompilerOptions::default(),
        );
        assert!(compilation.output.is_none());
        let failures = compilation
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code != DiagnosticCode::UnsupportedSyntax)
            .collect::<Vec<_>>();
        assert_eq!(failures.len(), 1, "{failures:#?}");
        assert_eq!(failures[0].code, DiagnosticCode::TypeMismatch);
        assert_eq!(failures[0].span.module, ENTRY);
        assert_eq!(
            &source[failures[0].span.start..failures[0].span.end],
            member,
            "{failures:#?}"
        );
    }
}

#[test]
fn fixed_derived_overrides_compare_base_array_rest_positions() {
    for valid in [
        include_str!("fixtures/typescript_oracle/class-override-base-rest-valid/main.ts"),
        include_str!("fixtures/typescript_oracle/class-override-base-rest-required/main.ts"),
    ] {
        let accepted = compile(
            ENTRY,
            &MapLoader::from([ModuleSource::new(ENTRY, valid)]),
            CompilerOptions::default(),
        );
        assert!(accepted.output.is_some());
        assert!(
            accepted.diagnostics.is_empty(),
            "{:#?}",
            accepted.diagnostics
        );
    }
    for (source, member) in [
        (
            include_str!(
                "fixtures/typescript_oracle/class-override-base-rest-extra-required/main.ts"
            ),
            "read(prefix: number, label: string, active: boolean): number { return prefix; }",
        ),
        (
            include_str!(
                "fixtures/typescript_oracle/class-override-base-rest-instance-type-error/main.ts"
            ),
            "read(prefix: number, label?: number): number { return prefix; }",
        ),
        (
            include_str!(
                "fixtures/typescript_oracle/class-override-base-rest-later-type-error/main.ts"
            ),
            "read(prefix: number, label: string, active?: boolean): number { return prefix; }",
        ),
        (
            include_str!(
                "fixtures/typescript_oracle/class-override-base-rest-static-type-error/main.ts"
            ),
            "static parse(prefix: number, label?: number): number { return prefix; }",
        ),
    ] {
        let compilation = compile(
            ENTRY,
            &MapLoader::from([ModuleSource::new(ENTRY, source)]),
            CompilerOptions::default(),
        );
        assert!(compilation.output.is_none());
        let failures = compilation
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code != DiagnosticCode::UnsupportedSyntax)
            .collect::<Vec<_>>();
        assert_eq!(failures.len(), 1, "{failures:#?}");
        assert_eq!(failures[0].code, DiagnosticCode::TypeMismatch);
        assert_eq!(failures[0].span.module, ENTRY);
        assert_eq!(
            &source[failures[0].span.start..failures[0].span.end],
            member,
            "{failures:#?}"
        );
    }
}

#[test]
fn shifted_array_rest_overrides_align_fixed_and_element_types() {
    for valid in [
        include_str!("fixtures/typescript_oracle/class-override-shifted-rest-valid/main.ts"),
        include_str!(
            "fixtures/typescript_oracle/class-override-shifted-rest-extra-required/main.ts"
        ),
    ] {
        let accepted = compile(
            ENTRY,
            &MapLoader::from([ModuleSource::new(ENTRY, valid)]),
            CompilerOptions::default(),
        );
        assert!(accepted.output.is_some());
        assert!(
            accepted.diagnostics.is_empty(),
            "{:#?}",
            accepted.diagnostics
        );
    }
    for (source, member) in [
        (
            include_str!("fixtures/typescript_oracle/class-override-shifted-rest-prefix-error/main.ts"),
            "read(prefix: number, ...labels: number[]): number { return prefix; }",
        ),
        (
            include_str!("fixtures/typescript_oracle/class-override-shifted-rest-tail-error/main.ts"),
            "read(prefix: number, ...labels: string[]): number { return prefix; }",
        ),
        (
            include_str!("fixtures/typescript_oracle/class-override-shifted-rest-static-prefix-error/main.ts"),
            "static parse(prefix: number, label: number, ...labels: string[]): number { return prefix; }",
        ),
    ] {
        let compilation = compile(
            ENTRY,
            &MapLoader::from([ModuleSource::new(ENTRY, source)]),
            CompilerOptions::default(),
        );
        assert!(compilation.output.is_none());
        let failures = compilation
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code != DiagnosticCode::UnsupportedSyntax)
            .collect::<Vec<_>>();
        assert_eq!(failures.len(), 1, "{failures:#?}");
        assert_eq!(failures[0].code, DiagnosticCode::TypeMismatch);
        assert_eq!(failures[0].span.module, ENTRY);
        assert_eq!(
            &source[failures[0].span.start..failures[0].span.end],
            member,
            "{failures:#?}"
        );
    }
}

#[test]
fn class_parameters_accept_fixed_tuple_rest_annotations() {
    let valid = include_str!("fixtures/typescript_oracle/class-tuple-rest-valid/main.ts");
    let accepted = compile(
        ENTRY,
        &MapLoader::from([ModuleSource::new(ENTRY, valid)]),
        CompilerOptions::default(),
    );
    assert!(accepted.output.is_some());
    assert!(
        accepted.diagnostics.is_empty(),
        "{:#?}",
        accepted.diagnostics
    );

    for (source, code, span) in [
        (
            include_str!("fixtures/typescript_oracle/class-tuple-rest-primitive-error/main.ts"),
            DiagnosticCode::TypeMismatch,
            "...parts: number",
        ),
        (
            include_str!("fixtures/typescript_oracle/class-tuple-rest-unknown-element/main.ts"),
            DiagnosticCode::UnknownType,
            "...parts: [Missing, string]",
        ),
    ] {
        let compilation = compile(
            ENTRY,
            &MapLoader::from([ModuleSource::new(ENTRY, source)]),
            CompilerOptions::default(),
        );
        assert!(compilation.output.is_none());
        let failures = compilation
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code != DiagnosticCode::UnsupportedSyntax)
            .collect::<Vec<_>>();
        assert_eq!(failures.len(), 1, "{failures:#?}");
        assert_eq!(failures[0].code, code);
        assert_eq!(failures[0].span.module, ENTRY);
        assert_eq!(&source[failures[0].span.start..failures[0].span.end], span);
    }
}

#[test]
fn derived_tuple_rest_overrides_align_expanded_positions() {
    let valid =
        include_str!("fixtures/typescript_oracle/class-override-derived-tuple-valid/main.ts");
    let accepted = compile(
        ENTRY,
        &MapLoader::from([ModuleSource::new(ENTRY, valid)]),
        CompilerOptions::default(),
    );
    assert!(accepted.output.is_some());
    assert!(
        accepted.diagnostics.is_empty(),
        "{:#?}",
        accepted.diagnostics
    );

    for (source, member) in [
        (
            include_str!(
                "fixtures/typescript_oracle/class-override-derived-tuple-fixed-type-error/main.ts"
            ),
            "read(a: number, ...parts: [number]): number { return a; }",
        ),
        (
            include_str!(
                "fixtures/typescript_oracle/class-override-derived-tuple-fixed-arity-error/main.ts"
            ),
            "read(a: number, ...parts: [string, string]): number { return a; }",
        ),
        (
            include_str!(
                "fixtures/typescript_oracle/class-override-derived-tuple-array-type-error/main.ts"
            ),
            "static parse(a: number, ...parts: [string, number]): number { return a; }",
        ),
    ] {
        let compilation = compile(
            ENTRY,
            &MapLoader::from([ModuleSource::new(ENTRY, source)]),
            CompilerOptions::default(),
        );
        assert!(compilation.output.is_none());
        let failures = compilation
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code != DiagnosticCode::UnsupportedSyntax)
            .collect::<Vec<_>>();
        assert_eq!(failures.len(), 1, "{failures:#?}");
        assert_eq!(failures[0].code, DiagnosticCode::TypeMismatch);
        assert_eq!(failures[0].span.module, ENTRY);
        assert_eq!(
            &source[failures[0].span.start..failures[0].span.end],
            member,
            "{failures:#?}"
        );
    }
}

#[test]
fn inherited_tuple_rest_overrides_align_fixed_and_array_positions() {
    let valid = include_str!("fixtures/typescript_oracle/class-override-base-tuple-valid/main.ts");
    let accepted = compile(
        ENTRY,
        &MapLoader::from([ModuleSource::new(ENTRY, valid)]),
        CompilerOptions::default(),
    );
    assert!(accepted.output.is_some());
    assert!(
        accepted.diagnostics.is_empty(),
        "{:#?}",
        accepted.diagnostics
    );

    for (source, member) in [
        (
            include_str!(
                "fixtures/typescript_oracle/class-override-base-tuple-fixed-type-error/main.ts"
            ),
            "read(a: number, b: number): number { return a; }",
        ),
        (
            include_str!(
                "fixtures/typescript_oracle/class-override-base-tuple-fixed-arity-error/main.ts"
            ),
            "read(a: number, b: string, c: boolean): number { return a; }",
        ),
        (
            include_str!(
                "fixtures/typescript_oracle/class-override-base-tuple-array-type-error/main.ts"
            ),
            "static parse(...parts: number[]): number { return 1; }",
        ),
    ] {
        let compilation = compile(
            ENTRY,
            &MapLoader::from([ModuleSource::new(ENTRY, source)]),
            CompilerOptions::default(),
        );
        assert!(compilation.output.is_none());
        let failures = compilation
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code != DiagnosticCode::UnsupportedSyntax)
            .collect::<Vec<_>>();
        assert_eq!(failures.len(), 1, "{failures:#?}");
        assert_eq!(failures[0].code, DiagnosticCode::TypeMismatch);
        assert_eq!(failures[0].span.module, ENTRY);
        assert_eq!(
            &source[failures[0].span.start..failures[0].span.end],
            member,
            "{failures:#?}"
        );
    }
}

#[test]
fn both_tuple_rest_overrides_compare_expanded_positions() {
    let valid = include_str!("fixtures/typescript_oracle/class-override-both-tuple-valid/main.ts");
    let accepted = compile(
        ENTRY,
        &MapLoader::from([ModuleSource::new(ENTRY, valid)]),
        CompilerOptions::default(),
    );
    assert!(accepted.output.is_some());
    assert!(
        accepted.diagnostics.is_empty(),
        "{:#?}",
        accepted.diagnostics
    );

    for (source, member) in [
        (
            include_str!("fixtures/typescript_oracle/class-override-both-tuple-type-error/main.ts"),
            "read(...parts: [number, number, boolean]): number { return parts[0]; }",
        ),
        (
            include_str!(
                "fixtures/typescript_oracle/class-override-both-tuple-arity-error/main.ts"
            ),
            "read(a: number, b: string, ...parts: [boolean]): number { return a; }",
        ),
        (
            include_str!(
                "fixtures/typescript_oracle/class-override-both-tuple-static-type-error/main.ts"
            ),
            "static parse(...parts: [number, string, number]): number { return parts[0]; }",
        ),
    ] {
        let compilation = compile(
            ENTRY,
            &MapLoader::from([ModuleSource::new(ENTRY, source)]),
            CompilerOptions::default(),
        );
        assert!(compilation.output.is_none());
        let failures = compilation
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code != DiagnosticCode::UnsupportedSyntax)
            .collect::<Vec<_>>();
        assert_eq!(failures.len(), 1, "{failures:#?}");
        assert_eq!(failures[0].code, DiagnosticCode::TypeMismatch);
        assert_eq!(failures[0].span.module, ENTRY);
        assert_eq!(
            &source[failures[0].span.start..failures[0].span.end],
            member,
            "{failures:#?}"
        );
    }
}

#[test]
fn optional_tuple_rest_overrides_compare_declared_positions() {
    let valid =
        include_str!("fixtures/typescript_oracle/class-override-optional-tuple-valid/main.ts");
    let accepted = compile(
        ENTRY,
        &MapLoader::from([ModuleSource::new(ENTRY, valid)]),
        CompilerOptions::default(),
    );
    assert!(accepted.output.is_some());
    assert!(
        accepted.diagnostics.is_empty(),
        "{:#?}",
        accepted.diagnostics
    );

    for (source, member) in [
        (
            include_str!("fixtures/typescript_oracle/class-override-optional-tuple-element-error/main.ts"),
            "read(...parts: [number, number?]): number { return 1; }",
        ),
        (
            include_str!("fixtures/typescript_oracle/class-override-optional-tuple-arity-error/main.ts"),
            "read(a: number, b: string, c: boolean): number { return a; }",
        ),
        (
            include_str!("fixtures/typescript_oracle/class-override-optional-tuple-static-element-error/main.ts"),
            "static parse(...parts: [number, number?]): number { return 1; }",
        ),
    ] {
        let compilation = compile(
            ENTRY,
            &MapLoader::from([ModuleSource::new(ENTRY, source)]),
            CompilerOptions::default(),
        );
        assert!(compilation.output.is_none());
        let failures = compilation
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code != DiagnosticCode::UnsupportedSyntax)
            .collect::<Vec<_>>();
        assert_eq!(failures.len(), 1, "{failures:#?}");
        assert_eq!(failures[0].code, DiagnosticCode::TypeMismatch);
        assert_eq!(failures[0].span.module, ENTRY);
        assert_eq!(
            &source[failures[0].span.start..failures[0].span.end],
            member,
            "{failures:#?}"
        );
    }
}

#[test]
fn labeled_tuple_rest_overrides_compare_positions_not_names() {
    let valid =
        include_str!("fixtures/typescript_oracle/class-override-labeled-tuple-valid/main.ts");
    let accepted = compile(
        ENTRY,
        &MapLoader::from([ModuleSource::new(ENTRY, valid)]),
        CompilerOptions::default(),
    );
    assert!(accepted.output.is_some());
    assert!(
        accepted.diagnostics.is_empty(),
        "{:#?}",
        accepted.diagnostics
    );

    for (source, member) in [
        (
            include_str!(
                "fixtures/typescript_oracle/class-override-labeled-tuple-instance-error/main.ts"
            ),
            "read(...parts: [left: number, right: number]): number { return 1; }",
        ),
        (
            include_str!(
                "fixtures/typescript_oracle/class-override-labeled-tuple-static-error/main.ts"
            ),
            "static parse(...parts: [key: number, text?: number]): number { return 1; }",
        ),
        (
            include_str!(
                "fixtures/typescript_oracle/class-override-labeled-tuple-arity-error/main.ts"
            ),
            "read(...parts: [left: number, right: string, extra: boolean]): number { return 1; }",
        ),
    ] {
        let compilation = compile(
            ENTRY,
            &MapLoader::from([ModuleSource::new(ENTRY, source)]),
            CompilerOptions::default(),
        );
        assert!(compilation.output.is_none());
        let failures = compilation
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code != DiagnosticCode::UnsupportedSyntax)
            .collect::<Vec<_>>();
        assert_eq!(failures.len(), 1, "{failures:#?}");
        assert_eq!(failures[0].code, DiagnosticCode::TypeMismatch);
        assert_eq!(failures[0].span.module, ENTRY);
        assert_eq!(
            &source[failures[0].span.start..failures[0].span.end],
            member,
            "{failures:#?}"
        );
    }
}

#[test]
fn derived_trailing_tuple_rest_overrides_compare_repeated_tail() {
    let valid =
        include_str!("fixtures/typescript_oracle/class-override-derived-tail-valid/main.ts");
    let accepted = compile(
        ENTRY,
        &MapLoader::from([ModuleSource::new(ENTRY, valid)]),
        CompilerOptions::default(),
    );
    assert!(accepted.output.is_some());
    assert!(
        accepted.diagnostics.is_empty(),
        "{:#?}",
        accepted.diagnostics
    );

    for (source, member) in [
        (
            include_str!("fixtures/typescript_oracle/class-override-derived-tail-fixed-error/main.ts"),
            "read(...parts: [first: number, ...tail: string[]]): number { return parts[0]; }",
        ),
        (
            include_str!("fixtures/typescript_oracle/class-override-derived-tail-arity-error/main.ts"),
            "read(...parts: [first: number, second: string, ...tail: boolean[]]): number { return parts[0]; }",
        ),
        (
            include_str!("fixtures/typescript_oracle/class-override-derived-tail-array-error/main.ts"),
            "static parse(...parts: [first: number, ...tail: string[]]): number { return parts[0]; }",
        ),
    ] {
        let compilation = compile(
            ENTRY,
            &MapLoader::from([ModuleSource::new(ENTRY, source)]),
            CompilerOptions::default(),
        );
        assert!(compilation.output.is_none());
        let failures = compilation
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code != DiagnosticCode::UnsupportedSyntax)
            .collect::<Vec<_>>();
        assert_eq!(failures.len(), 1, "{failures:#?}");
        assert_eq!(failures[0].code, DiagnosticCode::TypeMismatch);
        assert_eq!(failures[0].span.module, ENTRY);
        assert_eq!(
            &source[failures[0].span.start..failures[0].span.end],
            member,
            "{failures:#?}"
        );
    }
}

#[test]
fn derived_middle_tuple_rest_overrides_align_required_suffixes() {
    let valid =
        include_str!("fixtures/typescript_oracle/class-override-derived-middle-valid/main.ts");
    let accepted = compile(
        ENTRY,
        &MapLoader::from([ModuleSource::new(ENTRY, valid)]),
        CompilerOptions::default(),
    );
    assert!(accepted.output.is_some());
    assert!(
        accepted.diagnostics.is_empty(),
        "{:#?}",
        accepted.diagnostics
    );

    for (source, member) in [
        (
            include_str!("fixtures/typescript_oracle/class-override-derived-middle-suffix-error/main.ts"),
            "read(...parts: [head: number, ...middle: string[], done: number]): number { return 1; }",
        ),
        (
            include_str!("fixtures/typescript_oracle/class-override-derived-middle-element-error/main.ts"),
            "read(...parts: [head: number, ...middle: boolean[], done: boolean]): number { return 1; }",
        ),
        (
            include_str!("fixtures/typescript_oracle/class-override-derived-middle-arity-error/main.ts"),
            "read(...parts: [head: number, ...middle: string[], done: boolean]): number { return 1; }",
        ),
        (
            include_str!("fixtures/typescript_oracle/class-override-derived-middle-static-error/main.ts"),
            "static parse(a: number, ...parts: [...middle: boolean[], end: string]): number { return a; }",
        ),
        (
            include_str!("fixtures/typescript_oracle/class-override-derived-middle-optional-error/main.ts"),
            "read(...parts: [head: number, ...middle: string[], done: boolean]): number { return 1; }",
        ),
    ] {
        let compilation = compile(
            ENTRY,
            &MapLoader::from([ModuleSource::new(ENTRY, source)]),
            CompilerOptions::default(),
        );
        assert!(compilation.output.is_none());
        let failures = compilation
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code != DiagnosticCode::UnsupportedSyntax)
            .collect::<Vec<_>>();
        assert_eq!(failures.len(), 1, "{failures:#?}");
        assert_eq!(failures[0].code, DiagnosticCode::TypeMismatch);
        assert_eq!(failures[0].span.module, ENTRY);
        assert_eq!(
            &source[failures[0].span.start..failures[0].span.end],
            member,
            "{failures:#?}"
        );
    }
}

#[test]
fn inherited_trailing_tuple_rest_overrides_compare_repeated_tail() {
    for valid in [
        include_str!("fixtures/typescript_oracle/class-override-base-tail-valid/main.ts"),
        include_str!("fixtures/typescript_oracle/class-override-base-tail-zero-valid/main.ts"),
    ] {
        let accepted = compile(
            ENTRY,
            &MapLoader::from([ModuleSource::new(ENTRY, valid)]),
            CompilerOptions::default(),
        );
        assert!(accepted.output.is_some());
        assert!(
            accepted.diagnostics.is_empty(),
            "{:#?}",
            accepted.diagnostics
        );
    }

    for (source, member) in [
        (
            include_str!("fixtures/typescript_oracle/class-override-base-tail-fixed-error/main.ts"),
            "read(a: number, b: boolean): number { return a; }",
        ),
        (
            include_str!("fixtures/typescript_oracle/class-override-base-tail-array-error/main.ts"),
            "static parse(a: number, ...tail: boolean[]): number { return a; }",
        ),
    ] {
        let compilation = compile(
            ENTRY,
            &MapLoader::from([ModuleSource::new(ENTRY, source)]),
            CompilerOptions::default(),
        );
        assert!(compilation.output.is_none());
        let failures = compilation
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code != DiagnosticCode::UnsupportedSyntax)
            .collect::<Vec<_>>();
        assert_eq!(failures.len(), 1, "{failures:#?}");
        assert_eq!(failures[0].code, DiagnosticCode::TypeMismatch);
        assert_eq!(failures[0].span.module, ENTRY);
        assert_eq!(
            &source[failures[0].span.start..failures[0].span.end],
            member,
            "{failures:#?}"
        );
    }
}

#[test]
fn inherited_middle_tuple_rest_overrides_align_required_suffixes() {
    let valid = include_str!("fixtures/typescript_oracle/class-override-base-middle-valid/main.ts");
    let accepted = compile(
        ENTRY,
        &MapLoader::from([ModuleSource::new(ENTRY, valid)]),
        CompilerOptions::default(),
    );
    assert!(accepted.output.is_some());
    assert!(
        accepted.diagnostics.is_empty(),
        "{:#?}",
        accepted.diagnostics
    );

    for (source, member) in [
        (
            include_str!(
                "fixtures/typescript_oracle/class-override-base-middle-suffix-error/main.ts"
            ),
            "read(a: number, b: string): number { return a; }",
        ),
        (
            include_str!(
                "fixtures/typescript_oracle/class-override-base-middle-element-error/main.ts"
            ),
            "read(a: number, b: boolean, c: boolean): number { return a; }",
        ),
        (
            include_str!(
                "fixtures/typescript_oracle/class-override-base-middle-arity-error/main.ts"
            ),
            "read(a: number): number { return a; }",
        ),
        (
            include_str!(
                "fixtures/typescript_oracle/class-override-base-middle-optional-error/main.ts"
            ),
            "read(a: number, b?: boolean): number { return a; }",
        ),
        (
            include_str!(
                "fixtures/typescript_oracle/class-override-base-middle-array-error/main.ts"
            ),
            "read(a: number, ...parts: string[]): number { return a; }",
        ),
        (
            include_str!(
                "fixtures/typescript_oracle/class-override-base-middle-shift-error/main.ts"
            ),
            "read(a: number, b: string, ...parts: string[]): number { return a; }",
        ),
        (
            include_str!(
                "fixtures/typescript_oracle/class-override-base-middle-static-error/main.ts"
            ),
            "static parse(a: number, ...parts: string[]): number { return a; }",
        ),
    ] {
        let compilation = compile(
            ENTRY,
            &MapLoader::from([ModuleSource::new(ENTRY, source)]),
            CompilerOptions::default(),
        );
        assert!(compilation.output.is_none());
        let failures = compilation
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code != DiagnosticCode::UnsupportedSyntax)
            .collect::<Vec<_>>();
        assert_eq!(failures.len(), 1, "{failures:#?}");
        assert_eq!(failures[0].code, DiagnosticCode::TypeMismatch);
        assert_eq!(failures[0].span.module, ENTRY);
        assert_eq!(
            &source[failures[0].span.start..failures[0].span.end],
            member,
            "{failures:#?}"
        );
    }
}

#[test]
fn both_trailing_tuple_rest_overrides_align_prefixes_and_tails() {
    let valid = include_str!("fixtures/typescript_oracle/class-override-both-tail-valid/main.ts");
    let accepted = compile(
        ENTRY,
        &MapLoader::from([ModuleSource::new(ENTRY, valid)]),
        CompilerOptions::default(),
    );
    assert!(accepted.output.is_some());
    assert!(
        accepted.diagnostics.is_empty(),
        "{:#?}",
        accepted.diagnostics
    );

    for (source, member) in [
        (
            include_str!(
                "fixtures/typescript_oracle/class-override-both-tail-element-error/main.ts"
            ),
            "read(...parts: [number, ...boolean[]]): number { return 1; }",
        ),
        (
            include_str!(
                "fixtures/typescript_oracle/class-override-both-tail-prefix-error/main.ts"
            ),
            "static parse(...parts: [number, boolean, ...boolean[]]): number { return parts[0]; }",
        ),
    ] {
        let compilation = compile(
            ENTRY,
            &MapLoader::from([ModuleSource::new(ENTRY, source)]),
            CompilerOptions::default(),
        );
        assert!(compilation.output.is_none());
        let failures = compilation
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code != DiagnosticCode::UnsupportedSyntax)
            .collect::<Vec<_>>();
        assert_eq!(failures.len(), 1, "{failures:#?}");
        assert_eq!(failures[0].code, DiagnosticCode::TypeMismatch);
        assert_eq!(failures[0].span.module, ENTRY);
        assert_eq!(
            &source[failures[0].span.start..failures[0].span.end],
            member,
            "{failures:#?}"
        );
    }
}

#[test]
fn both_variable_tuple_rest_overrides_align_middle_and_suffixes() {
    let valid = include_str!("fixtures/typescript_oracle/class-override-both-middle-valid/main.ts");
    let accepted = compile(
        ENTRY,
        &MapLoader::from([ModuleSource::new(ENTRY, valid)]),
        CompilerOptions::default(),
    );
    assert!(accepted.output.is_some());
    assert!(
        accepted.diagnostics.is_empty(),
        "{:#?}",
        accepted.diagnostics
    );

    for (source, member) in [
        (
            include_str!(
                "fixtures/typescript_oracle/class-override-both-middle-suffix-error/main.ts"
            ),
            "read(...parts: [number, ...string[], number]): number { return 1; }",
        ),
        (
            include_str!(
                "fixtures/typescript_oracle/class-override-both-middle-element-error/main.ts"
            ),
            "read(...parts: [number, ...boolean[], boolean]): number { return 1; }",
        ),
        (
            include_str!(
                "fixtures/typescript_oracle/class-override-both-middle-shift-error/main.ts"
            ),
            "read(...parts: [number, string, ...boolean[], boolean]): number { return 1; }",
        ),
        (
            include_str!(
                "fixtures/typescript_oracle/class-override-both-middle-tail-error/main.ts"
            ),
            "read(...parts: [number, ...string[]]): number { return 1; }",
        ),
        (
            include_str!(
                "fixtures/typescript_oracle/class-override-both-middle-static-error/main.ts"
            ),
            "static parse(...parts: [string, ...string[], boolean]): number { return 1; }",
        ),
    ] {
        let compilation = compile(
            ENTRY,
            &MapLoader::from([ModuleSource::new(ENTRY, source)]),
            CompilerOptions::default(),
        );
        assert!(compilation.output.is_none());
        let failures = compilation
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code != DiagnosticCode::UnsupportedSyntax)
            .collect::<Vec<_>>();
        assert_eq!(failures.len(), 1, "{failures:#?}");
        assert_eq!(failures[0].code, DiagnosticCode::TypeMismatch);
        assert_eq!(failures[0].span.module, ENTRY);
        assert_eq!(
            &source[failures[0].span.start..failures[0].span.end],
            member,
            "{failures:#?}"
        );
    }
}

#[test]
fn optional_trailing_tuple_prefixes_compare_with_middle_rests() {
    let valid =
        include_str!("fixtures/typescript_oracle/class-override-optional-variable-valid/main.ts");
    let accepted = compile(
        ENTRY,
        &MapLoader::from([ModuleSource::new(ENTRY, valid)]),
        CompilerOptions::default(),
    );
    assert!(accepted.output.is_some());
    assert!(
        accepted.diagnostics.is_empty(),
        "{:#?}",
        accepted.diagnostics
    );

    for (source, member) in [
        (
            include_str!(
                "fixtures/typescript_oracle/class-override-optional-variable-base-error/main.ts"
            ),
            "read(...parts: [...string[], string]): number { return 1; }",
        ),
        (
            include_str!(
                "fixtures/typescript_oracle/class-override-optional-variable-derived-error/main.ts"
            ),
            "read(...parts: [number?, ...string[]]): number { return 1; }",
        ),
        (
            include_str!(
                "fixtures/typescript_oracle/class-override-optional-variable-shift-error/main.ts"
            ),
            "read(...parts: [...string[], string]): number { return 1; }",
        ),
        (
            include_str!(
                "fixtures/typescript_oracle/class-override-optional-variable-static-error/main.ts"
            ),
            "static parse(...parts: [number?, ...string[]]): number { return 1; }",
        ),
    ] {
        let compilation = compile(
            ENTRY,
            &MapLoader::from([ModuleSource::new(ENTRY, source)]),
            CompilerOptions::default(),
        );
        assert!(compilation.output.is_none());
        let failures = compilation
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code != DiagnosticCode::UnsupportedSyntax)
            .collect::<Vec<_>>();
        assert_eq!(failures.len(), 1, "{failures:#?}");
        assert_eq!(failures[0].code, DiagnosticCode::TypeMismatch);
        assert_eq!(failures[0].span.module, ENTRY);
        assert_eq!(
            &source[failures[0].span.start..failures[0].span.end],
            member,
            "{failures:#?}"
        );
    }
}

#[test]
fn optional_fixed_tuple_rest_does_not_supply_middle_required_suffix() {
    for (source, member) in [
        (
            include_str!("fixtures/typescript_oracle/class-override-optional-fixed-middle-base-error/main.ts"),
            "read(...parts: [...string[], string]): number { return 1; }",
        ),
        (
            include_str!("fixtures/typescript_oracle/class-override-optional-fixed-middle-derived-error/main.ts"),
            "read(...parts: [string, string?]): number { return 1; }",
        ),
        (
            include_str!("fixtures/typescript_oracle/class-override-optional-fixed-middle-long-error/main.ts"),
            "read(...parts: [...string[], string]): number { return 1; }",
        ),
        (
            include_str!("fixtures/typescript_oracle/class-override-optional-fixed-middle-static-error/main.ts"),
            "static parse(...parts: [string, string?]): number { return 1; }",
        ),
    ] {
        let compilation = compile(
            ENTRY,
            &MapLoader::from([ModuleSource::new(ENTRY, source)]),
            CompilerOptions::default(),
        );
        assert!(compilation.output.is_none());
        let failures = compilation
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code != DiagnosticCode::UnsupportedSyntax)
            .collect::<Vec<_>>();
        assert_eq!(failures.len(), 1, "{failures:#?}");
        assert_eq!(failures[0].code, DiagnosticCode::TypeMismatch);
        assert_eq!(failures[0].span.module, ENTRY);
        assert_eq!(
            &source[failures[0].span.start..failures[0].span.end],
            member,
            "{failures:#?}"
        );
    }
}

#[test]
fn optional_ordinary_prefixes_align_before_variable_tuple_rests() {
    let valid = include_str!(
        "fixtures/typescript_oracle/class-override-ordinary-optional-both-valid/main.ts"
    );
    let accepted = compile(
        ENTRY,
        &MapLoader::from([ModuleSource::new(ENTRY, valid)]),
        CompilerOptions::default(),
    );
    assert!(accepted.output.is_some());
    assert!(
        accepted.diagnostics.is_empty(),
        "{:#?}",
        accepted.diagnostics
    );

    for (source, member) in [
        (
            include_str!("fixtures/typescript_oracle/class-override-ordinary-optional-both-prefix-error/main.ts"),
            "read(a?: string, ...parts: [...string[], boolean]): number { return 1; }",
        ),
        (
            include_str!("fixtures/typescript_oracle/class-override-ordinary-optional-both-suffix-error/main.ts"),
            "read(a?: number, ...parts: [...string[], number]): number { return 1; }",
        ),
        (
            include_str!("fixtures/typescript_oracle/class-override-ordinary-optional-both-shift-error/main.ts"),
            "read(a?: string, ...parts: [...string[], boolean]): number { return 1; }",
        ),
        (
            include_str!("fixtures/typescript_oracle/class-override-ordinary-optional-both-static-error/main.ts"),
            "static parse(a?: string, ...parts: [...string[], boolean]): number { return 1; }",
        ),
    ] {
        let compilation = compile(
            ENTRY,
            &MapLoader::from([ModuleSource::new(ENTRY, source)]),
            CompilerOptions::default(),
        );
        assert!(compilation.output.is_none());
        let failures = compilation
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code != DiagnosticCode::UnsupportedSyntax)
            .collect::<Vec<_>>();
        assert_eq!(failures.len(), 1, "{failures:#?}");
        assert_eq!(failures[0].code, DiagnosticCode::TypeMismatch);
        assert_eq!(failures[0].span.module, ENTRY);
        assert_eq!(
            &source[failures[0].span.start..failures[0].span.end],
            member,
            "{failures:#?}"
        );
    }
}

#[test]
fn optional_ordinary_prefixes_align_with_fixed_and_array_rest_bases() {
    let valid = include_str!(
        "fixtures/typescript_oracle/class-override-ordinary-optional-direction-valid/main.ts"
    );
    let accepted = compile(
        ENTRY,
        &MapLoader::from([ModuleSource::new(ENTRY, valid)]),
        CompilerOptions::default(),
    );
    assert!(accepted.output.is_some());
    assert!(
        accepted.diagnostics.is_empty(),
        "{:#?}",
        accepted.diagnostics
    );

    for (source, member) in [
        (
            include_str!("fixtures/typescript_oracle/class-override-ordinary-optional-direction-derived-fixed-error/main.ts"),
            "read(a?: string, ...parts: [...string[], boolean]): number { return 1; }",
        ),
        (
            include_str!("fixtures/typescript_oracle/class-override-ordinary-optional-direction-derived-array-error/main.ts"),
            "read(a?: string, ...parts: [...string[], string]): number { return 1; }",
        ),
        (
            include_str!("fixtures/typescript_oracle/class-override-ordinary-optional-direction-inherited-fixed-error/main.ts"),
            "read(a: string, b: boolean): number { return 1; }",
        ),
        (
            include_str!("fixtures/typescript_oracle/class-override-ordinary-optional-direction-inherited-array-error/main.ts"),
            "read(a?: string, ...parts: string[]): number { return 1; }",
        ),
        (
            include_str!("fixtures/typescript_oracle/class-override-ordinary-optional-direction-arity-error/main.ts"),
            "read(a?: number, ...parts: [...string[], boolean]): number { return 1; }",
        ),
        (
            include_str!("fixtures/typescript_oracle/class-override-ordinary-optional-direction-derived-arity-error/main.ts"),
            "read(a?: number, ...parts: [...string[], boolean]): number { return 1; }",
        ),
        (
            include_str!("fixtures/typescript_oracle/class-override-ordinary-optional-direction-inherited-arity-error/main.ts"),
            "read(a: number): number { return 1; }",
        ),
        (
            include_str!("fixtures/typescript_oracle/class-override-ordinary-optional-direction-static-error/main.ts"),
            "static parse(a?: string, ...parts: string[]): number { return 1; }",
        ),
    ] {
        let compilation = compile(
            ENTRY,
            &MapLoader::from([ModuleSource::new(ENTRY, source)]),
            CompilerOptions::default(),
        );
        assert!(compilation.output.is_none());
        let failures = compilation
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code != DiagnosticCode::UnsupportedSyntax)
            .collect::<Vec<_>>();
        assert_eq!(failures.len(), 1, "{failures:#?}");
        assert_eq!(failures[0].code, DiagnosticCode::TypeMismatch);
        assert_eq!(failures[0].span.module, ENTRY);
        assert_eq!(
            &source[failures[0].span.start..failures[0].span.end],
            member,
            "{failures:#?}"
        );
    }
}

#[test]
fn optional_ordinary_prefixes_align_with_fixed_tuple_rests() {
    let valid = include_str!(
        "fixtures/typescript_oracle/class-override-ordinary-optional-fixedtuple-valid/main.ts"
    );
    let accepted = compile(
        ENTRY,
        &MapLoader::from([ModuleSource::new(ENTRY, valid)]),
        CompilerOptions::default(),
    );
    assert!(accepted.output.is_some());
    assert!(
        accepted.diagnostics.is_empty(),
        "{:#?}",
        accepted.diagnostics
    );

    for (source, member) in [
        (
            include_str!("fixtures/typescript_oracle/class-override-ordinary-optional-fixedtuple-derived-error/main.ts"),
            "read(a?: string, ...parts: [...string[], boolean]): number { return 1; }",
        ),
        (
            include_str!("fixtures/typescript_oracle/class-override-ordinary-optional-fixedtuple-inherited-error/main.ts"),
            "read(...parts: [string, boolean]): number { return 1; }",
        ),
        (
            include_str!("fixtures/typescript_oracle/class-override-ordinary-optional-fixedtuple-empty-base-error/main.ts"),
            "read(...parts: [...number[], number]): number { return 1; }",
        ),
        (
            include_str!("fixtures/typescript_oracle/class-override-ordinary-optional-fixedtuple-empty-derived-error/main.ts"),
            "read(a?: number, ...parts: []): number { return 1; }",
        ),
        (
            include_str!("fixtures/typescript_oracle/class-override-ordinary-optional-fixedtuple-static-error/main.ts"),
            "static parse(a?: string, ...parts: [...string[], boolean]): number { return 1; }",
        ),
    ] {
        let compilation = compile(
            ENTRY,
            &MapLoader::from([ModuleSource::new(ENTRY, source)]),
            CompilerOptions::default(),
        );
        assert!(compilation.output.is_none());
        let failures = compilation
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code != DiagnosticCode::UnsupportedSyntax)
            .collect::<Vec<_>>();
        assert_eq!(failures.len(), 1, "{failures:#?}");
        assert_eq!(failures[0].code, DiagnosticCode::TypeMismatch);
        assert_eq!(failures[0].span.module, ENTRY);
        assert_eq!(
            &source[failures[0].span.start..failures[0].span.end],
            member,
            "{failures:#?}"
        );
    }
}

#[test]
fn middle_and_fixed_tuple_rest_overrides_align_total_arity() {
    let valid =
        include_str!("fixtures/typescript_oracle/class-override-middle-fixed-valid/main.ts");
    let accepted = compile(
        ENTRY,
        &MapLoader::from([ModuleSource::new(ENTRY, valid)]),
        CompilerOptions::default(),
    );
    assert!(accepted.output.is_some());
    assert!(
        accepted.diagnostics.is_empty(),
        "{:#?}",
        accepted.diagnostics
    );

    for (source, member) in [
        (
            include_str!("fixtures/typescript_oracle/class-override-middle-fixed-base-suffix-error/main.ts"),
            "read(...parts: [number, string]): number { return 1; }",
        ),
        (
            include_str!("fixtures/typescript_oracle/class-override-middle-fixed-base-element-error/main.ts"),
            "read(...parts: [number, boolean, boolean]): number { return 1; }",
        ),
        (
            include_str!("fixtures/typescript_oracle/class-override-middle-fixed-base-arity-error/main.ts"),
            "read(...parts: [number]): number { return 1; }",
        ),
        (
            include_str!("fixtures/typescript_oracle/class-override-middle-fixed-derived-suffix-error/main.ts"),
            "read(...parts: [number, ...string[], number]): number { return 1; }",
        ),
        (
            include_str!("fixtures/typescript_oracle/class-override-middle-fixed-derived-element-error/main.ts"),
            "read(...parts: [number, ...boolean[], boolean]): number { return 1; }",
        ),
        (
            include_str!("fixtures/typescript_oracle/class-override-middle-fixed-derived-arity-error/main.ts"),
            "read(...parts: [number, ...string[], boolean]): number { return 1; }",
        ),
        (
            include_str!("fixtures/typescript_oracle/class-override-middle-fixed-static-error/main.ts"),
            "static parse(...parts: [number, string]): number { return 1; }",
        ),
    ] {
        let compilation = compile(
            ENTRY,
            &MapLoader::from([ModuleSource::new(ENTRY, source)]),
            CompilerOptions::default(),
        );
        assert!(compilation.output.is_none());
        let failures = compilation
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code != DiagnosticCode::UnsupportedSyntax)
            .collect::<Vec<_>>();
        assert_eq!(failures.len(), 1, "{failures:#?}");
        assert_eq!(failures[0].code, DiagnosticCode::TypeMismatch);
        assert_eq!(failures[0].span.module, ENTRY);
        assert_eq!(
            &source[failures[0].span.start..failures[0].span.end],
            member,
            "{failures:#?}"
        );
    }
}

#[test]
fn class_construction_scan_obeys_the_type_expansion_budget() {
    let source =
        "class Box { constructor(value: number) {} } const pair = [new Box(1), new Box(2)];";
    let compilation = compile(
        ENTRY,
        &MapLoader::from([ModuleSource::new(ENTRY, source)]),
        CompilerOptions {
            limits: CompilerLimits {
                max_type_expansions: 1,
                ..CompilerLimits::default()
            },
            ..CompilerOptions::default()
        },
    );
    assert!(compilation.output.is_none());
    assert_eq!(
        compilation
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == DiagnosticCode::ResourceLimit)
            .count(),
        1,
        "{:#?}",
        compilation.diagnostics
    );
}

#[test]
fn local_class_method_uses_keep_instance_and_constructor_sides_separate() {
    for accepted in [
        include_str!("fixtures/typescript_oracle/class-local-instance-method/main.ts"),
        include_str!("fixtures/typescript_oracle/class-local-shadowed-call/main.ts"),
    ] {
        let compilation = compile_with_helper(accepted);
        assert!(compilation.output.is_some());
        assert!(
            compilation.diagnostics.is_empty(),
            "{:#?}",
            compilation.diagnostics
        );
    }

    for (source, expected_line, expected_span) in [
        (
            include_str!("fixtures/typescript_oracle/class-local-instance-wrong-side-call/main.ts"),
            6,
            "Box.read(1)",
        ),
        (
            include_str!("fixtures/typescript_oracle/class-local-instance-wrong-side-read/main.ts"),
            6,
            "Box.read",
        ),
        (
            include_str!("fixtures/typescript_oracle/class-local-called-without-new/main.ts"),
            6,
            "Box()",
        ),
        (
            include_str!("fixtures/typescript_oracle/class-local-nested-call-without-new/main.ts"),
            7,
            "Box()",
        ),
    ] {
        let compilation = compile_with_helper(source);
        assert!(compilation.output.is_none());
        let failures = compilation
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code != DiagnosticCode::UnsupportedSyntax)
            .collect::<Vec<_>>();
        assert_eq!(failures.len(), 1, "{:#?}", compilation.diagnostics);
        let failure = failures[0];
        assert_eq!(failure.code, DiagnosticCode::TypeMismatch);
        assert_eq!(
            source[..failure.span.start]
                .bytes()
                .filter(|byte| *byte == b'\n')
                .count()
                + 1,
            expected_line
        );
        assert_eq!(&source[failure.span.start..failure.span.end], expected_span);
    }
}

#[test]
fn local_class_call_scan_obeys_the_type_expansion_budget() {
    let source = "class Box {} const pair = [Box(), Box()];";
    let compilation = compile(
        ENTRY,
        &MapLoader::from([ModuleSource::new(ENTRY, source)]),
        CompilerOptions {
            limits: CompilerLimits {
                max_type_expansions: 1,
                ..CompilerLimits::default()
            },
            ..CompilerOptions::default()
        },
    );
    assert!(compilation.output.is_none());
    assert_eq!(
        compilation
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == DiagnosticCode::ResourceLimit)
            .count(),
        1,
        "{:#?}",
        compilation.diagnostics
    );
}

#[test]
fn static_class_method_shells_keep_sides_and_source_spans() {
    let source = include_str!("fixtures/typescript_oracle/class-static-method-shell/main.ts");
    let module = parse_module(ENTRY, source).unwrap();
    let Declaration::Class(class) = &module.declarations[0] else {
        panic!("expected a class declaration");
    };
    assert_eq!(class.members.len(), 4);
    let methods = class
        .members
        .iter()
        .map(|member| {
            let method = member.method.as_ref().expect("parsed method");
            (
                method.name.as_str(),
                method.is_static,
                method.body.is_some(),
                &source[member.span.start..member.span.end],
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        methods,
        [
            ("read", true, false, "static read(value: number): number;"),
            (
                "read",
                true,
                true,
                "static read(value: number): number { return value; }"
            ),
            ("read", false, true, "read(): number { return 1; }"),
            ("static", false, true, "static(): number { return 2; }"),
        ]
    );
    assert_eq!(
        class
            .method_groups
            .iter()
            .map(|group| (
                group.name.as_str(),
                group.is_static,
                group.signature_member_indices.as_slice(),
                group.implementation_member_index,
            ))
            .collect::<Vec<_>>(),
        [
            ("read", true, &[0][..], Some(1)),
            ("read", false, &[][..], Some(2)),
            ("static", false, &[][..], Some(3)),
        ]
    );
    let compilation = compile_with_helper(source);
    assert!(compilation.output.is_some());
    assert!(
        compilation.diagnostics.is_empty(),
        "{:#?}",
        compilation.diagnostics
    );
}

#[test]
fn static_class_methods_bind_to_constructor_side_and_check_calls() {
    for accepted in [
        include_str!("fixtures/typescript_oracle/class-static-binding/main.ts"),
        include_str!("fixtures/typescript_oracle/class-static-overload-binding/main.ts"),
    ] {
        let compilation = compile_with_helper(accepted);
        assert!(compilation.output.is_some());
        assert!(
            compilation.diagnostics.is_empty(),
            "{:#?}",
            compilation.diagnostics
        );
    }

    for (source, expected_line, expected_span) in [
        (
            include_str!("fixtures/typescript_oracle/class-static-wrong-instance-call/main.ts"),
            7,
            "counter.read(1)",
        ),
        (
            include_str!("fixtures/typescript_oracle/class-static-wrong-instance-read/main.ts"),
            7,
            "counter.read",
        ),
        (
            include_str!("fixtures/typescript_oracle/class-static-argument-error/main.ts"),
            6,
            "Counter.read('text')",
        ),
        (
            include_str!("fixtures/typescript_oracle/class-static-overload-error/main.ts"),
            10,
            "Converter.parse(true)",
        ),
    ] {
        let compilation = compile_with_helper(source);
        assert!(compilation.output.is_none());
        let failures = compilation
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code != DiagnosticCode::UnsupportedSyntax)
            .collect::<Vec<_>>();
        assert_eq!(failures.len(), 1, "{:#?}", compilation.diagnostics);
        let failure = failures[0];
        assert_eq!(failure.code, DiagnosticCode::TypeMismatch);
        assert_eq!(
            source[..failure.span.start]
                .bytes()
                .filter(|byte| *byte == b'\n')
                .count()
                + 1,
            expected_line
        );
        assert_eq!(&source[failure.span.start..failure.span.end], expected_span);
    }
}

#[test]
fn closed_module_type_imports_retain_class_instance_method_shapes() {
    for (main, box_module) in [
        (
            include_str!("fixtures/typescript_oracle/class-type-import/main.ts"),
            include_str!("fixtures/typescript_oracle/class-type-import/box.ts"),
        ),
        (
            include_str!("fixtures/typescript_oracle/class-type-import-alias/main.ts"),
            include_str!("fixtures/typescript_oracle/class-type-import-alias/box.ts"),
        ),
        (
            include_str!("fixtures/typescript_oracle/class-type-import-type-export/main.ts"),
            include_str!("fixtures/typescript_oracle/class-type-import-type-export/box.ts"),
        ),
        (
            include_str!("fixtures/typescript_oracle/class-type-import-self-reference/main.ts"),
            include_str!("fixtures/typescript_oracle/class-type-import-self-reference/box.ts"),
        ),
    ] {
        let compilation = compile_class_module_pair(main, box_module);
        assert!(compilation.output.is_some());
        assert!(
            compilation.diagnostics.is_empty(),
            "{:#?}",
            compilation.diagnostics
        );
    }

    let main = include_str!("fixtures/typescript_oracle/class-type-import-wrong-side/main.ts");
    let box_module = include_str!("fixtures/typescript_oracle/class-type-import-wrong-side/box.ts");
    let compilation = compile_class_module_pair(main, box_module);
    assert!(compilation.output.is_none());
    let failures = compilation
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code != DiagnosticCode::UnsupportedSyntax)
        .collect::<Vec<_>>();
    assert_eq!(failures.len(), 1, "{:#?}", compilation.diagnostics);
    let failure = failures[0];
    assert_eq!(failure.code, DiagnosticCode::TypeMismatch);
    assert_eq!(failure.span.module, ENTRY);
    assert_eq!(&main[failure.span.start..failure.span.end], "box.make()");

    let main = include_str!("fixtures/typescript_oracle/class-type-import-private/main.ts");
    let box_module = include_str!("fixtures/typescript_oracle/class-type-import-private/box.ts");
    let compilation = compile_class_module_pair(main, box_module);
    let failures = compilation
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code != DiagnosticCode::UnsupportedSyntax)
        .collect::<Vec<_>>();
    assert_eq!(failures.len(), 1, "{:#?}", compilation.diagnostics);
    assert_eq!(failures[0].code, DiagnosticCode::UnknownType);
    assert_eq!(failures[0].span.module, ENTRY);
    assert_eq!(
        &main[failures[0].span.start..failures[0].span.end],
        "import type { Box } from './box.ts';"
    );
}

#[test]
fn closed_module_value_imports_bind_both_class_sides_with_local_names() {
    for (main, box_module) in [
        (
            include_str!("fixtures/typescript_oracle/class-value-import-binding/main.ts"),
            include_str!("fixtures/typescript_oracle/class-value-import-binding/box.ts"),
        ),
        (
            include_str!("fixtures/typescript_oracle/class-value-import-alias-binding/main.ts"),
            include_str!("fixtures/typescript_oracle/class-value-import-alias-binding/box.ts"),
        ),
    ] {
        let compilation = compile_class_module_pair(main, box_module);
        assert!(compilation.output.is_some());
        assert!(
            compilation.diagnostics.is_empty(),
            "{:#?}",
            compilation.diagnostics
        );
    }

    let main =
        include_str!("fixtures/typescript_oracle/class-value-import-wrong-side-binding/main.ts");
    let box_module =
        include_str!("fixtures/typescript_oracle/class-value-import-wrong-side-binding/box.ts");
    let compilation = compile_class_module_pair(main, box_module);
    assert!(compilation.output.is_none());
    let failures = compilation
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code != DiagnosticCode::UnsupportedSyntax)
        .collect::<Vec<_>>();
    assert_eq!(failures.len(), 1, "{:#?}", compilation.diagnostics);
    assert_eq!(failures[0].code, DiagnosticCode::TypeMismatch);
    assert_eq!(failures[0].span.module, ENTRY);
    assert_eq!(
        &main[failures[0].span.start..failures[0].span.end],
        "const invalid: LocalBox = LocalBox;"
    );
}

#[test]
fn closed_module_class_value_calls_check_both_sides_and_type_only_use() {
    let box_module = include_str!("fixtures/typescript_oracle/class-value-import-calls/box.ts");
    for (main, box_module) in [
        (
            include_str!("fixtures/typescript_oracle/class-value-import-calls/main.ts"),
            box_module,
        ),
        (
            include_str!("fixtures/typescript_oracle/class-value-import-calls-alias/main.ts"),
            include_str!("fixtures/typescript_oracle/class-value-import-calls-alias/box.ts"),
        ),
        (
            include_str!("fixtures/typescript_oracle/class-type-import-value-safe/main.ts"),
            box_module,
        ),
    ] {
        let compilation = compile_class_module_pair(main, box_module);
        assert!(compilation.output.is_some());
        assert!(
            compilation.diagnostics.is_empty(),
            "{:#?}",
            compilation.diagnostics
        );
    }

    for (main, line, expected_span, code) in [
        (
            include_str!("fixtures/typescript_oracle/class-value-import-constructor-error/main.ts"),
            6,
            "new LocalBox('text')",
            DiagnosticCode::TypeMismatch,
        ),
        (
            include_str!("fixtures/typescript_oracle/class-value-import-static-error/main.ts"),
            6,
            "LocalBox.make('text')",
            DiagnosticCode::TypeMismatch,
        ),
        (
            include_str!(
                "fixtures/typescript_oracle/class-value-import-wrong-constructor-side/main.ts"
            ),
            6,
            "LocalBox.read(1)",
            DiagnosticCode::TypeMismatch,
        ),
        (
            include_str!(
                "fixtures/typescript_oracle/class-value-import-wrong-instance-side/main.ts"
            ),
            7,
            "box.make(2)",
            DiagnosticCode::TypeMismatch,
        ),
        (
            include_str!("fixtures/typescript_oracle/class-value-import-bare-call/main.ts"),
            6,
            "LocalBox(1)",
            DiagnosticCode::TypeMismatch,
        ),
        (
            include_str!("fixtures/typescript_oracle/class-type-import-value-use/main.ts"),
            6,
            "LocalBox",
            DiagnosticCode::UnknownName,
        ),
        (
            include_str!("fixtures/typescript_oracle/class-type-import-bare-value/main.ts"),
            6,
            "LocalBox",
            DiagnosticCode::UnknownName,
        ),
    ] {
        let compilation = compile_class_module_pair(main, box_module);
        assert!(compilation.output.is_none());
        let failures = compilation
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code != DiagnosticCode::UnsupportedSyntax)
            .collect::<Vec<_>>();
        assert_eq!(failures.len(), 1, "{:#?}", compilation.diagnostics);
        let failure = failures[0];
        assert_eq!(failure.code, code);
        assert_eq!(failure.span.module, ENTRY);
        assert_eq!(
            main[..failure.span.start]
                .bytes()
                .filter(|byte| *byte == b'\n')
                .count()
                + 1,
            line
        );
        assert_eq!(&main[failure.span.start..failure.span.end], expected_span);
    }

    let main = include_str!("fixtures/typescript_oracle/class-type-export-value-use/main.ts");
    let box_module =
        include_str!("fixtures/typescript_oracle/class-type-import-type-export/box.ts");
    let compilation = compile_class_module_pair(main, box_module);
    assert!(compilation.output.is_none());
    let failures = compilation
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code != DiagnosticCode::UnsupportedSyntax)
        .collect::<Vec<_>>();
    assert_eq!(failures.len(), 1, "{:#?}", compilation.diagnostics);
    assert_eq!(failures[0].code, DiagnosticCode::UnknownName);
    assert_eq!(failures[0].span.module, ENTRY);
    assert_eq!(
        &main[failures[0].span.start..failures[0].span.end],
        "LocalBox"
    );
}

#[test]
fn braced_try_catch_finally_retains_the_catch_name_bodies_and_source_span() {
    let source =
        "function f(): void { try { throw 1; } catch (caught) { throw caught; } finally { 0; } }";
    let module = parse_module(ENTRY, source).unwrap();
    let [Declaration::Function(function)] = module.declarations.as_slice() else {
        panic!("expected one named function");
    };
    let [FunctionBodyItem::Try(statement)] = function.body.as_slice() else {
        panic!("the selected try/catch/finally form must be structured");
    };
    assert!(matches!(
        statement.block.as_slice(),
        [FunctionBodyItem::Throw { .. }]
    ));
    let handler = statement.handler.as_ref().expect("catch must be retained");
    assert_eq!(handler.binding, "caught");
    assert!(matches!(
        handler.body.as_slice(),
        [FunctionBodyItem::Throw { .. }]
    ));
    assert!(matches!(
        statement.finalizer.as_deref(),
        Some([FunctionBodyItem::Expression { .. }])
    ));
    assert_eq!(
        &source[statement.span.start..statement.span.end],
        "try { throw 1; } catch (caught) { throw caught; } finally { 0; }"
    );

    for source in [
        "function f(): void { try { throw 1; } catch (caught) { throw caught; } }",
        "function f(): void { try { 1; } finally { 2; } }",
    ] {
        let module = parse_module(ENTRY, source).unwrap();
        let [Declaration::Function(function)] = module.declarations.as_slice() else {
            panic!("expected one named function");
        };
        assert!(matches!(
            function.body.as_slice(),
            [FunctionBodyItem::Try(_)]
        ));
    }
}

#[test]
fn excluded_try_shapes_remain_opaque_to_the_bounded_function_parser() {
    for source in [
        "function f(): void { try { 1; } catch { 2; } }",
        "function f(): void { try { 1; } }",
    ] {
        let module = parse_module(ENTRY, source).unwrap();
        let [Declaration::Function(function)] = module.declarations.as_slice() else {
            panic!("expected one named function");
        };
        assert!(matches!(
            function.body.first(),
            Some(FunctionBodyItem::Opaque(_))
        ));
    }
    // A typed catch binding is parsed structurally and its annotation erased.
    let module = parse_module(
        ENTRY,
        "function f(): void { try { 1; } catch (caught: any) { 2; } }",
    )
    .unwrap();
    let [Declaration::Function(function)] = module.declarations.as_slice() else {
        panic!("expected one named function");
    };
    let [FunctionBodyItem::Try(statement)] = function.body.as_slice() else {
        panic!("expected a structured try");
    };
    assert_eq!(statement.handler.as_ref().unwrap().binding, "caught");
}

#[test]
fn catch_binding_is_unknown_only_inside_its_lexical_body() {
    let source = "function takesNumber(value: number): void {} function f(caught: number): void { try { takesNumber(caught); throw 'bad'; } catch (caught) { takesNumber(caught); } finally { takesNumber(caught); } takesNumber(caught); }";
    let found = diagnostics(source);
    assert_eq!(
        found.len(),
        1,
        "only the catch-shadowed call should fail: {found:?}"
    );
    assert_eq!(found[0].code.to_string(), "BTS3003");
    assert!(found[0].message.contains("argument 1 has type `unknown`"));
    assert_eq!(
        found[0].span.start,
        source.find("takesNumber(caught); } finally").unwrap(),
    );
}

#[test]
fn catch_return_uses_unknown_instead_of_the_shadowed_outer_type() {
    assert_rejected(
        "function f(caught: number): number { try { throw 1; } catch (caught) { return caught; } finally { 0; } return 0; }",
        "BTS3004",
        "return expression has type `unknown`",
    );
    assert_accepted(
        "function f(): unknown { try { throw 1; } catch (caught) { return caught; } finally { 0; } }",
    );
    assert_accepted(
        "type Maybe = number | unknown; function f(): Maybe { try { throw 1; } catch (caught) { return caught; } }",
    );
    // A `try` whose block returns ends every path even with a `finally` that
    // falls through, as in `tsc`; only when nothing in it returns can it end.
    assert_accepted("function f(): number { try { return 1; } finally { 0; } }");
    assert_rejected(
        "function f(): number { try { 0; } finally { 0; } }",
        "BTS3004",
        "can complete without returning a value",
    );
}

#[test]
fn catch_checks_nested_calls_and_unannotated_returns_without_scope_leakage() {
    let prelude = "function takesNumber(value: number): number { return value; } ";
    assert_rejected(
        &format!("{prelude}function f(caught: number) {{ try {{ throw 1; }} catch (caught) {{ return takesNumber(caught); }} }}"),
        "BTS3003",
        "argument 1 has type `unknown`",
    );
    let source = format!("{prelude}function f(caught: number): number {{ try {{ throw 1; }} catch (caught) {{ if (true) {{ takesNumber(caught); }} return 0; }} finally {{ return caught; }} return 0; }}");
    let found = diagnostics(&source);
    assert_eq!(
        found.len(),
        1,
        "only the nested catch call should fail: {found:?}"
    );
    assert!(found[0].message.contains("argument 1 has type `unknown`"));
}

#[test]
fn immutable_typeof_guard_narrows_each_branch_and_inequality_inverse() {
    assert_accepted(
        "function takesString(value: string): void {} function takesNumber(value: number): void {} function f(input: string | number): number { const value: string | number = input; if (typeof value === 'string') { takesString(value); return 1; } else { takesNumber(value); return 2; } }",
    );
    assert_accepted(
        "function takesString(value: string): void {} function takesNumber(value: number): void {} function f(input: string | number): number { const value: string | number = input; if (typeof value !== \"string\") { takesNumber(value); return value; } else { takesString(value); return 0; } }",
    );
}

#[test]
fn immutable_typeof_guard_carries_the_surviving_type_after_early_completion() {
    assert_accepted(
        "function takesString(value: string): void {} function f(input: string | number): number { const value: string | number = input; if (typeof value !== 'string') { return value; } takesString(value); return 0; }",
    );
    assert_accepted(
        "function takesNumber(value: number): void {} function f(input: string | number): number { const value: string | number = input; if (typeof value === 'string') { throw value; } takesNumber(value); return value; }",
    );
    assert_rejected(
        "function f(input: string | number): number { const value: string | number = input; if (typeof value === 'string') { return 1; } }",
        "BTS3004",
        "can complete without returning a value",
    );
}

#[test]
fn immutable_typeof_guard_does_not_leak_into_siblings_or_two_live_paths() {
    assert_rejected(
        "function takesString(value: string): void {} function f(input: string | number): void { const value: string | number = input; if (typeof value === 'string') { 0; } takesString(value); }",
        "BTS3003",
        "argument 1 has type `string | number`",
    );
    let source = "function takesString(value: string): void {} function takesNumber(value: number): void {} function f(input: string | number): void { const value: string | number = input; if (typeof value === 'string') { takesNumber(value); } else { takesString(value); } }";
    let found = diagnostics(source);
    assert_eq!(
        found.len(),
        2,
        "both siblings must keep distinct narrowed types: {found:?}"
    );
    assert!(found[0].message.contains("argument 1 has type `string`"));
    assert!(found[1].message.contains("argument 1 has type `number`"));
}

#[test]
fn first_typeof_guard_does_not_narrow_mutable_or_parameter_bindings() {
    for source in [
        "function takesString(value: string): void {} function f(input: string | number): void { let value: string | number = input; if (typeof value === 'string') { takesString(value); } }",
        "function takesString(value: string): void {} function f(value: string | number): void { if (typeof value === 'string') { takesString(value); } }",
    ] {
        assert_rejected(source, "BTS3003", "argument 1 has type `string | number`");
    }
}

#[test]
fn first_typeof_guard_does_not_claim_later_or_repeated_guards() {
    assert_rejected(
        "function takesString(value: string): void {} function f(input: string | number): void { if (typeof value === 'string') { takesString(value); } const value: string | number = input; }",
        "BTS3003",
        "argument 1 has type `string | number`",
    );
    assert_rejected(
        "function takesString(value: string): void {} function f(input: string | number): void { const value: string | number = input; if (typeof value === 'string') { 0; } if (typeof value === 'string') { takesString(value); } }",
        "BTS3003",
        "argument 1 has type `string | number`",
    );
}

#[test]
fn immutable_typeof_guard_emits_the_original_runtime_condition_without_types() {
    let javascript = emitted(
        "function f(input: string | number): number { const value: string | number = input; if (typeof value === 'string') { return 1; } else { return value; } }",
    );
    assert!(
        javascript.contains("typeof value === 'string'"),
        "{javascript}"
    );
    assert!(javascript.contains("if ("), "{javascript}");
    assert!(!javascript.contains(": string | number"), "{javascript}");
    assert!(!javascript.contains("): number"), "{javascript}");
}

#[test]
fn callback_method_overloads_retain_both_literal_tag_signatures() {
    let source = "interface Visitor { visit(kind: 'text', callback: (value: string) => void): void; visit(kind: 'count', callback: (value: number) => void): void; } type RecordVisitor = { visit(kind: 'text', callback: (value: string) => void): void; visit(kind: 'count', callback: (value: number) => void): void; };";
    let module = parse_module(ENTRY, source).unwrap();
    let [Declaration::Interface(interface), Declaration::TypeAlias(alias)] =
        module.declarations.as_slice()
    else {
        panic!("expected one interface and record alias");
    };
    let blueice_bluets::Type::Record(record_fields) = &alias.value else {
        panic!("expected record alias");
    };
    for fields in [&interface.fields, record_fields] {
        assert_eq!(fields.len(), 2);
        assert!(fields.iter().all(|field| field.name == "visit"));
        for (field, tag) in fields.iter().zip(["'text'", "'count'"]) {
            let blueice_bluets::Type::Function { parameters, result } = &field.value else {
                panic!("each visit overload must retain its function signature");
            };
            assert_eq!(parameters.len(), 2);
            assert_eq!(
                parameters[0].annotation,
                Some(blueice_bluets::Type::Literal(tag.into()))
            );
            assert!(matches!(
                parameters[1].annotation,
                Some(blueice_bluets::Type::Function { .. })
            ));
            assert_eq!(result.as_ref(), &blueice_bluets::Type::Void);
        }
    }
}

#[test]
fn optional_dot_read_retains_exact_tokens_and_source_span() {
    let source = "const receiver: { value: number } | null = null; const answer: number | undefined = receiver?.value; const computed = receiver?.[key]; const called = receiver?.value();";
    let module = parse_module(ENTRY, source).unwrap();
    let [Declaration::Variable(receiver), Declaration::Variable(answer), Declaration::Variable(computed), Declaration::Variable(called)] =
        module.declarations.as_slice()
    else {
        panic!("expected four source-level variable declarations");
    };
    assert!(matches!(
        &receiver.annotation,
        Some(blueice_bluets::Type::Union(parts)) if parts.len() == 2
    ));
    assert_eq!(
        answer
            .initializer
            .iter()
            .map(|token| token.text.as_str())
            .collect::<Vec<_>>(),
        ["receiver", "?.", "value"]
    );
    let optional_dot = &answer.initializer[1];
    assert_eq!(&source[optional_dot.start..optional_dot.end], "?.");
    assert_eq!(
        &source[answer.initializer[0].start..answer.initializer[2].end],
        "receiver?.value"
    );
    assert_eq!(
        computed
            .initializer
            .iter()
            .map(|token| token.text.as_str())
            .collect::<Vec<_>>(),
        ["receiver", "?.", "[", "key", "]"]
    );
    assert_eq!(
        called
            .initializer
            .iter()
            .map(|token| token.text.as_str())
            .collect::<Vec<_>>(),
        ["receiver", "?.", "value", "(", ")"]
    );
}

#[test]
fn optional_dot_read_checks_nullish_result_and_rejects_unproven_shapes() {
    let prefix = "function choose(flag: boolean): { value: number } | null { return flag ? { value: 41 } : null; } const receiver: { value: number } | null = choose(true); ";
    let check = |suffix: &str| diagnostics(&format!("{prefix}{suffix}"));
    let accepted = check("const answer: number = receiver?.value ?? 0;");
    assert!(accepted.is_empty(), "{accepted:#?}");
    let named = diagnostics("interface Box { value: number; } function choose(flag: boolean): Box | null { return flag ? { value: 41 } : null; } const receiver: Box | null = choose(true); const answer: number = receiver?.value ?? 0;");
    assert!(named.is_empty(), "{named:#?}");
    for (suffix, message) in [
        (
            "const wrong: number = receiver?.value;",
            "initializer has type `number | undefined`",
        ),
        (
            "const wrong: number = receiver?.missing ?? 0;",
            "property `missing` does not exist",
        ),
        (
            "const wrong: number = receiver?.[value] ?? 0;",
            "unsupported optional property read",
        ),
        (
            "const wrong: number = receiver?.value() ?? 0;",
            "unsupported optional property read",
        ),
        (
            "const wrong: number = choose(true)?.value ?? 0;",
            "unsupported optional property read",
        ),
        (
            "function takesNumber(value: number): number { return value; } const wrong: number = receiver?.value ?? takesNumber('wrong');",
            "argument 1 has type `'wrong'`",
        ),
    ] {
        let found = check(suffix);
        assert!(
            found.iter().any(|diagnostic| diagnostic.message.contains(message)),
            "{suffix}: {found:#?}"
        );
    }
    let mutable = diagnostics("let receiver: { value: number } | null = null; const wrong: number = receiver?.value ?? 0;");
    assert!(
        mutable.iter().any(|diagnostic| diagnostic
            .message
            .contains("unsupported optional property read")),
        "{mutable:#?}"
    );
    let optional_field = diagnostics("const receiver: { value?: number } | null = null; const wrong: number = receiver?.value ?? 0;");
    assert!(
        optional_field.iter().any(|diagnostic| diagnostic
            .message
            .contains("unsupported optional property read")),
        "{optional_field:#?}"
    );
}

#[test]
fn optional_dot_read_emits_original_short_circuit_syntax_without_types() {
    let source = "interface Box { value: number; } function choose(flag: boolean): Box | null { return flag ? { value: 41 } : null; } const receiver: Box | null = choose(true); const answer: number = receiver?.value ?? 0; answer;";
    let javascript = emitted(source);
    assert!(!javascript.contains("interface Box"), "{javascript}");
    assert!(!javascript.contains(": Box"), "{javascript}");
    assert!(!javascript.contains(": number"), "{javascript}");
    assert!(javascript.contains("receiver?.value ?? 0"), "{javascript}");
    assert!(
        javascript.contains("const answer= receiver?.value ?? 0"),
        "{javascript}"
    );
}

#[test]
fn callback_method_overloads_select_by_tag_and_report_distinct_failures() {
    let ambient = ModuleSource::new(
        "memory:///visitor.d.ts",
        "interface Visitor { visit(kind: 'text', callback: (value: string) => void): void; visit(kind: 'count', callback: (value: number) => void): void; } declare const visitor: Visitor; declare const kind: 'text' | 'count'; declare const opaque: any;",
    );
    let check = |call: &str| {
        let source = format!(
            "function onText(value: string): void {{}} function onCount(value: number): void {{}} {call}"
        );
        let result = compile(
            ENTRY,
            &MapLoader::from([ModuleSource::new(ENTRY, source.clone())]),
            CompilerOptions {
                ambient_declaration_modules: vec![ambient.clone()],
                require_declared_global_calls: true,
                ..CompilerOptions::default()
            },
        );
        (source, result)
    };
    for call in [
        "visitor.visit('text', onText);",
        "visitor.visit(\"count\", onCount);",
    ] {
        let (_, result) = check(call);
        assert!(
            result.diagnostics.is_empty(),
            "{call}: {:#?}",
            result.diagnostics
        );
    }
    for (call, message) in [
        (
            "visitor.visit(kind, onText);",
            "ambiguous overload of method visit",
        ),
        (
            "visitor.visit('other', onText);",
            "no overload of method visit matches",
        ),
        (
            "visitor.visit('text', onCount);",
            "argument 2 has type `function`",
        ),
        (
            "visitor.visit('text', 'wrong');",
            "argument 2 has type `'wrong'`",
        ),
        (
            "visitor.visit('text', opaque);",
            "requires a named function callback",
        ),
    ] {
        let (source, result) = check(call);
        assert_eq!(
            result.diagnostics.len(),
            1,
            "{call}: {:#?}",
            result.diagnostics
        );
        let diagnostic = &result.diagnostics[0];
        assert_eq!(diagnostic.code.to_string(), "BTS3003");
        assert!(
            diagnostic.message.contains(message),
            "{call}: {diagnostic:?}"
        );
        let start = source.find(call).expect("call text");
        assert_eq!(diagnostic.span.start, start, "{call}: {diagnostic:?}");
        assert_eq!(
            diagnostic.span.end,
            start + call.len() - 1,
            "{call}: {diagnostic:?}"
        );
    }
}

#[test]
fn callback_method_overloads_erase_types_but_keep_one_runtime_property() {
    let source = "interface Visitor { visit(kind: 'text', callback: (value: string) => void): void; visit(kind: 'count', callback: (value: number) => void): void; } let observed: number = 0; function onText(value: string): void { if (value === 'A') { observed += 1; } } function onCount(value: number): void { observed += value; } function dispatch(kind: string, listener: any): void { if (kind === 'text') { listener('A'); } else { listener(2); } } const visitor: Visitor = { visit: dispatch }; visitor.visit('text', onText); visitor.visit('count', onCount); observed;";
    let javascript = emitted(source);
    assert!(!javascript.contains("interface Visitor"), "{javascript}");
    assert!(!javascript.contains(": Visitor"), "{javascript}");
    assert!(!javascript.contains(": string"), "{javascript}");
    assert!(!javascript.contains(": number"), "{javascript}");
    assert_eq!(javascript.matches("function dispatch(").count(), 1);
    assert!(javascript.contains("visit: dispatch"), "{javascript}");
    assert!(
        javascript.contains("visitor.visit('text', onText)"),
        "{javascript}"
    );
    assert!(
        javascript.contains("visitor.visit('count', onCount)"),
        "{javascript}"
    );
}

#[test]
fn braced_while_retains_its_condition_and_body_without_proving_a_return() {
    let source =
        "function count(value: number): number { while (value > 0) { value -= 1; } return value; }";
    let module = parse_module(ENTRY, source).unwrap();
    let [Declaration::Function(function)] = module.declarations.as_slice() else {
        panic!("expected one named function");
    };
    let [FunctionBodyItem::While(statement), FunctionBodyItem::Return { .. }] =
        function.body.as_slice()
    else {
        panic!("the braced while must be a structured body item");
    };
    assert_eq!(
        statement
            .test
            .iter()
            .map(|token| token.text.as_str())
            .collect::<Vec<_>>(),
        ["value", ">", "0"]
    );
    assert!(matches!(
        statement.body.as_slice(),
        [FunctionBodyItem::Expression { .. }]
    ));
    assert_eq!(
        &source[statement.span.start..statement.span.end],
        "while (value > 0) { value -= 1; }"
    );
    assert_accepted(source);
    assert_rejected(
        "function count(value: number): number { while (value > 0) { return value; } }",
        "BTS3004",
        "can complete without returning a value",
    );
}

#[test]
fn braced_while_checks_condition_and_body_calls_with_existing_rules() {
    let prelude = "function positive(value: number): boolean { return value > 0; } ";
    assert_rejected(
        &format!("{prelude}function count(): number {{ while (positive('bad')) {{ return 1; }} return 0; }}"),
        "BTS3003",
        "argument 1 has type",
    );
    assert_rejected(
        &format!("{prelude}function count(): number {{ while (positive(1)) {{ positive('bad'); return 1; }} return 0; }}"),
        "BTS3003",
        "argument 1 has type",
    );
}

#[test]
fn unsupported_and_misplaced_syntax_is_diagnosed_not_passed_through() {
    for (source, message) in [
        (
            "export default 1;",
            "default export expressions are not in the initial BlueTS matrix",
        ),
        (
            "export default function () { return 1; }",
            "anonymous default function exports are not in the initial BlueTS matrix",
        ),
        (
            "export = foo;",
            "`export =` is not in the initial BlueTS matrix",
        ),
        (
            "<div/>;",
            "decorators and TSX/JSX are not in the initial BlueTS matrix",
        ),
        (
            "@dec class A {}",
            "decorators and TSX/JSX are not in the initial BlueTS matrix",
        ),
        (
            "class A { x = 1; }",
            "a class member other than a constructor or method",
        ),
        (
            "abstract class A {}",
            "`abstract` declarations are not in the initial BlueTS matrix",
        ),
        (
            "declare abstract class A {}",
            "`abstract` is not in the initial BlueTS matrix",
        ),
        (
            "function make() { abstract class A {} }",
            "`abstract` declarations are not in the initial BlueTS matrix",
        ),
        (
            "if (true) { abstract class A {} }",
            "`abstract` declarations are not in the initial BlueTS matrix",
        ),
        (
            "const make = () => { abstract class A {} };",
            "`abstract` declarations are not in the initial BlueTS matrix",
        ),
        (
            "function make() { class Local {} }",
            "`class` is not in the initial BlueTS matrix",
        ),
        (
            "if (true) { enum State { Ready } }",
            "`enum` is not in the initial BlueTS matrix",
        ),
        (
            "const make = () => { namespace Internal {} };",
            "`namespace` is not in the initial BlueTS matrix",
        ),
        (
            "const make = () => { module Internal {} };",
            "`module` is not in the initial BlueTS matrix",
        ),
        (
            "const decorated = () => { @sealed class A {} };",
            "decorators and TSX/JSX are not in the initial BlueTS matrix",
        ),
        (
            "const enum State { Ready }",
            "`enum` is not in the initial BlueTS matrix",
        ),
        ("enum E { A }", "`enum` is not in the initial BlueTS matrix"),
        (
            "namespace N {}",
            "`namespace` is not in the initial BlueTS matrix",
        ),
        (
            "const x?: number = 1;",
            "optional variables are not valid TypeScript declarations",
        ),
        (
            "a as number;",
            "TypeScript assertions outside a supported declaration",
        ),
        (
            "const x = 1; x satisfies number;",
            "TypeScript assertions outside a supported declaration",
        ),
        (
            "import { type A, b } from './a.ts';",
            "mixed value/type imports are not in the initial BlueTS matrix",
        ),
        (
            "export { a } from './a.ts';",
            "value re-exports from another module are not in the initial BlueTS matrix",
        ),
        (
            "type A = number; interface B extends A { x: string }",
            "interface heritage A must name an interface declaration",
        ),
        (
            "interface B<T> extends T { x: string }",
            "interface heritage T must name an interface declaration",
        ),
    ] {
        assert_rejected(source, "BTS1001", message);
    }
}

#[test]
fn syntax_errors_carry_a_precise_expectation() {
    for (source, message) in [
        ("export import a from 'x';", "an import cannot be exported"),
        (
            "declare foo;",
            "`declare` must introduce a supported declaration",
        ),
        (
            "declare interface A { x: number }",
            "`declare` must introduce a supported declaration",
        ),
        (
            "declare type A = number;",
            "`declare` must introduce a supported declaration",
        ),
        (
            "async foo;",
            "`async` must precede a function declaration in the initial matrix",
        ),
        (
            "import { type } from './a.ts';",
            "expected an imported binding",
        ),
        (
            "import { , } from './a.ts';",
            "expected an imported binding",
        ),
        (
            "import { a as } from './a.ts';",
            "expected a local import name",
        ),
        ("import { a b } from './a.ts';", "expected `}`"),
        ("import * from './a.ts';", "expected `as`"),
        (
            "import 1 from './a.ts';",
            "expected an import clause or module specifier",
        ),
        ("import", "expected an import clause or module specifier"),
        ("import x", "expected `from`"),
        ("import { a } from 1;", "expected a string module specifier"),
        (
            "export type { A } from 1;",
            "expected a string module specifier",
        ),
        (
            "export type { A as } from './a.ts';",
            "expected an exported type name",
        ),
        ("export type { A B } from './a.ts';", "expected `}`"),
        ("const x: = 1;", "expected a type"),
        ("const x: number[] | = 1;", "expected a type"),
        ("const x: [number,, string] = [1, 'a'];", "expected a type"),
        ("const x: Array<number = [];", "expected `>`"),
        ("const x: { a number } = 1;", "expected `:`"),
        ("const x: { : number } = 1;", "expected a record field name"),
        ("interface A { a }", "expected `:`"),
        ("interface A { a: number", "expected `}`"),
        ("interface { a: number }", "expected an interface name"),
        ("interface A<T extends> { a: T }", "expected a type"),
        ("interface A<T = > { a: T }", "expected a type"),
        ("interface A<T { a: T }", "expected `>`"),
        ("type = number;", "expected a type alias name"),
        ("type A<T> = ;", "expected a type"),
        ("type A = ", "expected a type"),
        ("function () {}", "expected a function name"),
        ("function f(: number) {}", "expected a parameter name"),
        ("function f(a: number", "expected `)`"),
        (
            "function f(a: number) number { return 1; }",
            "expected a function body",
        ),
        ("function f(a: number): number", "expected a function body"),
        (
            "function f(a: number): number { return 1;",
            "unterminated function body",
        ),
        ("declare function f(a: number): number", "expected `;`"),
        ("const { a } = obj;", "expected a variable name"),
        ("const [a] = arr;", "expected a variable name"),
        (
            "function f<T>(a: T): T { return a; } const x = f<number string>(1);",
            "expected a comma between type arguments",
        ),
        ("const s = 'unterminated;", "unterminated string literal"),
        ("const c = /* unterminated", "unterminated block comment"),
    ] {
        assert_rejected(source, "BTS1000", message);
    }
    // A flat destructured parameter is valid; a nested one is unsupported syntax.
    assert_accepted("function f({ a }: { a: number }): number { return a; }");
    assert_rejected(
        "function f({ a: { b } }: { a: { b: number } }): number { return b; }",
        "BTS1001",
        "this destructuring pattern is not supported yet",
    );
}

#[test]
fn checker_diagnostics_name_the_offending_types() {
    let cases: &[(&str, &str, &str)] = &[
        // Names, arity and duplicates.
        ("interface B extends Missing { x: string }", "BTS3002", "cannot find type `Missing`"),
        ("const x: Missing<number> = 1;", "BTS3002", "cannot find type `Missing`"),
        ("function f<T extends Missing>(a: T): T { return a; }", "BTS3002", "cannot find type `Missing`"),
        ("function f<T = Missing>(a: T): T { return a; }", "BTS3002", "cannot find type `Missing`"),
        (
            "interface Box<T> { v: T } const b: Box = { v: 1 };",
            "BTS3003",
            "type `Box` requires 1 to 1 type argument(s), got 0",
        ),
        (
            "interface Box<T> { v: T } const b: Box<number, string> = { v: 1 };",
            "BTS3003",
            "type `Box` requires 1 to 1 type argument(s), got 2",
        ),
        ("const a = 1; const a = 2;", "BTS3000", "duplicate declaration of `a`"),
        (
            "function f(a: number): number { return a; } function f(a: number): number { return a; }",
            "BTS3000",
            "duplicate declaration of `f`",
        ),
        ("function f<T, T>(a: T): T { return a; }", "BTS3000", "duplicate type parameter `T`"),
        (
            "function f<T = string, U>(a: T): T { return a; }",
            "BTS3003",
            "required type parameter `U` cannot follow a defaulted type parameter",
        ),
        (
            "function f<T extends number = string>(a: T): T { return a; }",
            "BTS3003",
            "default type `string` does not satisfy constraint `number` for `T`",
        ),
        // Overloads.
        (
            "function f(a: number): number { return a; } function f(a: string): string;",
            "BTS3003",
            "overload signature for f must precede its implementation",
        ),
        (
            "function f(a: number): number;",
            "BTS3003",
            "overload signature for f requires an implementation",
        ),
        // Interface heritage compatibility.
        (
            "interface P { a: number } interface C extends P { a: string }",
            "BTS3003",
            "property `a` is not compatible with the inherited type `number`",
        ),
        (
            "interface P { a: number } interface C extends P { a?: number }",
            "BTS3003",
            "property `a` is not compatible with the inherited type `number`",
        ),
        (
            "interface P<T> { a: T } interface C extends P<number> { a: string }",
            "BTS3003",
            "property `a` is not compatible with the inherited type `number`",
        ),
        // Assignability of initializers and returns.
        (
            "const x: number = 1; const y: string = x;",
            "BTS3003",
            "initializer has type `number`, which is not assignable to `string`",
        ),
        (
            "function f(a: number) { const b: string = a; return b; }",
            "BTS3003",
            "initializer has type `number`, which is not assignable to `string`",
        ),
        (
            "function f(a: number): number { return a; } const x: string = f(1);",
            "BTS3003",
            "initializer has type `number`, which is not assignable to `string`",
        ),
        (
            "declare const x: number; const y: string = x;",
            "BTS3003",
            "initializer has type `number`, which is not assignable to `string`",
        ),
        (
            "declare function f(a: number): number; const y: string = f(1);",
            "BTS3003",
            "initializer has type `number`, which is not assignable to `string`",
        ),
        (
            "type T = { x: number }; const t: T = { x: 1 }; const n: string = t.x;",
            "BTS3003",
            "initializer has type `number`, which is not assignable to `string`",
        ),
        (
            "const a: number | string = true;",
            "BTS3003",
            "initializer has type `boolean`, which is not assignable to `number | string`",
        ),
        (
            "const a: 'x' | 'y' = 'z';",
            "BTS3003",
            "initializer has type `string`, which is not assignable to `'x' | 'y'`",
        ),
        (
            "const a: number[] = [1, 'x'];",
            "BTS3003",
            "which is not assignable to `number[]`",
        ),
        (
            "const a: [number, string] = [1];",
            "BTS3003",
            "which is not assignable to `[number, string]`",
        ),
        (
            "const a: [number, string] = [1, 'x', 3];",
            "BTS3003",
            "which is not assignable to `[number, string]`",
        ),
        (
            "interface A { x: number } const a: A = { x: 'no' };",
            "BTS3003",
            "which is not assignable to `A`",
        ),
        (
            "interface A { x: number } const a: A = {};",
            "BTS3003",
            "which is not assignable to `A`",
        ),
        (
            "const a: { x: number } & { y: number } = { x: 1 };",
            "BTS3003",
            "which is not assignable to",
        ),
        (
            "interface A { x: number } const a: A = { x: 1 }; const b: number = a.y;",
            "BTS3003",
            "property `y` does not exist on type `A`",
        ),
        // Type re-exports.
        ("export type { A };", "BTS3002", "cannot re-export unknown type `A`"),
        (
            "export type { A } from './a.ts';",
            "BTS3002",
            "cannot re-export unknown type `A`",
        ),
    ];
    for (source, code, message) in cases {
        assert_rejected(source, code, message);
    }
}

#[test]
fn supported_programs_are_accepted() {
    for source in [
        "const x: readonly number[] = [1];",
        "const x: { a: number, b?: string; readonly c: boolean } = { a: 1, c: true };",
        "interface A { readonly a: number; b?: string, c: number }",
        "interface A<T,> { a: T }",
        "interface P { a?: number } interface C extends P { a: number }",
        "function f(a: number = 1, b?: string): number { return a; }",
        "function f(a: number, ...rest: number[]): number { return a; }",
        "declare function f(a: number): number;",
        "declare const x: number;",
        "const x: number = 1 as number;",
        "function f<T>(a: T): T { return a; } const x = f<number>(1);",
        "function f<T>(a: T): T { return a; } const x = f<number,>(1);",
        "const x = y!;",
        "const x = y!.z;",
        "const keywords = { class: 1, enum: 2, namespace: 3, module: 4 };",
        "interface Box<T> { value: T } const box: Box<Box<number>> = { value: { value: 1 } };",
        "let a = 1, b = 2;",
        "function f() { const x: number = 1; let y: string = 'a'; var z = 3; return x; }",
        "function f() { if (true) { return 1; } return 2; }",
        "function f() { return; }",
        "export function f(): number { return 1; }",
        "const a = 1; export { a };",
        "export type X = number; export interface Y {}",
        "type A = number; export type { A }; export type { A as B };",
        "import type { Shape } from './a.ts'; const s: Shape = { x: 1 };",
        "import type { Shape as S, Id } from './a.ts'; const s: S = { x: 1 }; const i: Id = 1;",
        "import type * as N from './a.ts'; const s: N.Shape = { x: 1 };",
        "import type * as N from './a.ts'; const box: N.Box<N.Box<number>> = { value: { value: 1 } };",
        "import './a.ts';",
        "import { a, b as c } from './a.ts'; export const total: number = a;",
        "import * as ns from './a.ts';",
        "export type { Shape } from './a.ts';",
        "export type { Shape as Renamed, Id } from './a.ts';",
        "export type * from './a.ts';",
        "export type {} from './a.ts';",
        "const template = `t ${1}`; const re = /re/g; const big = 1n; const hex = 0x1F + 1e3 + .5;",
        "const text = \"a\\\"b\";",
        "function g(a: number): number; function g(a: string): string; function g(a: number | string): number | string { return a; }",
    ] {
        assert_accepted(source);
    }
}

#[test]
fn type_syntax_is_erased_from_the_emitted_javascript() {
    let source = "\
const ro: readonly number[] = [1];
interface A<T,> { readonly a: T; b?: string, c: number }
function withDefault(a: number = 1, b?: string): number { return a; }
function rest(a: number, ...others: number[]): number { return a; }
declare function external(a: number): number;
declare const externalValue: number;
function id<T>(a: T): T { return a; }
const called = id<number>(1);
const nonNull = called!;
const asserted = called as number;
function locals() { const x: number = 1; let y: string = 'a'; var z = 3; return x as number; }
type Alias = number | string;
export type { Alias };
export { nonNull };
";
    let javascript = emitted(source);
    for erased in [
        ": number",
        ": string",
        "interface",
        "readonly",
        "declare",
        "external",
        "<number>",
        "<T>",
        "as number",
        "type Alias",
        "b?",
        "!;",
    ] {
        assert!(
            !javascript.contains(erased),
            "`{erased}` must be erased from:\n{javascript}"
        );
    }
    for kept in [
        "const ro",
        "function withDefault(a",
        "= 1, b)",
        "function rest(a, ...others)",
        "function id(a)",
        "const called = id(1);",
        "const nonNull = called;",
        "const asserted = called",
        "function locals()",
        "export { nonNull };",
    ] {
        assert!(
            javascript.contains(kept),
            "`{kept}` must survive in:\n{javascript}"
        );
    }
}

#[test]
fn optional_tuple_elements_check_and_emit_at_public_boundary() {
    let valid = include_str!("fixtures/typescript_oracle/tuple-optional-valid/main.ts");
    let accepted = compile(
        ENTRY,
        &MapLoader::from([ModuleSource::new(ENTRY, valid)]),
        CompilerOptions {
            declaration: true,
            ..CompilerOptions::default()
        },
    );
    assert!(
        accepted.diagnostics.is_empty(),
        "{:#?}",
        accepted.diagnostics
    );
    let artifact = &accepted.output.unwrap().artifacts[ENTRY];
    assert!(artifact.javascript.contains("export const one"));
    assert!(!artifact.javascript.contains("string?"));
    let declaration = artifact.declaration.as_deref().unwrap();
    for name in ["one", "two", "undef"] {
        assert!(
            declaration.contains(&format!("{name}: [number, string?]")),
            "{declaration}"
        );
    }

    for (source, code) in [
        (
            include_str!("fixtures/typescript_oracle/tuple-optional-wrong-type/main.ts"),
            DiagnosticCode::TypeMismatch,
        ),
        (
            include_str!("fixtures/typescript_oracle/tuple-optional-required-after/main.ts"),
            DiagnosticCode::ParseError,
        ),
    ] {
        let rejected = compile(
            ENTRY,
            &MapLoader::from([ModuleSource::new(ENTRY, source)]),
            CompilerOptions::default(),
        );
        assert!(rejected.output.is_none());
        assert!(
            rejected
                .diagnostics
                .iter()
                .any(|diagnostic| { diagnostic.code == code && diagnostic.span.module == ENTRY }),
            "{:#?}",
            rejected.diagnostics
        );
    }

    let indexed = include_str!("fixtures/typescript_oracle/tuple-optional-index-valid/main.ts");
    let indexed_result = compile(
        ENTRY,
        &MapLoader::from([ModuleSource::new(ENTRY, indexed)]),
        CompilerOptions::default(),
    );
    assert!(
        indexed_result.diagnostics.is_empty(),
        "{:#?}",
        indexed_result.diagnostics
    );
    let indexed_error =
        include_str!("fixtures/typescript_oracle/tuple-optional-index-error/main.ts");
    let indexed_result = compile(
        ENTRY,
        &MapLoader::from([ModuleSource::new(ENTRY, indexed_error)]),
        CompilerOptions::default(),
    );
    assert!(indexed_result.output.is_none());
    assert!(
        indexed_result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == DiagnosticCode::TypeMismatch),
        "{:#?}",
        indexed_result.diagnostics
    );
}

#[test]
fn labeled_tuple_elements_check_and_emit_at_public_boundary() {
    for (source, expected) in [
        (
            include_str!("fixtures/typescript_oracle/tuple-labeled-valid/main.ts"),
            "Pair = [first: number, second?: string]",
        ),
        (
            include_str!("fixtures/typescript_oracle/tuple-labeled-mixed-valid/main.ts"),
            "Mixed = [first: number, string]",
        ),
    ] {
        let accepted = compile(
            ENTRY,
            &MapLoader::from([ModuleSource::new(ENTRY, source)]),
            CompilerOptions {
                declaration: true,
                ..CompilerOptions::default()
            },
        );
        assert!(
            accepted.diagnostics.is_empty(),
            "{:#?}",
            accepted.diagnostics
        );
        let artifact = &accepted.output.unwrap().artifacts[ENTRY];
        assert!(!artifact.javascript.contains("first:"));
        assert!(
            artifact.declaration.as_deref().unwrap().contains(expected),
            "{:#?}",
            artifact.declaration
        );
    }

    let source = include_str!("fixtures/typescript_oracle/tuple-labeled-type-error/main.ts");
    let rejected = compile(
        ENTRY,
        &MapLoader::from([ModuleSource::new(ENTRY, source)]),
        CompilerOptions::default(),
    );
    assert!(rejected.output.is_none());
    assert!(
        rejected.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == DiagnosticCode::TypeMismatch && diagnostic.span.module == ENTRY
        }),
        "{:#?}",
        rejected.diagnostics
    );
}

#[test]
fn trailing_tuple_rest_checks_and_emits_at_public_boundary() {
    let source = include_str!("fixtures/typescript_oracle/tuple-rest-trailing-valid/main.ts");
    let accepted = compile(
        ENTRY,
        &MapLoader::from([ModuleSource::new(ENTRY, source)]),
        CompilerOptions {
            declaration: true,
            ..CompilerOptions::default()
        },
    );
    assert!(
        accepted.diagnostics.is_empty(),
        "{:#?}",
        accepted.diagnostics
    );
    let artifact = &accepted.output.unwrap().artifacts[ENTRY];
    assert!(!artifact.javascript.contains("...tail: string[]"));
    let declaration = artifact.declaration.as_deref().unwrap();
    for expected in [
        "Trail = [head: number, ...tail: string[]]",
        "empty: [head: number, ...tail: string[]]",
        "many: [head: number, ...tail: string[]]",
        "optional: [head?: number, ...tail: string[]]",
        "optionalFilled: [head?: number, ...tail: string[]]",
        "optionalFromArray: [head?: number, ...tail: number[]]",
    ] {
        assert!(declaration.contains(expected), "{declaration}");
    }

    for (source, code) in [
        (
            include_str!("fixtures/typescript_oracle/tuple-rest-trailing-type-error/main.ts"),
            DiagnosticCode::TypeMismatch,
        ),
        (
            include_str!("fixtures/typescript_oracle/tuple-rest-trailing-arity-error/main.ts"),
            DiagnosticCode::TypeMismatch,
        ),
        (
            include_str!("fixtures/typescript_oracle/tuple-rest-trailing-index-error/main.ts"),
            DiagnosticCode::TypeMismatch,
        ),
        (
            include_str!(
                "fixtures/typescript_oracle/tuple-rest-trailing-optional-after-error/main.ts"
            ),
            DiagnosticCode::ParseError,
        ),
    ] {
        let rejected = compile(
            ENTRY,
            &MapLoader::from([ModuleSource::new(ENTRY, source)]),
            CompilerOptions::default(),
        );
        assert!(rejected.output.is_none());
        assert!(
            rejected
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == code && diagnostic.span.module == ENTRY),
            "{:#?}",
            rejected.diagnostics
        );
    }
}

#[test]
fn concrete_named_tuple_spreads_check_and_emit_at_public_boundary() {
    let source = include_str!("fixtures/typescript_oracle/tuple-spread-concrete-valid/main.ts");
    let accepted = compile(
        ENTRY,
        &MapLoader::from([ModuleSource::new(ENTRY, source)]),
        CompilerOptions {
            declaration: true,
            ..CompilerOptions::default()
        },
    );
    assert!(
        accepted.diagnostics.is_empty(),
        "{:#?}",
        accepted.diagnostics
    );
    let artifact = &accepted.output.unwrap().artifacts[ENTRY];
    let declaration = artifact.declaration.as_deref().unwrap();
    for expected in [
        "WithHead = [boolean, ...Pair]",
        "WithTail = [...WithHead, null]",
        "RequireTail = [...OptionalPrefix, string]",
        "direct: [...Pair]",
    ] {
        assert!(declaration.contains(expected), "{declaration}");
    }

    for source in [
        include_str!("fixtures/typescript_oracle/tuple-spread-concrete-arity-error/main.ts"),
        include_str!("fixtures/typescript_oracle/tuple-spread-concrete-type-error/main.ts"),
        include_str!(
            "fixtures/typescript_oracle/tuple-spread-concrete-optional-suffix-error/main.ts"
        ),
        include_str!(
            "fixtures/typescript_oracle/tuple-spread-concrete-optional-arity-error/main.ts"
        ),
    ] {
        let rejected = compile(
            ENTRY,
            &MapLoader::from([ModuleSource::new(ENTRY, source)]),
            CompilerOptions::default(),
        );
        assert!(rejected.output.is_none());
        assert!(
            rejected.diagnostics.iter().any(|diagnostic| {
                diagnostic.code == DiagnosticCode::TypeMismatch && diagnostic.span.module == ENTRY
            }),
            "{:#?}",
            rejected.diagnostics
        );
    }

    for source in [
        "type Primitive = number; export type Bad = [...Primitive];",
        "export type Loop = [...Loop];",
        "export type Missing = [...Absent];",
    ] {
        let rejected = compile(
            ENTRY,
            &MapLoader::from([ModuleSource::new(ENTRY, source)]),
            CompilerOptions::default(),
        );
        assert!(rejected.output.is_none(), "{source}");
        assert!(
            rejected.diagnostics.iter().any(|diagnostic| {
                matches!(
                    diagnostic.code,
                    DiagnosticCode::UnsupportedSyntax | DiagnosticCode::UnknownType
                ) && diagnostic.span.module == ENTRY
            }),
            "{source}: {:#?}",
            rejected.diagnostics
        );
    }

    let bounded = compile(
        ENTRY,
        &MapLoader::from([ModuleSource::new(
            ENTRY,
            "type Pair = [number, string]; export type Spread = [...Pair];",
        )]),
        CompilerOptions {
            limits: CompilerLimits {
                max_type_expansions: 2,
                ..CompilerLimits::default()
            },
            ..CompilerOptions::default()
        },
    );
    assert!(bounded.output.is_none());
    assert!(bounded
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == DiagnosticCode::ResourceLimit));

    let override_pending = compile(
        ENTRY,
        &MapLoader::from([ModuleSource::new(
            ENTRY,
            "type Pair = [number, string]; class Base { method(...args: [...Pair]): void {} } class Derived extends Base { override method(...args: [string, string]): void {} }",
        )]),
        CompilerOptions::default(),
    );
    assert!(override_pending.output.is_none());
    assert!(override_pending
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == DiagnosticCode::UnsupportedSyntax));
}

#[test]
fn concrete_generic_tuple_spreads_specialize_at_public_boundary() {
    let source = include_str!("fixtures/typescript_oracle/tuple-spread-generic-valid/main.ts");
    let accepted = compile(
        ENTRY,
        &MapLoader::from([ModuleSource::new(ENTRY, source)]),
        CompilerOptions {
            declaration: true,
            ..CompilerOptions::default()
        },
    );
    assert!(
        accepted.diagnostics.is_empty(),
        "{:#?}",
        accepted.diagnostics
    );
    let declaration = accepted.output.unwrap().artifacts[ENTRY]
        .declaration
        .clone()
        .unwrap();
    assert!(
        declaration.contains("Prefix<T extends unknown[]> = [number, ...T]"),
        "{declaration}"
    );
    assert!(
        declaration.contains("Tail<T extends string[] = string[]> = [boolean, ...T, number]"),
        "{declaration}"
    );

    for source in [
        include_str!("fixtures/typescript_oracle/tuple-spread-generic-arity-error/main.ts"),
        include_str!("fixtures/typescript_oracle/tuple-spread-generic-type-error/main.ts"),
        include_str!("fixtures/typescript_oracle/tuple-spread-generic-constraint-error/main.ts"),
        include_str!("fixtures/typescript_oracle/tuple-spread-generic-tail-error/main.ts"),
    ] {
        let rejected = compile(
            ENTRY,
            &MapLoader::from([ModuleSource::new(ENTRY, source)]),
            CompilerOptions::default(),
        );
        assert!(
            rejected.output.is_none(),
            "{source}: {:#?}",
            rejected.diagnostics
        );
        assert!(
            rejected.diagnostics.iter().any(|diagnostic| {
                diagnostic.code == DiagnosticCode::TypeMismatch && diagnostic.span.module == ENTRY
            }),
            "{:#?}",
            rejected.diagnostics
        );
    }
    for source in [
        include_str!("fixtures/typescript_oracle/tuple-spread-generic-nontuple-error/main.ts"),
        include_str!("fixtures/typescript_oracle/tuple-spread-generic-unconstrained-error/main.ts"),
    ] {
        let rejected = compile(
            ENTRY,
            &MapLoader::from([ModuleSource::new(ENTRY, source)]),
            CompilerOptions::default(),
        );
        assert!(rejected.output.is_none());
        assert!(rejected.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == DiagnosticCode::UnsupportedSyntax && diagnostic.span.module == ENTRY
        }));
    }
}

#[test]
fn named_tuple_spread_class_overrides_compare_expanded_positions() {
    let source =
        include_str!("fixtures/typescript_oracle/class-override-named-spread-valid/main.ts");
    let accepted = compile(
        ENTRY,
        &MapLoader::from([ModuleSource::new(ENTRY, source)]),
        CompilerOptions::default(),
    );
    assert!(accepted.output.is_some());
    assert!(
        accepted.diagnostics.is_empty(),
        "{:#?}",
        accepted.diagnostics
    );

    for source in [
        include_str!(
            "fixtures/typescript_oracle/class-override-named-spread-derived-error/main.ts"
        ),
        include_str!(
            "fixtures/typescript_oracle/class-override-named-spread-inherited-error/main.ts"
        ),
        include_str!("fixtures/typescript_oracle/class-override-named-spread-static-error/main.ts"),
    ] {
        let rejected = compile(
            ENTRY,
            &MapLoader::from([ModuleSource::new(ENTRY, source)]),
            CompilerOptions::default(),
        );
        assert!(rejected.output.is_none());
        assert!(
            rejected.diagnostics.iter().any(|diagnostic| {
                diagnostic.code == DiagnosticCode::TypeMismatch && diagnostic.span.module == ENTRY
            }),
            "{source}: {:#?}",
            rejected.diagnostics
        );
    }

    let bounded = compile(
        ENTRY,
        &MapLoader::from([ModuleSource::new(
            ENTRY,
            "type Trio = [number, string, boolean]; class Base { method(...args: Trio): void {} }",
        )]),
        CompilerOptions {
            limits: CompilerLimits {
                max_type_expansions: 2,
                ..CompilerLimits::default()
            },
            ..CompilerOptions::default()
        },
    );
    assert!(bounded.output.is_none());
    assert!(bounded
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == DiagnosticCode::ResourceLimit));
}

#[test]
fn generic_functions_retain_symbolic_tuple_spread_constraints() {
    let source =
        include_str!("fixtures/typescript_oracle/tuple-spread-symbolic-function-valid/main.ts");
    let accepted = compile(
        ENTRY,
        &MapLoader::from([ModuleSource::new(ENTRY, source)]),
        CompilerOptions {
            declaration: true,
            ..CompilerOptions::default()
        },
    );
    assert!(
        accepted.diagnostics.is_empty(),
        "{:#?}",
        accepted.diagnostics
    );
    let artifact = &accepted.output.unwrap().artifacts[ENTRY];
    assert!(!artifact.javascript.contains("extends string[]"));
    let declaration = artifact.declaration.as_deref().unwrap();
    assert!(
        declaration.contains("consume<T extends string[]>"),
        "{declaration}"
    );
    assert!(
        declaration.contains("direct<T extends string[]>"),
        "{declaration}"
    );

    for (source, code) in [
        (
            include_str!("fixtures/typescript_oracle/tuple-spread-symbolic-function-unconstrained-error/main.ts"),
            DiagnosticCode::UnsupportedSyntax,
        ),
        (
            include_str!("fixtures/typescript_oracle/tuple-spread-symbolic-function-constraint-error/main.ts"),
            DiagnosticCode::TypeMismatch,
        ),
    ] {
        let rejected = compile(
            ENTRY,
            &MapLoader::from([ModuleSource::new(ENTRY, source)]),
            CompilerOptions::default(),
        );
        assert!(rejected.output.is_none());
        assert!(rejected.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == code && diagnostic.span.module == ENTRY
        }));
    }
}

#[test]
fn symbolic_tuple_spreads_compare_identical_tails_and_safe_widening() {
    let source =
        include_str!("fixtures/typescript_oracle/tuple-spread-symbolic-compare-valid/main.ts");
    let accepted = compile(
        ENTRY,
        &MapLoader::from([ModuleSource::new(ENTRY, source)]),
        CompilerOptions::default(),
    );
    assert!(
        accepted.diagnostics.is_empty(),
        "{:#?}",
        accepted.diagnostics
    );
    assert!(accepted.output.is_some());

    for source in [
        include_str!(
            "fixtures/typescript_oracle/tuple-spread-symbolic-compare-distinct-error/main.ts"
        ),
        include_str!(
            "fixtures/typescript_oracle/tuple-spread-symbolic-compare-narrowing-error/main.ts"
        ),
    ] {
        let rejected = compile(
            ENTRY,
            &MapLoader::from([ModuleSource::new(ENTRY, source)]),
            CompilerOptions::default(),
        );
        assert!(rejected.output.is_none());
        assert!(rejected.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == DiagnosticCode::ReturnTypeMismatch && diagnostic.span.module == ENTRY
        }));
    }
}

#[test]
fn symbolic_tuple_spreads_close_unresolved_cyclic_unsupported_and_over_budget() {
    let compile_source = |source: &str, max_type_expansions: usize| {
        compile(
            ENTRY,
            &MapLoader::from([ModuleSource::new(ENTRY, source)]),
            CompilerOptions {
                limits: CompilerLimits {
                    max_type_expansions,
                    ..CompilerLimits::default()
                },
                ..CompilerOptions::default()
            },
        )
    };
    let accepted = compile_source(
        include_str!("fixtures/typescript_oracle/tuple-spread-symbolic-close-nested-valid/main.ts"),
        256,
    );
    assert!(
        accepted.diagnostics.is_empty(),
        "{:#?}",
        accepted.diagnostics
    );
    assert!(accepted.output.is_some());

    // Rejected by both BlueTS and pinned TypeScript.
    for source in [
        include_str!(
            "fixtures/typescript_oracle/tuple-spread-symbolic-close-unresolved-error/main.ts"
        ),
        include_str!("fixtures/typescript_oracle/tuple-spread-symbolic-close-cyclic-error/main.ts"),
        include_str!(
            "fixtures/typescript_oracle/tuple-spread-symbolic-close-nonarray-error/main.ts"
        ),
    ] {
        let rejected = compile_source(source, 256);
        assert!(rejected.output.is_none(), "{source}");
        assert!(
            rejected
                .diagnostics
                .iter()
                .all(|diagnostic| diagnostic.span.module == ENTRY),
            "{:#?}",
            rejected.diagnostics
        );
        assert!(!rejected.diagnostics.is_empty(), "{source}");
    }

    // Accepted by pinned TypeScript, deliberately rejected by BlueTS before
    // output: two symbolic rests and a union constraint have no bounded
    // comparison yet.
    for (source, code) in [
        (
            "export function f<T extends string[], U extends number[]>(v: [...T, ...U]): void {}",
            DiagnosticCode::ParseError,
        ),
        (
            "export function f<T extends string[] | number[]>(v: [...T]): void {}",
            DiagnosticCode::UnsupportedSyntax,
        ),
    ] {
        let rejected = compile_source(source, 256);
        assert!(rejected.output.is_none(), "{source}");
        assert!(
            rejected
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == code),
            "{source}: {:#?}",
            rejected.diagnostics
        );
    }

    let bounded = compile_source(
        include_str!("fixtures/typescript_oracle/tuple-spread-symbolic-close-nested-valid/main.ts"),
        1,
    );
    assert!(bounded.output.is_none());
    assert!(bounded
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == DiagnosticCode::ResourceLimit));
}

#[test]
fn class_overrides_compare_inherited_overload_sets() {
    let compile_source = |source: &str| {
        compile(
            ENTRY,
            &MapLoader::from([ModuleSource::new(ENTRY, source)]),
            CompilerOptions::default(),
        )
    };
    for source in [
        include_str!("fixtures/typescript_oracle/class-override-overload-valid/main.ts"),
        include_str!("fixtures/typescript_oracle/class-override-overload-edge-valid/main.ts"),
        // Rest-bearing signatures are outside the compared subset.
        "class A { m(...a: string[]): void; m(a: number): void; m(a: any): void {} } \
         class B extends A { m(a: string): void { } }",
    ] {
        let accepted = compile_source(source);
        assert!(accepted.output.is_some());
        assert!(
            accepted.diagnostics.is_empty(),
            "{source}: {:#?}",
            accepted.diagnostics
        );
    }

    for source in [
        include_str!(
            "fixtures/typescript_oracle/class-override-overload-single-wide-error/main.ts"
        ),
        include_str!("fixtures/typescript_oracle/class-override-overload-missing-error/main.ts"),
        include_str!("fixtures/typescript_oracle/class-override-overload-return-error/main.ts"),
        include_str!("fixtures/typescript_oracle/class-override-overload-static-error/main.ts"),
        include_str!("fixtures/typescript_oracle/class-override-overload-ancestor-error/main.ts"),
        include_str!("fixtures/typescript_oracle/class-override-overload-required-error/main.ts"),
    ] {
        let rejected = compile_source(source);
        assert!(rejected.output.is_none());
        assert!(
            rejected.diagnostics.iter().any(|diagnostic| {
                diagnostic.code == DiagnosticCode::TypeMismatch
                    && diagnostic.span.module == ENTRY
                    && diagnostic.message.contains("inherited overload set")
            }),
            "{source}: {:#?}",
            rejected.diagnostics
        );
    }

    let bounded = compile(
        ENTRY,
        &MapLoader::from([ModuleSource::new(
            ENTRY,
            "type N = number; \
             class A { m(a: N): N; m(a: string): string; m(a: N | string): N | string { return a; } } \
             class B extends A { m(a: number): number; m(a: string): string; \
             m(a: number | string): number | string { return a; } }",
        )]),
        CompilerOptions {
            limits: CompilerLimits {
                max_type_expansions: 1,
                ..CompilerLimits::default()
            },
            ..CompilerOptions::default()
        },
    );
    assert!(bounded.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == DiagnosticCode::ResourceLimit
            && diagnostic.message.contains("overload comparison")
    }));
}

#[test]
fn derived_constructors_check_super_calls_and_placement() {
    let compile_source = |source: &str, max_type_expansions: usize| {
        compile(
            ENTRY,
            &MapLoader::from([ModuleSource::new(ENTRY, source)]),
            CompilerOptions {
                limits: CompilerLimits {
                    max_type_expansions,
                    ..CompilerLimits::default()
                },
                ..CompilerOptions::default()
            },
        )
    };
    let accepted = compile_source(
        include_str!("fixtures/typescript_oracle/class-super-call-valid/main.ts"),
        256,
    );
    assert!(accepted.output.is_some());
    assert!(
        accepted.diagnostics.is_empty(),
        "{:#?}",
        accepted.diagnostics
    );

    for source in [
        include_str!("fixtures/typescript_oracle/class-super-call-missing-error/main.ts"),
        include_str!("fixtures/typescript_oracle/class-super-call-count-error/main.ts"),
        include_str!("fixtures/typescript_oracle/class-super-call-type-error/main.ts"),
        include_str!("fixtures/typescript_oracle/class-super-call-this-error/main.ts"),
        include_str!("fixtures/typescript_oracle/class-super-call-overload-error/main.ts"),
        include_str!("fixtures/typescript_oracle/class-super-call-base-error/main.ts"),
    ] {
        let rejected = compile_source(source, 256);
        assert!(rejected.output.is_none());
        assert!(
            rejected.diagnostics.iter().any(|diagnostic| {
                diagnostic.code == DiagnosticCode::TypeMismatch
                    && diagnostic.span.module == ENTRY
                    && diagnostic.message.contains("`super`")
            }),
            "{source}: {:#?}",
            rejected.diagnostics
        );
    }

    let bounded = compile_source(
        "type N = number; class A { constructor(x: N, y: N, z: N) {} } \
         class D extends A { constructor() { super(1, 2, 3); } }",
        1,
    );
    assert!(bounded.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == DiagnosticCode::ResourceLimit && diagnostic.message.contains("`super`")
    }));
}

#[test]
fn super_member_reads_and_calls_use_the_base_side() {
    let compile_source = |source: &str| {
        compile(
            ENTRY,
            &MapLoader::from([ModuleSource::new(ENTRY, source)]),
            CompilerOptions::default(),
        )
    };
    let accepted = compile_source(include_str!(
        "fixtures/typescript_oracle/class-super-member-valid/main.ts"
    ));
    assert!(accepted.output.is_some());
    assert!(
        accepted.diagnostics.is_empty(),
        "{:#?}",
        accepted.diagnostics
    );

    for (source, code) in [
        (
            include_str!("fixtures/typescript_oracle/class-super-member-result-error/main.ts"),
            DiagnosticCode::ReturnTypeMismatch,
        ),
        (
            include_str!("fixtures/typescript_oracle/class-super-member-argtype-error/main.ts"),
            DiagnosticCode::TypeMismatch,
        ),
        (
            include_str!("fixtures/typescript_oracle/class-super-member-argcount-error/main.ts"),
            DiagnosticCode::TypeMismatch,
        ),
        (
            include_str!("fixtures/typescript_oracle/class-super-member-missing-error/main.ts"),
            DiagnosticCode::TypeMismatch,
        ),
        (
            include_str!(
                "fixtures/typescript_oracle/class-super-member-instance-side-error/main.ts"
            ),
            DiagnosticCode::TypeMismatch,
        ),
        (
            include_str!("fixtures/typescript_oracle/class-super-member-static-side-error/main.ts"),
            DiagnosticCode::TypeMismatch,
        ),
        (
            include_str!("fixtures/typescript_oracle/class-super-member-overload-error/main.ts"),
            DiagnosticCode::ReturnTypeMismatch,
        ),
        (
            include_str!("fixtures/typescript_oracle/class-super-member-nobase-error/main.ts"),
            DiagnosticCode::TypeMismatch,
        ),
        (
            include_str!("fixtures/typescript_oracle/class-super-member-method-call-error/main.ts"),
            DiagnosticCode::TypeMismatch,
        ),
    ] {
        let rejected = compile_source(source);
        assert!(rejected.output.is_none());
        assert!(
            rejected
                .diagnostics
                .iter()
                .any(|diagnostic| { diagnostic.code == code && diagnostic.span.module == ENTRY }),
            "{source}: {:#?}",
            rejected.diagnostics
        );
    }
}

fn emit_all_artifacts_options() -> CompilerOptions {
    CompilerOptions {
        source_map: true,
        declaration: true,
        ..CompilerOptions::default()
    }
}

fn normalized_javascript(javascript: &str) -> String {
    javascript
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[test]
fn class_output_erases_types_and_overload_signatures() {
    let source = include_str!("fixtures/typescript_oracle/class-emit-runtime/main.ts");
    let compiled = compile(
        ENTRY,
        &MapLoader::from([ModuleSource::new(ENTRY, source)]),
        emit_all_artifacts_options(),
    );
    assert!(
        compiled.diagnostics.is_empty(),
        "{:#?}",
        compiled.diagnostics
    );
    let artifact = &compiled.output.unwrap().artifacts[ENTRY];
    assert_eq!(
        normalized_javascript(&artifact.javascript),
        "class Shape { constructor(name, sides) { console.log('shape ' + name); } \
         describe(prefix){ return prefix + 'shape'; } scale(factor){ return factor; } \
         static create(name){ return 'made ' + name; } } \
         class Square extends Shape { constructor() { super('square', 4); console.log('square'); } \
         describe(prefix){ return super.describe(prefix) + ' square'; } \
         static create(name){ return super.create(name) + '!'; } } \
         const square = new Square(); console.log(square.describe('a ')); \
         console.log(square.scale(2)); console.log(Square.create('x'));"
    );
    let map = artifact.source_map.as_ref().expect("source map requested");
    assert!(!map.mappings.is_empty());
}

#[test]
fn class_declaration_emit_matches_typescript_shape() {
    let source = include_str!("fixtures/typescript_oracle/class-emit-declaration/main.ts");
    let compiled = compile(
        ENTRY,
        &MapLoader::from([ModuleSource::new(ENTRY, source)]),
        emit_all_artifacts_options(),
    );
    assert!(
        compiled.diagnostics.is_empty(),
        "{:#?}",
        compiled.diagnostics
    );
    let artifact = &compiled.output.unwrap().artifacts[ENTRY];
    assert_eq!(
        artifact.declaration.as_deref(),
        Some(
            "export declare class Shape {\n    constructor(name: string);\n    \
             constructor(name: string, sides: number);\n    describe(prefix: string): string;\n    \
             scale(factor: number): number;\n    scale(factor: string): string;\n    \
             static create(name: string): string;\n}\n\
             export declare class Square extends Shape {\n    constructor();\n    \
             describe(prefix: string): string;\n}\n"
        )
    );
}

#[test]
fn class_output_stays_atomic_and_refuses_unstructured_classes() {
    // A checker error suppresses all output, even with the switch on.
    let rejected = compile(
        ENTRY,
        &MapLoader::from([ModuleSource::new(
            ENTRY,
            "class A { constructor(x: number) {} } \
             class B extends A { constructor() { super('a'); } }",
        )]),
        emit_all_artifacts_options(),
    );
    assert!(rejected.output.is_none());
    assert!(rejected
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == DiagnosticCode::TypeMismatch));

    // A class with a member outside the structured grammar cannot be emitted
    // faithfully, so it stays refused.
    let opaque = compile(
        ENTRY,
        &MapLoader::from([ModuleSource::new(ENTRY, "class A { value = 1; }")]),
        emit_all_artifacts_options(),
    );
    assert!(opaque.output.is_none());
    assert!(opaque
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == DiagnosticCode::UnsupportedSyntax));

    // Declaration output needs a stated result type for every method.
    let untyped = compile(
        ENTRY,
        &MapLoader::from([ModuleSource::new(
            ENTRY,
            "export class A { m() { return 1; } }",
        )]),
        emit_all_artifacts_options(),
    );
    assert!(untyped.output.is_none());
}

#[test]
fn parenthesized_types_group_unions_and_function_types() {
    let compiled = compile(
        ENTRY,
        &MapLoader::from([ModuleSource::new(
            ENTRY,
            include_str!("fixtures/typescript_oracle/paren-type-valid/main.ts"),
        )]),
        CompilerOptions {
            declaration: true,
            ..CompilerOptions::default()
        },
    );
    assert!(
        compiled.diagnostics.is_empty(),
        "{:#?}",
        compiled.diagnostics
    );
    let declaration = compiled.output.unwrap().artifacts[ENTRY]
        .declaration
        .clone()
        .unwrap();
    for line in [
        "export declare function f(v: (string | number)[]): void;",
        "export declare function g(v: [boolean, ...(string | number)[]]): void;",
        "export declare function h(v: ((a: number) => string)[]): void;",
        "export type Mixed = (string | number)[];",
        "export declare function m(v: Mixed): void;",
    ] {
        assert!(declaration.contains(line), "{line}\n{declaration}");
    }

    let accepted = compile(
        ENTRY,
        &MapLoader::from([ModuleSource::new(
            ENTRY,
            include_str!("fixtures/typescript_oracle/paren-type-class-override-valid/main.ts"),
        )]),
        CompilerOptions::default(),
    );
    assert!(
        accepted.diagnostics.is_empty(),
        "{:#?}",
        accepted.diagnostics
    );
    let rejected = compile(
        ENTRY,
        &MapLoader::from([ModuleSource::new(
            ENTRY,
            include_str!("fixtures/typescript_oracle/paren-type-class-override-error/main.ts"),
        )]),
        CompilerOptions::default(),
    );
    assert!(rejected
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == DiagnosticCode::TypeMismatch));

    // A grouped element type is not the same type as an ungrouped union.
    let identity = compile(
        ENTRY,
        &MapLoader::from([ModuleSource::new(
            ENTRY,
            include_str!("fixtures/typescript_oracle/paren-type-identity-error/main.ts"),
        )]),
        CompilerOptions::default(),
    );
    assert!(identity.output.is_none());
    assert!(identity
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == DiagnosticCode::ReturnTypeMismatch));
}

#[test]
fn optional_tuple_spreads_check_every_possible_length() {
    let compile_source = |source: &str| {
        compile(
            ENTRY,
            &MapLoader::from([ModuleSource::new(ENTRY, source)]),
            CompilerOptions::default(),
        )
    };
    let accepted = compile_source(include_str!(
        "fixtures/typescript_oracle/tuple-spread-optional-valid/main.ts"
    ));
    assert!(
        accepted.diagnostics.is_empty(),
        "{:#?}",
        accepted.diagnostics
    );
    for source in [
        include_str!("fixtures/typescript_oracle/tuple-spread-optional-call-error/main.ts"),
        include_str!("fixtures/typescript_oracle/tuple-spread-optional-multi-error/main.ts"),
        include_str!("fixtures/typescript_oracle/tuple-spread-optional-method-error/main.ts"),
        include_str!("fixtures/typescript_oracle/tuple-spread-optional-constructor-error/main.ts"),
        include_str!("fixtures/typescript_oracle/tuple-spread-optional-super-error/main.ts"),
        include_str!("fixtures/typescript_oracle/tuple-spread-optional-literal-error/main.ts"),
    ] {
        let rejected = compile_source(source);
        assert!(rejected.output.is_none());
        assert!(
            rejected.diagnostics.iter().any(|diagnostic| {
                diagnostic.code == DiagnosticCode::TypeMismatch && diagnostic.span.module == ENTRY
            }),
            "{source}: {:#?}",
            rejected.diagnostics
        );
    }
}

#[test]
fn returns_use_the_declared_tuple_as_literal_context() {
    let compile_source = |source: &str| {
        compile(
            ENTRY,
            &MapLoader::from([ModuleSource::new(ENTRY, source)]),
            CompilerOptions::default(),
        )
    };
    let accepted = compile_source(include_str!(
        "fixtures/typescript_oracle/tuple-literal-return-valid/main.ts"
    ));
    assert!(
        accepted.diagnostics.is_empty(),
        "{:#?}",
        accepted.diagnostics
    );
    assert!(accepted.output.is_some());

    // Class methods are still refused before output, but must add nothing else.
    let classes = compile_source(include_str!(
        "fixtures/typescript_oracle/tuple-literal-return-class-valid/main.ts"
    ));
    assert!(classes.diagnostics.is_empty(), "{:#?}", classes.diagnostics);

    for source in [
        include_str!("fixtures/typescript_oracle/tuple-literal-return-type-error/main.ts"),
        include_str!("fixtures/typescript_oracle/tuple-literal-return-short-error/main.ts"),
        include_str!("fixtures/typescript_oracle/tuple-literal-return-long-error/main.ts"),
        include_str!("fixtures/typescript_oracle/tuple-literal-return-spread-error/main.ts"),
        include_str!("fixtures/typescript_oracle/tuple-literal-return-class-error/main.ts"),
        include_str!("fixtures/typescript_oracle/tuple-literal-return-local-error/main.ts"),
        include_str!("fixtures/typescript_oracle/tuple-literal-return-nested-error/main.ts"),
    ] {
        let rejected = compile_source(source);
        assert!(rejected.output.is_none());
        assert!(
            rejected.diagnostics.iter().any(|diagnostic| {
                diagnostic.code == DiagnosticCode::ReturnTypeMismatch
                    && diagnostic.span.module == ENTRY
            }),
            "{source}: {:#?}",
            rejected.diagnostics
        );
    }
}

#[test]
fn call_arguments_use_the_parameter_tuple_as_literal_context() {
    let compile_source = |source: &str| {
        compile(
            ENTRY,
            &MapLoader::from([ModuleSource::new(ENTRY, source)]),
            CompilerOptions::default(),
        )
    };
    let accepted = compile_source(include_str!(
        "fixtures/typescript_oracle/tuple-literal-argument-valid/main.ts"
    ));
    assert!(
        accepted.diagnostics.is_empty(),
        "{:#?}",
        accepted.diagnostics
    );
    let classes = compile_source(include_str!(
        "fixtures/typescript_oracle/tuple-literal-argument-class-valid/main.ts"
    ));
    assert!(classes.diagnostics.is_empty(), "{:#?}", classes.diagnostics);
    for source in [
        include_str!("fixtures/typescript_oracle/tuple-literal-argument-type-error/main.ts"),
        include_str!("fixtures/typescript_oracle/tuple-literal-argument-short-error/main.ts"),
        include_str!("fixtures/typescript_oracle/tuple-literal-argument-overload-error/main.ts"),
        include_str!("fixtures/typescript_oracle/tuple-literal-argument-method-error/main.ts"),
        include_str!("fixtures/typescript_oracle/tuple-literal-argument-constructor-error/main.ts"),
        include_str!("fixtures/typescript_oracle/tuple-literal-argument-super-error/main.ts"),
    ] {
        let rejected = compile_source(source);
        assert!(rejected.output.is_none());
        assert!(
            rejected.diagnostics.iter().any(|diagnostic| {
                diagnostic.code == DiagnosticCode::TypeMismatch && diagnostic.span.module == ENTRY
            }),
            "{source}: {:#?}",
            rejected.diagnostics
        );
    }
}

#[test]
fn function_annotations_that_survive_erasure_are_refused_before_output() {
    // Every form below is valid TypeScript whose annotation the parser does
    // not structurally erase. Emitting it would produce invalid JavaScript, so
    // it must be refused instead.
    for source in [
        "export const h = <T>({ x: { y } }) => y;",
        "export const o = { *m(a: number) { yield a; } };",
    ] {
        let result = compile(
            ENTRY,
            &MapLoader::from([ModuleSource::new(ENTRY, source)]),
            CompilerOptions::default(),
        );
        assert!(result.output.is_none(), "{source}");
        assert!(
            result.diagnostics.iter().any(|diagnostic| {
                diagnostic.code == DiagnosticCode::UnsupportedSyntax
                    && diagnostic.span.module == ENTRY
            }),
            "{source}: {:#?}",
            result.diagnostics
        );
    }
}

#[test]
fn unannotated_function_forms_and_colons_are_not_mistaken_for_annotations() {
    for source in [
        "export const h = (a) => a;",
        "export const h = a => a;",
        "export const h = (a, b = 1) => a + b;",
        "export const h = async (a) => a;",
        "export const h = function (a, b) { return a + b; };",
        "export const c = true ? (x) => x : (y) => y;",
        "export const o = { a: 1, b: 2, m(x) { return x; }, get v() { return 1; } };",
        "export const o = { a: true ? 1 : 2, b: [1, 2].map((n) => n) };",
        "export function f(flag: boolean) { return flag ? (1) : 2; }",
        "export function f(a: number, b?: number): number { return a > 0 ? a : (b ?? 0); }",
        "export function f(o: { a?: number }) { return o.a ? 1 : 2; }",
        "export function f(x: number) { const y = (x) ? 1 : 2; return y; }",
        "export function f(x: number) { switch (x) { case 1: return 1; default: return 2; } }",
        "export function f(x: number) { outer: for (;;) { if (x) { break outer; } } return x; }",
        "export function f(cb: (a: number) => number) { return cb(1); }",
        "export function f(xs: number[]) { return xs.map((n) => n).filter(function (n) { return n; }); }",
    ] {
        let result = compile(
            ENTRY,
            &MapLoader::from([ModuleSource::new(ENTRY, source)]),
            CompilerOptions::default(),
        );
        assert!(
            result.diagnostics.is_empty(),
            "{source}: {:#?}",
            result.diagnostics
        );
        assert!(result.output.is_some(), "{source}");
    }
}

#[test]
fn typed_arrow_functions_are_parsed_erased_and_checked() {
    let compile_source = |source: &str| {
        compile(
            ENTRY,
            &MapLoader::from([ModuleSource::new(ENTRY, source)]),
            CompilerOptions::default(),
        )
    };
    let compiled = compile_source(include_str!(
        "fixtures/typescript_oracle/arrow-typed-runtime/main.ts"
    ));
    assert!(
        compiled.diagnostics.is_empty(),
        "{:#?}",
        compiled.diagnostics
    );
    let javascript = compiled.output.unwrap().artifacts[ENTRY].javascript.clone();
    assert_eq!(
        normalized_javascript(&javascript),
        "const double = (n) => n * 2; \
         const greet = (name, punct= \"!\") => { return \"hi \" + name + punct; }; \
         const twice = (f, n) => f(f(n)); \
         const add = (a) => (b) => a + b; \
         const label = (n) => { if (n) { return \"n\"; } return \"none\"; }; \
         const pair = () => [1, \"a\"]; \
         console.log(double(4)); console.log(greet(\"a\")); console.log(greet(\"a\", \"?\")); \
         console.log(twice(double, 3)); console.log(add(1)(2)); console.log(label()); \
         console.log(pair()[1]);"
    );

    let accepted = compile_source(include_str!(
        "fixtures/typescript_oracle/arrow-typed-valid/main.ts"
    ));
    assert!(
        accepted.diagnostics.is_empty(),
        "{:#?}",
        accepted.diagnostics
    );

    for (source, code) in [
        (
            include_str!("fixtures/typescript_oracle/arrow-typed-result-error/main.ts"),
            DiagnosticCode::ReturnTypeMismatch,
        ),
        (
            include_str!("fixtures/typescript_oracle/arrow-typed-block-error/main.ts"),
            DiagnosticCode::ReturnTypeMismatch,
        ),
        (
            include_str!("fixtures/typescript_oracle/arrow-typed-missing-return-error/main.ts"),
            DiagnosticCode::ReturnTypeMismatch,
        ),
        (
            include_str!("fixtures/typescript_oracle/arrow-typed-parameter-use-error/main.ts"),
            DiagnosticCode::ReturnTypeMismatch,
        ),
        (
            include_str!("fixtures/typescript_oracle/arrow-typed-closure-error/main.ts"),
            DiagnosticCode::ReturnTypeMismatch,
        ),
        (
            include_str!("fixtures/typescript_oracle/arrow-typed-default-error/main.ts"),
            DiagnosticCode::TypeMismatch,
        ),
        (
            include_str!("fixtures/typescript_oracle/arrow-typed-nested-error/main.ts"),
            DiagnosticCode::ReturnTypeMismatch,
        ),
        (
            include_str!("fixtures/typescript_oracle/arrow-typed-tuple-error/main.ts"),
            DiagnosticCode::ReturnTypeMismatch,
        ),
        (
            include_str!("fixtures/typescript_oracle/arrow-typed-assign-error/main.ts"),
            DiagnosticCode::TypeMismatch,
        ),
    ] {
        let rejected = compile_source(source);
        assert!(rejected.output.is_none());
        assert!(
            rejected
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == code && diagnostic.span.module == ENTRY),
            "{source}: {:#?}",
            rejected.diagnostics
        );
    }

    // Shapes the structured parser does not take stay refused rather than
    // reaching the output with their annotations.
    // A nested pattern is outside the supported subset and stays refused.
    let nested = "export const h = ({ x: { y } }: { x: { y: number } }) => y;";
    let refused = compile_source(nested);
    assert!(refused.output.is_none(), "{nested}");
    assert!(
        refused
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == DiagnosticCode::UnsupportedSyntax),
        "{nested}"
    );
}

#[test]
fn function_expressions_are_parsed_erased_and_checked() {
    let compile_source = |source: &str| {
        compile(
            ENTRY,
            &MapLoader::from([ModuleSource::new(ENTRY, source)]),
            CompilerOptions::default(),
        )
    };
    let compiled = compile_source(include_str!(
        "fixtures/typescript_oracle/function-expression-runtime/main.ts"
    ));
    assert!(
        compiled.diagnostics.is_empty(),
        "{:#?}",
        compiled.diagnostics
    );
    let javascript = compiled.output.unwrap().artifacts[ENTRY].javascript.clone();
    assert_eq!(
        normalized_javascript(&javascript),
        "const double = function (n) { return n * 2; }; \
         const greet = function named(name, punct= \"!\") { return \"hi \" + name + punct; }; \
         const twice = function (f, n) { return f(f(n)); }; \
         const xs = [1, 2, 3].map(function (n) { return n + 1; }); \
         console.log(double(4)); console.log(greet(\"a\")); \
         console.log(twice(double, 3)); console.log(xs[2]);"
    );

    let accepted = compile_source(include_str!(
        "fixtures/typescript_oracle/function-expression-valid/main.ts"
    ));
    assert!(
        accepted.diagnostics.is_empty(),
        "{:#?}",
        accepted.diagnostics
    );

    for (source, code) in [
        (
            include_str!("fixtures/typescript_oracle/function-expression-result-error/main.ts"),
            DiagnosticCode::ReturnTypeMismatch,
        ),
        (
            include_str!(
                "fixtures/typescript_oracle/function-expression-missing-return-error/main.ts"
            ),
            DiagnosticCode::ReturnTypeMismatch,
        ),
        (
            include_str!(
                "fixtures/typescript_oracle/function-expression-parameter-use-error/main.ts"
            ),
            DiagnosticCode::ReturnTypeMismatch,
        ),
        (
            include_str!("fixtures/typescript_oracle/function-expression-closure-error/main.ts"),
            DiagnosticCode::ReturnTypeMismatch,
        ),
        (
            include_str!("fixtures/typescript_oracle/function-expression-default-error/main.ts"),
            DiagnosticCode::TypeMismatch,
        ),
        (
            include_str!("fixtures/typescript_oracle/function-expression-assign-error/main.ts"),
            DiagnosticCode::TypeMismatch,
        ),
        (
            include_str!("fixtures/typescript_oracle/function-expression-call-error/main.ts"),
            DiagnosticCode::TypeMismatch,
        ),
    ] {
        let rejected = compile_source(source);
        assert!(rejected.output.is_none());
        assert!(
            rejected
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == code && diagnostic.span.module == ENTRY),
            "{source}: {:#?}",
            rejected.diagnostics
        );
    }

    // A named expression is visible inside its own body only.
    let recursive = compile_source(
        "export const f = function fact(n: number): number { return n ? n * fact(n - 1) : 1; };",
    );
    assert!(
        recursive.diagnostics.is_empty(),
        "{:#?}",
        recursive.diagnostics
    );

    // Generators stay refused.
    let generator = "export const h = function* (a: number) { yield a; };";
    assert!(compile_source(generator).output.is_none(), "{generator}");
}

#[test]
fn object_methods_and_accessors_are_parsed_erased_and_checked() {
    let compile_source = |source: &str| {
        compile(
            ENTRY,
            &MapLoader::from([ModuleSource::new(ENTRY, source)]),
            CompilerOptions::default(),
        )
    };
    let compiled = compile_source(include_str!(
        "fixtures/typescript_oracle/object-method-runtime/main.ts"
    ));
    assert!(
        compiled.diagnostics.is_empty(),
        "{:#?}",
        compiled.diagnostics
    );
    let javascript = compiled.output.unwrap().artifacts[ENTRY].javascript.clone();
    assert_eq!(
        normalized_javascript(&javascript),
        "const counter = { base: 10, add(n) { return n + 1; }, \
         label(prefix, suffix= \"!\") { return prefix + suffix; }, \
         get answer() { return 42; }, compose(f, n) { return f(f(n)); }, }; \
         console.log(counter.add(1)); console.log(counter.label(\"a\")); \
         console.log(counter.answer); console.log(counter.compose(counter.add, 5));"
    );

    let accepted = compile_source(include_str!(
        "fixtures/typescript_oracle/object-method-valid/main.ts"
    ));
    assert!(
        accepted.diagnostics.is_empty(),
        "{:#?}",
        accepted.diagnostics
    );

    for (source, code) in [
        (
            include_str!("fixtures/typescript_oracle/object-method-result-error/main.ts"),
            DiagnosticCode::ReturnTypeMismatch,
        ),
        (
            include_str!("fixtures/typescript_oracle/object-method-parameter-use-error/main.ts"),
            DiagnosticCode::ReturnTypeMismatch,
        ),
        (
            include_str!("fixtures/typescript_oracle/object-method-missing-return-error/main.ts"),
            DiagnosticCode::ReturnTypeMismatch,
        ),
        (
            include_str!("fixtures/typescript_oracle/object-method-default-error/main.ts"),
            DiagnosticCode::TypeMismatch,
        ),
        (
            include_str!("fixtures/typescript_oracle/object-method-call-error/main.ts"),
            DiagnosticCode::TypeMismatch,
        ),
        (
            include_str!("fixtures/typescript_oracle/object-method-getter-error/main.ts"),
            DiagnosticCode::ReturnTypeMismatch,
        ),
    ] {
        let rejected = compile_source(source);
        assert!(rejected.output.is_none());
        assert!(
            rejected
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == code && diagnostic.span.module == ENTRY),
            "{source}: {:#?}",
            rejected.diagnostics
        );
    }

    // A setter's parameter annotation is erased and typed.
    let setter =
        compile_source("export const o = { set v(x: number) { }, get v(): number { return 1; } };");
    assert!(setter.diagnostics.is_empty(), "{:#?}", setter.diagnostics);
    assert!(
        !normalized_javascript(&setter.output.unwrap().artifacts[ENTRY].javascript)
            .contains("number")
    );
}

#[test]
fn nested_function_declarations_are_parsed_erased_hoisted_and_checked() {
    let compile_source = |source: &str| {
        compile(
            ENTRY,
            &MapLoader::from([ModuleSource::new(ENTRY, source)]),
            CompilerOptions::default(),
        )
    };
    let compiled = compile_source(include_str!(
        "fixtures/typescript_oracle/nested-function-runtime/main.ts"
    ));
    assert!(
        compiled.diagnostics.is_empty(),
        "{:#?}",
        compiled.diagnostics
    );
    let javascript = compiled.output.unwrap().artifacts[ENTRY].javascript.clone();
    assert_eq!(
        normalized_javascript(&javascript),
        "export function outer(base){ \
         function add(n) { return n + base; } \
         function twice(f, n) { return f(f(n)); } \
         function greet(name, punct= \"!\") { return \"hi \" + name + punct; } \
         console.log(greet(\"a\")); return twice(add, 1); } \
         console.log(outer(10));"
    );

    for source in [
        include_str!("fixtures/typescript_oracle/nested-function-valid/main.ts"),
        // A declaration is hoisted, so it may be called before it appears.
        include_str!("fixtures/typescript_oracle/nested-function-hoist/main.ts"),
    ] {
        let accepted = compile_source(source);
        assert!(
            accepted.diagnostics.is_empty(),
            "{source}: {:#?}",
            accepted.diagnostics
        );
    }

    for (source, code) in [
        (
            include_str!("fixtures/typescript_oracle/nested-function-result-error/main.ts"),
            DiagnosticCode::ReturnTypeMismatch,
        ),
        (
            include_str!("fixtures/typescript_oracle/nested-function-parameter-use-error/main.ts"),
            DiagnosticCode::ReturnTypeMismatch,
        ),
        (
            include_str!("fixtures/typescript_oracle/nested-function-closure-error/main.ts"),
            DiagnosticCode::ReturnTypeMismatch,
        ),
        (
            include_str!("fixtures/typescript_oracle/nested-function-missing-return-error/main.ts"),
            DiagnosticCode::ReturnTypeMismatch,
        ),
        (
            include_str!("fixtures/typescript_oracle/nested-function-call-error/main.ts"),
            DiagnosticCode::TypeMismatch,
        ),
    ] {
        let rejected = compile_source(source);
        assert!(rejected.output.is_none());
        assert!(
            rejected
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == code && diagnostic.span.module == ENTRY),
            "{source}: {:#?}",
            rejected.diagnostics
        );
    }

    // A nested function's own variables do not leak into the enclosing
    // function: the two `x` have different types, and each return is valid
    // only against its own.
    let scoped = compile_source(
        "export function outer(): string { const x: string = \"s\"; \
         function a(): number { const x: number = 1; \
         function b(): number { return x; } return b(); } return x; }",
    );
    assert!(scoped.diagnostics.is_empty(), "{:#?}", scoped.diagnostics);

    // Generators stay refused.
    let generator = "export function outer() { function* g(a: number) { yield a; } return g; }";
    assert!(compile_source(generator).output.is_none(), "{generator}");
}

#[test]
fn catch_binding_annotations_are_erased_and_try_termination_is_analyzed() {
    let compile_source = |source: &str| {
        compile(
            ENTRY,
            &MapLoader::from([ModuleSource::new(ENTRY, source)]),
            CompilerOptions::default(),
        )
    };
    let compiled = compile_source(include_str!(
        "fixtures/typescript_oracle/catch-annotation-runtime/main.ts"
    ));
    assert!(
        compiled.diagnostics.is_empty(),
        "{:#?}",
        compiled.diagnostics
    );
    let javascript = compiled.output.unwrap().artifacts[ENTRY].javascript.clone();
    assert!(javascript.contains("catch (e)"), "{javascript}");
    assert!(!javascript.contains("unknown"), "{javascript}");

    let accepted = compile_source(include_str!(
        "fixtures/typescript_oracle/catch-annotation-valid/main.ts"
    ));
    assert!(
        accepted.diagnostics.is_empty(),
        "{:#?}",
        accepted.diagnostics
    );

    for (source, code) in [
        (
            include_str!("fixtures/typescript_oracle/catch-annotation-type-error/main.ts"),
            DiagnosticCode::ParseError,
        ),
        (
            include_str!("fixtures/typescript_oracle/catch-annotation-unknown-use-error/main.ts"),
            DiagnosticCode::ReturnTypeMismatch,
        ),
        (
            include_str!("fixtures/typescript_oracle/catch-annotation-fallthrough-error/main.ts"),
            DiagnosticCode::ReturnTypeMismatch,
        ),
        (
            include_str!(
                "fixtures/typescript_oracle/catch-annotation-try-fallthrough-error/main.ts"
            ),
            DiagnosticCode::ReturnTypeMismatch,
        ),
    ] {
        let rejected = compile_source(source);
        assert!(rejected.output.is_none());
        assert!(
            rejected
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == code && diagnostic.span.module == ENTRY),
            "{source}: {:#?}",
            rejected.diagnostics
        );
    }
}

#[test]
fn generic_nested_functions_are_parsed_erased_and_checked() {
    let compile_source = |source: &str| {
        compile(
            ENTRY,
            &MapLoader::from([ModuleSource::new(ENTRY, source)]),
            CompilerOptions::default(),
        )
    };
    let compiled = compile_source(include_str!(
        "fixtures/typescript_oracle/generic-nested-runtime/main.ts"
    ));
    assert!(
        compiled.diagnostics.is_empty(),
        "{:#?}",
        compiled.diagnostics
    );
    let javascript = compiled.output.unwrap().artifacts[ENTRY].javascript.clone();
    assert_eq!(
        normalized_javascript(&javascript),
        "const identity = (value) => value; \
         const pair = function (a, b) { return [a, b]; }; \
         const named = function pick(xs) { return xs[0]; }; \
         function outer(){ function first(xs) { return xs[0]; } return first([1, 2]); } \
         console.log(identity(1)); console.log(pair(\"a\", 2)[1]); \
         console.log(named([7, 8])); console.log(outer());"
    );

    let accepted = compile_source(include_str!(
        "fixtures/typescript_oracle/generic-nested-valid/main.ts"
    ));
    assert!(
        accepted.diagnostics.is_empty(),
        "{:#?}",
        accepted.diagnostics
    );

    for (source, code) in [
        (
            include_str!("fixtures/typescript_oracle/generic-nested-result-error/main.ts"),
            DiagnosticCode::ReturnTypeMismatch,
        ),
        (
            include_str!("fixtures/typescript_oracle/generic-nested-parameter-use-error/main.ts"),
            DiagnosticCode::ReturnTypeMismatch,
        ),
        (
            include_str!(
                "fixtures/typescript_oracle/generic-nested-unknown-parameter-error/main.ts"
            ),
            DiagnosticCode::UnknownType,
        ),
        (
            include_str!("fixtures/typescript_oracle/generic-nested-constraint-error/main.ts"),
            DiagnosticCode::ReturnTypeMismatch,
        ),
    ] {
        let rejected = compile_source(source);
        assert!(rejected.output.is_none());
        assert!(
            rejected
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == code && diagnostic.span.module == ENTRY),
            "{source}: {:#?}",
            rejected.diagnostics
        );
    }

    // Untyped generic arrows are erased too, in any expression position.
    for source in [
        "export const identity = <T>(value) => value;",
        "export function make() { return <T>(value) => value; }",
        "export function make() { let identity; identity = <T>(value) => value; return identity; }",
    ] {
        let result = compile_source(source);
        assert!(
            result.diagnostics.is_empty(),
            "{source}: {:#?}",
            result.diagnostics
        );
        assert!(
            !result.output.unwrap().artifacts[ENTRY]
                .javascript
                .contains("<T>"),
            "{source}"
        );
    }
}

#[test]
fn async_functions_and_await_are_typed_against_promise() {
    let compile_source = |source: &str| {
        compile(
            ENTRY,
            &MapLoader::from([ModuleSource::new(ENTRY, source)]),
            CompilerOptions::default(),
        )
    };
    let compiled = compile_source(include_str!(
        "fixtures/typescript_oracle/async-function-runtime/main.ts"
    ));
    assert!(
        compiled.diagnostics.is_empty(),
        "{:#?}",
        compiled.diagnostics
    );
    let javascript = compiled.output.unwrap().artifacts[ENTRY].javascript.clone();
    assert!(
        javascript.contains("async function double(n)"),
        "{javascript}"
    );
    assert!(!javascript.contains("Promise"), "{javascript}");

    let accepted = compile_source(include_str!(
        "fixtures/typescript_oracle/async-function-valid/main.ts"
    ));
    assert!(
        accepted.diagnostics.is_empty(),
        "{:#?}",
        accepted.diagnostics
    );

    for (source, code) in [
        (
            include_str!("fixtures/typescript_oracle/async-function-return-error/main.ts"),
            DiagnosticCode::ReturnTypeMismatch,
        ),
        (
            include_str!("fixtures/typescript_oracle/async-function-annotation-error/main.ts"),
            DiagnosticCode::TypeMismatch,
        ),
        (
            include_str!("fixtures/typescript_oracle/async-function-await-type-error/main.ts"),
            DiagnosticCode::TypeMismatch,
        ),
        (
            include_str!("fixtures/typescript_oracle/async-function-await-context-error/main.ts"),
            DiagnosticCode::TypeMismatch,
        ),
        (
            include_str!("fixtures/typescript_oracle/async-function-missing-return-error/main.ts"),
            DiagnosticCode::ReturnTypeMismatch,
        ),
        (
            include_str!(
                "fixtures/typescript_oracle/async-function-promise-mismatch-error/main.ts"
            ),
            DiagnosticCode::ReturnTypeMismatch,
        ),
    ] {
        let rejected = compile_source(source);
        assert!(rejected.output.is_none());
        assert!(
            rejected
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == code && diagnostic.span.module == ENTRY),
            "{source}: {:#?}",
            rejected.diagnostics
        );
    }

    // Top-level await and await inside a nested non-async function are not
    // valid in a classic function body, and must not reach the output.
    for source in [
        "async function f(): Promise<number> { return 1; } export const g = await f();",
        "export async function f(): Promise<number> { const g = (): number => await f(); return g(); }",
    ] {
        let refused = compile_source(source);
        assert!(refused.output.is_none(), "{source}");
    }

    // A host that declares its own Promise keeps it.
    let hosted = compile(
        ENTRY,
        &MapLoader::from([ModuleSource::new(
            ENTRY,
            "export async function f(): Promise<number> { return 1; }",
        )]),
        CompilerOptions {
            ambient_declaration_modules: vec![ModuleSource::new(
                "memory:///lib.d.ts",
                "interface Promise<T> { readonly value: T; }",
            )],
            ..CompilerOptions::default()
        },
    );
    assert!(hosted.diagnostics.is_empty(), "{:#?}", hosted.diagnostics);
}

#[test]
fn async_nested_functions_are_parsed_erased_typed_and_checked() {
    let compile_source = |source: &str| {
        compile(
            ENTRY,
            &MapLoader::from([ModuleSource::new(ENTRY, source)]),
            CompilerOptions::default(),
        )
    };
    let compiled = compile_source(include_str!(
        "fixtures/typescript_oracle/async-nested-runtime/main.ts"
    ));
    assert!(
        compiled.diagnostics.is_empty(),
        "{:#?}",
        compiled.diagnostics
    );
    let javascript = compiled.output.unwrap().artifacts[ENTRY].javascript.clone();
    for expected in [
        "const arrow = async (n) => (await base(n)) * 2;",
        "const expr = async function (n) {",
        "const named = async function twice(n) {",
        "async m(n) {",
        "async function local(n) {",
    ] {
        assert!(javascript.contains(expected), "{expected}\n{javascript}");
    }
    assert!(!javascript.contains("Promise"), "{javascript}");

    let accepted = compile_source(include_str!(
        "fixtures/typescript_oracle/async-nested-valid/main.ts"
    ));
    assert!(
        accepted.diagnostics.is_empty(),
        "{:#?}",
        accepted.diagnostics
    );

    for (source, code) in [
        (
            include_str!("fixtures/typescript_oracle/async-nested-return-error/main.ts"),
            DiagnosticCode::ReturnTypeMismatch,
        ),
        (
            include_str!("fixtures/typescript_oracle/async-nested-annotation-error/main.ts"),
            DiagnosticCode::TypeMismatch,
        ),
        (
            include_str!("fixtures/typescript_oracle/async-nested-await-type-error/main.ts"),
            DiagnosticCode::TypeMismatch,
        ),
        (
            include_str!("fixtures/typescript_oracle/async-nested-sync-await-error/main.ts"),
            DiagnosticCode::TypeMismatch,
        ),
        (
            include_str!("fixtures/typescript_oracle/async-nested-call-error/main.ts"),
            DiagnosticCode::TypeMismatch,
        ),
        (
            include_str!("fixtures/typescript_oracle/async-nested-value-error/main.ts"),
            DiagnosticCode::TypeMismatch,
        ),
    ] {
        let rejected = compile_source(source);
        assert!(rejected.output.is_none());
        assert!(
            rejected
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == code && diagnostic.span.module == ENTRY),
            "{source}: {:#?}",
            rejected.diagnostics
        );
    }

    // An await in a sync function is still refused next to a structured
    // async arrow in the same expression.
    let mixed = compile_source(
        "async function f(): Promise<number> { return 1; } \
         export function g() { const xs = [async () => 1, await f()]; return xs; }",
    );
    assert!(mixed.output.is_none());
}

#[test]
fn destructured_parameters_are_parsed_erased_typed_and_checked() {
    let compile_source = |source: &str| {
        compile(
            ENTRY,
            &MapLoader::from([ModuleSource::new(ENTRY, source)]),
            CompilerOptions {
                declaration: true,
                ..CompilerOptions::default()
            },
        )
    };
    let compiled = compile_source(include_str!(
        "fixtures/typescript_oracle/destructured-parameter-runtime/main.ts"
    ));
    assert!(
        compiled.diagnostics.is_empty(),
        "{:#?}",
        compiled.diagnostics
    );
    let javascript = compiled.output.unwrap().artifacts[ENTRY].javascript.clone();
    let normalized = normalized_javascript(&javascript);
    for expected in [
        "const norm = ({ x, y }) => x * x + y * y;",
        "const first = ([a, b]) => b + a;",
        "function describe({ x, y }, [k])",
        "const renamed = ({ x: px, y: py = 5 }) => px + py;",
    ] {
        assert!(normalized.contains(expected), "{expected}\n{normalized}");
    }
    assert!(!normalized.contains("Point"), "{normalized}");

    let accepted = compile_source(include_str!(
        "fixtures/typescript_oracle/destructured-parameter-valid/main.ts"
    ));
    assert!(
        accepted.diagnostics.is_empty(),
        "{:#?}",
        accepted.diagnostics
    );
    let declaration = accepted.output.unwrap().artifacts[ENTRY]
        .declaration
        .clone()
        .unwrap();
    assert!(
        declaration.contains("export declare function h({ a, b }: Props): number;"),
        "{declaration}"
    );

    for (source, code) in [
        (
            include_str!("fixtures/typescript_oracle/destructured-parameter-use-error/main.ts"),
            DiagnosticCode::ReturnTypeMismatch,
        ),
        (
            include_str!("fixtures/typescript_oracle/destructured-parameter-missing-error/main.ts"),
            DiagnosticCode::TypeMismatch,
        ),
        (
            include_str!("fixtures/typescript_oracle/destructured-parameter-call-error/main.ts"),
            DiagnosticCode::TypeMismatch,
        ),
        (
            include_str!("fixtures/typescript_oracle/destructured-parameter-tuple-error/main.ts"),
            DiagnosticCode::ReturnTypeMismatch,
        ),
        (
            include_str!("fixtures/typescript_oracle/destructured-parameter-named-error/main.ts"),
            DiagnosticCode::ReturnTypeMismatch,
        ),
    ] {
        let rejected = compile_source(source);
        assert!(rejected.output.is_none());
        assert!(
            rejected
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == code && diagnostic.span.module == ENTRY),
            "{source}: {:#?}",
            rejected.diagnostics
        );
    }

    // Rest, computed and nested patterns are outside the supported subset.
    for source in [
        "export const h = ({ a, ...rest }: { a: number; b: number }) => a;",
        "export const h = ({ [\"a\"]: a }: { a: number }) => a;",
        "export const h = ([a, [b]]: [number, [number]]) => a;",
    ] {
        let refused = compile_source(source);
        assert!(refused.output.is_none(), "{source}");
    }
}

#[test]
fn middle_tuple_rest_checks_suffix_and_emits_at_public_boundary() {
    let source = include_str!("fixtures/typescript_oracle/tuple-rest-middle-valid/main.ts");
    let accepted = compile(
        ENTRY,
        &MapLoader::from([ModuleSource::new(ENTRY, source)]),
        CompilerOptions {
            declaration: true,
            ..CompilerOptions::default()
        },
    );
    assert!(
        accepted.diagnostics.is_empty(),
        "{:#?}",
        accepted.diagnostics
    );
    let artifact = &accepted.output.unwrap().artifacts[ENTRY];
    assert!(!artifact.javascript.contains("...body: string[]"));
    let declaration = artifact.declaration.as_deref().unwrap();
    for expected in [
        "Packet = [head: number, ...body: string[], done: boolean]",
        "short: [head: number, ...body: string[], done: boolean]",
        "long: [head: number, ...body: string[], done: boolean]",
        "leading: [...names: string[], enabled: boolean]",
    ] {
        assert!(declaration.contains(expected), "{declaration}");
    }

    for (source, code) in [
        (
            include_str!("fixtures/typescript_oracle/tuple-rest-middle-tail-error/main.ts"),
            DiagnosticCode::TypeMismatch,
        ),
        (
            include_str!("fixtures/typescript_oracle/tuple-rest-middle-rest-error/main.ts"),
            DiagnosticCode::TypeMismatch,
        ),
        (
            include_str!("fixtures/typescript_oracle/tuple-rest-middle-arity-error/main.ts"),
            DiagnosticCode::TypeMismatch,
        ),
        (
            include_str!("fixtures/typescript_oracle/tuple-rest-middle-index-error/main.ts"),
            DiagnosticCode::TypeMismatch,
        ),
        (
            include_str!("fixtures/typescript_oracle/tuple-rest-middle-index-narrow-error/main.ts"),
            DiagnosticCode::TypeMismatch,
        ),
        (
            include_str!("fixtures/typescript_oracle/tuple-rest-middle-guarantee-error/main.ts"),
            DiagnosticCode::TypeMismatch,
        ),
        (
            include_str!("fixtures/typescript_oracle/tuple-rest-middle-assignment-error/main.ts"),
            DiagnosticCode::TypeMismatch,
        ),
        (
            include_str!("fixtures/typescript_oracle/tuple-rest-middle-double-rest-error/main.ts"),
            DiagnosticCode::ParseError,
        ),
        (
            include_str!(
                "fixtures/typescript_oracle/tuple-rest-middle-optional-prefix-error/main.ts"
            ),
            DiagnosticCode::ParseError,
        ),
    ] {
        let rejected = compile(
            ENTRY,
            &MapLoader::from([ModuleSource::new(ENTRY, source)]),
            CompilerOptions::default(),
        );
        assert!(rejected.output.is_none());
        assert!(
            rejected
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == code && diagnostic.span.module == ENTRY),
            "{:#?}",
            rejected.diagnostics
        );
    }
}

#[test]
fn named_default_function_exports_preserve_esm_and_emit_a_public_declaration() {
    let source = "export default function greeting(name: string): string { return `Hello, ${name}`; }\nconsole.log(greeting('Ada'));\n";
    let compilation = compile_with_helper(source);
    assert!(
        compilation.diagnostics.is_empty(),
        "{:#?}",
        compilation.diagnostics
    );
    let javascript = &compilation.output.unwrap().artifacts[ENTRY].javascript;
    assert!(javascript.contains("export default function greeting(name)"));
    assert!(!javascript.contains(": string"));

    let output = compile(
        ENTRY,
        &MapLoader::from([ModuleSource::new(ENTRY, source)]),
        CompilerOptions {
            declaration: true,
            ..CompilerOptions::default()
        },
    )
    .output
    .unwrap();
    let declaration = output.artifacts[ENTRY].declaration.as_deref().unwrap();
    assert_eq!(
        declaration,
        "export default function greeting(name: string): string;\n"
    );
}

#[test]
fn named_default_value_exports_preserve_esm_and_emit_a_public_declaration() {
    let source = "const greeting: string = 'Hello, Ada';\nexport default greeting;\nconsole.log(greeting);\n";
    let compilation = compile_with_helper(source);
    assert!(
        compilation.diagnostics.is_empty(),
        "{:#?}",
        compilation.diagnostics
    );
    let javascript = &compilation.output.unwrap().artifacts[ENTRY].javascript;
    assert!(javascript.contains("const greeting"));
    assert!(javascript.contains("'Hello, Ada'"));
    assert!(javascript.contains("export default greeting"));
    assert!(!javascript.contains(": string"));

    let output = compile(
        ENTRY,
        &MapLoader::from([ModuleSource::new(ENTRY, source)]),
        CompilerOptions {
            declaration: true,
            ..CompilerOptions::default()
        },
    )
    .output
    .unwrap();
    let declaration = output.artifacts[ENTRY].declaration.as_deref().unwrap();
    assert_eq!(
        declaration,
        "declare const greeting: string;\nexport default greeting;\n"
    );
}

#[test]
fn named_default_value_exports_require_a_local_runtime_declaration() {
    assert_rejected(
        "export default missing;",
        "BTS3001",
        "default export `missing` must name a local runtime declaration",
    );
    assert_rejected(
        "declare const ambient: string;\nexport default ambient;",
        "BTS3001",
        "default export `ambient` must name a local runtime declaration",
    );
}

#[test]
fn named_value_exports_preserve_esm_and_emit_a_public_declaration() {
    let source =
        "const label: string = 'Hello, Ada';\nexport { label as greeting };\nconsole.log(label);\n";
    let compilation = compile_with_helper(source);
    assert!(
        compilation.diagnostics.is_empty(),
        "{:#?}",
        compilation.diagnostics
    );
    let javascript = &compilation.output.unwrap().artifacts[ENTRY].javascript;
    assert!(javascript.contains("const label"));
    assert!(javascript.contains("export { label as greeting }"));
    assert!(!javascript.contains(": string"));

    let output = compile(
        ENTRY,
        &MapLoader::from([ModuleSource::new(ENTRY, source)]),
        CompilerOptions {
            declaration: true,
            ..CompilerOptions::default()
        },
    )
    .output
    .unwrap();
    let declaration = output.artifacts[ENTRY].declaration.as_deref().unwrap();
    assert_eq!(
        declaration,
        "declare const label: string;\nexport { label as greeting };\n"
    );
}

#[test]
fn named_value_exports_require_a_local_runtime_declaration() {
    assert_rejected(
        "export { missing };",
        "BTS3001",
        "exported value `missing` must name a local runtime declaration",
    );
}

#[test]
fn imports_and_reexports_link_modules_and_keep_value_imports() {
    let javascript = emitted(
        "import { a, b as renamed } from './a.ts';\nimport type { Shape } from './a.ts';\nconst s: Shape = { x: a };\nexport const total: number = a + renamed;\n",
    );
    assert!(
        javascript.contains("import { a, b as renamed } from './a.js'"),
        "{javascript}"
    );
    assert!(!javascript.contains("Shape"), "{javascript}");
    assert!(javascript.contains("export const total"), "{javascript}");

    // A type-only import leaves no runtime import behind.
    let javascript = emitted("import type { Shape } from './a.ts';\nconst s: Shape = { x: 1 };\n");
    assert!(!javascript.contains("import"), "{javascript}");
}

#[test]
fn compiled_output_is_deterministic_and_tracks_the_source() {
    let first = compile_with_helper("export const value: number = 1;\n");
    let again = compile_with_helper("export const value: number = 1;\n");
    let changed = compile_with_helper("export const value: number = 2;\n");
    let fingerprint = |compilation: &blueice_bluets::Compilation| {
        compilation.output.as_ref().unwrap().fingerprint.clone()
    };
    assert_eq!(fingerprint(&first), fingerprint(&again));
    assert_ne!(fingerprint(&first), fingerprint(&changed));
}
