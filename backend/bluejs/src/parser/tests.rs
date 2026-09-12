// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn regexp_lexical_goals_are_visible_in_the_public_ast() {
    use crate::{parse, Expr, Stmt};
    let program = parse("/a/g").unwrap();
    assert!(
        matches!(&program.body[0],Stmt::Expr(Expr::RegExp {pattern,flags}) if pattern == "a" && flags == "g")
    );
    assert!(parse("delete object.x").is_ok());
    for source in ["/(/", "/a\n/", "String.raw`unterminated", "'\\u{110000}'"] {
        assert!(parse(source).is_err(), "{source}");
    }
}

#[test]
fn advance_stops_at_the_terminal_token() {
    let mut parser = Parser::new("value");
    assert_eq!(parser.advance(), Token::Identifier("value".into()));
    assert!(parser.at_eof());
    assert_eq!(parser.advance(), Token::Eof);
    assert!(parser.at_eof());
}

#[test]
fn contextual_async_and_class_lookahead_accept_the_supported_forms() {
    let mut with_statement = Parser::new("with({value:1})value");
    assert!(with_statement.parse_with_stmt().is_ok());

    let mut class = Parser::new("class C{async method(){} static async *items(){}}");
    class.advance();
    assert!(class.parse_class().is_ok());

    assert!(Parser::new("async function named(){}").async_function_follows());
    assert!(!Parser::new("async\nfunction named(){}").async_function_follows());
    assert!(Parser::new("async value=>value").async_arrow_follows());
    assert!(Parser::new("async (value)=>value").async_arrow_follows());
    assert!(!Parser::new("async value").async_arrow_follows());
    assert!(!Parser::new("async (value)").async_arrow_follows());
    assert!(!Parser::new("async (value").async_arrow_follows());
    assert!(!Parser::new("async").async_arrow_follows());
    assert!(!Parser::new("value").async_arrow_follows());
    assert!(!Parser::new("async\n(value)=>value").async_arrow_follows());
    assert_eq!(
        class_element_name(&PropertyKey::Computed(Box::new(Expr::Identifier(
            "key".into()
        )))),
        ""
    );
}

fn program(src: &str) -> Program {
    parse(src).expect(src)
}

fn expr(src: &str) -> Expr {
    parse_expression_from_source(src).expect(src)
}

fn only_stmt(src: &str) -> Stmt {
    let p = program(src);
    assert_eq!(
        p.body.len(),
        1,
        "expected exactly one statement in {src:?}, got {:?}",
        p.body
    );
    p.body.into_iter().next().unwrap()
}

#[test]
fn parses_literals() {
    assert_eq!(expr("42"), Expr::Number(42.0));
    assert_eq!(expr("\"hi\""), Expr::String("hi".into()));
    assert_eq!(expr("true"), Expr::Bool(true));
    assert_eq!(expr("false"), Expr::Bool(false));
    assert_eq!(expr("null"), Expr::Null);
    assert_eq!(expr("this"), Expr::This);
    assert_eq!(expr("undefined"), Expr::Identifier("undefined".to_string()));
    assert_eq!(expr("x"), Expr::Identifier("x".to_string()));
}

#[test]
fn rejects_super_calls_and_properties_in_script_code() {
    for source in [
        "super()",
        "super.property",
        "()=>super()",
        "()=>super.property",
    ] {
        assert!(parse(source).is_err(), "{source}");
    }
}

#[test]
fn parses_template_literal_with_expressions() {
    assert_eq!(
        expr("`sum: ${a + b}!`"),
        Expr::Template {
            quasis: vec!["sum: ".into(), "!".into()],
            expressions: vec![Expr::Binary {
                op: BinaryOp::Add,
                left: Box::new(Expr::Identifier("a".to_string())),
                right: Box::new(Expr::Identifier("b".to_string()))
            }]
        }
    );
    assert!(
        matches!(expr(r"tag`value: ${1}`"), Expr::TaggedTemplate { expressions, .. } if expressions == vec![Expr::Number(1.0)])
    );
}

#[test]
fn operator_precedence_multiplicative_over_additive() {
    assert_eq!(
        expr("1 + 2 * 3"),
        Expr::Binary {
            op: BinaryOp::Add,
            left: Box::new(Expr::Number(1.0)),
            right: Box::new(Expr::Binary {
                op: BinaryOp::Mul,
                left: Box::new(Expr::Number(2.0)),
                right: Box::new(Expr::Number(3.0))
            })
        }
    );
}

#[test]
fn operator_precedence_relational_over_equality() {
    assert_eq!(
        expr("a < b === c"),
        Expr::Binary {
            op: BinaryOp::StrictEq,
            left: Box::new(Expr::Binary {
                op: BinaryOp::Lt,
                left: Box::new(Expr::Identifier("a".to_string())),
                right: Box::new(Expr::Identifier("b".to_string()))
            }),
            right: Box::new(Expr::Identifier("c".to_string())),
        }
    );
}

#[test]
fn logical_and_binds_tighter_than_logical_or() {
    assert_eq!(
        expr("a || b && c"),
        Expr::Logical {
            op: LogicalOp::Or,
            left: Box::new(Expr::Identifier("a".to_string())),
            right: Box::new(Expr::Logical {
                op: LogicalOp::And,
                left: Box::new(Expr::Identifier("b".to_string())),
                right: Box::new(Expr::Identifier("c".to_string()))
            })
        }
    );
}

#[test]
fn parses_nullish_coalescing_typeof_instanceof_in() {
    assert_eq!(
        expr("a ?? b"),
        Expr::Logical {
            op: LogicalOp::Nullish,
            left: Box::new(Expr::Identifier("a".to_string())),
            right: Box::new(Expr::Identifier("b".to_string()))
        }
    );
    assert_eq!(
        expr("typeof x"),
        Expr::Unary {
            op: UnaryOp::Typeof,
            arg: Box::new(Expr::Identifier("x".to_string()))
        }
    );
    assert_eq!(
        expr("x instanceof Foo"),
        Expr::Binary {
            op: BinaryOp::Instanceof,
            left: Box::new(Expr::Identifier("x".to_string())),
            right: Box::new(Expr::Identifier("Foo".to_string()))
        }
    );
    assert_eq!(
        expr("'k' in obj"),
        Expr::Binary {
            op: BinaryOp::In,
            left: Box::new(Expr::String("k".into())),
            right: Box::new(Expr::Identifier("obj".to_string()))
        }
    );
}

