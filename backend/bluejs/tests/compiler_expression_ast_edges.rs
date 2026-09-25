// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The public AST can be constructed without the parser; compilation must
//! reject malformed expression shapes at the relevant boundary.

use blueice_bluejs::{
    compile, compile_module, compile_with_limit, parse, parse_module, Argument, ArrowBody,
    AssignOp, Bytecode, Class, ClassElement, CompileError, Expr, ForHead, Function, Pattern,
    Program, PropertyKey, RuntimeError, SourceText, Stmt, UnaryOp, UpdateOp, Value, Vm,
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
fn malformed_ordinary_and_unbound_private_members_fail_during_compilation() {
    for (expr, expected) in [
        (
            Expr::Member {
                object: Box::new(Expr::Number(1.0)),
                property: Box::new(Expr::Number(2.0)),
                computed: false,
            },
            "invalid non-computed member AST",
        ),
        (
            Expr::Member {
                object: Box::new(Expr::Number(1.0)),
                property: Box::new(Expr::Identifier("#missing".into())),
                computed: false,
            },
            "private name is not declared in an enclosing class",
        ),
    ] {
        assert!(matches!(
            compile(&expression_program(expr)),
            Err(CompileError::InvalidSyntax(message)) if message == expected
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

#[test]
fn parenthesized_nonmember_calls_use_the_ordinary_callee_value() {
    for expression in [
        Expr::Call {
            callee: Box::new(Expr::Parenthesized(Box::new(Expr::Identifier("f".into())))),
            args: Vec::new(),
        },
        Expr::OptionalCall {
            callee: Box::new(Expr::Parenthesized(Box::new(Expr::Identifier("f".into())))),
            args: Vec::new(),
        },
    ] {
        let mut program = parse("let f = () => 13;").unwrap();
        program.body.push(Stmt::Expr(expression));
        assert_eq!(
            Vm::default().execute(&compile(&program).unwrap()),
            Ok(Value::Number(13.0))
        );
        assert_every_byte_limit(&program);
    }
}

#[test]
fn parenthesized_optional_computed_and_private_methods_keep_their_receivers() {
    for (source, expected) in [
        (
            "let object = { value: 11, method() { return this.value; } }; \
             (object?.['method'])();",
            11.0,
        ),
        (
            "class C { #method() { return this.value; } value = 12; \
             run() { return (this?.#method)(); } } new C().run();",
            12.0,
        ),
    ] {
        let program = parse(source).unwrap();
        assert_eq!(
            Vm::default().execute(&compile(&program).unwrap()),
            Ok(Value::Number(expected)),
            "{source}"
        );
        assert_every_byte_limit(&program);
    }
}

#[test]
fn externally_built_call_targets_obey_strict_assignment_rules() {
    let call = Expr::Call {
        callee: Box::new(Expr::Identifier("f".into())),
        args: Vec::new(),
    };
    for statement in [
        Stmt::Expr(Expr::Update {
            op: UpdateOp::Inc,
            arg: Box::new(call.clone()),
            prefix: false,
        }),
        Stmt::Expr(Expr::Assign {
            op: AssignOp::Assign,
            target: Box::new(call.clone()),
            value: Box::new(Expr::Number(1.0)),
        }),
        Stmt::ForIn {
            left: ForHead::Expr(call),
            right: Expr::Object(Vec::new()),
            body: Box::new(Stmt::Empty),
        },
    ] {
        let mut program = parse("'use strict';").unwrap();
        program.body.push(statement);
        assert!(matches!(
            compile(&program),
            Err(CompileError::InvalidSyntax(
                "a CallExpression cannot be an assignment target in strict code"
            ))
        ));
    }
}

#[test]
fn a_sloppy_call_update_evaluates_the_call_before_rejecting_its_target() {
    let mut program = parse("function f() { throw 42; } f();").unwrap();
    let Stmt::Expr(call @ Expr::Call { .. }) = &mut program.body[1] else {
        panic!("expected call expression");
    };
    *call = Expr::Update {
        op: UpdateOp::Inc,
        arg: Box::new(call.clone()),
        prefix: false,
    };
    let code = compile(&program).unwrap();
    assert_eq!(
        Vm::default().execute(&code),
        Err(RuntimeError::Thrown(Value::Number(42.0)))
    );
    assert_every_byte_limit(&program);
}

#[test]
fn invalid_nested_super_expressions_are_rejected_at_their_original_site() {
    let malformed_key = Expr::OptionalMember {
        object: Box::new(Expr::Number(1.0)),
        property: Box::new(Expr::Super),
        computed: true,
    };
    for expr in [
        Expr::Parenthesized(Box::new(Expr::Super)),
        malformed_key.clone(),
        Expr::Arrow {
            params: Vec::new(),
            body: ArrowBody::Expr(Box::new(Expr::Super)),
            is_async: false,
            source_text: SourceText::default(),
        },
        Expr::Call {
            callee: Box::new(Expr::Parenthesized(Box::new(malformed_key))),
            args: Vec::new(),
        },
    ] {
        assert!(matches!(
            compile(&expression_program(expr)),
            Err(CompileError::InvalidSyntax(
                "super must be used as a property access or constructor call"
            ))
        ));
    }

    let mut program =
        parse("class A {} class B extends A { method() { return super.x; } }").unwrap();
    let Stmt::ClassDecl(class) = &mut program.body[1] else {
        panic!("expected derived class");
    };
    let ClassElement::Method { function, .. } = &mut class.elements[0] else {
        panic!("expected method");
    };
    let Stmt::Return(Some(Expr::Member {
        property, computed, ..
    })) = &mut function.body[0]
    else {
        panic!("expected super member return");
    };
    **property = Expr::Super;
    *computed = true;
    assert!(matches!(
        compile(&program),
        Err(CompileError::InvalidSyntax(
            "super must be used as a property access or constructor call"
        ))
    ));

    for (arg, expected) in [
        (
            Expr::Super,
            "super must be used as a property access or constructor call",
        ),
        (Expr::ImportMeta, "import.meta is only valid in module code"),
    ] {
        for argument in [Argument::Spread(arg.clone()), Argument::Normal(arg)] {
            let mut program =
                parse("class A {} class B extends A { constructor() { super(...[]); } }").unwrap();
            let Stmt::ClassDecl(class) = &mut program.body[1] else {
                panic!("expected derived class");
            };
            let ClassElement::Method { function, .. } = &mut class.elements[0] else {
                panic!("expected constructor");
            };
            let Stmt::Expr(Expr::Call { args, .. }) = &mut function.body[0] else {
                panic!("expected super call");
            };
            args[0] = argument;
            assert!(matches!(
                compile(&program),
                Err(CompileError::InvalidSyntax(message)) if message == expected
            ));
        }
    }
}

#[test]
fn annex_b_for_in_initializer_requires_an_identifier_in_external_ast() {
    let program = Program {
        body: vec![Stmt::ForIn {
            left: ForHead::AnnexBVarInit(Pattern::Array(Vec::new()), Expr::Number(1.0)),
            right: Expr::Object(Vec::new()),
            body: Box::new(Stmt::Empty),
        }],
    };
    assert!(matches!(
        compile(&program),
        Err(CompileError::InvalidSyntax(
            "Annex B for-in initializer requires an identifier"
        ))
    ));
}
