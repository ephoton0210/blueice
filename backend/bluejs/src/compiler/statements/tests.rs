// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

fn bare_compiler() -> Compiler {
    let limits = CompileLimits::default();
    Compiler {
        bytecode: Bytecode::empty(),
        names: vec![HashMap::new()],
        private_scopes: Vec::new(),
        next_private_scope: 0,
        scopes: Vec::new(),
        loops: Vec::new(),
        catch_var_slots: Vec::new(),
        max_bytecode_bytes: limits.max_bytecode_bytes,
        max_metadata_entries: limits.max_metadata_entries,
        max_list_items: limits.max_list_items,
        function: false,
        local_scope: 0,
        with_depth: 0,
        with_scope_depths: Vec::new(),
        annex_b_parameter_names: BTreeSet::new(),
        tail_call_blockers: 0,
        tail_call_pending: false,
    }
}

#[test]
fn malformed_internal_class_fields_are_rejected() {
    let assign = |target| {
        Stmt::Expr(Expr::Assign {
            op: AssignOp::Assign,
            target: Box::new(target),
            value: Box::new(Expr::Number(1.0)),
        })
    };
    assert!(public_field_definition(&Stmt::Empty).is_none());
    assert!(public_field_definition(&assign(Expr::Identifier("x".into()))).is_none());
    assert!(public_field_definition(&assign(Expr::Member {
        object: Box::new(Expr::Identifier("other".into())),
        property: Box::new(Expr::Identifier("field".into())),
        computed: false,
    }))
    .is_none());
    assert!(public_field_definition(&assign(Expr::Member {
        object: Box::new(Expr::This),
        property: Box::new(Expr::Identifier("#private".into())),
        computed: false,
    }))
    .is_none());
    assert_eq!(
        bare_compiler().class_field(&Stmt::Empty, None),
        Err(CompileError::InvalidSyntax("invalid class field AST"))
    );
    assert_eq!(
        bare_compiler().statement(
            &Stmt::ClassDecoratedField {
                field: Box::new(Stmt::Empty),
                record: "record".into(),
            },
            true,
        ),
        Err(CompileError::InvalidSyntax(
            "decoration record binding is not available in this function"
        ))
    );
    let mut compiler = bare_compiler();
    compiler.names[0].insert("record".into(), 0);
    assert_eq!(
        compiler.statement(
            &Stmt::ClassDecoratedField {
                field: Box::new(Stmt::Empty),
                record: "record".into(),
            },
            true,
        ),
        Err(CompileError::InvalidSyntax("invalid decorated field AST"))
    );
    assert_eq!(
        bare_compiler().statement(&Stmt::ClassExtraInitializers("missing".into()), true),
        Err(CompileError::InvalidSyntax(
            "decoration record binding is not available in this function"
        ))
    );
    assert_eq!(
        bare_compiler().statement(&Stmt::ClassPrivateBrand("missing".into()), true),
        Err(CompileError::InvalidSyntax(
            "private brand binding is not available in this function"
        ))
    );
}

#[test]
fn direct_statement_validation_catches_unreachable_parser_shapes() {
    let mut strict = bare_compiler();
    strict.bytecode.strict = true;
    assert_eq!(
        strict.statement(
            &Stmt::With {
                object: Expr::Number(1.0),
                body: Box::new(Stmt::Empty),
            },
            true,
        ),
        Err(CompileError::InvalidSyntax(
            "with is forbidden in strict mode"
        ))
    );
    bare_compiler()
        .statement(
            &Stmt::ModuleDefaultFunction {
                function: Function::default(),
                binding: MODULE_DEFAULT_BINDING.into(),
            },
            true,
        )
        .unwrap();

    let catch = CatchClause {
        param: Some(Pattern::Identifier("eval".into())),
        body: Vec::new(),
    };
    assert_eq!(
        strict.try_statement(&[], Some(&catch), None),
        Err(CompileError::InvalidSyntax(
            "strict catch parameters cannot bind eval or arguments"
        ))
    );
    let mut dynamic = bare_compiler();
    dynamic
        .bind_pattern(&Pattern::Identifier("unbound".into()), DeclKind::Var)
        .unwrap();
    assert!(dynamic
        .bytecode
        .instructions()
        .any(|instruction| instruction.opcode == Opcode::SetUnboundName));
    bare_compiler()
        .assignment_pattern_default(
            Some(&Expr::Number(1.0)),
            &AssignmentPattern::Target(Box::new(Expr::Member {
                object: Box::new(Expr::Identifier("object".into())),
                property: Box::new(Expr::Identifier("property".into())),
                computed: false,
            })),
        )
        .unwrap();
}