#[test]
fn parses_ternary_right_associative() {
    assert_eq!(
        expr("a ? b : c ? d : e"),
        Expr::Conditional {
            test: Box::new(Expr::Identifier("a".to_string())),
            consequent: Box::new(Expr::Identifier("b".to_string())),
            alternate: Box::new(Expr::Conditional {
                test: Box::new(Expr::Identifier("c".to_string())),
                consequent: Box::new(Expr::Identifier("d".to_string())),
                alternate: Box::new(Expr::Identifier("e".to_string())),
            }),
        }
    );
}

#[test]
fn parses_assignment_and_compound_assignment() {
    assert_eq!(
        expr("x = 1"),
        Expr::Assign {
            op: AssignOp::Assign,
            target: Box::new(Expr::Identifier("x".to_string())),
            value: Box::new(Expr::Number(1.0))
        }
    );
    assert_eq!(
        expr("x += 1"),
        Expr::Assign {
            op: AssignOp::AddAssign,
            target: Box::new(Expr::Identifier("x".to_string())),
            value: Box::new(Expr::Number(1.0))
        }
    );
    assert_eq!(
        expr("x -= 1"),
        Expr::Assign {
            op: AssignOp::SubAssign,
            target: Box::new(Expr::Identifier("x".to_string())),
            value: Box::new(Expr::Number(1.0))
        }
    );
    assert_eq!(
        expr("x *= 2"),
        Expr::Assign {
            op: AssignOp::MulAssign,
            target: Box::new(Expr::Identifier("x".to_string())),
            value: Box::new(Expr::Number(2.0))
        }
    );
    assert_eq!(
        expr("x /= 2"),
        Expr::Assign {
            op: AssignOp::DivAssign,
            target: Box::new(Expr::Identifier("x".to_string())),
            value: Box::new(Expr::Number(2.0))
        }
    );
    assert_eq!(
        expr("x %= 2"),
        Expr::Assign {
            op: AssignOp::ModAssign,
            target: Box::new(Expr::Identifier("x".to_string())),
            value: Box::new(Expr::Number(2.0))
        }
    );
    assert_eq!(
        expr("x = y = 1"),
        Expr::Assign {
            op: AssignOp::Assign,
            target: Box::new(Expr::Identifier("x".to_string())),
            value: Box::new(Expr::Assign {
                op: AssignOp::Assign,
                target: Box::new(Expr::Identifier("y".to_string())),
                value: Box::new(Expr::Number(1.0))
            })
        }
    );
}

#[test]
fn invalid_assignment_target_is_an_error() {
    assert!(parse_expression_from_source("1 = 2").is_err());
    assert!(parse_expression_from_source("(a + b) = 2").is_err());
    assert!(parse_expression_from_source("([1] = source)").is_err());
    assert!(parse_expression_from_source("[").is_err());
    assert!(parse_expression_from_source("([a, ...b, c] = source)").is_err());
    assert!(parse_expression_from_source("({a, ...rest, b} = source)").is_err());
}

#[test]
fn parses_prefix_and_postfix_update_expressions() {
    assert_eq!(
        expr("++x"),
        Expr::Update {
            op: UpdateOp::Inc,
            arg: Box::new(Expr::Identifier("x".to_string())),
            prefix: true
        }
    );
    assert_eq!(
        expr("x++"),
        Expr::Update {
            op: UpdateOp::Inc,
            arg: Box::new(Expr::Identifier("x".to_string())),
            prefix: false
        }
    );
    assert_eq!(
        expr("--x"),
        Expr::Update {
            op: UpdateOp::Dec,
            arg: Box::new(Expr::Identifier("x".to_string())),
            prefix: true
        }
    );
    assert_eq!(
        expr("x--"),
        Expr::Update {
            op: UpdateOp::Dec,
            arg: Box::new(Expr::Identifier("x".to_string())),
            prefix: false
        }
    );
}

#[test]
fn postfix_update_is_suppressed_across_a_newline_asi() {
    // `x\n++y` is `x; ++y`, not `x++; y` -- ASI's restricted-token rule.
    let p = program("x\n++y");
    assert_eq!(
        p.body,
        vec![
            Stmt::Expr(Expr::Identifier("x".to_string())),
            Stmt::Expr(Expr::Update {
                op: UpdateOp::Inc,
                arg: Box::new(Expr::Identifier("y".to_string())),
                prefix: true
            })
        ]
    );
}

#[test]
fn invalid_update_operand_is_an_error() {
    assert!(parse_expression_from_source("1++").is_err());
    assert!(parse_expression_from_source("++1").is_err());
}

#[test]
fn parses_member_and_computed_member_and_call_chains() {
    assert_eq!(
        expr("a.b[c](d)"),
        Expr::Call {
            callee: Box::new(Expr::Member {
                object: Box::new(Expr::Member {
                    object: Box::new(Expr::Identifier("a".to_string())),
                    property: Box::new(Expr::Identifier("b".to_string())),
                    computed: false
                }),
                property: Box::new(Expr::Identifier("c".to_string())),
                computed: true,
            }),
            args: vec![Argument::Normal(Expr::Identifier("d".to_string()))],
        }
    );
}

#[test]
fn parses_new_expression_with_a_computed_member_callee() {
    assert_eq!(
        expr("new a[b]()"),
        Expr::New {
            callee: Box::new(Expr::Member {
                object: Box::new(Expr::Identifier("a".to_string())),
                property: Box::new(Expr::Identifier("b".to_string())),
                computed: true
            }),
            args: vec![]
        }
    );
}

