// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Direct compilation and statement-lowering regressions.

use super::*;

#[test]
fn lowers_typed_source_directly_to_bluejs_ast_and_bytecode() {
    let artifact = compile_direct_script(
        ENTRY,
        &MapLoader::from([ModuleSource::new(
            ENTRY,
            "type Count = number; var answer: Count = 40 + 2; answer;",
        )]),
        CompilerOptions::default(),
    )
    .unwrap();
    assert_eq!(artifact.bridge_abi, BLUE_TS_BLUEJS_BRIDGE_ABI_V1);
    assert_eq!(artifact.program_abi(), bluejs::BLUEJS_PROGRAM_ABI_V1);
    assert_eq!(artifact.provenance.len(), 2);
    assert_eq!(
        artifact.program,
        bluejs::BlueJsProgramV1::Script(bluejs::Program {
            body: vec![
                bluejs::Stmt::VarDecl(
                    bluejs::DeclKind::Var,
                    vec![bluejs::VarDeclarator {
                        pattern: bluejs::Pattern::Identifier("answer".to_string()),
                        init: Some(bluejs::Expr::Binary {
                            op: bluejs::BinaryOp::Add,
                            left: Box::new(bluejs::Expr::Number(40.0)),
                            right: Box::new(bluejs::Expr::Number(2.0)),
                        }),
                    }],
                ),
                bluejs::Stmt::Expr(bluejs::Expr::Identifier("answer".to_string())),
            ],
        })
    );
    assert_eq!(
        bluejs::Vm::default().execute(&artifact.bytecode).unwrap(),
        bluejs::Value::Number(42.0)
    );
}

#[test]
fn installs_direct_bytecode_with_its_checked_canonical_source_identity() {
    let artifact = compile_direct_script(
        ENTRY,
        &MapLoader::from([ModuleSource::new(
            ENTRY,
            "const answer: number = 42; answer;",
        )]),
        CompilerOptions::default(),
    )
    .unwrap();
    let expected_source = artifact.sources[0].clone();
    let mut registry = bluejs::BlueJsProgramRegistry::default();
    let attachment = artifact.attach_in(&mut registry).unwrap();
    let handle = attachment.handle;
    let installed = registry.get(handle).unwrap();

    assert_eq!(
        installed.source().canonical_module_id(),
        expected_source.module
    );
    assert_eq!(
        installed.source().source_hash(),
        expected_source.content_hash
    );
    assert_eq!(installed.bytecode().bytes(), artifact.bytecode.bytes());
    let root = installed.ast_nodes()[0];
    assert_eq!(root.kind(), bluejs::BlueJsAstNodeKind::Script);
    registry.validate_ast_node(handle, root.id()).unwrap();
    assert_eq!(attachment.provenance.len(), artifact.provenance.len());
    assert_eq!(
        attachment.provenance[0].source,
        artifact.provenance[0].source
    );
    registry
        .validate_ast_node(handle, attachment.provenance[0].node_id)
        .unwrap();
    let DirectSafePointBinding::Bound(safe_point) = attachment.provenance[0].safe_point else {
        panic!("the lowered variable declaration must have a bound safe point")
    };
    registry.validate_safe_point(handle, safe_point).unwrap();
    assert_eq!(
        attachment.safe_point_map.format,
        BLUEJS_SAFE_POINT_MAP_ABI_V1
    );
    assert!(attachment.safe_point_map.entries.len() >= attachment.provenance.len());
    for provenance in &attachment.provenance {
        if let DirectSafePointBinding::Bound(safe_point) = provenance.safe_point {
            let entry = attachment
                .safe_point_map
                .source_span_for_safe_point(
                    safe_point.code_unit.ordinal(),
                    safe_point.bytecode_offset,
                )
                .expect("every bound top-level statement retains its source span");
            assert_eq!(
                (entry.start_byte, entry.end_byte),
                (provenance.source.start, provenance.source.end)
            );
        }
    }
    attachment
        .safe_point_map
        .validate_against(&registry, handle)
        .unwrap();
    registry
        .validate_safe_point(handle, installed.safe_points().next().unwrap())
        .unwrap();
}

#[test]
fn existing_program_with_same_instructions_but_different_binding_refuses_attachment() {
    let artifact = compile_direct_script(
        ENTRY,
        &MapLoader::from([ModuleSource::new(ENTRY, "const answer: number = 42;")]),
        CompilerOptions::default(),
    )
    .unwrap();
    let mut registry = bluejs::BlueJsProgramRegistry::default();
    let identity =
        bluejs::BlueJsSourceIdentity::new(ENTRY, artifact.sources[0].content_hash.clone()).unwrap();
    let replacement = bluejs::BlueJsProgramV1::Script(bluejs::parse("const other=42;").unwrap());
    let handle = registry.install(identity, &replacement).unwrap();
    let installed = registry.get(handle).unwrap().bytecode();
    assert_eq!(installed.bytes(), artifact.bytecode.bytes());
    assert_eq!(installed.constants(), artifact.bytecode.constants());
    assert_eq!(
        installed.root_declaration_binding_slots(),
        artifact.bytecode.root_declaration_binding_slots()
    );
    assert!(matches!(
        artifact.attach_existing_in(&registry, handle),
        Err(BridgeError::ProvenanceAttachment(_))
    ));
}