#[test]
fn labelled_statement_lowering_handles_all_loop_and_error_forms() {
    let while_loop = Stmt::While {
        test: Expr::Bool(false),
        body: Box::new(Stmt::Empty),
    };
    let do_while = Stmt::DoWhile {
        body: Box::new(Stmt::Empty),
        test: Expr::Bool(false),
    };
    let switch = Stmt::Switch {
        discriminant: Expr::Number(1.0),
        cases: Vec::new(),
    };
    for item in [&while_loop, &do_while, &switch] {
        let mut compiler = bare_compiler();
        compiler.labelled_statement("outer", item).unwrap();
        assert!(compiler.loops.is_empty());
        assert!(!compiler.bytecode.bytes().is_empty());
    }

    let mut strict = bare_compiler();
    strict.bytecode.strict = true;
    assert_eq!(
        strict.labelled_statement("yield", &Stmt::Empty),
        Err(CompileError::InvalidSyntax(
            "yield cannot be used as a label in strict code"
        ))
    );
    assert_eq!(
        bare_compiler().labelled_statement(
            "same",
            &Stmt::Labelled {
                label: "same".into(),
                item: Box::new(Stmt::Empty),
            },
        ),
        Err(CompileError::InvalidSyntax("duplicate label"))
    );
    for (item, expected) in [
        (
            Stmt::VarDecl(DeclKind::Let, Vec::new()),
            "a labelled statement cannot contain a lexical declaration",
        ),
        (
            Stmt::ClassDecl(Class {
                name: Some("C".into()),
                extends: None,
                elements: Vec::new(),
                decorators: Vec::new(),
                source_text: SourceText::default(),
            }),
            "a labelled statement cannot contain a class declaration",
        ),
        (
            Stmt::FunctionDecl(Function::default()),
            "invalid labelled function declaration",
        ),
    ] {
        assert_eq!(
            bare_compiler().labelled_statement("outer", &item),
            Err(CompileError::InvalidSyntax(expected))
        );
    }
}

#[test]
fn parenthesized_tail_calls_and_pattern_defaults_keep_their_paths() {
    let call = Expr::Call {
        callee: Box::new(Expr::Identifier("callee".into())),
        args: Vec::new(),
    };
    let mut compiler = bare_compiler();
    compiler.function = true;
    compiler.bytecode.strict = true;
    assert!(compiler
        .tail_position_return(&Expr::Parenthesized(Box::new(call)))
        .unwrap());
    assert!(compiler
        .bytecode
        .instructions()
        .any(|instruction| instruction.opcode == Opcode::TailCall));

    let mut compiler = bare_compiler();
    compiler
        .assignment_pattern_default(
            Some(&Expr::Number(1.0)),
            &AssignmentPattern::Array(Vec::new()),
        )
        .unwrap();
    assert!(compiler
        .bytecode
        .instructions()
        .any(|instruction| instruction.opcode == Opcode::JumpIfFalse));
}