#[test]
fn parses_new_expressions() {
    assert_eq!(
        expr("new Error(\"boom\")"),
        Expr::New {
            callee: Box::new(Expr::Identifier("Error".to_string())),
            args: vec![Argument::Normal(Expr::String("boom".into()))]
        }
    );
    assert_eq!(
        expr("new Foo"),
        Expr::New {
            callee: Box::new(Expr::Identifier("Foo".to_string())),
            args: vec![]
        }
    );
    assert_eq!(
        expr("new a.b.C()"),
        Expr::New {
            callee: Box::new(Expr::Member {
                object: Box::new(Expr::Member {
                    object: Box::new(Expr::Identifier("a".to_string())),
                    property: Box::new(Expr::Identifier("b".to_string())),
                    computed: false
                }),
                property: Box::new(Expr::Identifier("C".to_string())),
                computed: false,
            }),
            args: vec![],
        }
    );
    // A call immediately after `new Foo()` attaches to the `New`
    // node via the outer left-hand-side loop, not `parse_new_expression` itself.
    assert_eq!(
        expr("new Foo()()"),
        Expr::Call {
            callee: Box::new(Expr::New {
                callee: Box::new(Expr::Identifier("Foo".to_string())),
                args: vec![]
            }),
            args: vec![]
        }
    );
}

#[test]
fn parses_call_with_spread_argument() {
    assert_eq!(
        expr("f(1, ...xs, 2)"),
        Expr::Call {
            callee: Box::new(Expr::Identifier("f".to_string())),
            args: vec![
                Argument::Normal(Expr::Number(1.0)),
                Argument::Spread(Expr::Identifier("xs".to_string())),
                Argument::Normal(Expr::Number(2.0))
            ],
        }
    );
}

#[test]
fn parses_array_literal_with_holes_and_spread_and_trailing_comma() {
    assert_eq!(
        expr("[1, 2, 3]"),
        Expr::Array(vec![
            Some(ArrayElement::Normal(Expr::Number(1.0))),
            Some(ArrayElement::Normal(Expr::Number(2.0))),
            Some(ArrayElement::Normal(Expr::Number(3.0)))
        ])
    );
    assert_eq!(
        expr("[1,,3]"),
        Expr::Array(vec![
            Some(ArrayElement::Normal(Expr::Number(1.0))),
            None,
            Some(ArrayElement::Normal(Expr::Number(3.0)))
        ])
    );
    assert_eq!(
        expr("[1, 2,]"),
        Expr::Array(vec![
            Some(ArrayElement::Normal(Expr::Number(1.0))),
            Some(ArrayElement::Normal(Expr::Number(2.0)))
        ])
    );
    assert_eq!(
        expr("[...xs]"),
        Expr::Array(vec![Some(ArrayElement::Spread(Expr::Identifier(
            "xs".to_string()
        )))])
    );
}

#[test]
fn parses_object_literal_with_shorthand_computed_and_spread() {
    assert_eq!(
        expr("{a: 1, b, [c]: 2, ...rest}"),
        Expr::Object(vec![
            ObjectProp::KeyValue {
                key: PropertyKey::Identifier("a".to_string()),
                value: Expr::Number(1.0),
                shorthand: false
            },
            ObjectProp::KeyValue {
                key: PropertyKey::Identifier("b".to_string()),
                value: Expr::Identifier("b".to_string()),
                shorthand: true
            },
            ObjectProp::KeyValue {
                key: PropertyKey::Computed(Box::new(Expr::Identifier("c".to_string()))),
                value: Expr::Number(2.0),
                shorthand: false
            },
            ObjectProp::Spread(Expr::Identifier("rest".to_string())),
        ])
    );
    assert!(matches!(
        expr("{get 'quoted'(){return 1},set 3(value){}}"),
        Expr::Object(properties)
            if matches!(
                &properties[..],
                [
                    ObjectProp::Accessor { function, getter: true, .. },
                    ObjectProp::Accessor { function: setter, getter: false, .. },
                ] if function.name.as_deref() == Some("get quoted") && setter.name.as_deref() == Some("set 3")
            )
    ));
    assert!(matches!(
        only_stmt("({get 'quoted'(){return 1}})"),
        Stmt::Expr(Expr::Object(properties))
            if matches!(
                &properties[..],
                [ObjectProp::Accessor { function, getter: true, .. }]
                    if function.name.as_deref() == Some("get quoted")
            )
    ));
    assert!(matches!(
        expr("{* generated(){yield 1},async resolved(){return 2},async * streamed(){yield 3}}"),
        Expr::Object(properties)
            if matches!(
                &properties[..],
                [
                    ObjectProp::Method { function: generated, .. },
                    ObjectProp::Method { function: resolved, .. },
                    ObjectProp::Method { function: streamed, .. },
                ] if generated.generator
                    && !generated.is_async
                    && !resolved.generator
                    && resolved.is_async
                    && streamed.generator
                    && streamed.is_async
            )
    ));
}

#[test]
fn object_literal_used_as_a_statement_needs_parens_or_context() {
    // `{a: 1}` alone at statement position is a block containing a
    // labeled-looking statement in real JS; this parser doesn't
    // support labels, so bare `{a:1}` as a *statement* parses as a
    // block (consistent with the grammar ambiguity real engines
    // resolve the same way at statement position) -- confirming
    // object literals are unambiguous only in expression position,
    // e.g. wrapped in parens.
    assert_eq!(
        expr("({a: 1})"),
        Expr::Object(vec![ObjectProp::KeyValue {
            key: PropertyKey::Identifier("a".to_string()),
            value: Expr::Number(1.0),
            shorthand: false
        }])
    );
}

#[test]
fn parses_function_declaration() {
    assert_eq!(
        only_stmt("function add(a, b) { return a + b; }"),
        Stmt::FunctionDecl(Function {
            name: Some("add".to_string()),
            params: vec![
                Param {
                    pattern: Pattern::Identifier("a".to_string()),
                    default: None,
                    rest: false
                },
                Param {
                    pattern: Pattern::Identifier("b".to_string()),
                    default: None,
                    rest: false
                }
            ],
            body: vec![Stmt::Return(Some(Expr::Binary {
                op: BinaryOp::Add,
                left: Box::new(Expr::Identifier("a".to_string())),
                right: Box::new(Expr::Identifier("b".to_string()))
            }))],
            generator: false,
            is_async: false,
        })
    );
}

