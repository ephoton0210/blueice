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
        assert!(compilation.output.is_none(), "{name} emitted an artifact");
        let unsupported = compilation
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == DiagnosticCode::UnsupportedSyntax)
            .collect::<Vec<_>>();
        assert_eq!(
            unsupported.len(),
            1,
            "{name}: {:#?}",
            compilation.diagnostics
        );
        assert_eq!(unsupported[0].span, class.span, "{name}");
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
        assert!(compilation.output.is_none(), "{name} emitted an artifact");
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
fn transpile_only_does_not_emit_unchecked_classes() {
    let source = include_str!("fixtures/typescript_oracle/class-method-overloads/main.ts");
    let result = compile(
        ENTRY,
        &MapLoader::from([ModuleSource::new(ENTRY, source)]),
        CompilerOptions {
            runtime_policy: RuntimePolicy::TranspileOnly,
            ..CompilerOptions::default()
        },
    );
    assert!(result.output.is_none());
    assert!(result
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
        assert!(compilation.output.is_none(), "{name} emitted an artifact");
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
        assert!(compilation.output.is_none());
        assert!(
            compilation
                .diagnostics
                .iter()
                .all(|diagnostic| diagnostic.code != DiagnosticCode::DuplicateDeclaration),
            "class/interface merging is a later leaf, not an illegal name collision: {:#?}",
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
        assert!(compilation.output.is_none());
        assert!(
            compilation
                .diagnostics
                .iter()
                .all(|diagnostic| diagnostic.code == DiagnosticCode::UnsupportedSyntax),
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
        assert!(compilation.output.is_none(), "{name} emitted an artifact");
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
    assert!(accepted.output.is_none());
    assert!(accepted
        .diagnostics
        .iter()
        .all(|diagnostic| diagnostic.code == DiagnosticCode::UnsupportedSyntax));
    let accepted = compile_with_helper(include_str!(
        "fixtures/typescript_oracle/class-constructor-valid-default/main.ts"
    ));
    assert!(accepted.output.is_none());
    assert!(accepted
        .diagnostics
        .iter()
        .all(|diagnostic| diagnostic.code == DiagnosticCode::UnsupportedSyntax));

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
        assert!(compilation.output.is_none());
        assert!(
            compilation
                .diagnostics
                .iter()
                .all(|diagnostic| diagnostic.code == DiagnosticCode::UnsupportedSyntax),
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
    assert!(accepted.output.is_none());
    assert!(
        accepted
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code == DiagnosticCode::UnsupportedSyntax),
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
    assert!(accepted.output.is_none());
    assert!(
        accepted
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code == DiagnosticCode::UnsupportedSyntax),
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
    assert!(accepted.output.is_none());
    assert!(
        accepted
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code == DiagnosticCode::UnsupportedSyntax),
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
    assert!(accepted.output.is_none());
    assert!(
        accepted
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code == DiagnosticCode::UnsupportedSyntax),
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
    assert!(accepted.output.is_none());
    assert!(
        accepted
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code == DiagnosticCode::UnsupportedSyntax),
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
    assert!(accepted.output.is_none());
    assert!(
        accepted
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code == DiagnosticCode::UnsupportedSyntax),
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
    assert!(accepted.output.is_none());
    assert!(
        accepted
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code == DiagnosticCode::UnsupportedSyntax),
        "{:#?}",
        accepted.diagnostics
    );
    let imported = compile_class_module_pair(
        include_str!("fixtures/typescript_oracle/class-heritage-imported-valid/main.ts"),
        include_str!("fixtures/typescript_oracle/class-heritage-imported-valid/box.ts"),
    );
    assert!(imported.output.is_none());
    assert!(
        imported
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code == DiagnosticCode::UnsupportedSyntax),
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
        assert!(accepted.output.is_none());
        assert!(
            accepted
                .diagnostics
                .iter()
                .all(|diagnostic| diagnostic.code == DiagnosticCode::UnsupportedSyntax),
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
        assert!(compilation.output.is_none());
        assert!(
            compilation
                .diagnostics
                .iter()
                .all(|diagnostic| diagnostic.code == DiagnosticCode::UnsupportedSyntax),
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
    assert!(compilation.output.is_none());
    assert!(
        compilation
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code == DiagnosticCode::UnsupportedSyntax),
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
        assert!(compilation.output.is_none());
        assert!(
            compilation
                .diagnostics
                .iter()
                .all(|diagnostic| diagnostic.code == DiagnosticCode::UnsupportedSyntax),
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
        assert!(compilation.output.is_none());
        assert!(
            compilation
                .diagnostics
                .iter()
                .all(|diagnostic| diagnostic.code == DiagnosticCode::UnsupportedSyntax),
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
        assert!(compilation.output.is_none());
        assert!(
            compilation
                .diagnostics
                .iter()
                .all(|diagnostic| diagnostic.code == DiagnosticCode::UnsupportedSyntax),
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
        assert!(compilation.output.is_none());
        assert!(
            compilation
                .diagnostics
                .iter()
                .all(|diagnostic| diagnostic.code == DiagnosticCode::UnsupportedSyntax),
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
        "function f(): void { try { 1; } catch (caught: any) { 2; } }",
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
    assert_rejected(
        "function f(): number { try { return 1; } finally { 0; } }",
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
            "class A {}",
            "class members and runtime semantics are not installed yet",
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
            "const x = (a: number) => a;",
            "typed arrow parameters are not in the initial BlueTS matrix",
        ),
        (
            "const identity = <T>(value) => value;",
            "generic arrow functions are not in the initial BlueTS matrix",
        ),
        (
            "function make() { return <T>(value) => value; }",
            "generic arrow functions are not in the initial BlueTS matrix",
        ),
        (
            "function make() { let identity; identity = <T>(value) => value; }",
            "generic arrow functions are not in the initial BlueTS matrix",
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
        (
            "function f({ a }: { a: number }): number { return a; }",
            "expected a parameter name",
        ),
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
