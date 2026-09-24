// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Hand-built syntax trees for shapes the parser never produces (or rejects
//! earlier), so the compiler's own early errors are checked through the
//! public `compile` entry point: undeclared private names, private deletes,
//! `import.meta` outside modules, call expressions as assignment targets in
//! strict code, and malformed optional chains.

use blueice_bluejs::{
    compile, compile_with_limit, parse, AssignOp, AssignmentPattern, AssignmentPatternElement,
    CompileError, Expr, ForHead, Pattern, Program, Stmt, UnaryOp, UpdateOp,
};

const UNDECLARED: CompileError =
    CompileError::InvalidSyntax("private name is not declared in an enclosing class");

fn id(name: &str) -> Expr {
    Expr::Identifier(name.into())
}

fn boxed(expr: Expr) -> Box<Expr> {
    Box::new(expr)
}

/// `object.#name`.
fn private(object: Expr, name: &str) -> Expr {
    Expr::Member {
        object: boxed(object),
        property: boxed(id(&format!("#{name}"))),
        computed: false,
    }
}

/// `object?.#name`.
fn optional_private(object: Expr, name: &str) -> Expr {
    Expr::OptionalMember {
        object: boxed(object),
        property: boxed(id(&format!("#{name}"))),
        computed: false,
    }
}

fn expression_error(expr: Expr) -> Option<CompileError> {
    compile(&Program {
        body: vec![Stmt::Expr(expr)],
    })
    .err()
}

fn strict_error(statement: Stmt) -> Option<CompileError> {
    compile(&Program {
        body: vec![Stmt::Expr(Expr::String("use strict".into())), statement],
    })
    .err()
}

fn call(callee: Expr) -> Expr {
    Expr::Call {
        callee: boxed(callee),
        args: Vec::new(),
    }
}

#[test]
fn undeclared_private_names_are_rejected_wherever_they_are_read_or_written() {
    let this = || Expr::This;
    let cases = [
        // `#p in o`
        Expr::PrivateIn {
            name: "p".into(),
            object: boxed(this()),
        },
        // `this.#p`, `this.#p()`, `this.#p++`, `this.#p = 1`, `this.#p ||= 1`
        private(this(), "p"),
        call(private(this(), "p")),
        Expr::Update {
            op: UpdateOp::Inc,
            arg: boxed(private(this(), "p")),
            prefix: false,
        },
        Expr::Assign {
            op: AssignOp::Assign,
            target: boxed(private(this(), "p")),
            value: boxed(Expr::Number(1.0)),
        },
        Expr::Assign {
            op: AssignOp::LogicalOrAssign,
            target: boxed(private(this(), "p")),
            value: boxed(Expr::Number(1.0)),
        },
        // this.#p`t`
        Expr::TaggedTemplate {
            tag: boxed(private(this(), "p")),
            raw: vec!["t".into()],
            cooked: vec![Some("t".into())],
            expressions: Vec::new(),
        },
        // `a?.#p`, `a?.b.#p`, `a?.b.#p()`, `a?.#p()`, `(a?.#p)()`
        optional_private(id("a"), "p"),
        private(
            Expr::OptionalMember {
                object: boxed(id("a")),
                property: boxed(id("b")),
                computed: false,
            },
            "p",
        ),
        call(private(
            Expr::OptionalMember {
                object: boxed(id("a")),
                property: boxed(id("b")),
                computed: false,
            },
            "p",
        )),
        Expr::OptionalCall {
            callee: boxed(optional_private(id("a"), "p")),
            args: Vec::new(),
        },
        call(Expr::Parenthesized(boxed(optional_private(id("a"), "p")))),
        // `[this.#p] = []` and `({ a: this.#p } = {})`
        Expr::DestructureAssign {
            pattern: AssignmentPattern::Array(vec![Some(AssignmentPatternElement {
                pattern: AssignmentPattern::Target(boxed(private(this(), "p"))),
                default: None,
                rest: false,
            })]),
            value: boxed(Expr::Array(Vec::new())),
        },
        Expr::DestructureAssign {
            pattern: AssignmentPattern::Array(vec![Some(AssignmentPatternElement {
                pattern: AssignmentPattern::Target(boxed(private(this(), "p"))),
                default: None,
                rest: true,
            })]),
            value: boxed(Expr::Array(Vec::new())),
        },
    ];
    for (index, case) in cases.into_iter().enumerate() {
        assert_eq!(expression_error(case), Some(UNDECLARED), "case {index}");
    }
    // for (this.#p of []);
    assert_eq!(
        compile(&Program {
            body: vec![Stmt::ForOf {
                left: ForHead::Assignment(AssignmentPattern::Target(boxed(private(
                    Expr::This,
                    "p"
                )))),
                right: Expr::Array(Vec::new()),
                body: Box::new(Stmt::Empty),
                is_await: false,
            }],
        })
        .err(),
        Some(UNDECLARED)
    );
}