#[test]
fn anonymous_function_declaration_is_an_error() {
    assert!(parse("function (a) { return a; }").is_err());
}

#[test]
fn parses_function_with_default_and_rest_params() {
    assert_eq!(
        expr("function f(a, b = 1, ...rest) {}"),
        Expr::Function(Function {
            name: Some("f".to_string()),
            params: vec![
                Param {
                    pattern: Pattern::Identifier("a".to_string()),
                    default: None,
                    rest: false
                },
                Param {
                    pattern: Pattern::Identifier("b".to_string()),
                    default: Some(Expr::Number(1.0)),
                    rest: false
                },
                Param {
                    pattern: Pattern::Identifier("rest".to_string()),
                    default: None,
                    rest: true
                },
            ],
            body: vec![],
            generator: false,
            is_async: false,
        })
    );
}

#[test]
fn parses_arrow_functions_all_shapes() {
    assert_eq!(
        expr("x => x + 1"),
        Expr::Arrow {
            params: vec![Param {
                pattern: Pattern::Identifier("x".to_string()),
                default: None,
                rest: false
            }],
            body: ArrowBody::Expr(Box::new(Expr::Binary {
                op: BinaryOp::Add,
                left: Box::new(Expr::Identifier("x".to_string())),
                right: Box::new(Expr::Number(1.0))
            })),
            is_async: false,
        }
    );
    assert_eq!(
        expr("() => {}"),
        Expr::Arrow {
            params: vec![],
            body: ArrowBody::Block(vec![]),
            is_async: false
        }
    );
    assert_eq!(
        expr("(a, b) => { return a + b; }"),
        Expr::Arrow {
            params: vec![
                Param {
                    pattern: Pattern::Identifier("a".to_string()),
                    default: None,
                    rest: false
                },
                Param {
                    pattern: Pattern::Identifier("b".to_string()),
                    default: None,
                    rest: false
                }
            ],
            body: ArrowBody::Block(vec![Stmt::Return(Some(Expr::Binary {
                op: BinaryOp::Add,
                left: Box::new(Expr::Identifier("a".to_string())),
                right: Box::new(Expr::Identifier("b".to_string()))
            }))]),
            is_async: false,
        }
    );
    assert!(matches!(
        expr("async value => await value"),
        Expr::Arrow { is_async: true, .. }
    ));
}

#[test]
fn nested_parentheses_in_arrow_defaults_preserve_the_parameter_boundary() {
    // Drive the public parser; an inner ')' must not terminate arrow
    // lookahead before the actual parameter list's ')' and '=>'.
    assert_eq!(
        parse("(x=((1+2)*3))=>x").unwrap(),
        Program {
            body: vec![Stmt::Expr(Expr::Arrow {
                params: vec![Param {
                    pattern: Pattern::Identifier("x".into()),
                    default: Some(Expr::Binary {
                        op: BinaryOp::Mul,
                        left: Box::new(Expr::Binary {
                            op: BinaryOp::Add,
                            left: Box::new(Expr::Number(1.0)),
                            right: Box::new(Expr::Number(2.0))
                        }),
                        right: Box::new(Expr::Number(3.0)),
                    }),
                    rest: false,
                }],
                body: ArrowBody::Expr(Box::new(Expr::Identifier("x".into()))),
                is_async: false,
            })],
        }
    );
    assert!(parse("(x=((1+2)*3)=>x").is_err());
}

#[test]
fn arrow_function_closes_over_outer_scope_syntactically() {
    // No runtime scoping to test here (that's the interpreter's
    // job, a separate checklist item) -- just that nested function
    // bodies parse as ordinary nested statement lists referencing
    // outer identifiers by name, which is all a closure needs
    // *syntactically*.
    assert_eq!(
        expr("(x) => () => x"),
        Expr::Arrow {
            params: vec![Param {
                pattern: Pattern::Identifier("x".to_string()),
                default: None,
                rest: false
            }],
            body: ArrowBody::Expr(Box::new(Expr::Arrow {
                params: vec![],
                body: ArrowBody::Expr(Box::new(Expr::Identifier("x".to_string()))),
                is_async: false
            })),
            is_async: false,
        }
    );
}

#[test]
fn parses_destructuring_in_declarations_and_params() {
    assert_eq!(
        only_stmt("let [a, , b, ...rest] = arr;"),
        Stmt::VarDecl(
            DeclKind::Let,
            vec![VarDeclarator {
                pattern: Pattern::Array(vec![
                    Some(ArrayPatternElement {
                        pattern: Pattern::Identifier("a".to_string()),
                        default: None,
                        rest: false
                    }),
                    None,
                    Some(ArrayPatternElement {
                        pattern: Pattern::Identifier("b".to_string()),
                        default: None,
                        rest: false
                    }),
                    Some(ArrayPatternElement {
                        pattern: Pattern::Identifier("rest".to_string()),
                        default: None,
                        rest: true
                    }),
                ]),
                init: Some(Expr::Identifier("arr".to_string())),
            }],
        )
    );
    assert_eq!(
        only_stmt("const {a, b: renamed = 1, ...rest} = obj;"),
        Stmt::VarDecl(
            DeclKind::Const,
            vec![VarDeclarator {
                pattern: Pattern::Object(vec![
                    ObjectPatternProp::KeyValue {
                        key: PropertyKey::Identifier("a".to_string()),
                        value: Pattern::Identifier("a".to_string()),
                        default: None
                    },
                    ObjectPatternProp::KeyValue {
                        key: PropertyKey::Identifier("b".to_string()),
                        value: Pattern::Identifier("renamed".to_string()),
                        default: Some(Expr::Number(1.0))
                    },
                    ObjectPatternProp::Rest(Pattern::Identifier("rest".to_string())),
                ]),
                init: Some(Expr::Identifier("obj".to_string())),
            }],
        )
    );
    assert_eq!(
        expr("function f([a, b]) {}"),
        Expr::Function(Function {
            name: Some("f".to_string()),
            params: vec![Param {
                pattern: Pattern::Array(vec![
                    Some(ArrayPatternElement {
                        pattern: Pattern::Identifier("a".to_string()),
                        default: None,
                        rest: false
                    }),
                    Some(ArrayPatternElement {
                        pattern: Pattern::Identifier("b".to_string()),
                        default: None,
                        rest: false
                    }),
                ]),
                default: None,
                rest: false,
            }],
            body: vec![],
            generator: false,
            is_async: false,
        })
    );
    assert!(matches!(
        expr("([a,,b=3,...rest]=source)"),
        Expr::DestructureAssign {
            pattern: AssignmentPattern::Array(_),
            ..
        }
    ));
    assert!(matches!(
        expr("({a,b:c=2,...rest}=source)"),
        Expr::DestructureAssign {
            pattern: AssignmentPattern::Object(_),
            ..
        }
    ));
}

