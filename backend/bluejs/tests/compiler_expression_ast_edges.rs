// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The public AST can be constructed without the parser; compilation must
//! reject malformed expression shapes at the relevant boundary.

use blueice_bluejs::{
    compile, compile_module, parse, parse_module, ClassElement, CompileError, Expr, Function,
    Program, Stmt, UnaryOp,
};

fn expression_program(expr: Expr) -> Program {
    Program {
        body: vec![Stmt::Expr(expr)],
    }
}

#[test]
fn import_meta_is_module_only_even_for_an_externally_built_ast() {
    assert!(matches!(
        compile(&expression_program(Expr::ImportMeta)),
        Err(CompileError::InvalidSyntax(
            "import.meta is only valid in module code"
        ))
    ));
    assert!(compile_module(&parse_module("import.meta;").unwrap()).is_ok());
}

#[test]
fn malformed_optional_member_keys_fail_during_compilation() {
    let optional = Expr::OptionalMember {
        object: Box::new(Expr::Number(1.0)),
        property: Box::new(Expr::Number(2.0)),
        computed: false,
    };
    for expr in [
        optional.clone(),
        Expr::Call {
            callee: Box::new(Expr::Parenthesized(Box::new(optional))),
            args: Vec::new(),
        },
    ] {
        assert!(matches!(
            compile(&expression_program(expr)),
            Err(CompileError::InvalidSyntax(
                "invalid non-computed optional-chain member AST"
            ))
        ));
    }
}

#[test]
fn generator_delegation_requires_an_operand_in_an_external_ast() {
    let function = Function {
        name: Some("g".into()),
        generator: true,
        body: vec![Stmt::Expr(Expr::Yield {
            value: None,
            delegate: true,
        })],
        ..Function::default()
    };
    assert!(matches!(
        compile(&Program {
            body: vec![Stmt::FunctionDecl(function)],
        }),
        Err(CompileError::InvalidSyntax("yield* requires an operand"))
    ));
}

#[test]
fn super_call_requires_a_derived_constructor_even_in_an_external_ast() {
    assert!(matches!(
        compile(&expression_program(Expr::Call {
            callee: Box::new(Expr::Super),
            args: Vec::new(),
        })),
        Err(CompileError::InvalidSyntax(
            "super() is only valid in a derived constructor"
        ))
    ));
}

#[test]
fn deletion_of_a_private_member_after_an_optional_suffix_is_rejected() {
    let mut program = parse("class C { #x; m() { return this?.next.#x; } }").unwrap();
    let Stmt::ClassDecl(class) = &mut program.body[0] else {
        panic!("expected class declaration");
    };
    let ClassElement::Method { function, .. } = &mut class.elements[1] else {
        panic!("expected method");
    };
    let Stmt::Return(Some(value)) = &mut function.body[0] else {
        panic!("expected return expression");
    };
    *value = Expr::Unary {
        op: UnaryOp::Delete,
        arg: Box::new(value.clone()),
    };
    assert!(matches!(
        compile(&program),
        Err(CompileError::InvalidSyntax(
            "cannot delete a private element"
        ))
    ));
}
