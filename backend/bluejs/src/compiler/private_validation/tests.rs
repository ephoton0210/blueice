// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

fn missing_private() -> Expr {
    Expr::PrivateIn {
        name: "missing".into(),
        object: Box::new(Expr::Number(1.0)),
    }
}

fn computed_private_key() -> PropertyKey {
    PropertyKey::Computed(Box::new(missing_private()))
}

fn private_pattern() -> Pattern {
    Pattern::Object(vec![ObjectPatternProp::KeyValue {
        key: computed_private_key(),
        value: Pattern::Identifier("value".into()),
        default: None,
    }])
}

fn private_assignment_pattern() -> AssignmentPattern {
    AssignmentPattern::Target(Box::new(missing_private()))
}

fn assert_private_error(label: &str, statement: Stmt) {
    let result = validate_private_early_errors(&Program {
        body: vec![statement],
    });
    assert_eq!(
        result,
        Err(CompileError::InvalidSyntax(
            "private name is not declared in an enclosing class"
        )),
        "{label}"
    );
}

fn class_with(element: ClassElement) -> Stmt {
    Stmt::ClassDecl(Class {
        name: Some("C".into()),
        extends: None,
        elements: vec![element],
        decorators: Vec::new(),
        source_text: SourceText::default(),
    })
}

#[test]
fn private_name_errors_propagate_from_statement_positions() {
    let cases = vec![
        (
            "decorated field wrapper",
            Stmt::ClassDecoratedField {
                field: Box::new(Stmt::Expr(missing_private())),
                record: "record".into(),
            },
        ),
        (
            "class field wrapper",
            Stmt::ClassField(Box::new(Stmt::Expr(missing_private()))),
        ),
        (
            "variable pattern",
            Stmt::VarDecl(
                DeclKind::Let,
                vec![VarDeclarator {
                    pattern: private_pattern(),
                    init: None,
                }],
            ),
        ),
        (
            "variable initializer",
            Stmt::VarDecl(
                DeclKind::Let,
                vec![VarDeclarator {
                    pattern: Pattern::Identifier("value".into()),
                    init: Some(missing_private()),
                }],
            ),
        ),
        (
            "if test",
            Stmt::If {
                test: missing_private(),
                consequent: Box::new(Stmt::Empty),
                alternate: None,
            },
        ),
        (
            "if consequent",
            Stmt::If {
                test: Expr::Bool(true),
                consequent: Box::new(Stmt::Expr(missing_private())),
                alternate: None,
            },
        ),
        (
            "if alternate",
            Stmt::If {
                test: Expr::Bool(true),
                consequent: Box::new(Stmt::Empty),
                alternate: Some(Box::new(Stmt::Expr(missing_private()))),
            },
        ),
        (
            "for initializer",
            Stmt::For {
                init: Some(ForInit::Expr(missing_private())),
                test: None,
                update: None,
                body: Box::new(Stmt::Empty),
            },
        ),
        (
            "for test",
            Stmt::For {
                init: None,
                test: Some(missing_private()),
                update: None,
                body: Box::new(Stmt::Empty),
            },
        ),
        (
            "for update",
            Stmt::For {
                init: None,
                test: None,
                update: Some(missing_private()),
                body: Box::new(Stmt::Empty),
            },
        ),
        (
            "for-in head",
            Stmt::ForIn {
                left: ForHead::Expr(missing_private()),
                right: Expr::Number(1.0),
                body: Box::new(Stmt::Empty),
            },
        ),
        (
            "for-in right",
            Stmt::ForIn {
                left: ForHead::Expr(Expr::Identifier("value".into())),
                right: missing_private(),
                body: Box::new(Stmt::Empty),
            },
        ),
        (
            "while test",
            Stmt::While {
                test: missing_private(),
                body: Box::new(Stmt::Empty),
            },
        ),
        (
            "switch discriminant",
            Stmt::Switch {
                discriminant: missing_private(),
                cases: Vec::new(),
            },
        ),
        (
            "switch case test",
            Stmt::Switch {
                discriminant: Expr::Number(1.0),
                cases: vec![SwitchCase {
                    test: Some(missing_private()),
                    consequent: Vec::new(),
                }],
            },
        ),
        (
            "switch case body",
            Stmt::Switch {
                discriminant: Expr::Number(1.0),
                cases: vec![SwitchCase {
                    test: None,
                    consequent: vec![Stmt::Expr(missing_private())],
                }],
            },
        ),
        (
            "try block",
            Stmt::Try {
                block: vec![Stmt::Expr(missing_private())],
                handler: None,
                finalizer: None,
            },
        ),
        (
            "catch pattern",
            Stmt::Try {
                block: Vec::new(),
                handler: Some(CatchClause {
                    param: Some(private_pattern()),
                    body: Vec::new(),
                }),
                finalizer: None,
            },
        ),
        (
            "catch body",
            Stmt::Try {
                block: Vec::new(),
                handler: Some(CatchClause {
                    param: None,
                    body: vec![Stmt::Expr(missing_private())],
                }),
                finalizer: None,
            },
        ),
        (
            "finally block",
            Stmt::Try {
                block: Vec::new(),
                handler: None,
                finalizer: Some(vec![Stmt::Expr(missing_private())]),
            },
        ),
        (
            "with object",
            Stmt::With {
                object: missing_private(),
                body: Box::new(Stmt::Empty),
            },
        ),
        (
            "for declaration pattern",
            Stmt::For {
                init: Some(ForInit::VarDecl(
                    DeclKind::Let,
                    vec![VarDeclarator {
                        pattern: private_pattern(),
                        init: None,
                    }],
                )),
                test: None,
                update: None,
                body: Box::new(Stmt::Empty),
            },
        ),
        (
            "for-in declaration pattern",
            Stmt::ForIn {
                left: ForHead::Decl(DeclKind::Let, private_pattern()),
                right: Expr::Number(1.0),
                body: Box::new(Stmt::Empty),
            },
        ),
        (
            "Annex B for-in declaration pattern",
            Stmt::ForIn {
                left: ForHead::AnnexBVarInit(private_pattern(), Expr::Number(1.0)),
                right: Expr::Number(2.0),
                body: Box::new(Stmt::Empty),
            },
        ),
    ];
    for (label, statement) in cases {
        assert_private_error(label, statement);
    }
}