#[test]
fn parses_var_let_const_with_multiple_declarators() {
    assert_eq!(
        only_stmt("var a = 1, b = 2;"),
        Stmt::VarDecl(
            DeclKind::Var,
            vec![
                VarDeclarator {
                    pattern: Pattern::Identifier("a".to_string()),
                    init: Some(Expr::Number(1.0))
                },
                VarDeclarator {
                    pattern: Pattern::Identifier("b".to_string()),
                    init: Some(Expr::Number(2.0))
                }
            ]
        )
    );
    assert_eq!(
        only_stmt("let x;"),
        Stmt::VarDecl(
            DeclKind::Let,
            vec![VarDeclarator {
                pattern: Pattern::Identifier("x".to_string()),
                init: None
            }]
        )
    );
}

#[test]
fn parses_if_else() {
    assert_eq!(
        only_stmt("if (a) b; else c;"),
        Stmt::If {
            test: Expr::Identifier("a".to_string()),
            consequent: Box::new(Stmt::Expr(Expr::Identifier("b".to_string()))),
            alternate: Some(Box::new(Stmt::Expr(Expr::Identifier("c".to_string())))),
        }
    );
}

#[test]
fn parses_while_and_do_while() {
    assert_eq!(
        only_stmt("while (a) b;"),
        Stmt::While {
            test: Expr::Identifier("a".to_string()),
            body: Box::new(Stmt::Expr(Expr::Identifier("b".to_string())))
        }
    );
    assert_eq!(
        only_stmt("do a; while (b);"),
        Stmt::DoWhile {
            body: Box::new(Stmt::Expr(Expr::Identifier("a".to_string()))),
            test: Expr::Identifier("b".to_string())
        }
    );
}

#[test]
fn parses_classic_for_loop() {
    assert_eq!(
        only_stmt("for (let i = 0; i < 10; i++) {}"),
        Stmt::For {
            init: Some(ForInit::VarDecl(
                DeclKind::Let,
                vec![VarDeclarator {
                    pattern: Pattern::Identifier("i".to_string()),
                    init: Some(Expr::Number(0.0))
                }]
            )),
            test: Some(Expr::Binary {
                op: BinaryOp::Lt,
                left: Box::new(Expr::Identifier("i".to_string())),
                right: Box::new(Expr::Number(10.0))
            }),
            update: Some(Expr::Update {
                op: UpdateOp::Inc,
                arg: Box::new(Expr::Identifier("i".to_string())),
                prefix: false
            }),
            body: Box::new(Stmt::Block(vec![])),
        }
    );
    // All three clauses empty.
    assert_eq!(
        only_stmt("for (;;) {}"),
        Stmt::For {
            init: None,
            test: None,
            update: None,
            body: Box::new(Stmt::Block(vec![]))
        }
    );
}

#[test]
fn for_loop_with_existing_variable_does_not_misparse_in_as_a_binary_operator() {
    assert_eq!(
        only_stmt("for (i = 0; i < 10; i++) {}"),
        Stmt::For {
            init: Some(ForInit::Expr(Expr::Assign {
                op: AssignOp::Assign,
                target: Box::new(Expr::Identifier("i".to_string())),
                value: Box::new(Expr::Number(0.0))
            })),
            test: Some(Expr::Binary {
                op: BinaryOp::Lt,
                left: Box::new(Expr::Identifier("i".to_string())),
                right: Box::new(Expr::Number(10.0))
            }),
            update: Some(Expr::Update {
                op: UpdateOp::Inc,
                arg: Box::new(Expr::Identifier("i".to_string())),
                prefix: false
            }),
            body: Box::new(Stmt::Block(vec![])),
        }
    );
}

#[test]
fn parses_for_in_and_for_of() {
    assert_eq!(
        only_stmt("for (let k in obj) {}"),
        Stmt::ForIn {
            left: ForHead::Decl(DeclKind::Let, Pattern::Identifier("k".to_string())),
            right: Expr::Identifier("obj".to_string()),
            body: Box::new(Stmt::Block(vec![]))
        }
    );
    assert_eq!(
        only_stmt("for (const item of items) {}"),
        Stmt::ForOf {
            left: ForHead::Decl(DeclKind::Const, Pattern::Identifier("item".to_string())),
            right: Expr::Identifier("items".to_string()),
            body: Box::new(Stmt::Block(vec![])),
            is_await: false,
        }
    );
    assert_eq!(
        only_stmt("for (x of items) {}"),
        Stmt::ForOf {
            left: ForHead::Assignment(AssignmentPattern::Target(Box::new(Expr::Identifier(
                "x".to_string(),
            )))),
            right: Expr::Identifier("items".to_string()),
            body: Box::new(Stmt::Block(vec![])),
            is_await: false,
        }
    );
}

#[test]
fn parses_switch_with_default() {
    assert_eq!(
        only_stmt("switch (x) { case 1: a; break; default: b; }"),
        Stmt::Switch {
            discriminant: Expr::Identifier("x".to_string()),
            cases: vec![
                SwitchCase {
                    test: Some(Expr::Number(1.0)),
                    consequent: vec![
                        Stmt::Expr(Expr::Identifier("a".to_string())),
                        Stmt::Break(None)
                    ]
                },
                SwitchCase {
                    test: None,
                    consequent: vec![Stmt::Expr(Expr::Identifier("b".to_string()))]
                },
            ],
        }
    );
}