#[test]
fn resolves_a_ts_byte_position_to_the_next_verified_safe_point_or_unbound() {
    let artifact = compile_direct_script(
        ENTRY,
        &MapLoader::from([ModuleSource::new(
            ENTRY,
            "const first: number = 1; const second: number = 2; second;",
        )]),
        CompilerOptions::default(),
    )
    .unwrap();
    let mut registry = bluejs::BlueJsProgramRegistry::default();
    let attachment = artifact.attach_in(&mut registry).unwrap();
    let first = &attachment.provenance[0];
    let second = &attachment.provenance[1];
    let DirectSafePointBinding::Bound(first_safe_point) = first.safe_point else {
        panic!("the first declaration must have a verified safe point")
    };
    let DirectSafePointBinding::Bound(second_safe_point) = second.safe_point else {
        panic!("the second declaration must have a verified safe point")
    };

    let exact = attachment
        .safe_point_map
        .source_span_for_safe_point(
            second_safe_point.code_unit.ordinal(),
            second_safe_point.bytecode_offset,
        )
        .expect("the second bound instruction must retain its original span");
    assert_eq!(exact.source, ENTRY);
    assert_eq!(
        (exact.start_byte, exact.end_byte),
        (second.source.start, second.source.end)
    );
    assert!(attachment
        .safe_point_map
        .source_span_for_safe_point(second_safe_point.code_unit.ordinal(), u32::MAX,)
        .is_none());

    assert_eq!(
        attachment.breakpoint_at_or_after(ENTRY, first.source.start),
        DirectSafePointBinding::Bound(first_safe_point)
    );
    assert_eq!(
        attachment.breakpoint_at_or_after(ENTRY, first.source.end),
        DirectSafePointBinding::Bound(second_safe_point),
        "a position after the first exclusive span end binds to the next statement"
    );
    assert_eq!(
        attachment
            .safe_point_map
            .nearest_bound_safe_point_at_or_after(ENTRY, first.source.end),
        Some(second_safe_point)
    );
    assert_eq!(
        attachment.breakpoint_at_or_after(ENTRY, usize::MAX),
        DirectSafePointBinding::Unbound
    );
    assert_eq!(
        attachment.breakpoint_at_or_after("page:///other.ts", 0),
        DirectSafePointBinding::Unbound
    );
}

#[test]
fn safe_point_map_retains_original_utf16_coordinates_without_source_text() {
    let source = "// original\r\n/* 🚀 */ const answer: number = 42;";
    let artifact = compile_direct_script(
        ENTRY,
        &MapLoader::from([ModuleSource::new(ENTRY, source)]),
        CompilerOptions::default(),
    )
    .unwrap();
    let mut registry = bluejs::BlueJsProgramRegistry::default();
    let attachment = artifact.attach_in(&mut registry).unwrap();
    let entry = &attachment.safe_point_map.entries[0];
    assert_eq!(entry.start_byte, source.find("const answer").unwrap());
    assert_eq!(entry.location.start.line, 1);
    assert_eq!(entry.location.start.column_utf16, 9);
    assert_eq!(entry.location.end.line, 1);
    assert!(entry.location.end.column_utf16 > entry.location.start.column_utf16);
    assert!(!format!("{entry:?}").contains("const answer"));

    let mut malformed = attachment.safe_point_map.clone();
    malformed.entries[0].location.end.column_utf16 = usize::MAX;
    assert!(malformed
        .validate_against(&registry, attachment.handle)
        .is_err());
}

#[test]
fn classic_and_module_maps_own_root_call_and_child_entry_without_mapping_halt() {
    let source = "/* 🚀 */ function inner(a: number): number { return a + 1; } inner(3);";
    let function_start = source.find("function inner").unwrap();
    let function_end = source.find("} inner").unwrap() + 1;
    let call_start = source.find("inner(3)").unwrap();
    for module in [false, true] {
        let mut registry = bluejs::BlueJsProgramRegistry::default();
        let loader = MapLoader::from([ModuleSource::new(ENTRY, source)]);
        let attachment = if module {
            compile_direct_module(ENTRY, &loader, CompilerOptions::default())
                .unwrap()
                .attach_in(&mut registry)
                .unwrap()
        } else {
            compile_direct_script(ENTRY, &loader, CompilerOptions::default())
                .unwrap()
                .attach_in(&mut registry)
                .unwrap()
        };
        let installed = registry.get(attachment.handle).unwrap();
        let root = installed.code_units()[0].id();
        let call = installed
            .bytecode()
            .instructions()
            .find(|instruction| instruction.opcode == bluejs::Opcode::Call)
            .expect("the source call must emit a root Call instruction");
        let root_span = attachment
            .safe_point_map
            .source_span_for_safe_point(root.ordinal(), call.offset as u32)
            .unwrap();
        assert_eq!(
            (root_span.start_byte, root_span.end_byte),
            (call_start, source.len())
        );
        let child = &installed.code_units()[1];
        let child_span = attachment
            .safe_point_map
            .source_span_for_safe_point(child.id().ordinal(), child.instruction_offsets()[0])
            .unwrap();
        assert_eq!(
            (child_span.start_byte, child_span.end_byte),
            (function_start, function_end)
        );
        assert_eq!(child_span.location.start.column_utf16, 9);
        let halt = installed
            .bytecode()
            .instructions()
            .find(|instruction| instruction.opcode == bluejs::Opcode::Halt)
            .unwrap();
        assert!(attachment
            .safe_point_map
            .source_span_for_safe_point(root.ordinal(), halt.offset as u32)
            .is_none());
    }
}