#[test]
fn recursive_tail_call_errors_propagate_at_every_emission_boundary() {
    let call = Expr::Call {
        callee: Box::new(Expr::Identifier("self".into())),
        args: vec![Argument::Normal(Expr::Number(1.0))],
    };
    let mut full = bare_compiler();
    full.function = true;
    full.bytecode.strict = true;
    full.bytecode.self_slot = Some(0);
    full.names[0].insert("self".into(), 0);
    full.bytecode.bindings.push(Binding {
        name: "self".into(),
        mutable: false,
        strict_immutable: true,
        lexical: true,
        catch_parameter: false,
        eval_var: false,
    });
    full.loops.push(Loop {
        labels: Vec::new(),
        breakable: true,
        scope_depth: 0,
        breaks: Vec::new(),
        continues: Some(Vec::new()),
        iterator: Some(0),
    });
    full.tail_position_return(&call).unwrap();
    let bytes = u32::try_from(full.bytecode.bytes().len()).unwrap();
    assert!(full
        .bytecode
        .instructions()
        .any(|instruction| instruction.opcode == Opcode::IteratorClose));
    full.max_list_items = 0;
    assert_eq!(
        full.tail_position_return(&call),
        Err(CompileError::ProgramTooLarge)
    );
    for limit in 0..bytes {
        let mut limited = bare_compiler();
        limited.function = true;
        limited.bytecode.strict = true;
        limited.bytecode.self_slot = Some(0);
        limited.names[0].insert("self".into(), 0);
        limited.bytecode.bindings = full.bytecode.bindings.clone();
        limited.loops.push(Loop {
            labels: Vec::new(),
            breakable: true,
            scope_depth: 0,
            breaks: Vec::new(),
            continues: Some(Vec::new()),
            iterator: Some(0),
        });
        limited.max_bytecode_bytes = limit;
        assert_eq!(
            limited.tail_position_return(&call),
            Err(CompileError::ProgramTooLarge),
            "{limit}"
        );
    }
}

#[test]
fn annex_b_if_clause_and_active_labels_propagate_compilation_errors() {
    let declaration = Stmt::FunctionDecl(Function {
        name: Some("f".into()),
        ..Function::default()
    });
    let mut no_metadata = bare_compiler();
    no_metadata.max_metadata_entries = 0;
    assert_eq!(
        no_metadata.if_clause_statement(&declaration),
        Err(CompileError::ProgramTooLarge)
    );

    let mut full = bare_compiler();
    full.if_clause_statement(&declaration).unwrap();
    let size = u32::try_from(full.bytecode.bytes().len()).unwrap();
    for limit in 0..size {
        let mut limited = bare_compiler();
        limited.max_bytecode_bytes = limit;
        assert_eq!(
            limited.if_clause_statement(&declaration),
            Err(CompileError::ProgramTooLarge),
            "{limit}"
        );
    }

    let mut active_label = bare_compiler();
    active_label.loops.push(Loop {
        labels: vec!["outer".into()],
        breakable: false,
        scope_depth: 0,
        breaks: Vec::new(),
        continues: None,
        iterator: None,
    });
    assert_eq!(
        active_label.labelled_statement("outer", &Stmt::Empty),
        Err(CompileError::InvalidSyntax("duplicate label"))
    );
    let labelled_function = Stmt::Labelled {
        label: "outer".into(),
        item: Box::new(declaration),
    };
    assert!(is_labelled_function(&labelled_function));
    assert!(!is_labelled_function(&Stmt::Empty));
}

#[test]
fn empty_catch_environment_and_plain_try_are_handled() {
    let mut compiler = bare_compiler();
    compiler.names.clear();
    assert_eq!(compiler.eval_catch_parameter_slot("missing"), None);

    let mut compiler = bare_compiler();
    compiler.try_statement(&[], None, Some(&[])).unwrap();
    assert!(compiler.bytecode.handlers[0].finally.is_some());
    let mut compiler = bare_compiler();
    compiler
        .try_statement(
            &[],
            Some(&CatchClause {
                param: None,
                body: Vec::new(),
            }),
            None,
        )
        .unwrap();
    assert!(compiler.bytecode.handlers[0].catch.is_some());
}

fn total_code_bytes(code: &Bytecode) -> usize {
    code.bytes().len() + code.child_code_units().map(total_code_bytes).sum::<usize>()
}

fn assert_compiler_byte_boundaries(
    label: &str,
    setup: impl Fn(&mut Compiler),
    lower: impl Fn(&mut Compiler) -> Result<(), CompileError>,
) {
    let mut full = bare_compiler();
    setup(&mut full);
    lower(&mut full).unwrap();
    let upper = u32::try_from(total_code_bytes(&full.bytecode)).unwrap();
    let mut first_success = None;
    for limit in 0..=upper {
        let mut limited = bare_compiler();
        setup(&mut limited);
        limited.max_bytecode_bytes = limit;
        match lower(&mut limited) {
            Ok(()) => {
                first_success.get_or_insert(limit);
                assert_eq!(limited.bytecode.bytes(), full.bytecode.bytes());
            }
            Err(CompileError::ProgramTooLarge) => {
                assert!(first_success.is_none(), "late failure at {limit}: {label}");
            }
            Err(error) => panic!("{label}: {limit}: {error:?}"),
        }
    }
    assert!(first_success.is_some(), "{label}");
}