#[test]
fn duplicate_switch_default_is_a_known_syntax_error() {
    let error = parse("switch (value) { default: first; default: second; }").unwrap_err();
    assert!(error.known_syntax);
    assert!(error
        .message
        .starts_with("a switch statement can contain only one default clause"));
}

#[test]
fn malformed_switch_productions_are_known_syntax_errors() {
    for source in [
        "switch() {}",
        "switch {}",
        "switch(value);",
        "switch(value) { case: }",
        "switch(value) { value = 2; case 0: }",
    ] {
        let error = parse(source).unwrap_err();
        assert!(error.known_syntax, "{source}: {error:?}");
    }
}

#[test]
fn parses_try_catch_finally_and_requires_at_least_one() {
    assert_eq!(
        only_stmt("try { a; } catch (e) { b; } finally { c; }"),
        Stmt::Try {
            block: vec![Stmt::Expr(Expr::Identifier("a".to_string()))],
            handler: Some(CatchClause {
                param: Some(Pattern::Identifier("e".to_string())),
                body: vec![Stmt::Expr(Expr::Identifier("b".to_string()))]
            }),
            finalizer: Some(vec![Stmt::Expr(Expr::Identifier("c".to_string()))]),
        }
    );
    assert_eq!(
        only_stmt("try { a; } catch { b; }"),
        Stmt::Try {
            block: vec![Stmt::Expr(Expr::Identifier("a".to_string()))],
            handler: Some(CatchClause {
                param: None,
                body: vec![Stmt::Expr(Expr::Identifier("b".to_string()))]
            }),
            finalizer: None
        }
    );
    assert!(parse("try { a; }").is_err());
}

#[test]
fn parses_throw_and_forbids_newline_before_its_expression() {
    assert_eq!(
        only_stmt("throw new Error(\"x\");"),
        Stmt::Throw(Expr::New {
            callee: Box::new(Expr::Identifier("Error".to_string())),
            args: vec![Argument::Normal(Expr::String("x".into()))]
        })
    );
    assert!(parse("throw\nnew Error(\"x\");").is_err());
}

#[test]
fn automatic_semicolon_insertion_covers_the_common_cases() {
    let p = program("let a = 1\nlet b = 2");
    assert_eq!(p.body.len(), 2);
    // return with a newline before the value returns nothing, and
    // the value becomes its own separate expression statement.
    assert_eq!(
        only_stmt("function f() { return\n1; }"),
        Stmt::FunctionDecl(Function {
            name: Some("f".to_string()),
            params: vec![],
            body: vec![Stmt::Return(None), Stmt::Expr(Expr::Number(1.0))],
            generator: false,
            is_async: false
        })
    );
}

#[test]
fn missing_semicolon_with_no_asi_opportunity_is_an_error() {
    assert!(parse("let a = 1 let b = 2").is_err());
}

#[test]
fn dom_script_acceptance_bar_shaped_program_parses() {
    // Mirrors `phase-2-mvp-scope/PLAN.md`'s "Interactive-JS
    // acceptance bar" shape closely enough to prove this parser
    // covers what that bar needs syntactically (semantics are the
    // interpreter's job, not this crate's).
    let src = r#"
            var items = ["a", "b", "c"];
            for (var i = 0; i < items.length; i++) {
                var li = document.createElement("li");
                li.textContent = items[i];
                list.appendChild(li);
            }
            button.addEventListener("click", function () {
                el.style.display = el.style.display === "none" ? "block" : "none";
            });
        "#;
    let p = program(src);
    assert_eq!(p.body.len(), 3);
}

#[test]
fn unexpected_token_in_expression_is_a_typed_error_not_a_panic() {
    assert!(parse("let x = ;").is_err());
    assert!(parse(")").is_err());
    assert!(parse("function").is_err());
}

#[test]
fn bitwise_operators_follow_their_grammar_precedence() {
    assert_eq!(
        expr("1|2^3&4<<5+6"),
        Expr::Binary {
            op: BinaryOp::BitOr,
            left: Box::new(Expr::Number(1.0)),
            right: Box::new(Expr::Binary {
                op: BinaryOp::BitXor,
                left: Box::new(Expr::Number(2.0)),
                right: Box::new(Expr::Binary {
                    op: BinaryOp::BitAnd,
                    left: Box::new(Expr::Number(3.0)),
                    right: Box::new(Expr::Binary {
                        op: BinaryOp::ShiftLeft,
                        left: Box::new(Expr::Number(4.0)),
                        right: Box::new(Expr::Binary {
                            op: BinaryOp::Add,
                            left: Box::new(Expr::Number(5.0)),
                            right: Box::new(Expr::Number(6.0)),
                        }),
                    }),
                }),
            }),
        }
    );
    assert!(parse("let x=1;x&=2;x|=4;x^=3;x<<=1;x>>=1;x>>>=0").is_ok());
}

#[test]
fn parses_bare_semicolon_and_continue_statements() {
    assert_eq!(only_stmt(";"), Stmt::Empty);
    assert_eq!(only_stmt("continue;"), Stmt::Continue(None));
}

#[test]
fn keywords_are_accepted_as_member_names_and_object_property_keys() {
    // `.default`/`.in`/etc. are ordinary property accesses in real
    // ECMAScript (keywords are only reserved as *identifiers*, not
    // as property names) -- exercises `keyword_as_str` broadly.
    assert_eq!(
        expr("obj.default"),
        Expr::Member {
            object: Box::new(Expr::Identifier("obj".to_string())),
            property: Box::new(Expr::Identifier("default".to_string())),
            computed: false
        }
    );
    assert_eq!(
        expr("obj.in"),
        Expr::Member {
            object: Box::new(Expr::Identifier("obj".to_string())),
            property: Box::new(Expr::Identifier("in".to_string())),
            computed: false
        }
    );
    assert_eq!(
        expr("obj.function"),
        Expr::Member {
            object: Box::new(Expr::Identifier("obj".to_string())),
            property: Box::new(Expr::Identifier("function".to_string())),
            computed: false
        }
    );
    assert_eq!(
        expr("{default: 1, case: 2, new: 3}"),
        Expr::Object(vec![
            ObjectProp::KeyValue {
                key: PropertyKey::Identifier("default".to_string()),
                value: Expr::Number(1.0),
                shorthand: false
            },
            ObjectProp::KeyValue {
                key: PropertyKey::Identifier("case".to_string()),
                value: Expr::Number(2.0),
                shorthand: false
            },
            ObjectProp::KeyValue {
                key: PropertyKey::Identifier("new".to_string()),
                value: Expr::Number(3.0),
                shorthand: false
            },
        ])
    );
}

