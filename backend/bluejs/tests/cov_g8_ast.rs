// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Syntax trees the parser never produces but the compiler still has to
//! reject, and the compiler-internal statements it lowers classes into, built
//! by hand and compiled directly.

mod cov_g8_common;

use blueice_bluejs::{
    compile, compile_with_limit, Argument, AssignOp, CatchClause, CompileError, DeclKind, Expr,
    Function, JsString, Pattern, Program, Stmt, VarDeclarator,
};

fn id(name: &str) -> Expr {
    Expr::Identifier(name.to_string())
}

fn string(text: &str) -> Expr {
    Expr::String(JsString::from(text))
}

fn strict(mut body: Vec<Stmt>) -> Program {
    body.insert(0, Stmt::Expr(string("use strict")));
    Program { body }
}

fn var(name: &str) -> Stmt {
    Stmt::VarDecl(
        DeclKind::Var,
        vec![VarDeclarator {
            pattern: Pattern::Identifier(name.to_string()),
            init: None,
        }],
    )
}

fn error_of(program: &Program) -> Option<CompileError> {
    compile(program).err()
}

fn invalid(message: &'static str) -> Option<CompileError> {
    Some(CompileError::InvalidSyntax(message))
}

fn assign(target: Expr) -> Stmt {
    Stmt::Expr(Expr::Assign {
        op: AssignOp::Assign,
        target: Box::new(target),
        value: Box::new(Expr::Number(1.0)),
    })
}

fn member(object: Expr, property: &str) -> Expr {
    Expr::Member {
        object: Box::new(object),
        property: Box::new(id(property)),
        computed: false,
    }
}

#[test]
fn a_class_field_wrapper_must_hold_an_assignment() {
    let program = Program {
        body: vec![Stmt::ClassField(Box::new(Stmt::Empty))],
    };
    assert_eq!(error_of(&program), invalid("invalid class field AST"));
}

#[test]
fn a_class_field_assignment_must_target_a_member_of_this() {
    for target in [
        id("a"),
        member(id("o"), "p"),
        member(Expr::This, "#missing"),
    ] {
        let program = Program {
            body: vec![Stmt::ClassField(Box::new(assign(target.clone())))],
        };
        assert!(error_of(&program).is_some(), "{target:?}");
    }
}

#[test]
fn with_is_rejected_in_strict_code() {
    let program = strict(vec![Stmt::With {
        object: id("o"),
        body: Box::new(Stmt::Empty),
    }]);
    assert_eq!(
        error_of(&program),
        invalid("with is forbidden in strict mode")
    );
}

#[test]
fn compiler_internal_statements_need_their_hidden_bindings() {
    let missing_record = invalid("decoration record binding is not available in this function");
    let missing_brand = invalid("private brand binding is not available in this function");
    for (statement, expected) in [
        (
            Stmt::ClassDecoratedField {
                field: Box::new(Stmt::ClassField(Box::new(Stmt::Empty))),
                record: "missing".into(),
            },
            missing_record.clone(),
        ),
        (
            Stmt::ClassExtraInitializers("missing".into()),
            missing_record,
        ),
        (Stmt::ClassPrivateBrand("missing".into()), missing_brand),
    ] {
        let program = Program {
            body: vec![statement],
        };
        assert_eq!(error_of(&program), expected);
    }
    let program = Program {
        body: vec![
            var("record"),
            Stmt::ClassDecoratedField {
                field: Box::new(Stmt::Empty),
                record: "record".into(),
            },
        ],
    };
    assert_eq!(error_of(&program), invalid("invalid decorated field AST"));
}

#[test]
fn labels_are_checked_before_their_statement_is_compiled() {
    let labelled = |label: &str, item: Stmt| Stmt::Labelled {
        label: label.to_string(),
        item: Box::new(item),
    };
    let lexical = Stmt::VarDecl(
        DeclKind::Let,
        vec![VarDeclarator {
            pattern: Pattern::Identifier("x".into()),
            init: None,
        }],
    );
    let nested_loop = labelled(
        "a",
        Stmt::While {
            test: Expr::Bool(false),
            body: Box::new(labelled(
                "a",
                Stmt::While {
                    test: Expr::Bool(false),
                    body: Box::new(Stmt::Empty),
                },
            )),
        },
    );
    for (statement, program_is_strict, expected) in [
        (
            labelled("yield", Stmt::Empty),
            true,
            invalid("yield cannot be used as a label in strict code"),
        ),
        (
            labelled("a", labelled("a", Stmt::Empty)),
            false,
            invalid("duplicate label"),
        ),
        (nested_loop, false, invalid("duplicate label")),
        (
            labelled("a", lexical),
            false,
            invalid("a labelled statement cannot contain a lexical declaration"),
        ),
        (
            labelled(
                "a",
                Stmt::ClassDecl(blueice_bluejs::Class {
                    name: Some("C".into()),
                    extends: None,
                    elements: Vec::new(),
                    decorators: Vec::new(),
                    source_text: Default::default(),
                }),
            ),
            false,
            invalid("a labelled statement cannot contain a class declaration"),
        ),
        (
            labelled(
                "a",
                Stmt::FunctionDecl(Function {
                    name: Some("f".into()),
                    ..Function::default()
                }),
            ),
            false,
            invalid("invalid labelled function declaration"),
        ),
    ] {
        let program = if program_is_strict {
            strict(vec![statement])
        } else {
            Program {
                body: vec![statement],
            }
        };
        assert_eq!(error_of(&program), expected);
    }
}

