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
fn optional_spread_calls_and_lexical_iteration_heads_compile() {
    for source in [
        "const object = { method(value) { return value; } }; object?.method(...[1]);",
        "for (let value of [1, 2]) { value; }",
        "for (const key in { first: 1 }) { key; }",
    ] {
        let program = crate::parse(source).unwrap();
        crate::compile(&program).unwrap();
    }
}

fn missing_private_member() -> Expr {
    Expr::Member {
        object: Box::new(Expr::Number(1.0)),
        property: Box::new(Expr::Identifier("#missing".into())),
        computed: false,
    }
}

#[test]
fn template_site_ids_stop_before_wrapping() {
    let counter = std::sync::atomic::AtomicU64::new(u64::MAX - 1);
    assert!(matches!(next_template_site_id(&counter), Ok(id) if id == u64::MAX - 1));
    assert!(matches!(
        next_template_site_id(&counter),
        Err(CompileError::ProgramTooLarge)
    ));
    assert_eq!(counter.load(std::sync::atomic::Ordering::Relaxed), u64::MAX);
}

#[test]
fn tagged_template_compilation_propagates_site_id_exhaustion() {
    let counter = std::sync::atomic::AtomicU64::new(u64::MAX - 1);
    let mut compiler = bare_compiler();
    let raw = vec![JsString::from("raw")];
    let cooked = vec![Some(JsString::from("raw"))];
    compiler
        .tagged_template_expression(&Expr::Number(1.0), &raw, &cooked, &[], &counter)
        .unwrap();
    assert_eq!(compiler.bytecode.templates.len(), 1);
    assert_eq!(compiler.bytecode.templates[0].id, u64::MAX - 1);

    let mut exhausted = bare_compiler();
    assert!(matches!(
        exhausted.tagged_template_expression(&Expr::Number(1.0), &raw, &cooked, &[], &counter),
        Err(CompileError::ProgramTooLarge)
    ));
    assert!(exhausted.bytecode.templates.is_empty());
    assert_eq!(counter.load(std::sync::atomic::Ordering::Relaxed), u64::MAX);
}

#[test]
fn private_update_operands_reject_unrepresentable_owners() {
    assert!(matches!(
        private_update_operand(u32::MAX / 4, UpdateOp::Dec, true),
        Ok(u32::MAX)
    ));
    assert!(matches!(
        private_update_operand(u32::MAX / 4 + 1, UpdateOp::Inc, false),
        Err(CompileError::ProgramTooLarge)
    ));
}

#[test]
fn private_update_rejects_an_owner_slot_that_cannot_fit_its_operand() {
    let mut compiler = bare_compiler();
    compiler.names[0].insert("owner".into(), u32::MAX / 4 + 1);
    compiler
        .private_scopes
        .push(HashMap::from([("missing".into(), "owner".into())]));
    let expression = Expr::Update {
        op: UpdateOp::Inc,
        arg: Box::new(missing_private_member()),
        prefix: false,
    };
    assert!(matches!(
        compiler.expression(&expression),
        Err(CompileError::ProgramTooLarge)
    ));
}

#[test]
fn internal_optional_chain_fallback_matches_ordinary_expression_lowering() {
    let expression = Expr::Number(7.0);
    let mut expected = bare_compiler();
    expected.expression(&expression).unwrap();
    let mut actual = bare_compiler();
    actual
        .optional_chain_expression(&expression, &mut Vec::new())
        .unwrap();
    assert_eq!(actual.bytecode.bytes(), expected.bytecode.bytes());
    assert!(matches!(
        bare_compiler().optional_chain_expression(&Expr::Super, &mut Vec::new()),
        Err(CompileError::InvalidSyntax(
            "super must be used as a property access or constructor call"
        ))
    ));
}

#[test]
fn an_unbound_update_rejects_an_exhausted_metadata_table() {
    let mut compiler = bare_compiler();
    compiler.max_metadata_entries = 0;
    let expression = Expr::Update {
        op: UpdateOp::Inc,
        arg: Box::new(Expr::Identifier("missing".into())),
        prefix: false,
    };
    assert!(matches!(
        compiler.expression(&expression),
        Err(CompileError::ProgramTooLarge)
    ));
}

#[test]
fn super_spread_arguments_preserve_their_compilation_error() {
    let mut compiler = bare_compiler();
    compiler.names[0].insert(DERIVED_CONSTRUCTOR_BINDING.into(), 0);
    let expression = Expr::Call {
        callee: Box::new(Expr::Super),
        args: vec![Argument::Spread(Expr::ImportMeta)],
    };
    assert!(matches!(
        compiler.expression(&expression),
        Err(CompileError::InvalidSyntax(
            "import.meta is only valid in module code"
        ))
    ));
}

#[test]
fn internal_member_helpers_reject_invalid_shapes_and_unresolved_private_names() {
    assert!(matches!(
        bare_compiler().optional_chain_member_reference(&Expr::Number(1.0), &mut Vec::new()),
        Err(CompileError::InvalidSyntax(
            "invalid optional-chain member AST"
        ))
    ));
    assert!(matches!(
        bare_compiler().parenthesized_optional_member_method(&Expr::Number(1.0)),
        Err(CompileError::InvalidSyntax(
            "invalid parenthesized optional member AST"
        ))
    ));
    assert!(matches!(
        bare_compiler().member_reference_uncoerced(&Expr::Number(1.0)),
        Err(CompileError::InvalidSyntax("invalid assignment/member AST"))
    ));

    let private = missing_private_member();
    let expected = "private name is not declared in an enclosing class";
    assert!(matches!(
        bare_compiler().private_member_chain_reference(&Expr::Number(1.0), "missing", &mut Vec::new()),
        Err(CompileError::InvalidSyntax(message)) if message == expected
    ));
    assert!(matches!(
        bare_compiler().assign_pattern_target(&private),
        Err(CompileError::InvalidSyntax(message)) if message == expected
    ));
    assert!(matches!(
        bare_compiler().member_reference_uncoerced(&private),
        Err(CompileError::InvalidSyntax(message)) if message == expected
    ));

    let optional_private = Expr::OptionalMember {
        object: Box::new(Expr::Number(1.0)),
        property: Box::new(Expr::Identifier("#missing".into())),
        computed: false,
    };
    assert!(matches!(
        bare_compiler().optional_chain_member_reference(&optional_private, &mut Vec::new()),
        Err(CompileError::InvalidSyntax(message)) if message == expected
    ));
    assert!(matches!(
        bare_compiler().parenthesized_optional_member_method(&optional_private),
        Err(CompileError::InvalidSyntax(message)) if message == expected
    ));
}

#[test]
fn direct_plain_expression_helpers_reject_optional_ast_shapes() {
    let optional_member = Expr::OptionalMember {
        object: Box::new(Expr::Number(1.0)),
        property: Box::new(Expr::Identifier("name".into())),
        computed: false,
    };
    let optional_call = Expr::OptionalCall {
        callee: Box::new(Expr::Number(1.0)),
        args: Vec::new(),
    };
    for expression in [optional_member, optional_call] {
        assert!(matches!(
            bare_compiler().expression_plain(&expression),
            Err(CompileError::Unsupported("optional chaining"))
        ));
    }
}