#[test]
fn a_private_element_cannot_be_deleted_through_an_optional_chain() {
    let chained = private(
        Expr::OptionalMember {
            object: boxed(id("a")),
            property: boxed(id("b")),
            computed: false,
        },
        "p",
    );
    assert_eq!(
        expression_error(Expr::Unary {
            op: UnaryOp::Delete,
            arg: boxed(chained),
        }),
        Some(CompileError::InvalidSyntax(
            "cannot delete a private element"
        ))
    );
}

#[test]
fn import_meta_is_rejected_in_scripts() {
    assert_eq!(
        expression_error(Expr::ImportMeta),
        Some(CompileError::InvalidSyntax(
            "import.meta is only valid in module code"
        ))
    );
}

#[test]
fn yield_star_without_an_operand_is_rejected() {
    let mut program = parse("function* g(){ yield* a }").unwrap();
    let Stmt::FunctionDecl(function) = &mut program.body[0] else {
        panic!("a function declaration was parsed");
    };
    let Stmt::Expr(Expr::Yield { value, .. }) = &mut function.body[0] else {
        panic!("a yield expression statement was parsed");
    };
    *value = None;
    assert_eq!(
        compile(&program).err(),
        Some(CompileError::InvalidSyntax("yield* requires an operand"))
    );
}

#[test]
fn call_expressions_are_not_strict_assignment_targets() {
    let message = CompileError::InvalidSyntax(
        "a CallExpression cannot be an assignment target in strict code",
    );
    let target = || call(id("f"));
    assert_eq!(
        strict_error(Stmt::Expr(Expr::Assign {
            op: AssignOp::Assign,
            target: boxed(target()),
            value: boxed(Expr::Number(1.0)),
        })),
        Some(message.clone())
    );
    assert_eq!(
        strict_error(Stmt::Expr(Expr::Update {
            op: UpdateOp::Inc,
            arg: boxed(target()),
            prefix: true,
        })),
        Some(message.clone())
    );
    assert_eq!(
        strict_error(Stmt::ForOf {
            left: ForHead::Expr(target()),
            right: Expr::Array(Vec::new()),
            body: Box::new(Stmt::Empty),
            is_await: false,
        }),
        Some(message)
    );
}

#[test]
fn for_in_declaration_initializers_are_sloppy_var_only() {
    let head = || ForHead::AnnexBVarInit(Pattern::Identifier("a".into()), Expr::Number(1.0));
    let for_in = |left| Stmt::ForIn {
        left,
        right: id("b"),
        body: Box::new(Stmt::Empty),
    };
    let message = CompileError::InvalidSyntax(
        "a for-in declaration initializer is valid only in sloppy var code",
    );
    assert_eq!(strict_error(for_in(head())), Some(message.clone()));
    assert_eq!(
        compile(&Program {
            body: vec![Stmt::ForOf {
                left: head(),
                right: id("b"),
                body: Box::new(Stmt::Empty),
                is_await: false,
            }],
        })
        .err(),
        Some(message)
    );
    // A destructuring pattern initializer has no name to give a function.
    assert!(compile(&Program {
        body: vec![for_in(ForHead::AnnexBVarInit(
            Pattern::Array(Vec::new()),
            Expr::Number(1.0)
        ))],
    })
    .is_ok());
}