#[test]
fn module_function_source_breakpoint_selects_child_entry_over_declaration_root() {
    let source = "export function inner(): number { return 41; }";
    let mut registry = bluejs::BlueJsProgramRegistry::default();
    let mut debug = DirectDebugRegistry::default();
    let attachment = compile_direct_module(
        ENTRY,
        &MapLoader::from([ModuleSource::new(ENTRY, source)]),
        CompilerOptions::default(),
    )
    .unwrap()
    .attach_debug_in(&mut registry, &mut debug)
    .unwrap();
    let bound = attachment.breakpoint_at_or_after(ENTRY, source.find("function inner").unwrap());
    let DirectSafePointBinding::Bound(point) = bound else {
        panic!("a verified module function entry must be source-bound")
    };
    assert_eq!(point.code_unit.ordinal(), 1);
    assert_eq!(point.bytecode_offset, 0);
    assert_eq!(
        debug
            .get(&registry, attachment.handle)
            .unwrap()
            .breakpoint_at_or_after(ENTRY, source.find("function inner").unwrap()),
        bound
    );
}

#[test]
fn rejects_object_methods_without_reparsing_emitted_javascript() {
    let result = compile_direct_script(
        ENTRY,
        &MapLoader::from([ModuleSource::new(
            ENTRY,
            "const value = { label() { return 'Ada'; } }; value;",
        )]),
        CompilerOptions::default(),
    );
    let Err(error) = result else {
        panic!("the direct bridge must reject an object method");
    };
    let BridgeError::UnsupportedRuntimeTarget { span, .. } = error else {
        panic!("the direct bridge must reject an unsupported runtime shape");
    };
    assert_eq!(span.module, ENTRY);
    assert!(span.start > 0);
}

#[test]
fn lowers_typed_local_functions_and_direct_calls() {
    let artifact = compile_direct_script(
        ENTRY,
        &MapLoader::from([ModuleSource::new(
            ENTRY,
            "function add(left: number, right: number): number { \
                 const sum: number = left + right; return sum; } add(20, 22);",
        )]),
        CompilerOptions::default(),
    )
    .unwrap();
    assert!(matches!(
        artifact.program,
        bluejs::BlueJsProgramV1::Script(bluejs::Program { ref body })
            if matches!(body.as_slice(), [bluejs::Stmt::FunctionDecl(_), bluejs::Stmt::Expr(bluejs::Expr::Call { .. })])
    ));
    assert_eq!(
        bluejs::Vm::default().execute(&artifact.bytecode).unwrap(),
        bluejs::Value::Number(42.0)
    );
}

#[test]
fn direct_lowered_functions_do_not_forge_javascript_source_text() {
    let artifact = compile_direct_script(
        ENTRY,
        &MapLoader::from([ModuleSource::new(
            ENTRY,
            "function identity(value: number): number { return value; } identity(42);",
        )]),
        CompilerOptions::default(),
    )
    .unwrap();
    let bluejs::BlueJsProgramV1::Script(bluejs::Program { body }) = artifact.program else {
        panic!("a direct script must lower to a script program");
    };
    let bluejs::Stmt::FunctionDecl(function) = &body[0] else {
        panic!("the first lowered statement must be the local function");
    };

    assert_eq!(function.source_text.as_str(), None);
}

#[test]
fn lowers_boolean_comparison_logical_and_unary_expressions() {
    let artifact = compile_direct_script(
        ENTRY,
        &MapLoader::from([ModuleSource::new(
            ENTRY,
            "function matches(value: number) { \
                 return !(value < 42) && ~0 === -1 && +value === 42; } matches(42);",
        )]),
        CompilerOptions::default(),
    )
    .unwrap();
    assert_eq!(
        bluejs::Vm::default().execute(&artifact.bytecode).unwrap(),
        bluejs::Value::Bool(true)
    );
}

#[test]
fn lowers_typeof_and_void_expressions() {
    let artifact = compile_direct_script(
            ENTRY,
            &MapLoader::from([ModuleSource::new(
                ENTRY,
                "const absent: undefined = void 0; typeof 1 === 'number' && typeof absent === 'undefined';",
            )]),
            CompilerOptions::default(),
        )
        .unwrap();
    assert_eq!(
        bluejs::Vm::default().execute(&artifact.bytecode).unwrap(),
        bluejs::Value::Bool(true)
    );
}