#[test]
fn private_name_errors_propagate_from_class_and_function_positions() {
    let function_with_bad_pattern = Function {
        params: vec![Param {
            pattern: private_pattern(),
            default: None,
            rest: false,
        }],
        ..Function::default()
    };
    let function_with_bad_default = Function {
        params: vec![Param {
            pattern: Pattern::Identifier("value".into()),
            default: Some(missing_private()),
            rest: false,
        }],
        ..Function::default()
    };
    let cases = vec![
        (
            "class decorator uses the outer private environment",
            Stmt::ClassDecl(Class {
                decorators: vec![missing_private()],
                ..empty_class()
            }),
        ),
        (
            "class heritage uses the outer private environment",
            Stmt::ClassDecl(Class {
                extends: Some(Box::new(missing_private())),
                ..empty_class()
            }),
        ),
        (
            "method decorator",
            class_with(ClassElement::Method {
                key: PropertyKey::Identifier("method".into()),
                function: Function::default(),
                is_static: false,
                decorators: vec![missing_private()],
            }),
        ),
        (
            "method computed key",
            class_with(ClassElement::Method {
                key: computed_private_key(),
                function: Function::default(),
                is_static: false,
                decorators: Vec::new(),
            }),
        ),
        (
            "method body",
            class_with(ClassElement::Method {
                key: PropertyKey::Identifier("method".into()),
                function: Function {
                    body: vec![Stmt::Expr(missing_private())],
                    ..Function::default()
                },
                is_static: false,
                decorators: Vec::new(),
            }),
        ),
        (
            "field decorator",
            class_with(ClassElement::Field {
                key: PropertyKey::Identifier("field".into()),
                initializer: None,
                is_static: false,
                accessor: false,
                decorators: vec![missing_private()],
            }),
        ),
        (
            "field computed key",
            class_with(ClassElement::Field {
                key: computed_private_key(),
                initializer: None,
                is_static: false,
                accessor: false,
                decorators: Vec::new(),
            }),
        ),
        (
            "field initializer",
            class_with(ClassElement::Field {
                key: PropertyKey::Identifier("field".into()),
                initializer: Some(missing_private()),
                is_static: false,
                accessor: false,
                decorators: Vec::new(),
            }),
        ),
        (
            "static block",
            class_with(ClassElement::StaticBlock(vec![Stmt::Expr(
                missing_private(),
            )])),
        ),
        (
            "function parameter pattern",
            Stmt::FunctionDecl(function_with_bad_pattern.clone()),
        ),
        (
            "function parameter default",
            Stmt::FunctionDecl(function_with_bad_default),
        ),
        (
            "function body",
            Stmt::FunctionDecl(Function {
                body: vec![Stmt::Expr(missing_private())],
                ..Function::default()
            }),
        ),
        (
            "object method parameter pattern",
            Stmt::Expr(Expr::Object(vec![ObjectProp::Method {
                key: PropertyKey::Identifier("method".into()),
                function: function_with_bad_pattern,
            }])),
        ),
    ];
    for (label, statement) in cases {
        assert_private_error(label, statement);
    }
}

fn empty_class() -> Class {
    Class {
        name: Some("C".into()),
        extends: None,
        elements: Vec::new(),
        decorators: Vec::new(),
        source_text: SourceText::default(),
    }
}