#[test]
fn member_access_with_a_non_identifier_property_name_is_an_error() {
    assert!(parse_expression_from_source("a.1").is_err());
    assert!(parse_expression_from_source("a.").is_err());
}

#[test]
fn for_of_and_for_in_assignment_targets_are_parsed_without_a_declaration_keyword() {
    assert_eq!(
        only_stmt("for (target.value of items) {}"),
        Stmt::ForOf {
            left: ForHead::Assignment(AssignmentPattern::Target(Box::new(Expr::Member {
                object: Box::new(Expr::Identifier("target".to_string())),
                property: Box::new(Expr::Identifier("value".to_string())),
                computed: false,
            }))),
            right: Expr::Identifier("items".to_string()),
            body: Box::new(Stmt::Block(vec![])),
            is_await: false,
        }
    );
    assert_eq!(
        only_stmt("for ([first, {value: target.value}] in object) {}"),
        Stmt::ForIn {
            left: ForHead::Assignment(AssignmentPattern::Array(vec![
                Some(AssignmentPatternElement {
                    pattern: AssignmentPattern::Target(Box::new(Expr::Identifier(
                        "first".to_string(),
                    ))),
                    default: None,
                    rest: false,
                }),
                Some(AssignmentPatternElement {
                    pattern: AssignmentPattern::Object(vec![AssignmentPatternProp::KeyValue {
                        key: PropertyKey::Identifier("value".to_string()),
                        value: AssignmentPattern::Target(Box::new(Expr::Member {
                            object: Box::new(Expr::Identifier("target".to_string())),
                            property: Box::new(Expr::Identifier("value".to_string())),
                            computed: false,
                        })),
                        default: None,
                    }]),
                    default: None,
                    rest: false,
                }),
            ])),
            right: Expr::Identifier("object".to_string()),
            body: Box::new(Stmt::Block(vec![])),
        }
    );
    assert!(parse("for ({value:1};;) {}").is_ok());
    assert!(parse("for ([value] of items) {}").is_ok());
    assert!(parse("for ({value=1} of items) {}").is_ok());
    assert!(parse("for (1 of items) {}").is_err());
    assert!(parse("for ([1] of items) {}").is_err());
}

#[test]
fn do_while_missing_the_while_keyword_is_an_error() {
    assert!(parse("do a;").is_err());
}

#[test]
fn unterminated_switch_statement_is_an_error() {
    assert!(parse("switch (x) { case 1: a;").is_err());
}

#[test]
fn unterminated_block_is_an_error() {
    assert!(parse("function f() { let x = 1;").is_err());
}