#[test]
fn gives_logical_and_higher_precedence_than_logical_or() {
    let artifact = compile_direct_script(
        ENTRY,
        &MapLoader::from([ModuleSource::new(
            ENTRY,
            "function isBelow(value: number) { \
                 return value < 42 || value === 42 && false; } isBelow(41);",
        )]),
        CompilerOptions::default(),
    )
    .unwrap();
    assert_eq!(
        bluejs::Vm::default().execute(&artifact.bytecode).unwrap(),
        bluejs::Value::Bool(true)
    );
}

#[test]
fn lowers_nullish_coalescing_with_bluejs_short_circuiting() {
    let artifact = compile_direct_script(
        ENTRY,
        &MapLoader::from([ModuleSource::new(
            ENTRY,
            "function fallback(): number { return 42; } \
                 function choose(value: number | null): number { return value ?? fallback(); } \
                 choose(null) + choose(0);",
        )]),
        CompilerOptions::default(),
    )
    .unwrap();
    assert!(matches!(
        artifact.program,
        bluejs::BlueJsProgramV1::Script(bluejs::Program { ref body })
            if matches!(
                body.as_slice(),
                [
                    bluejs::Stmt::FunctionDecl(_),
                    bluejs::Stmt::FunctionDecl(bluejs::Function {
                        body: ref choose_body,
                        ..
                    }),
                    bluejs::Stmt::Expr(_),
                ] if matches!(
                    choose_body.as_slice(),
                    [bluejs::Stmt::Return(Some(bluejs::Expr::Logical {
                        op: bluejs::LogicalOp::Nullish,
                        ..
                    }))]
                )
            )
    ));
    assert_eq!(
        bluejs::Vm::default().execute(&artifact.bytecode).unwrap(),
        bluejs::Value::Number(42.0)
    );
}

#[test]
fn lowers_bitwise_and_shift_expressions_with_ecmascript_precedence() {
    let artifact = compile_direct_script(
        ENTRY,
        &MapLoader::from([ModuleSource::new(
            ENTRY,
            "function combine(value: number): number { \
                 return ((((value << 1) | 1) & 62) ^ 10) + (value >> 1) + (value >>> 1); \
                 } combine(20);",
        )]),
        CompilerOptions::default(),
    )
    .unwrap();
    assert_eq!(
        bluejs::Vm::default().execute(&artifact.bytecode).unwrap(),
        bluejs::Value::Number(54.0)
    );
}

#[test]
fn lowers_right_associative_exponentiation_with_unary_right_operands() {
    let artifact = compile_direct_script(
        ENTRY,
        &MapLoader::from([ModuleSource::new(
            ENTRY,
            "function power(): number { return 2 ** 3 ** 2 + 2 ** -3 + (-2) ** 2; } power();",
        )]),
        CompilerOptions::default(),
    )
    .unwrap();
    assert_eq!(
        bluejs::Vm::default().execute(&artifact.bytecode).unwrap(),
        bluejs::Value::Number(516.125)
    );
}

#[test]
fn expression_lowerer_rejects_unparenthesized_unary_exponentiation_bases() {
    let tokens = expression_tokens(&[
        ("-", TokenKind::Punct),
        ("2", TokenKind::Number),
        ("**", TokenKind::Punct),
        ("2", TokenKind::Number),
    ]);
    let error = ExpressionLowerer::new(ENTRY, &tokens).parse().unwrap_err();
    let BridgeError::UnsupportedRuntimeTarget { message, .. } = error else {
        panic!("the direct bridge must reject an unparenthesized unary exponent base");
    };
    assert!(message.contains("unparenthesized base"));
}

#[test]
fn expression_lowerer_gives_equality_higher_precedence_than_bitwise_and() {
    let tokens = expression_tokens(&[
        ("1", TokenKind::Number),
        ("&", TokenKind::Punct),
        ("3", TokenKind::Number),
        ("===", TokenKind::Punct),
        ("0", TokenKind::Number),
    ]);
    assert_eq!(
        ExpressionLowerer::new(ENTRY, &tokens).parse().unwrap(),
        bluejs::Expr::Binary {
            op: bluejs::BinaryOp::BitAnd,
            left: Box::new(bluejs::Expr::Number(1.0)),
            right: Box::new(bluejs::Expr::Binary {
                op: bluejs::BinaryOp::StrictEq,
                left: Box::new(bluejs::Expr::Number(3.0)),
                right: Box::new(bluejs::Expr::Number(0.0)),
            }),
        }
    );
}

#[test]
fn lowers_parenthesized_nullish_logical_mixing() {
    let artifact = compile_direct_script(
        ENTRY,
        &MapLoader::from([ModuleSource::new(
            ENTRY,
            "const value = (0 || 4) ?? 7; value;",
        )]),
        CompilerOptions::default(),
    )
    .unwrap();
    assert_eq!(
        bluejs::Vm::default().execute(&artifact.bytecode).unwrap(),
        bluejs::Value::Number(4.0)
    );
}