#[test]
fn private_name_errors_propagate_through_nested_patterns() {
    let binding_patterns = vec![
        (
            "array element nested binding",
            Pattern::Array(vec![Some(ArrayPatternElement {
                pattern: private_pattern(),
                default: None,
                rest: false,
            })]),
        ),
        (
            "object nested binding",
            Pattern::Object(vec![ObjectPatternProp::KeyValue {
                key: PropertyKey::Identifier("key".into()),
                value: private_pattern(),
                default: None,
            }]),
        ),
    ];
    for (label, pattern) in binding_patterns {
        assert_private_error(
            label,
            Stmt::VarDecl(
                DeclKind::Let,
                vec![VarDeclarator {
                    pattern,
                    init: None,
                }],
            ),
        );
    }

    let assignment_patterns = vec![
        (
            "array element nested assignment",
            AssignmentPattern::Array(vec![Some(AssignmentPatternElement {
                pattern: private_assignment_pattern(),
                default: None,
                rest: false,
            })]),
        ),
        (
            "object assignment computed key",
            AssignmentPattern::Object(vec![AssignmentPatternProp::KeyValue {
                key: computed_private_key(),
                value: AssignmentPattern::Target(Box::new(Expr::Identifier("value".into()))),
                default: None,
            }]),
        ),
        (
            "object nested assignment",
            AssignmentPattern::Object(vec![AssignmentPatternProp::KeyValue {
                key: PropertyKey::Identifier("key".into()),
                value: private_assignment_pattern(),
                default: None,
            }]),
        ),
    ];
    for (label, pattern) in assignment_patterns {
        assert_private_error(
            label,
            Stmt::Expr(Expr::DestructureAssign {
                pattern,
                value: Box::new(Expr::Identifier("source".into())),
            }),
        );
    }
}

#[test]
fn private_name_errors_propagate_through_expression_operands() {
    let arrow = |pattern, default| Expr::Arrow {
        params: vec![Param {
            pattern,
            default,
            rest: false,
        }],
        body: ArrowBody::Expr(Box::new(Expr::Number(1.0))),
        is_async: false,
        source_text: SourceText::default(),
    };
    let cases = vec![
        (
            "dynamic import specifier",
            Expr::DynamicImport {
                specifier: Box::new(missing_private()),
                options: None,
                phase: ImportPhase::Evaluation,
            },
        ),
        (
            "tagged template tag",
            Expr::TaggedTemplate {
                tag: Box::new(missing_private()),
                raw: Vec::new(),
                cooked: Vec::new(),
                expressions: Vec::new(),
            },
        ),
        (
            "object property computed key",
            Expr::Object(vec![ObjectProp::KeyValue {
                key: computed_private_key(),
                value: Expr::Number(1.0),
                shorthand: false,
            }]),
        ),
        (
            "object method computed key",
            Expr::Object(vec![ObjectProp::Method {
                key: computed_private_key(),
                function: Function::default(),
            }]),
        ),
        ("arrow parameter pattern", arrow(private_pattern(), None)),
        (
            "arrow parameter default",
            arrow(Pattern::Identifier("value".into()), Some(missing_private())),
        ),
        (
            "binary left operand",
            Expr::Binary {
                op: BinaryOp::Add,
                left: Box::new(missing_private()),
                right: Box::new(Expr::Number(1.0)),
            },
        ),
        (
            "assignment target",
            Expr::Assign {
                op: AssignOp::Assign,
                target: Box::new(missing_private()),
                value: Box::new(Expr::Number(1.0)),
            },
        ),
        (
            "destructuring target",
            Expr::DestructureAssign {
                pattern: private_assignment_pattern(),
                value: Box::new(Expr::Number(1.0)),
            },
        ),
        (
            "conditional test",
            Expr::Conditional {
                test: Box::new(missing_private()),
                consequent: Box::new(Expr::Number(1.0)),
                alternate: Box::new(Expr::Number(2.0)),
            },
        ),
        (
            "conditional consequent",
            Expr::Conditional {
                test: Box::new(Expr::Bool(true)),
                consequent: Box::new(missing_private()),
                alternate: Box::new(Expr::Number(2.0)),
            },
        ),
        (
            "call callee",
            Expr::Call {
                callee: Box::new(missing_private()),
                args: Vec::new(),
            },
        ),
        (
            "member object",
            Expr::Member {
                object: Box::new(missing_private()),
                property: Box::new(Expr::Identifier("key".into())),
                computed: false,
            },
        ),
        (
            "computed member property",
            Expr::Member {
                object: Box::new(Expr::Identifier("value".into())),
                property: Box::new(missing_private()),
                computed: true,
            },
        ),
        (
            "private brand object",
            Expr::PrivateIn {
                name: "known".into(),
                object: Box::new(missing_private()),
            },
        ),
    ];
    for (label, expression) in cases {
        assert_private_error(label, Stmt::Expr(expression));
    }
}

#[test]
fn super_cannot_access_a_private_element() {
    let result = validate_private_early_errors(&Program {
        body: vec![Stmt::Expr(Expr::Member {
            object: Box::new(Expr::Super),
            property: Box::new(Expr::Identifier("#value".into())),
            computed: false,
        })],
    });
    assert_eq!(
        result,
        Err(CompileError::InvalidSyntax(
            "super cannot access a private element"
        ))
    );
}

#[test]
fn non_identifier_member_property_does_not_require_a_private_name() {
    let result = validate_private_early_errors(&Program {
        body: vec![Stmt::Expr(Expr::Member {
            object: Box::new(Expr::Identifier("value".into())),
            property: Box::new(Expr::Number(1.0)),
            computed: false,
        })],
    });
    assert_eq!(result, Ok(()));
}
