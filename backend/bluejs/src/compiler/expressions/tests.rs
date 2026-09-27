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

#[test]
fn assignment_references_and_disposable_iteration_compile_through_the_public_pipeline() {
    for source in [
        "let target = {}; [target.x] = [1]; ({ x: target.y } = { x: 2 }); target.x + target.y;",
        "class A { set x(value) {} } class B extends A { m() { super.x ||= 4; [super.x] = [5]; } }",
        "class C { #x; m() { [this.#x] = [1]; ({ x: this.#x } = { x: 2 }); } }",
        "let slot; slot ||= 3; slot &&= 4; slot ??= 5;",
        "for (using resource of [null]) { resource; }",
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
fn tagged_template_site_identity_survives_clone_and_distinguishes_compilations() {
    let raw = vec![JsString::from("raw")];
    let cooked = vec![Some(JsString::from("raw"))];
    let mut first = bare_compiler();
    first
        .tagged_template_expression(&Expr::Number(1.0), &raw, &cooked, &[])
        .unwrap();
    let cloned = first.bytecode.clone();
    assert!(first.bytecode.templates[0].id == cloned.templates[0].id);
    let mut second = bare_compiler();
    second
        .tagged_template_expression(&Expr::Number(1.0), &raw, &cooked, &[])
        .unwrap();
    assert!(first.bytecode.templates[0].id != second.bytecode.templates[0].id);
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
    assert!(optional_member_parts(&Expr::Number(1.0)).is_none());
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
        bare_compiler().optional_chain_member_reference(
            optional_member_parts(&optional_private).unwrap(),
            &mut Vec::new(),
        ),
        Err(CompileError::InvalidSyntax(message)) if message == expected
    ));
    assert!(matches!(
        bare_compiler().parenthesized_optional_member_method(&optional_private),
        Err(CompileError::InvalidSyntax(message)) if message == expected
    ));
}

#[test]
fn direct_plain_expression_helpers_compile_optional_ast_shapes() {
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
        let mut plain = bare_compiler();
        plain.expression_plain(&expression).unwrap();
        let mut ordinary = bare_compiler();
        ordinary.expression(&expression).unwrap();
        assert_eq!(plain.bytecode.bytes(), ordinary.bytecode.bytes());
    }
}
