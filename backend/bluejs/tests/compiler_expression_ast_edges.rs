// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The public AST can be constructed without the parser; compilation must
//! reject malformed expression shapes at the relevant boundary.

use blueice_bluejs::{
    compile, compile_module, compile_with_limit, parse, parse_module, Bytecode, Class,
    ClassElement, CompileError, Expr, Function, Program, PropertyKey, Stmt, UnaryOp, Value, Vm,
};

fn expression_program(expr: Expr) -> Program {
    Program {
        body: vec![Stmt::Expr(expr)],
    }
}

fn total_compiled_bytes(code: &Bytecode) -> usize {
    code.bytes().len()
        + code
            .child_code_units()
            .map(total_compiled_bytes)
            .sum::<usize>()
}

fn all_code_bytes(code: &Bytecode) -> Vec<Vec<u8>> {
    let mut units = vec![code.bytes().to_vec()];
    for child in code.child_code_units() {
        units.extend(all_code_bytes(child));
    }
    units
}

fn wrap_method_call_callee(class: &mut Class, method: &str) {
    let function = class
        .elements
        .iter_mut()
        .find_map(|element| match element {
            ClassElement::Method {
                key: PropertyKey::Identifier(name),
                function,
                ..
            } if name == method => Some(function),
            _ => None,
        })
        .expect("expected method");
    let Stmt::Return(Some(Expr::Call { callee, .. })) = &mut function.body[0] else {
        panic!("expected method call return");
    };
    **callee = Expr::Parenthesized(callee.clone());
}

fn assert_every_byte_limit(program: &Program) {
    let full = compile(program).unwrap();
    let expected = all_code_bytes(&full);
    let upper = u32::try_from(total_compiled_bytes(&full)).unwrap();
    let mut first_success = None;
    for limit in 0..=upper {
        match compile_with_limit(program, limit) {
            Ok(code) => {
                first_success.get_or_insert(limit);
                assert_eq!(all_code_bytes(&code), expected, "at {limit} bytes");
            }
            Err(CompileError::ProgramTooLarge) => {
                assert!(first_success.is_none(), "at {limit} bytes");
            }
            Err(error) => panic!("unexpected error at {limit} bytes: {error:?}"),
        }
    }
    assert!(first_success.is_some());
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

#[test]
fn parenthesized_super_and_private_method_references_keep_their_receiver() {
    let mut super_program = parse(
        "class A { m() { return this.x; } } \
         class B extends A { x = 7; f() { return super.m(); } } \
         new B().f();",
    )
    .unwrap();
    let Stmt::ClassDecl(class) = &mut super_program.body[1] else {
        panic!("expected derived class");
    };
    wrap_method_call_callee(class, "f");
    assert_eq!(
        Vm::default().execute(&compile(&super_program).unwrap()),
        Ok(Value::Number(7.0))
    );
    assert_every_byte_limit(&super_program);

    let mut private_program = parse(
        "class C { #m() { return this.x; } x = 8; f() { return this.#m(); } } \
         new C().f();",
    )
    .unwrap();
    let Stmt::ClassDecl(class) = &mut private_program.body[0] else {
        panic!("expected class");
    };
    wrap_method_call_callee(class, "f");
    assert_eq!(
        Vm::default().execute(&compile(&private_program).unwrap()),
        Ok(Value::Number(8.0))
    );
    assert_every_byte_limit(&private_program);
}

#[test]
fn parenthesized_ordinary_member_in_an_optional_call_keeps_its_receiver() {
    let mut program = parse(
        "let object = { value: 9, method() { return this.value; } }; \
         object.method?.();",
    )
    .unwrap();
    let Stmt::Expr(Expr::OptionalCall { callee, .. }) = &mut program.body[1] else {
        panic!("expected optional call");
    };
    **callee = Expr::Parenthesized(callee.clone());
    assert_eq!(
        Vm::default().execute(&compile(&program).unwrap()),
        Ok(Value::Number(9.0))
    );
    assert_every_byte_limit(&program);
}