#[test]
fn rejects_unparenthesized_nullish_and_logical_mixing_before_direct_lowering() {
    for source in ["false || null ?? 42;", "null ?? false || true;"] {
        let result = compile_direct_script(
            ENTRY,
            &MapLoader::from([ModuleSource::new(ENTRY, source)]),
            CompilerOptions::default(),
        );
        let Err(BridgeError::BlueTs(diagnostics)) = result else {
            panic!("BlueTS must reject mixed unparenthesized logical operators");
        };
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.code == blueice_bluets::DiagnosticCode::ParseError
                && diagnostic.message.contains("parentheses are required")
        }));
    }
}

#[test]
fn expression_lowerer_defensively_rejects_unparenthesized_nullish_logical_mixing() {
    for tokens in [
        expression_tokens(&[
            ("false", TokenKind::Keyword),
            ("||", TokenKind::Punct),
            ("null", TokenKind::Keyword),
            ("??", TokenKind::Punct),
            ("42", TokenKind::Number),
        ]),
        expression_tokens(&[
            ("null", TokenKind::Keyword),
            ("??", TokenKind::Punct),
            ("false", TokenKind::Keyword),
            ("||", TokenKind::Punct),
            ("true", TokenKind::Keyword),
        ]),
    ] {
        let error = ExpressionLowerer::new(ENTRY, &tokens).parse().unwrap_err();
        let BridgeError::UnsupportedRuntimeTarget { message, .. } = error else {
            panic!("the direct bridge must reject mixed unparenthesized logical operators");
        };
        assert!(message.contains("parentheses are required"));
    }
}

#[test]
fn lowers_identifier_assignments_and_compound_assignments() {
    let artifact = compile_direct_script(
        ENTRY,
        &MapLoader::from([ModuleSource::new(
            ENTRY,
            "let total: number = 40; total += 2; total;",
        )]),
        CompilerOptions::default(),
    )
    .unwrap();
    assert_eq!(
        bluejs::Vm::default().execute(&artifact.bytecode).unwrap(),
        bluejs::Value::Number(42.0)
    );
}

#[test]
fn lowers_left_to_right_comma_sequences() {
    let artifact = compile_direct_script(
        ENTRY,
        &MapLoader::from([ModuleSource::new(
            ENTRY,
            "let value: number = 0; (value += 1, value += 2, value);",
        )]),
        CompilerOptions::default(),
    )
    .unwrap();
    assert_eq!(
        bluejs::Vm::default().execute(&artifact.bytecode).unwrap(),
        bluejs::Value::Number(3.0)
    );
}

#[test]
fn lowers_checked_non_spread_array_literals() {
    let artifact = compile_direct_script(
        ENTRY,
        &MapLoader::from([ModuleSource::new(
            ENTRY,
            "const values: number[] = [1, 2, 3]; typeof values === 'object';",
        )]),
        CompilerOptions::default(),
    )
    .unwrap();
    assert!(matches!(
        artifact.program,
        bluejs::BlueJsProgramV1::Script(bluejs::Program { ref body })
            if matches!(
                body.as_slice(),
                [bluejs::Stmt::VarDecl(_, declarations), bluejs::Stmt::Expr(_)]
                    if matches!(
                        declarations.as_slice(),
                        [bluejs::VarDeclarator {
                            init: Some(bluejs::Expr::Array(elements)),
                            ..
                        }] if elements.len() == 3
                    )
            )
    ));
    assert_eq!(
        bluejs::Vm::default().execute(&artifact.bytecode).unwrap(),
        bluejs::Value::Bool(true)
    );
}

#[test]
fn lowers_checked_simple_object_literals_and_dot_property_reads() {
    let artifact = compile_direct_script(
        ENTRY,
        &MapLoader::from([ModuleSource::new(
            ENTRY,
            "const person: { name: string; age: number } = { name: 'Ada', age: 42 }; \
                 person.name + ':' + person.age;",
        )]),
        CompilerOptions::default(),
    )
    .unwrap();
    assert!(matches!(
        artifact.program,
        bluejs::BlueJsProgramV1::Script(bluejs::Program { ref body })
            if matches!(
                body.as_slice(),
                [bluejs::Stmt::VarDecl(_, declarations), bluejs::Stmt::Expr(_)]
                    if matches!(
                        declarations.as_slice(),
                        [bluejs::VarDeclarator {
                            init: Some(bluejs::Expr::Object(properties)),
                            ..
                        }] if matches!(
                            properties.as_slice(),
                            [
                                bluejs::ObjectProp::KeyValue {
                                    key: bluejs::PropertyKey::Identifier(name),
                                    shorthand: false,
                                    ..
                                },
                                bluejs::ObjectProp::KeyValue {
                                    key: bluejs::PropertyKey::Identifier(age),
                                    shorthand: false,
                                    ..
                                },
                            ] if name == "name" && age == "age"
                        )
                    )
            )
    ));
    assert_eq!(
        bluejs::Vm::default().execute(&artifact.bytecode).unwrap(),
        bluejs::Value::String("Ada:42".into())
    );
}