fn assert_statement_byte_boundaries(statement: &Stmt, setup: impl Fn(&mut Compiler)) {
    assert_compiler_byte_boundaries(&format!("{statement:?}"), setup, |compiler| {
        compiler.statement(statement, true)
    });
}

#[test]
fn statement_forms_propagate_every_byte_budget_failure() {
    let no_setup = |_: &mut Compiler| {};
    let function = || Function {
        name: Some("f".into()),
        ..Function::default()
    };
    let class = || Class {
        name: Some("C".into()),
        extends: None,
        elements: Vec::new(),
        decorators: Vec::new(),
        source_text: SourceText::default(),
    };
    let var = |pattern, init| Stmt::VarDecl(DeclKind::Var, vec![VarDeclarator { pattern, init }]);

    let statements = [
        Stmt::ModuleDefaultFunction {
            function: Function::default(),
            binding: MODULE_DEFAULT_BINDING.into(),
        },
        Stmt::ModuleDefaultFunction {
            function: function(),
            binding: "dynamic".into(),
        },
        Stmt::Expr(Expr::Class(class())),
        Stmt::Try {
            block: vec![Stmt::Expr(Expr::Number(1.0))],
            handler: Some(CatchClause {
                param: None,
                body: vec![Stmt::Expr(Expr::Number(2.0))],
            }),
            finalizer: Some(vec![Stmt::Expr(Expr::Number(3.0))]),
        },
        Stmt::Switch {
            discriminant: Expr::Number(1.0),
            cases: vec![SwitchCase {
                test: Some(Expr::Number(1.0)),
                consequent: vec![Stmt::FunctionDecl(function())],
            }],
        },
        Stmt::For {
            init: Some(ForInit::Expr(Expr::Number(1.0))),
            test: Some(Expr::Bool(false)),
            update: None,
            body: Box::new(Stmt::Empty),
        },
        var(
            Pattern::Array(vec![Some(ArrayPatternElement {
                pattern: Pattern::Identifier("item".into()),
                default: Some(Expr::Number(1.0)),
                rest: false,
            })]),
            Some(Expr::Array(Vec::new())),
        ),
        var(
            Pattern::Object(vec![ObjectPatternProp::Rest(Pattern::Identifier(
                "rest".into(),
            ))]),
            Some(Expr::Object(Vec::new())),
        ),
        Stmt::ClassField(Box::new(Stmt::Expr(Expr::Assign {
            op: AssignOp::Assign,
            target: Box::new(Expr::Member {
                object: Box::new(Expr::This),
                property: Box::new(Expr::Identifier("field".into())),
                computed: false,
            }),
            value: Box::new(Expr::Function(Function::default())),
        }))),
    ];
    for statement in &statements {
        assert_statement_byte_boundaries(statement, no_setup);
    }
    assert_statement_byte_boundaries(&Stmt::ClassDecl(class()), |compiler| {
        compiler.names[0].insert("C".into(), 0);
        compiler.bytecode.bindings.push(Binding {
            name: "C".into(),
            mutable: false,
            strict_immutable: true,
            lexical: true,
            catch_parameter: false,
            eval_var: false,
        });
    });
    assert_statement_byte_boundaries(&Stmt::Return(None), |compiler| compiler.function = true);
    assert_statement_byte_boundaries(&Stmt::Return(Some(Expr::Number(1.0))), |compiler| {
        compiler.function = true;
        compiler.bytecode.async_function = true;
        compiler.bytecode.generator = true;
    });
    assert_statement_byte_boundaries(
        &Stmt::With {
            object: Expr::Object(Vec::new()),
            body: Box::new(var(
                Pattern::Identifier("dynamic".into()),
                Some(Expr::Function(Function::default())),
            )),
        },
        no_setup,
    );
    assert_statement_byte_boundaries(
        &Stmt::With {
            object: Expr::Object(Vec::new()),
            body: Box::new(var(
                Pattern::Object(vec![ObjectPatternProp::KeyValue {
                    key: PropertyKey::Identifier("value".into()),
                    value: Pattern::Identifier("dynamic".into()),
                    default: Some(Expr::Number(1.0)),
                }]),
                Some(Expr::Object(Vec::new())),
            )),
        },
        no_setup,
    );
}