#[test]
fn object_literal_accepts_a_string_key() {
    assert_eq!(
        expr(r#"{"a-b": 1}"#),
        Expr::Object(vec![ObjectProp::KeyValue {
            key: PropertyKey::String("a-b".into()),
            value: Expr::Number(1.0),
            shorthand: false
        }])
    );
}

#[test]
fn invalid_property_key_token_is_an_error() {
    assert!(parse_expression_from_source("({+: 1})").is_err());
}

#[test]
fn parses_every_equality_relational_additive_and_multiplicative_operator() {
    assert_eq!(
        expr("a != b"),
        Expr::Binary {
            op: BinaryOp::NotEq,
            left: Box::new(Expr::Identifier("a".to_string())),
            right: Box::new(Expr::Identifier("b".to_string()))
        }
    );
    assert_eq!(
        expr("a !== b"),
        Expr::Binary {
            op: BinaryOp::StrictNotEq,
            left: Box::new(Expr::Identifier("a".to_string())),
            right: Box::new(Expr::Identifier("b".to_string()))
        }
    );
    assert_eq!(
        expr("a > b"),
        Expr::Binary {
            op: BinaryOp::Gt,
            left: Box::new(Expr::Identifier("a".to_string())),
            right: Box::new(Expr::Identifier("b".to_string()))
        }
    );
    assert_eq!(
        expr("a <= b"),
        Expr::Binary {
            op: BinaryOp::LtEq,
            left: Box::new(Expr::Identifier("a".to_string())),
            right: Box::new(Expr::Identifier("b".to_string()))
        }
    );
    assert_eq!(
        expr("a >= b"),
        Expr::Binary {
            op: BinaryOp::GtEq,
            left: Box::new(Expr::Identifier("a".to_string())),
            right: Box::new(Expr::Identifier("b".to_string()))
        }
    );
    assert_eq!(
        expr("a - b"),
        Expr::Binary {
            op: BinaryOp::Sub,
            left: Box::new(Expr::Identifier("a".to_string())),
            right: Box::new(Expr::Identifier("b".to_string()))
        }
    );
    assert_eq!(
        expr("a / b"),
        Expr::Binary {
            op: BinaryOp::Div,
            left: Box::new(Expr::Identifier("a".to_string())),
            right: Box::new(Expr::Identifier("b".to_string()))
        }
    );
    assert_eq!(
        expr("a % b"),
        Expr::Binary {
            op: BinaryOp::Mod,
            left: Box::new(Expr::Identifier("a".to_string())),
            right: Box::new(Expr::Identifier("b".to_string()))
        }
    );
}

#[test]
fn parses_unary_not_neg_and_plus() {
    assert_eq!(
        expr("!a"),
        Expr::Unary {
            op: UnaryOp::Not,
            arg: Box::new(Expr::Identifier("a".to_string()))
        }
    );
    assert_eq!(
        expr("-a"),
        Expr::Unary {
            op: UnaryOp::Neg,
            arg: Box::new(Expr::Identifier("a".to_string()))
        }
    );
    assert_eq!(
        expr("+a"),
        Expr::Unary {
            op: UnaryOp::Plus,
            arg: Box::new(Expr::Identifier("a".to_string()))
        }
    );
    assert_eq!(
        expr("void a"),
        Expr::Unary {
            op: UnaryOp::Void,
            arg: Box::new(Expr::Identifier("a".to_string()))
        }
    );
}

#[test]
fn invalid_decrement_operand_is_an_error_prefix_and_postfix() {
    assert!(parse_expression_from_source("--1").is_err());
    assert!(parse_expression_from_source("1--").is_err());
}

#[test]
fn parses_loose_equality() {
    assert_eq!(
        expr("a == b"),
        Expr::Binary {
            op: BinaryOp::Eq,
            left: Box::new(Expr::Identifier("a".to_string())),
            right: Box::new(Expr::Identifier("b".to_string()))
        }
    );
}

#[test]
fn an_unclosed_grouping_paren_that_runs_out_of_input_is_an_error() {
    // Exercises `matching_close_paren` hitting EOF before a match,
    // not just its happy-path return.
    assert!(parse_expression_from_source("(1 + 2").is_err());
}

#[test]
fn invalid_binding_pattern_target_is_an_error() {
    assert!(parse("let 5 = x;").is_err());
}

#[test]
fn binding_rest_elements_and_properties_must_be_final() {
    for source in [
        "try {} catch ([...rest, next]) {}",
        "try {} catch ([...{value}, next]) {}",
        "try {} catch ([...rest,]) {}",
        "try {} catch ({...rest, next}) {}",
        "try {} catch ({...rest,}) {}",
    ] {
        assert!(parse(source).is_err(), "{source}");
    }
}

#[test]
fn destructuring_object_pattern_with_a_computed_key_requires_a_colon() {
    // A computed key (`[expr]`) can never be a valid shorthand
    // binding on its own -- there's no identifier to shorthand to.
    assert!(parse("let {[k]} = obj;").is_err());
}

#[test]
fn object_literal_shorthand_requires_an_identifier_shaped_key() {
    // A computed or numeric key with no value and no ':' has no
    // identifier to shorthand to either.
    assert!(parse_expression_from_source("({[k]})").is_err());
    assert!(parse_expression_from_source("({1})").is_err());
}

#[test]
fn classic_for_loop_supports_multiple_declarators_and_an_omitted_initializer() {
    assert_eq!(
        only_stmt("for (let i = 0, j = 10; i < j; i++) {}"),
        Stmt::For {
            init: Some(ForInit::VarDecl(
                DeclKind::Let,
                vec![
                    VarDeclarator {
                        pattern: Pattern::Identifier("i".to_string()),
                        init: Some(Expr::Number(0.0))
                    },
                    VarDeclarator {
                        pattern: Pattern::Identifier("j".to_string()),
                        init: Some(Expr::Number(10.0))
                    },
                ]
            )),
            test: Some(Expr::Binary {
                op: BinaryOp::Lt,
                left: Box::new(Expr::Identifier("i".to_string())),
                right: Box::new(Expr::Identifier("j".to_string()))
            }),
            update: Some(Expr::Update {
                op: UpdateOp::Inc,
                arg: Box::new(Expr::Identifier("i".to_string())),
                prefix: false
            }),
            body: Box::new(Stmt::Block(vec![])),
        }
    );
    // No initializer at all on the declarator.
    assert_eq!(
        only_stmt("for (let i; i < 10; i++) {}"),
        Stmt::For {
            init: Some(ForInit::VarDecl(
                DeclKind::Let,
                vec![VarDeclarator {
                    pattern: Pattern::Identifier("i".to_string()),
                    init: None
                }]
            )),
            test: Some(Expr::Binary {
                op: BinaryOp::Lt,
                left: Box::new(Expr::Identifier("i".to_string())),
                right: Box::new(Expr::Number(10.0))
            }),
            update: Some(Expr::Update {
                op: UpdateOp::Inc,
                arg: Box::new(Expr::Identifier("i".to_string())),
                prefix: false
            }),
            body: Box::new(Stmt::Block(vec![])),
        }
    );
}

#[test]
fn for_in_without_a_declaration_keyword_uses_the_existing_variable() {
    assert_eq!(
        only_stmt("for (k in obj) {}"),
        Stmt::ForIn {
            left: ForHead::Assignment(AssignmentPattern::Target(Box::new(Expr::Identifier(
                "k".to_string(),
            )))),
            right: Expr::Identifier("obj".to_string()),
            body: Box::new(Stmt::Block(vec![]))
        }
    );
}

#[test]
fn template_placeholder_with_trailing_tokens_is_an_error() {
    // `${1 2}` has no valid single-expression parse: `1` consumes
    // the whole expression grammar, leaving `2` as unexpected
    // trailing input -- surfaced through `parse_expression_from_source`.
    assert!(parse_expression_from_source("`${1 2}`").is_err());
}

#[test]
fn a_grouping_parenthesized_expression_is_not_mistaken_for_arrow_params() {
    // Exercises `matching_close_paren` finding a real match with no
    // trailing `=>`, so the parenthesized form falls through to an
    // ordinary grouped expression instead.
    assert_eq!(
        expr("(1 + 2) * 3"),
        Expr::Binary {
            op: BinaryOp::Mul,
            left: Box::new(Expr::Binary {
                op: BinaryOp::Add,
                left: Box::new(Expr::Number(1.0)),
                right: Box::new(Expr::Number(2.0))
            }),
            right: Box::new(Expr::Number(3.0))
        }
    );
}

#[test]
fn bare_yield_can_terminate_before_destructuring_delimiters() {
    assert!(parse("function* g(){[target[yield],] = values;}").is_ok());
    assert!(parse("function* g(){[target[yield]] = values;}").is_ok());
}

#[test]
fn literal_based_member_targets_are_not_nested_destructuring_patterns() {
    assert!(parse("function* g(){[...{}[yield]] = values;}").is_ok());
    assert!(parse("[{ get y() {}, set y(value) {} }.y] = values;").is_ok());
}