#[test]
fn lowers_checked_object_shorthand_properties() {
    let artifact = compile_direct_script(
            ENTRY,
            &MapLoader::from([ModuleSource::new(
                ENTRY,
                "const label: string = 'Ada'; const person: { label: string } = { label }; person.label;",
            )]),
            CompilerOptions::default(),
        )
        .unwrap();
    assert!(matches!(
        artifact.program,
        bluejs::BlueJsProgramV1::Script(bluejs::Program { ref body })
            if matches!(
                body.as_slice(),
                [bluejs::Stmt::VarDecl(_, _), bluejs::Stmt::VarDecl(_, declarations), bluejs::Stmt::Expr(_)]
                    if matches!(
                        declarations.as_slice(),
                        [bluejs::VarDeclarator {
                            init: Some(bluejs::Expr::Object(properties)),
                            ..
                        }] if matches!(
                            properties.as_slice(),
                            [bluejs::ObjectProp::KeyValue { shorthand: true, .. }]
                        )
                    )
            )
    ));
    assert_eq!(
        bluejs::Vm::default().execute(&artifact.bytecode).unwrap(),
        bluejs::Value::String("Ada".into())
    );
}

#[test]
fn lowers_checked_string_and_numeric_object_keys() {
    let artifact = compile_direct_script(
            ENTRY,
            &MapLoader::from([ModuleSource::new(
                ENTRY,
                "const value = { 'display-name': 'Ada', 42: 'answer' }; value['display-name'] + ':' + value[42];",
            )]),
            CompilerOptions::default(),
        )
        .unwrap();
    assert_eq!(
        bluejs::Vm::default().execute(&artifact.bytecode).unwrap(),
        bluejs::Value::String("Ada:answer".into())
    );
}

#[test]
fn lowers_checked_computed_object_property_keys() {
    let artifact = compile_direct_script(
        ENTRY,
        &MapLoader::from([ModuleSource::new(
            ENTRY,
            concat!(
                "const key: string = 'display-name';",
                "const value = { [key]: 'Ada', ['answer']: 42 };",
                "`${value[key]}:${value['answer']}`;"
            ),
        )]),
        CompilerOptions::default(),
    )
    .unwrap();
    assert!(matches!(
        artifact.program,
        bluejs::BlueJsProgramV1::Script(bluejs::Program { ref body })
            if matches!(
                body.as_slice(),
                [bluejs::Stmt::VarDecl(_, _), bluejs::Stmt::VarDecl(_, declarations), bluejs::Stmt::Expr(_)]
                    if matches!(
                        declarations.as_slice(),
                        [bluejs::VarDeclarator {
                            init: Some(bluejs::Expr::Object(properties)),
                            ..
                        }] if matches!(
                            properties.as_slice(),
                            [
                                bluejs::ObjectProp::KeyValue {
                                    key: bluejs::PropertyKey::Computed(_),
                                    shorthand: false,
                                    ..
                                },
                                bluejs::ObjectProp::KeyValue {
                                    key: bluejs::PropertyKey::Computed(_),
                                    shorthand: false,
                                    ..
                                },
                            ]
                        )
                    )
            )
    ));
    assert_eq!(
        bluejs::Vm::default().execute(&artifact.bytecode).unwrap(),
        bluejs::Value::String("Ada:42".into())
    );
}

#[test]
fn lowers_checked_member_calls_with_the_receiver() {
    let artifact = compile_direct_script(
        ENTRY,
        &MapLoader::from([ModuleSource::new(
            ENTRY,
            "const label: string = 'ada'; label.toUpperCase();",
        )]),
        CompilerOptions::default(),
    )
    .unwrap();
    assert_eq!(
        bluejs::Vm::default().execute(&artifact.bytecode).unwrap(),
        bluejs::Value::String("ADA".into())
    );
}

#[test]
fn lowers_checked_identifier_constructor_calls() {
    let artifact = compile_direct_script(
        ENTRY,
        &MapLoader::from([ModuleSource::new(
            ENTRY,
            "const value = new Object(); typeof value === 'object';",
        )]),
        CompilerOptions::default(),
    )
    .unwrap();
    assert_eq!(
        bluejs::Vm::default().execute(&artifact.bytecode).unwrap(),
        bluejs::Value::Bool(true)
    );
}

#[test]
fn lowers_checked_in_and_instanceof_expressions() {
    let artifact = compile_direct_script(
        ENTRY,
        &MapLoader::from([ModuleSource::new(
            ENTRY,
            "const record: { label: string } = { label: 'Ada' }; \
                 const hasLabel: boolean = 'label' in record; \
                 const value = new Object(); \
                 const isObject: boolean = value instanceof Object; \
                 hasLabel && isObject;",
        )]),
        CompilerOptions::default(),
    )
    .unwrap();
    assert_eq!(
        bluejs::Vm::default().execute(&artifact.bytecode).unwrap(),
        bluejs::Value::Bool(true)
    );
}

#[test]
fn lowers_checked_property_delete_expressions() {
    let artifact = compile_direct_script(
        ENTRY,
        &MapLoader::from([ModuleSource::new(
            ENTRY,
            "const value: { label?: string; title?: string } = { \
                    label: 'Ada', title: 'Countess' \
                 }; delete value.label; delete value['title']; \
                 value.label === undefined && value.title === undefined;",
        )]),
        CompilerOptions::default(),
    )
    .unwrap();
    assert_eq!(
        bluejs::Vm::default().execute(&artifact.bytecode).unwrap(),
        bluejs::Value::Bool(true)
    );
}