#[test]
fn metadata_limits_bound_handler_and_control_records() {
    let mut compiler = bare_compiler();
    compiler.max_metadata_entries = 0;
    assert_eq!(
        compiler.wrap_with_disposal(false, |_| Ok(())),
        Err(CompileError::ProgramTooLarge)
    );

    let mut compiler = bare_compiler();
    compiler.max_metadata_entries = 0;
    assert_eq!(
        compiler.try_statement(&[], None, Some(&[])),
        Err(CompileError::ProgramTooLarge)
    );

    let mut compiler = bare_compiler();
    compiler.max_metadata_entries = 0;
    compiler.loops.push(Loop {
        labels: Vec::new(),
        breakable: true,
        scope_depth: 0,
        breaks: Vec::new(),
        continues: Some(Vec::new()),
        iterator: None,
    });
    assert_eq!(
        compiler.control_transfer(None, false),
        Err(CompileError::ProgramTooLarge)
    );
}

#[test]
fn internal_declaration_and_cleanup_paths_propagate_byte_limits() {
    let default_function = Stmt::ModuleDefaultFunction {
        function: Function::default(),
        binding: MODULE_DEFAULT_BINDING.into(),
    };
    assert_compiler_byte_boundaries(
        "default function declaration",
        |_| {},
        |compiler| compiler.function_declaration(&default_function),
    );
    let dynamic_function = Stmt::FunctionDecl(Function {
        name: Some("f".into()),
        ..Function::default()
    });
    assert_compiler_byte_boundaries(
        "dynamic function declaration",
        |_| {},
        |compiler| compiler.function_declaration(&dynamic_function),
    );
    let mut name_limit = bare_compiler();
    name_limit.max_metadata_entries = 1;
    name_limit.bytecode.constants.push(Value::Undefined);
    assert_eq!(
        name_limit.function_declaration(&dynamic_function),
        Err(CompileError::ProgramTooLarge)
    );

    let annex_b_setup = |compiler: &mut Compiler| {
        compiler.names[0].insert("f".into(), 0);
        compiler.names.push(HashMap::from([("f".into(), 1)]));
        for lexical in [false, true] {
            compiler.bytecode.bindings.push(Binding {
                name: "f".into(),
                mutable: true,
                strict_immutable: false,
                lexical,
                catch_parameter: false,
                eval_var: false,
            });
        }
    };
    assert_statement_byte_boundaries(&dynamic_function, annex_b_setup);

    let iterator_setup = |compiler: &mut Compiler| {
        compiler.loops.push(Loop {
            labels: Vec::new(),
            breakable: true,
            scope_depth: 0,
            breaks: Vec::new(),
            continues: Some(Vec::new()),
            iterator: Some(0),
        });
    };
    assert_compiler_byte_boundaries("iterator cleanup", iterator_setup, |compiler| {
        compiler.control_transfer(None, false)
    });

    let field = Stmt::ClassField(Box::new(Stmt::Expr(Expr::Assign {
        op: AssignOp::Assign,
        target: Box::new(Expr::Member {
            object: Box::new(Expr::This),
            property: Box::new(Expr::Identifier("field".into())),
            computed: false,
        }),
        value: Box::new(Expr::Number(1.0)),
    })));
    let decorated = Stmt::ClassDecoratedField {
        field: Box::new(field),
        record: "record".into(),
    };
    let record_setup = |compiler: &mut Compiler| {
        compiler.names[0].insert("record".into(), 0);
        compiler.bytecode.bindings.push(Binding {
            name: "record".into(),
            mutable: false,
            strict_immutable: true,
            lexical: true,
            catch_parameter: false,
            eval_var: false,
        });
    };
    assert_statement_byte_boundaries(&decorated, record_setup);
    assert_statement_byte_boundaries(&Stmt::ClassExtraInitializers("record".into()), record_setup);
}