/// A function whose tail call sits inside parentheses, which the parser drops
/// but a hand-built tree may keep.
fn parenthesized_tail_call() -> Program {
    let call = |name: &str| Expr::Call {
        callee: Box::new(id(name)),
        args: vec![Argument::Normal(id("n"))],
    };
    let parenthesized = |expr: Expr| Expr::Parenthesized(Box::new(expr));
    let function = |name: &str, value: Expr| {
        Stmt::FunctionDecl(Function {
            name: Some(name.into()),
            params: vec![blueice_bluejs::Param {
                pattern: Pattern::Identifier("n".into()),
                default: None,
                rest: false,
            }],
            body: vec![Stmt::Return(Some(value))],
            ..Function::default()
        })
    };
    strict(vec![
        function("a", parenthesized(call("g"))),
        function("b", parenthesized(parenthesized(call("b")))),
        function(
            "c",
            Expr::Conditional {
                test: Box::new(id("n")),
                consequent: Box::new(parenthesized(call("g"))),
                alternate: Box::new(call("g")),
            },
        ),
        function("g", id("n")),
    ])
}

#[test]
fn a_parenthesized_tail_call_compiles_under_every_bytecode_limit() {
    let program = parenthesized_tail_call();
    let full = compile(&program).unwrap();
    let mut limit = 0;
    let mut sized = false;
    while limit < 4096 {
        match compile_with_limit(&program, limit) {
            Err(CompileError::ProgramTooLarge) => limit += 1,
            other => {
                assert!(other.is_ok());
                sized = true;
                break;
            }
        }
    }
    assert!(sized && full.bytes().len() > 5, "{limit}");
}

#[test]
fn a_strict_catch_parameter_is_rejected_with_the_restricted_names() {
    for name in ["eval", "arguments"] {
        let program = strict(vec![Stmt::Try {
            block: Vec::new(),
            handler: Some(CatchClause {
                param: Some(Pattern::Identifier(name.to_string())),
                body: Vec::new(),
            }),
            finalizer: None,
        }]);
        assert_eq!(
            error_of(&program),
            invalid("strict code cannot assign to eval or arguments")
        );
    }
}

#[test]
fn a_try_statement_with_neither_clause_compiles_to_its_block() {
    let program = Program {
        body: vec![Stmt::Try {
            block: vec![var("a")],
            handler: None,
            finalizer: None,
        }],
    };
    assert_eq!(error_of(&program), None);
}

/// Parses `source`, then lets `edit` change the tree the parser produced.
fn edited(source: &str, edit: impl FnOnce(&mut Program)) -> Program {
    let mut program = blueice_bluejs::parse(source).unwrap();
    edit(&mut program);
    program
}

#[test]
fn a_decorator_naming_an_undeclared_private_name_is_rejected_on_every_element() {
    let source = "class D { #y = 0; @(this.#y) x = 1; @(this.#y) m() {} @(this.#y) accessor a; }";
    // Without the declaration of `#y` the decorators can no longer resolve it.
    let program = edited(source, |program| {
        let Stmt::ClassDecl(class) = &mut program.body[0] else {
            panic!("a class declaration");
        };
        class.elements.retain(|element| {
            !matches!(element, blueice_bluejs::ClassElement::Field {
                key: blueice_bluejs::PropertyKey::Identifier(name), ..
            } if name == "#y")
        });
    });
    assert_eq!(
        error_of(&program),
        invalid("private name is not declared in an enclosing class")
    );
    // One element at a time, so each element kind's decorators are reached.
    for element in [
        "@(this.#y) x = 1;",
        "@(this.#y) m() {}",
        "@(this.#y) accessor a;",
        "@(this.#y) static s = 1;",
    ] {
        let source = format!("class D {{ #y = 0; {element} }}");
        let program = edited(&source, |program| {
            let Stmt::ClassDecl(class) = &mut program.body[0] else {
                panic!("a class declaration");
            };
            class.elements.remove(0);
        });
        assert_eq!(
            error_of(&program),
            invalid("private name is not declared in an enclosing class"),
            "{source}"
        );
    }
}

#[test]
fn a_parenthesized_anonymous_function_is_still_named_by_its_binding() {
    let program = edited("var f = function () {}; f.name", |program| {
        let Stmt::VarDecl(_, declarations) = &mut program.body[0] else {
            panic!("a variable declaration");
        };
        let initializer = declarations[0].init.take().unwrap();
        declarations[0].init = Some(Expr::Parenthesized(Box::new(initializer)));
    });
    let code = compile(&program).unwrap();
    assert_eq!(
        blueice_bluejs::Vm::default().execute(&code),
        Ok(blueice_bluejs::Value::String("f".into()))
    );
}