#[test]
fn malformed_optional_members_are_rejected() {
    let malformed = Expr::OptionalMember {
        object: boxed(id("a")),
        property: boxed(Expr::Number(1.0)),
        computed: false,
    };
    let message = CompileError::InvalidSyntax("invalid non-computed optional-chain member AST");
    assert_eq!(expression_error(malformed.clone()), Some(message.clone()));
    assert_eq!(
        expression_error(call(Expr::Parenthesized(boxed(malformed)))),
        Some(message)
    );
}

#[test]
fn a_parenthesized_member_callee_reports_the_budget_at_every_instruction() {
    // `(a.b)()`: the parser drops the parentheses of a plain member, but a
    // tree may keep them and the callee must still be read as a method.
    let program = Program {
        body: vec![Stmt::Expr(call(Expr::Parenthesized(boxed(Expr::Member {
            object: boxed(id("a")),
            property: boxed(id("b")),
            computed: false,
        }))))],
    };
    let full = compile(&program).unwrap();
    for limit in 0..u32::try_from(full.bytes().len()).unwrap() {
        assert_eq!(
            compile_with_limit(&program, limit).err(),
            Some(CompileError::ProgramTooLarge),
            "limit {limit}"
        );
    }
    assert!(compile_with_limit(&program, u32::MAX).is_ok());
}

#[test]
fn super_calls_need_a_derived_constructor() {
    assert_eq!(
        expression_error(call(Expr::Super)),
        Some(CompileError::InvalidSyntax(
            "super() is only valid in a derived constructor"
        ))
    );
}

#[test]
fn a_parenthesized_callee_that_is_not_a_member_is_an_ordinary_callee() {
    let parenthesized = || Expr::Parenthesized(boxed(id("f")));
    assert!(compile(&Program {
        body: vec![Stmt::Expr(call(parenthesized()))],
    })
    .is_ok());
    assert!(compile(&Program {
        body: vec![Stmt::Expr(Expr::OptionalCall {
            callee: boxed(parenthesized()),
            args: Vec::new(),
        })],
    })
    .is_ok());
}

#[test]
fn a_parenthesized_ordinary_callee_reports_the_budget_at_every_instruction() {
    let parenthesized = || Expr::Parenthesized(boxed(id("f")));
    for callee_call in [
        call(parenthesized()),
        Expr::OptionalCall {
            callee: boxed(parenthesized()),
            args: Vec::new(),
        },
    ] {
        let program = Program {
            body: vec![Stmt::Expr(callee_call)],
        };
        let full = compile(&program).unwrap();
        for limit in 0..u32::try_from(full.bytes().len()).unwrap() {
            assert_eq!(
                compile_with_limit(&program, limit).err(),
                Some(CompileError::ProgramTooLarge),
                "limit {limit}"
            );
        }
        assert!(compile_with_limit(&program, u32::MAX).is_ok());
    }
}

#[test]
fn a_parenthesized_super_member_callee_reports_the_budget_at_every_instruction() {
    let program = Program {
        body: vec![Stmt::Expr(call(Expr::Parenthesized(boxed(Expr::Member {
            object: boxed(Expr::Super),
            property: boxed(id("b")),
            computed: false,
        }))))],
    };
    let mut settled = false;
    for limit in 0..1 << 12 {
        match compile_with_limit(&program, limit) {
            Err(CompileError::ProgramTooLarge) => {}
            other => {
                assert_eq!(other.err(), None);
                settled = true;
                break;
            }
        }
    }
    assert!(settled);
}