#[test]
fn dynamic_and_with_binding_paths_propagate_limits() {
    let declaration = VarDeclarator {
        pattern: Pattern::Identifier("value".into()),
        init: Some(Expr::Number(1.0)),
    };
    assert_compiler_byte_boundaries(
        "with variable initializer",
        |compiler| compiler.with_depth = 1,
        |compiler| compiler.declarations(DeclKind::Var, std::slice::from_ref(&declaration)),
    );
    assert_compiler_byte_boundaries(
        "with variable binding",
        |compiler| compiler.with_depth = 1,
        |compiler| compiler.bind_pattern(&declaration.pattern, DeclKind::Var),
    );
    assert_compiler_byte_boundaries(
        "dynamic variable binding",
        |_| {},
        |compiler| compiler.bind_pattern(&declaration.pattern, DeclKind::Var),
    );

    let object = Pattern::Object(vec![ObjectPatternProp::KeyValue {
        key: PropertyKey::Identifier("value".into()),
        value: Pattern::Identifier("target".into()),
        default: Some(Expr::Number(1.0)),
    }]);
    assert_compiler_byte_boundaries(
        "object pattern default",
        |_| {},
        |compiler| compiler.bind_pattern(&object, DeclKind::Var),
    );

    for (label, setup, pattern) in [
        (
            "with variable initializer metadata",
            true,
            declaration.pattern.clone(),
        ),
        (
            "dynamic variable metadata",
            false,
            declaration.pattern.clone(),
        ),
        ("with object binding metadata", true, object),
    ] {
        let mut compiler = bare_compiler();
        compiler.max_metadata_entries = 0;
        compiler.with_depth = usize::from(setup);
        assert_eq!(
            compiler.bind_pattern(&pattern, DeclKind::Var),
            Err(CompileError::ProgramTooLarge),
            "{label}"
        );
    }
    let mut compiler = bare_compiler();
    compiler.max_metadata_entries = 0;
    compiler.with_depth = 1;
    assert_eq!(
        compiler.declarations(DeclKind::Var, &[declaration]),
        Err(CompileError::ProgramTooLarge)
    );

    let object = Pattern::Object(vec![ObjectPatternProp::KeyValue {
        key: PropertyKey::Identifier("value".into()),
        value: Pattern::Identifier("target".into()),
        default: None,
    }]);
    let mut compiler = bare_compiler();
    compiler.max_metadata_entries = 1;
    compiler.with_depth = 1;
    assert_eq!(
        compiler.bind_pattern(&object, DeclKind::Var),
        Err(CompileError::ProgramTooLarge)
    );

    bare_compiler().try_statement(&[], None, None).unwrap();
}

#[test]
fn recursive_calls_with_spread_keep_the_ordinary_return_path() {
    let mut compiler = bare_compiler();
    compiler.function = true;
    compiler.bytecode.strict = true;
    compiler.bytecode.self_slot = Some(0);
    compiler.names[0].insert("self".into(), 0);
    compiler.bytecode.bindings.push(Binding {
        name: "self".into(),
        mutable: false,
        strict_immutable: true,
        lexical: true,
        catch_parameter: false,
        eval_var: false,
    });
    let spread_call = Expr::Call {
        callee: Box::new(Expr::Identifier("self".into())),
        args: vec![Argument::Spread(Expr::Array(Vec::new()))],
    };
    assert!(compiler.self_tail_call_args(&spread_call).is_none());
    assert!(!compiler.tail_position_return(&spread_call).unwrap());
    compiler
        .tail_position_return_or_value(&spread_call)
        .unwrap();
    assert!(compiler
        .bytecode
        .instructions()
        .any(|instruction| instruction.opcode == Opcode::Return));
    assert!(!compiler
        .bytecode
        .instructions()
        .any(|instruction| matches!(instruction.opcode, Opcode::TailRecur | Opcode::TailCall)));
}
