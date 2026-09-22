// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Expression, template, and graph-lowering regressions.

use super::*;
use blueice_bluets::{AuthorizedModule, AuthorizedModuleLoader, AuthorizedModuleResolution};

#[test]
fn lowers_checked_simple_string_escapes() {
    let artifact = compile_direct_script(
        ENTRY,
        &MapLoader::from([ModuleSource::new(
            ENTRY,
            "const label: string = 'Ada\\nGrace'; label;",
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
fn lowers_checked_property_assignments() {
    let artifact = compile_direct_script(
        ENTRY,
        &MapLoader::from([ModuleSource::new(
            ENTRY,
            "const person: { name: string } = { name: 'Ada' }; \
                 person.name = 'Grace'; person.name;",
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
                    bluejs::Stmt::Expr(bluejs::Expr::Assign {
                        target,
                        ..
                    }),
                    bluejs::Stmt::Expr(_),
                ] if matches!(target.as_ref(), bluejs::Expr::Member { computed: false, ..})
            )
    ));
    assert_eq!(
        bluejs::Vm::default().execute(&artifact.bytecode).unwrap(),
        bluejs::Value::String("Grace".into())
    );
}

#[test]
fn lowers_checked_compound_property_assignments() {
    let artifact = compile_direct_script(
        ENTRY,
        &MapLoader::from([ModuleSource::new(
            ENTRY,
            "const values: number[] = [1, 2]; values[1] += 40; values[1];",
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
fn expression_lowerer_rejects_array_holes() {
    let tokens = expression_tokens(&[
        ("[", TokenKind::Punct),
        ("1", TokenKind::Number),
        (",", TokenKind::Punct),
        (",", TokenKind::Punct),
        ("]", TokenKind::Punct),
    ]);
    let error = ExpressionLowerer::new(ENTRY, &tokens).parse().unwrap_err();
    let BridgeError::UnsupportedRuntimeTarget { message, .. } = error else {
        panic!("the direct bridge must reject an array hole");
    };
    assert!(message.contains("array holes"));
}

#[test]
fn expression_lowerer_lowers_computed_object_properties() {
    let tokens = expression_tokens(&[
        ("{", TokenKind::Punct),
        ("[", TokenKind::Punct),
        ("name", TokenKind::Identifier),
        ("]", TokenKind::Punct),
        (":", TokenKind::Punct),
        ("1", TokenKind::Number),
        ("}", TokenKind::Punct),
    ]);
    assert!(matches!(
        ExpressionLowerer::new(ENTRY, &tokens).parse(),
        Ok(bluejs::Expr::Object(properties))
            if matches!(
                properties.as_slice(),
                [bluejs::ObjectProp::KeyValue {
                    key: bluejs::PropertyKey::Computed(expression),
                    value: bluejs::Expr::Number(1.0),
                    shorthand: false,
                }] if matches!(expression.as_ref(), bluejs::Expr::Identifier(name) if name == "name")
            )
    ));
}

#[test]
fn expression_lowerer_reports_malformed_computed_object_property_keys() {
    let unterminated = expression_tokens(&[
        ("{", TokenKind::Punct),
        ("[", TokenKind::Punct),
        ("name", TokenKind::Identifier),
    ]);
    let error = ExpressionLowerer::new(ENTRY, &unterminated)
        .parse()
        .unwrap_err();
    let BridgeError::UnsupportedRuntimeTarget { message, .. } = error else {
        panic!("the direct bridge must reject an unterminated computed object key");
    };
    assert!(message.contains("unterminated computed object property key"));

    let missing_closing = expression_tokens(&[
        ("{", TokenKind::Punct),
        ("[", TokenKind::Punct),
        ("name", TokenKind::Identifier),
        (":", TokenKind::Punct),
        ("1", TokenKind::Number),
        ("}", TokenKind::Punct),
    ]);
    let error = ExpressionLowerer::new(ENTRY, &missing_closing)
        .parse()
        .unwrap_err();
    let BridgeError::UnsupportedRuntimeTarget { message, .. } = error else {
        panic!("the direct bridge must reject an unclosed computed object key");
    };
    assert!(message.contains("expected `]`"));
}

#[test]
fn expression_lowerer_reports_invalid_static_object_property_keys() {
    let invalid_number = expression_tokens(&[
        ("{", TokenKind::Punct),
        ("not-a-number", TokenKind::Number),
        (":", TokenKind::Punct),
        ("1", TokenKind::Number),
        ("}", TokenKind::Punct),
    ]);
    let error = ExpressionLowerer::new(ENTRY, &invalid_number)
        .parse()
        .unwrap_err();
    let BridgeError::UnsupportedRuntimeTarget { message, .. } = error else {
        panic!("the direct bridge must reject an invalid numeric object key");
    };
    assert!(message.contains("unsupported numeric object key"));

    let invalid_key = expression_tokens(&[
        ("{", TokenKind::Punct),
        ("?", TokenKind::Punct),
        (":", TokenKind::Punct),
        ("1", TokenKind::Number),
        ("}", TokenKind::Punct),
    ]);
    let error = ExpressionLowerer::new(ENTRY, &invalid_key)
        .parse()
        .unwrap_err();
    let BridgeError::UnsupportedRuntimeTarget { message, .. } = error else {
        panic!("the direct bridge must reject an invalid object key");
    };
    assert!(message.contains("object property keys"));

    let missing_colon =
        expression_tokens(&[("{", TokenKind::Punct), ("label", TokenKind::Identifier)]);
    let error = ExpressionLowerer::new(ENTRY, &missing_colon)
        .parse()
        .unwrap_err();
    let BridgeError::UnsupportedRuntimeTarget { message, .. } = error else {
        panic!("the direct bridge must reject an object key without a value");
    };
    assert!(message.contains("unterminated object literal"));
}

#[test]
fn expression_lowerer_rejects_non_property_delete_targets() {
    let tokens = expression_tokens(&[
        ("delete", TokenKind::Keyword),
        ("value", TokenKind::Identifier),
    ]);
    let error = ExpressionLowerer::new(ENTRY, &tokens).parse().unwrap_err();
    let BridgeError::UnsupportedRuntimeTarget { message, .. } = error else {
        panic!("the direct bridge must reject an identifier delete target");
    };
    assert!(message.contains("property delete targets"));
}

#[test]
fn expression_lowerer_rejects_unimplemented_template_substitutions_and_escapes() {
    for text in [r"`value=${person?.name}`", r"`line\u0041`"] {
        let token = Token {
            kind: TokenKind::Template,
            text: text.to_string(),
            start: 0,
            end: text.len(),
        };
        let error = ExpressionLowerer::new(ENTRY, &[token]).parse().unwrap_err();
        let BridgeError::UnsupportedRuntimeTarget { message, .. } = error else {
            panic!("the direct bridge must reject an unimplemented template shape");
        };
        assert!(message.contains("unsupported expression") || message.contains("string escape"));
    }
}

#[test]
fn template_substitution_errors_retain_the_original_source_offset() {
    let source = concat!(
        "const person: { name: string } = { name: 'Ada' };",
        "`value=${person?.name}`;"
    );
    let error = match compile_direct_script(
        ENTRY,
        &MapLoader::from([ModuleSource::new(ENTRY, source)]),
        CompilerOptions::default(),
    ) {
        Err(error) => error,
        Ok(_) => panic!("the optional template expression must be rejected by the bridge"),
    };
    let BridgeError::UnsupportedRuntimeTarget { span, .. } = error else {
        panic!("the optional template expression must be rejected by the bridge");
    };
    assert_eq!(span.module, ENTRY);
    assert!(span.start > source.find('`').unwrap());
}

#[test]
fn lowers_identifier_prefix_and_postfix_updates() {
    let artifact = compile_direct_script(
        ENTRY,
        &MapLoader::from([ModuleSource::new(
            ENTRY,
            "let value: number = 1; const postfix = value++; const prefix = ++value; \
                 const decremented = --value; const tail = value--; \
                 postfix + ':' + prefix + ':' + decremented + ':' + tail + ':' + value;",
        )]),
        CompilerOptions::default(),
    )
    .unwrap();
    assert_eq!(
        bluejs::Vm::default().execute(&artifact.bytecode).unwrap(),
        bluejs::Value::String("1:3:2:2:1".into())
    );
}

#[test]
fn expression_lowerer_rejects_non_property_update_targets() {
    let tokens = expression_tokens(&[("++", TokenKind::Punct), ("1", TokenKind::Number)]);
    let error = ExpressionLowerer::new(ENTRY, &tokens).parse().unwrap_err();
    let BridgeError::UnsupportedRuntimeTarget { message, .. } = error else {
        panic!("the direct bridge must reject a non-property update target");
    };
    assert!(message.contains("identifier and property update targets"));
}

#[test]
fn lowers_exponentiation_bitwise_shift_and_logical_compound_assignments() {
    let artifact = compile_direct_script(
            ENTRY,
            &MapLoader::from([ModuleSource::new(
                ENTRY,
                "let value: number = 2; value **= 3; value <<= 1; value |= 1; value ^= 3; \
                 value &= 14; value >>= 1; value >>>= 0; let missing: number | undefined = undefined; \
                 missing ??= 42; let zero: number = 0; zero ||= 5; let present: number = 7; \
                 present &&= 2; value + missing + zero + present;",
            )]),
            CompilerOptions::default(),
        )
        .unwrap();
    assert_eq!(
        bluejs::Vm::default().execute(&artifact.bytecode).unwrap(),
        bluejs::Value::Number(50.0)
    );
}

#[test]
fn lowers_conditional_expressions_with_assignment_precedence() {
    let artifact = compile_direct_script(
        ENTRY,
        &MapLoader::from([ModuleSource::new(
            ENTRY,
            "function choose(value: number) { \
                 return value === 42 ? value + 1 : value - 1; } choose(42);",
        )]),
        CompilerOptions::default(),
    )
    .unwrap();
    assert_eq!(
        bluejs::Vm::default().execute(&artifact.bytecode).unwrap(),
        bluejs::Value::Number(43.0)
    );
}

#[test]
fn lowers_ordered_expression_statements_in_direct_functions() {
    let artifact = compile_direct_script(
            ENTRY,
            &MapLoader::from([ModuleSource::new(
                ENTRY,
                "let total: number = 0; function remember(value: number): number { total += value; return total; } remember(2); remember(3); total;",
            )]),
            CompilerOptions::default(),
        )
        .unwrap();
    let bluejs::BlueJsProgramV1::Script(program) = &artifact.program else {
        panic!("the direct script bridge must produce a script program");
    };
    assert!(matches!(
        program.body.as_slice(),
        [
            bluejs::Stmt::VarDecl(_, _),
            bluejs::Stmt::FunctionDecl(bluejs::Function { body, .. }),
            bluejs::Stmt::Expr(_),
            bluejs::Stmt::Expr(_),
            bluejs::Stmt::Expr(_),
        ] if matches!(
            body.as_slice(),
            [bluejs::Stmt::Expr(bluejs::Expr::Assign { .. }), bluejs::Stmt::Return(Some(_))]
        )
    ));
    assert_eq!(
        bluejs::Vm::default().execute(&artifact.bytecode).unwrap(),
        bluejs::Value::Number(5.0)
    );
}

#[test]
fn lowers_throw_statements_in_direct_functions() {
    let artifact = compile_direct_script(
        ENTRY,
        &MapLoader::from([ModuleSource::new(
            ENTRY,
            "function fail(): never { throw 'broken'; } fail();",
        )]),
        CompilerOptions::default(),
    )
    .unwrap();
    let bluejs::BlueJsProgramV1::Script(program) = &artifact.program else {
        panic!("the direct script bridge must produce a script program");
    };
    assert!(matches!(
        program.body.as_slice(),
        [
            bluejs::Stmt::FunctionDecl(bluejs::Function { body, .. }),
            bluejs::Stmt::Expr(bluejs::Expr::Call { .. }),
        ] if matches!(
            body.as_slice(),
            [bluejs::Stmt::Throw(bluejs::Expr::String(message))] if message == "broken"
        )
    ));
    assert_eq!(
        bluejs::Vm::default().execute(&artifact.bytecode),
        Err(bluejs::RuntimeError::Thrown(bluejs::Value::String(
            "broken".into()
        )))
    );
}

#[test]
fn lowers_braced_if_else_statements_in_direct_functions() {
    let artifact = compile_direct_script(
            ENTRY,
            &MapLoader::from([ModuleSource::new(
                ENTRY,
                "function label(value: number): string { if (value > 0) { return 'positive'; } else { return 'other'; } } label(2) + ':' + label(0);",
            )]),
            CompilerOptions::default(),
        )
        .unwrap();
    let bluejs::BlueJsProgramV1::Script(program) = &artifact.program else {
        panic!("the direct script bridge must produce a script program");
    };
    assert!(matches!(
        program.body.as_slice(),
        [
            bluejs::Stmt::FunctionDecl(bluejs::Function { body, .. }),
            bluejs::Stmt::Expr(_),
        ] if matches!(
            body.as_slice(),
            [bluejs::Stmt::If {
                test: bluejs::Expr::Binary { op: bluejs::BinaryOp::Gt, .. },
                consequent,
                alternate: Some(alternate),
            }]
                if matches!(consequent.as_ref(), bluejs::Stmt::Block(items) if matches!(items.as_slice(), [bluejs::Stmt::Return(Some(_))]))
                    && matches!(alternate.as_ref(), bluejs::Stmt::Block(items) if matches!(items.as_slice(), [bluejs::Stmt::Return(Some(_))]))
        )
    ));
    assert_eq!(
        bluejs::Vm::default().execute(&artifact.bytecode).unwrap(),
        bluejs::Value::String("positive:other".into())
    );
}

#[test]
fn lowers_nested_braced_if_statements_in_direct_functions() {
    let artifact = compile_direct_script(
            ENTRY,
            &MapLoader::from([ModuleSource::new(
                ENTRY,
                "function label(value: number): string { if (value > 0) { if (value > 1) { return 'many'; } else { return 'one'; } } else { return 'none'; } } label(2) + ':' + label(0);",
            )]),
            CompilerOptions::default(),
        )
        .unwrap();
    let bluejs::BlueJsProgramV1::Script(program) = &artifact.program else {
        panic!("the direct script bridge must produce a script program");
    };
    assert!(matches!(
        program.body.as_slice(),
        [bluejs::Stmt::FunctionDecl(bluejs::Function { body, .. }), bluejs::Stmt::Expr(_)]
            if matches!(
                body.as_slice(),
                [bluejs::Stmt::If { consequent, .. }]
                    if matches!(
                        consequent.as_ref(),
                        bluejs::Stmt::Block(items)
                            if matches!(items.as_slice(), [bluejs::Stmt::If { .. }])
                    )
            )
    ));
    assert_eq!(
        bluejs::Vm::default().execute(&artifact.bytecode).unwrap(),
        bluejs::Value::String("many:none".into())
    );
}

#[test]
fn lowers_braced_else_if_statements_in_direct_functions() {
    let artifact = compile_direct_script(
            ENTRY,
            &MapLoader::from([ModuleSource::new(
                ENTRY,
                "function label(value: number): string { if (value > 1) { return 'many'; } else if (value > 0) { return 'one'; } else { return 'none'; } } label(2) + ':' + label(1) + ':' + label(0);",
            )]),
            CompilerOptions::default(),
        )
        .unwrap();
    let bluejs::BlueJsProgramV1::Script(program) = &artifact.program else {
        panic!("the direct script bridge must produce a script program");
    };
    assert!(matches!(
        program.body.as_slice(),
        [bluejs::Stmt::FunctionDecl(bluejs::Function { body, .. }), bluejs::Stmt::Expr(_)]
            if matches!(
                body.as_slice(),
                [bluejs::Stmt::If { alternate: Some(alternate), .. }]
                    if matches!(alternate.as_ref(), bluejs::Stmt::If { .. })
            )
    ));
    assert_eq!(
        bluejs::Vm::default().execute(&artifact.bytecode).unwrap(),
        bluejs::Value::String("many:one:none".into())
    );
}

#[test]
fn refuses_to_silently_drop_an_unstructured_function_body_statement() {
    let result = compile_direct_script(
        ENTRY,
        &MapLoader::from([ModuleSource::new(
            ENTRY,
            "function answer(): number { if (true) return 42; return 0; } answer();",
        )]),
        CompilerOptions::default(),
    );
    let Err(BridgeError::UnsupportedRuntimeTarget { span, message }) = result else {
        panic!("the direct bridge must reject an opaque function body item");
    };
    assert_eq!(span.module, ENTRY);
    assert!(message.contains("function body syntax"));
}

#[test]
fn lowers_local_named_and_default_exports_to_a_bluejs_module() {
    let artifact = compile_direct_module(
        MODULE_ENTRY,
        &MapLoader::from([ModuleSource::new(
            MODULE_ENTRY,
            "const answer: number = 40 + 2; \
                 export { answer as publicAnswer }; export default answer; answer;",
        )]),
        CompilerOptions::default(),
    )
    .unwrap();
    assert_eq!(artifact.bridge_abi, BLUE_TS_BLUEJS_BRIDGE_ABI_V1);
    let bluejs::BlueJsProgramV1::Module(module) = &artifact.program else {
        panic!("the direct module bridge must produce a BlueJS module AST");
    };
    assert!(matches!(
        module.body.as_slice(),
        [bluejs::Stmt::VarDecl(_, _), bluejs::Stmt::Expr(bluejs::Expr::Identifier(name))]
            if name == "answer"
    ));
    assert!(module.imports.is_empty());
    assert!(module.requests.is_empty());
    assert_eq!(
        module.exports,
        vec![
            bluejs::ExportEntry::Local {
                export_name: "publicAnswer".to_string(),
                local_name: "answer".to_string(),
            },
            bluejs::ExportEntry::Local {
                export_name: "default".to_string(),
                local_name: "answer".to_string(),
            },
        ]
    );
    assert_eq!(
        bluejs::Vm::default()
            .execute_module(&artifact.bytecode)
            .unwrap(),
        bluejs::Value::Number(42.0)
    );
}

#[test]
fn preserves_bluets_resolved_targets_in_a_direct_module_graph() {
    let graph = compile_direct_module_graph(
        GRAPH_ENTRY,
        &MapLoader::from([
            ModuleSource::new(
                GRAPH_ENTRY,
                "import type { Shape } from './types.d.ts'; \
                     import { value } from './dep.ts'; \
                     export const answer: number = value + 1; answer;",
            ),
            ModuleSource::new("graph/dep.ts", "export const value: number = 41;"),
            ModuleSource::new(
                "graph/types.d.ts",
                "export interface Shape { label: string; }",
            ),
        ]),
        CompilerOptions::default(),
    )
    .unwrap();
    assert_eq!(graph.entry, GRAPH_ENTRY);
    assert_eq!(graph.modules.len(), 2);
    assert!(!graph.modules.contains_key("graph/types.d.ts"));
    let bluejs::BlueJsProgramV1::Module(main) = &graph.modules[GRAPH_ENTRY].program else {
        panic!("the direct graph entry must produce a BlueJS module AST");
    };
    assert_eq!(
        main.imports,
        vec![bluejs::ImportEntry {
            module_request: "graph/dep.ts".to_string(),
            import_name: bluejs::ImportName::Named("value".to_string()),
            local_name: Some("value".to_string()),
            module_type: bluejs::ModuleType::JavaScript,
        }]
    );
    assert_eq!(
        main.requests
            .iter()
            .map(|request| (request.specifier.as_str(), request.phase))
            .collect::<Vec<_>>(),
        vec![("graph/dep.ts", bluejs::ImportPhase::Evaluation)]
    );
    assert_eq!(
        bluejs::Vm::default()
            .execute_module_graph(&graph.entry, &graph.bytecode_map())
            .unwrap(),
        bluejs::Value::Number(42.0)
    );
}

#[test]
fn preserves_a_non_relative_caller_authorized_module_alias() {
    let graph = compile_direct_module_graph(
        "virtual/main.ts",
        &AliasedGraphLoader,
        CompilerOptions::default(),
    )
    .unwrap();
    let bluejs::BlueJsProgramV1::Module(main) = &graph.modules["virtual/main.ts"].program else {
        panic!("the direct graph entry must produce a BlueJS module AST");
    };
    assert_eq!(
        main.requests
            .iter()
            .map(|request| (request.specifier.as_str(), request.phase))
            .collect::<Vec<_>>(),
        vec![("canonical/runtime.ts", bluejs::ImportPhase::Evaluation)]
    );
    assert_eq!(
        bluejs::Vm::default()
            .execute_module_graph(&graph.entry, &graph.bytecode_map())
            .unwrap(),
        bluejs::Value::Number(42.0)
    );
}

#[test]
fn direct_graph_retains_host_authorized_canonical_module_records() {
    let entry = "page:///app/main.ts";
    let runtime = "page:///modules/runtime.ts";
    let loader = AuthorizedModuleLoader::new(
        [
            AuthorizedModule::new(
                entry,
                "import { value } from '@host/runtime'; \
                 export const answer: number = value + 1; answer;",
            ),
            AuthorizedModule::new(runtime, "export const value: number = 41;"),
        ],
        [AuthorizedModuleResolution::new(
            entry,
            "@host/runtime",
            runtime,
        )],
    )
    .unwrap();
    let graph = compile_direct_module_graph(
        entry,
        &loader,
        CompilerOptions {
            resolver_fingerprint: "page-authorized-resolver-v1".to_string(),
            ..CompilerOptions::default()
        },
    )
    .unwrap();
    let bluejs::BlueJsProgramV1::Module(main) = &graph.modules[entry].program else {
        panic!("the direct graph entry must produce a BlueJS module AST");
    };
    assert_eq!(
        main.requests
            .iter()
            .map(|request| request.specifier.as_str())
            .collect::<Vec<_>>(),
        vec![runtime]
    );
    assert_eq!(
        graph
            .sources
            .iter()
            .map(|source| source.module.as_str())
            .collect::<Vec<_>>(),
        vec![entry, runtime]
    );
    assert_eq!(
        bluejs::Vm::default()
            .execute_module_graph(&graph.entry, &graph.bytecode_map())
            .unwrap(),
        bluejs::Value::Number(42.0)
    );
}