#[test]
fn lowers_checked_array_holes_without_materializing_undefined_properties() {
    let artifact = compile_direct_script(
        ENTRY,
        &MapLoader::from([ModuleSource::new(
            ENTRY,
            "const values = [1,,3]; values.length === 3 && !(1 in values) && values[1] === undefined;",
        )]),
        CompilerOptions::default(),
    )
    .unwrap();
    assert!(matches!(
        artifact.program,
        bluejs::BlueJsProgramV1::Script(bluejs::Program { ref body })
            if matches!(
                body.as_slice(),
                [bluejs::Stmt::VarDecl(_, declarations), bluejs::Stmt::Expr(_)]
                    if matches!(
                        declarations.as_slice(),
                        [bluejs::VarDeclarator {
                            init: Some(bluejs::Expr::Array(elements)),
                            ..
                        }] if matches!(
                            elements.as_slice(),
                            [
                                Some(bluejs::ArrayElement::Normal(bluejs::Expr::Number(1.0))),
                                None,
                                Some(bluejs::ArrayElement::Normal(bluejs::Expr::Number(3.0))),
                            ]
                        )
                    )
            )
    ));
    assert_eq!(
        bluejs::Vm::default().execute(&artifact.bytecode).unwrap(),
        bluejs::Value::Bool(true)
    );
}

#[test]
fn rejects_array_literals_that_mix_holes_and_spread() {
    let error = match compile_direct_script(
        ENTRY,
        &MapLoader::from([ModuleSource::new(
            ENTRY,
            "const suffix = [2]; const mixed = [1,,...suffix];",
        )]),
        CompilerOptions::default(),
    ) {
        Ok(_) => panic!("the direct bridge must reject a mixed hole/spread array"),
        Err(error) => error,
    };
    let BridgeError::UnsupportedRuntimeTarget { message, .. } = error else {
        panic!("the direct bridge must reject a mixed hole/spread array");
    };
    assert!(message.contains("cannot combine holes and spread"));
}

#[test]
fn lowers_checked_property_update_expressions() {
    let artifact = compile_direct_script(
        ENTRY,
        &MapLoader::from([ModuleSource::new(
            ENTRY,
            "const values: { count: number; index: number } = { count: 1, index: 2 }; \
                 const postfix: number = values.count++; \
                 const prefix = ++values['index']; \
                 postfix + ':' + values.count + ':' + prefix + ':' + values['index'];",
        )]),
        CompilerOptions::default(),
    )
    .unwrap();
    assert_eq!(
        bluejs::Vm::default().execute(&artifact.bytecode).unwrap(),
        bluejs::Value::String("1:2:3:3".into())
    );
}

#[test]
fn lowers_checked_array_spread_elements() {
    let artifact = compile_direct_script(
        ENTRY,
        &MapLoader::from([ModuleSource::new(
            ENTRY,
            "const suffix: number[] = [2, 3]; \
                 const values: number[] = [1, ...suffix, 4]; \
                 values.length === 4 && values[2] === 3;",
        )]),
        CompilerOptions::default(),
    )
    .unwrap();
    assert_eq!(
        bluejs::Vm::default().execute(&artifact.bytecode).unwrap(),
        bluejs::Value::Bool(true)
    );
}

#[test]
fn lowers_checked_object_spread_properties() {
    let artifact = compile_direct_script(
        ENTRY,
        &MapLoader::from([ModuleSource::new(
            ENTRY,
            "const source: { label: string } = { label: 'Ada' }; \
                 const person: { label: string; title: string } = { \
                    ...source, label: 'Grace', title: 'Countess' \
                 }; person.label + ':' + person.title;",
        )]),
        CompilerOptions::default(),
    )
    .unwrap();
    assert_eq!(
        bluejs::Vm::default().execute(&artifact.bytecode).unwrap(),
        bluejs::Value::String("Grace:Countess".into())
    );
}

#[test]
fn lowers_checked_spread_call_and_constructor_arguments() {
    let artifact = compile_direct_script(
        ENTRY,
        &MapLoader::from([ModuleSource::new(
            ENTRY,
            "function add(left: number, right: number): number { return left + right; } \
                 const pair: [number, number] = [40, 2]; \
                 const empty: [] = []; \
                 const value = new Object(...empty); \
                 add(...pair) + ':' + typeof value;",
        )]),
        CompilerOptions::default(),
    )
    .unwrap();
    assert_eq!(
        bluejs::Vm::default().execute(&artifact.bytecode).unwrap(),
        bluejs::Value::String("42:object".into())
    );
}

#[test]
fn lowers_checked_array_typed_rest_parameters() {
    let artifact = compile_direct_script(
            ENTRY,
            &MapLoader::from([ModuleSource::new(
                ENTRY,
                concat!(
                    "function sum(base: number, ...values: number[]): number { return base + values[0]; }",
                    "const pair: [number] = [2];",
                    "sum(40, ...pair);"
                ),
            )]),
            CompilerOptions::default(),
        )
        .unwrap();
    assert_eq!(
        bluejs::Vm::default().execute(&artifact.bytecode).unwrap(),
        bluejs::Value::Number(42.0)
    );
}

#[test]
fn lowers_checked_default_parameter_expressions() {
    let artifact = compile_direct_script(
            ENTRY,
            &MapLoader::from([ModuleSource::new(
                ENTRY,
                concat!(
                    "function add(left: number, right: number): number { return left + right; }",
                    "function scale(value: number = add(20, 1), multiplier: number = 2): number { return value * multiplier; }",
                    "scale(undefined);"
                ),
            )]),
            CompilerOptions::default(),
        )
        .unwrap();
    assert_eq!(
        bluejs::Vm::default().execute(&artifact.bytecode).unwrap(),
        bluejs::Value::Number(42.0)
    );
}

#[test]
fn lowers_checked_optional_parameters() {
    let artifact = compile_direct_script(
        ENTRY,
        &MapLoader::from([ModuleSource::new(
            ENTRY,
            concat!(
                "function label(value?: string): string { return value ?? 'guest'; }",
                "const omitted: string = label();",
                "const explicitUndefined: string = label(undefined);",
                "omitted + ':' + explicitUndefined;"
            ),
        )]),
        CompilerOptions::default(),
    )
    .unwrap();
    assert_eq!(
        bluejs::Vm::default().execute(&artifact.bytecode).unwrap(),
        bluejs::Value::String("guest:guest".into())
    );
}

#[test]
fn lowers_checked_bracket_property_reads() {
    let artifact = compile_direct_script(
        ENTRY,
        &MapLoader::from([ModuleSource::new(
            ENTRY,
            "const values: number[] = [1, 2, 3]; values[1];",
        )]),
        CompilerOptions::default(),
    )
    .unwrap();
    assert!(matches!(
        artifact.program,
        bluejs::BlueJsProgramV1::Script(bluejs::Program { ref body })
            if matches!(
                body.as_slice(),
                [
                    bluejs::Stmt::VarDecl(_, _),
                    bluejs::Stmt::Expr(bluejs::Expr::Member {
                        property,
                        computed: true,
                        ..
                    }),
                ] if matches!(property.as_ref(), bluejs::Expr::Number(index) if *index == 1.0)
            )
    ));
    assert_eq!(
        bluejs::Vm::default().execute(&artifact.bytecode).unwrap(),
        bluejs::Value::Number(2.0)
    );
}

#[test]
fn lowers_checked_non_substituted_template_literals() {
    let artifact = compile_direct_script(
        ENTRY,
        &MapLoader::from([ModuleSource::new(
            ENTRY,
            r"const label: string = `BlueTS`; label + '!';",
        )]),
        CompilerOptions::default(),
    )
    .unwrap();
    assert!(matches!(
        artifact.program,
        bluejs::BlueJsProgramV1::Script(bluejs::Program { ref body })
            if matches!(
                body.as_slice(),
                [bluejs::Stmt::VarDecl(_, declarations), bluejs::Stmt::Expr(_)]
                    if matches!(
                        declarations.as_slice(),
                        [bluejs::VarDeclarator {
                            init: Some(bluejs::Expr::Template { quasis, expressions }),
                            ..
                        }] if quasis.as_slice() == [bluejs::JsString::from("BlueTS")]
                            && expressions.is_empty()
                    )
            )
    ));
    assert_eq!(
        bluejs::Vm::default().execute(&artifact.bytecode).unwrap(),
        bluejs::Value::String("BlueTS!".into())
    );
}

#[test]
fn lowers_checked_escaped_template_literals() {
    let artifact = compile_direct_script(
        ENTRY,
        &MapLoader::from([ModuleSource::new(
            ENTRY,
            "const label: string = `Ada\\nGrace`; label;",
        )]),
        CompilerOptions::default(),
    )
    .unwrap();
    assert_eq!(
        bluejs::Vm::default().execute(&artifact.bytecode).unwrap(),
        bluejs::Value::String("Ada\nGrace".into())
    );
}

#[test]
fn lowers_checked_identifier_template_substitutions() {
    let artifact = compile_direct_script(
        ENTRY,
        &MapLoader::from([ModuleSource::new(
            ENTRY,
            "const label: string = 'Ada'; `Hello, ${label}!`;",
        )]),
        CompilerOptions::default(),
    )
    .unwrap();
    assert_eq!(
        bluejs::Vm::default().execute(&artifact.bytecode).unwrap(),
        bluejs::Value::String("Hello, Ada!".into())
    );
}

#[test]
fn lowers_checked_template_substitution_expressions() {
    let artifact = compile_direct_script(
        ENTRY,
        &MapLoader::from([ModuleSource::new(
            ENTRY,
            concat!(
                "const values: number[] = [40, 2];",
                "`${({ label: 'BlueTSC' }).label}: ${values[0] + values[1]}`;"
            ),
        )]),
        CompilerOptions::default(),
    )
    .unwrap();
    assert_eq!(
        bluejs::Vm::default().execute(&artifact.bytecode).unwrap(),
        bluejs::Value::String("BlueTSC: 42".into())
    );
}
