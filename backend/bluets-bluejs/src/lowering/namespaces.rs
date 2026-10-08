// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Namespace/enum runtime objects and their binding contexts.

use super::*;

/// An enum as `var E = (function (E) { E[E["A"] = 0] = "A"; ...; return E; })(E || {});`.
/// One statement per declaration keeps a root statement and its source span one
/// to one, and merged declarations reuse the object through `E || {}`. A
/// `const enum` is lowered like any other: its object exists at run time and
/// every use reads it, which behaves as the inlined emit does. An ambient enum
/// has no runtime form (`None`), except an ambient `const enum`, whose uses
/// would need their values inlined, which the bridge does not do.
pub(super) fn lower_enum(
    module: &Module,
    declaration: &blueice_bluets::EnumDeclaration,
    evaluations: &mut std::vec::IntoIter<blueice_bluets::EvaluatedEnum>,
) -> Result<Option<bluejs::Stmt>, BridgeError> {
    let Some(function) = lower_enum_function(module, declaration, evaluations)? else {
        return Ok(None);
    };
    let name = declaration.name.clone();
    let call = iife(function, plain_argument(&name));
    Ok(Some(bluejs::Stmt::VarDecl(
        bluejs::DeclKind::Var,
        vec![bluejs::VarDeclarator {
            pattern: bluejs::Pattern::Identifier(name),
            init: Some(call),
        }],
    )))
}

/// The function an enum declaration builds its object in.
pub(super) fn lower_enum_function(
    module: &Module,
    declaration: &blueice_bluets::EnumDeclaration,
    evaluations: &mut std::vec::IntoIter<blueice_bluets::EvaluatedEnum>,
) -> Result<Option<bluejs::Function>, BridgeError> {
    let evaluation = evaluations
        .next()
        .expect("every enum declaration was evaluated");
    if declaration.declared {
        if declaration.is_const {
            return Err(unsupported(
                declaration.span.clone(),
                "an ambient const enum needs its uses inlined, which the direct bridge does not do",
            ));
        }
        return Ok(None);
    }
    let name = declaration.name.clone();
    let string = |text: &str| bluejs::Expr::String(bluejs::JsString::from(text));
    let object = || bluejs::Expr::Identifier(name.clone());
    let member_key = |member: &str| bluejs::Expr::Member {
        object: Box::new(object()),
        property: Box::new(string(member)),
        computed: true,
    };
    let mut statements = Vec::new();
    for (member, evaluated) in declaration.members.iter().zip(&evaluation.members) {
        let statement = match &evaluated.value {
            Some(blueice_bluets::EnumValue::Number(number)) => {
                // E[E["A"] = 0] = "A"
                let forward = assign_expr(member_key(&member.name), bluejs::Expr::Number(*number));
                assign_expr(
                    bluejs::Expr::Member {
                        object: Box::new(object()),
                        property: Box::new(forward),
                        computed: true,
                    },
                    string(&member.name),
                )
            }
            Some(blueice_bluets::EnumValue::Text(text)) => {
                assign_expr(member_key(&member.name), string(text))
            }
            None => {
                let value = member
                    .initializer
                    .as_deref()
                    .map(|tokens| ExpressionLowerer::for_module(module, tokens).parse())
                    .transpose()?
                    .unwrap_or(bluejs::Expr::Identifier("undefined".to_string()));
                let forward = assign_expr(member_key(&member.name), value);
                assign_expr(
                    bluejs::Expr::Member {
                        object: Box::new(object()),
                        property: Box::new(forward),
                        computed: true,
                    },
                    string(&member.name),
                )
            }
        };
        statements.push(bluejs::Stmt::Expr(statement));
    }
    statements.push(bluejs::Stmt::Return(Some(object())));
    Ok(Some(function_over(&name, statements)))
}

pub(super) fn assign_expr(target: bluejs::Expr, value: bluejs::Expr) -> bluejs::Expr {
    bluejs::Expr::Assign {
        op: bluejs::AssignOp::Assign,
        target: Box::new(target),
        value: Box::new(value),
    }
}

pub(super) fn identifier_expr(name: &str) -> bluejs::Expr {
    bluejs::Expr::Identifier(name.to_string())
}

pub(super) fn property_expr(object: &str, property: &str) -> bluejs::Expr {
    bluejs::Expr::Member {
        object: Box::new(identifier_expr(object)),
        property: Box::new(identifier_expr(property)),
        computed: false,
    }
}

/// `function (name) { .. }`, the function a namespace or enum body runs in.
pub(super) fn function_over(name: &str, body: Vec<bluejs::Stmt>) -> bluejs::Function {
    bluejs::Function {
        name: None,
        params: vec![bluejs::Param {
            pattern: bluejs::Pattern::Identifier(name.to_string()),
            default: None,
            rest: false,
        }],
        body,
        generator: false,
        is_async: false,
        source_text: Default::default(),
    }
}

pub(super) fn iife(function: bluejs::Function, argument: bluejs::Expr) -> bluejs::Expr {
    bluejs::Expr::Call {
        callee: Box::new(bluejs::Expr::Parenthesized(Box::new(
            bluejs::Expr::Function(function),
        ))),
        args: vec![bluejs::Argument::Normal(argument)],
    }
}

/// `name || (name = {})`
pub(super) fn plain_argument(name: &str) -> bluejs::Expr {
    bluejs::Expr::Logical {
        op: bluejs::LogicalOp::Or,
        left: Box::new(identifier_expr(name)),
        right: Box::new(bluejs::Expr::Parenthesized(Box::new(assign_expr(
            identifier_expr(name),
            bluejs::Expr::Object(Vec::new()),
        )))),
    }
}

/// `name = parent.name || (parent.name = {})`
pub(super) fn member_argument(parent: &str, name: &str) -> bluejs::Expr {
    assign_expr(
        identifier_expr(name),
        bluejs::Expr::Logical {
            op: bluejs::LogicalOp::Or,
            left: Box::new(property_expr(parent, name)),
            right: Box::new(bluejs::Expr::Parenthesized(Box::new(assign_expr(
                property_expr(parent, name),
                bluejs::Expr::Object(Vec::new()),
            )))),
        },
    )
}

/// Where a namespace is declared.
pub(super) enum NamespaceScope {
    Module,
    Namespace { parent: String },
}

/// Lowers namespaces the way TypeScript emits them: a function over the
/// namespace object, called with the object or a new one, in which an exported
/// variable is a property and every reference to it a property read.
pub(super) struct NamespaceLowering<'a> {
    pub(super) module: &'a Module,
    pub(super) tokens: &'a [Token],
    pub(super) exports: NamespaceExports,
    pub(super) define_class_fields: bool,
}

impl NamespaceLowering<'_> {
    /// The statements for one namespace declaration, none when it has nothing at
    /// run time or is ambient.
    pub(super) fn namespace(
        &self,
        namespace: &blueice_bluets::NamespaceDeclaration,
        scope: &NamespaceScope,
        declared: &mut BTreeSet<String>,
        parent_path: &str,
        outer: &BTreeMap<String, String>,
    ) -> Result<Vec<bluejs::Stmt>, BridgeError> {
        if namespace.declared || !has_runtime_values(&namespace.body) {
            return Ok(Vec::new());
        }
        let mut chain = vec![namespace];
        while let [Declaration::Namespace(inner)] = chain[chain.len() - 1].body.as_slice() {
            if !inner.implicit {
                break;
            }
            chain.push(inner);
        }
        let innermost = chain[chain.len() - 1];
        let first = declared.insert(namespace.name.clone());
        let param = innermost.name.clone();
        let own_path = chain.iter().fold(parent_path.to_string(), |path, segment| {
            if path.is_empty() {
                segment.name.clone()
            } else {
                format!("{path}.{}", segment.name)
            }
        });
        let references = self
            .exports
            .references(
                self.module,
                self.tokens,
                &BodyInput {
                    own_path: &own_path,
                    param: &param,
                    namespace,
                    innermost,
                    outer,
                },
            )
            .map_err(from_diagnostic)?;
        let mut statements = self.body(&innermost.body, &param, &own_path, &references)?;
        // Wrap the body in one function per segment, innermost first.
        for index in (0..chain.len()).rev() {
            let name = &chain[index].name;
            let argument = if index == 0 {
                match scope {
                    NamespaceScope::Namespace { parent } if namespace.exported => {
                        member_argument(parent, name)
                    }
                    _ => plain_argument(name),
                }
            } else {
                member_argument(&chain[index - 1].name, name)
            };
            let call = bluejs::Stmt::Expr(iife(function_over(name, statements), argument));
            if index == 0 {
                statements = Vec::new();
                if first {
                    let keyword = match scope {
                        NamespaceScope::Module => bluejs::DeclKind::Var,
                        NamespaceScope::Namespace { .. } => bluejs::DeclKind::Let,
                    };
                    statements.push(bluejs::Stmt::VarDecl(
                        keyword,
                        vec![bluejs::VarDeclarator {
                            pattern: bluejs::Pattern::Identifier(name.clone()),
                            init: None,
                        }],
                    ));
                }
                statements.push(call);
            } else {
                statements = vec![
                    bluejs::Stmt::VarDecl(
                        bluejs::DeclKind::Var,
                        vec![bluejs::VarDeclarator {
                            pattern: bluejs::Pattern::Identifier(name.clone()),
                            init: None,
                        }],
                    ),
                    call,
                ];
            }
        }
        Ok(statements)
    }

    pub(super) fn body(
        &self,
        body: &[Declaration],
        param: &str,
        own_path: &str,
        references: &BTreeMap<String, String>,
    ) -> Result<Vec<bluejs::Stmt>, BridgeError> {
        let module = self.module;
        let evaluations = evaluate_enums_in(body);
        let mut evaluations = evaluations.into_iter();
        let mut statements = Vec::new();
        let mut declared_here: BTreeSet<String> = BTreeSet::new();
        let export = |name: &str| {
            bluejs::Stmt::Expr(assign_expr(
                property_expr(param, name),
                identifier_expr(name),
            ))
        };
        for declaration in body {
            let rewritten =
                rewrite_declaration(declaration, references).map_err(from_diagnostic)?;
            match &rewritten {
                Declaration::Variable(variable) if !variable.declared => {
                    if variable.exported {
                        if !variable.initializer.is_empty() {
                            let value =
                                ExpressionLowerer::for_module(module, &variable.initializer)
                                    .parse()?;
                            statements.push(bluejs::Stmt::Expr(assign_expr(
                                property_expr(param, &variable.name),
                                value,
                            )));
                        }
                    } else {
                        statements.push(lower_variable(module, variable)?);
                    }
                }
                Declaration::Function(function) if !function.declared && !function.overload => {
                    statements.push(lower_function(module, function)?);
                    declared_here.insert(function.name.clone());
                    if function.exported {
                        statements.push(export(&function.name));
                    }
                }
                Declaration::Class(class) => {
                    statements.push(lower_class(module, class, self.define_class_fields)?);
                    declared_here.insert(class.name.clone());
                    if class.exported {
                        statements.push(export(&class.name));
                    }
                }
                Declaration::Enum(declaration) => {
                    if let Some(function) =
                        lower_enum_function(module, declaration, &mut evaluations)?
                    {
                        let name = &declaration.name;
                        if declared_here.insert(name.clone()) {
                            statements.push(bluejs::Stmt::VarDecl(
                                bluejs::DeclKind::Let,
                                vec![bluejs::VarDeclarator {
                                    pattern: bluejs::Pattern::Identifier(name.clone()),
                                    init: None,
                                }],
                            ));
                        }
                        let argument = if declaration.exported {
                            member_argument(param, name)
                        } else {
                            plain_argument(name)
                        };
                        statements.push(bluejs::Stmt::Expr(iife(function, argument)));
                    } else {
                        // An ambient enum has no run-time form.
                    }
                }
                Declaration::Namespace(inner) => {
                    statements.extend(self.namespace(
                        inner,
                        &NamespaceScope::Namespace {
                            parent: param.to_string(),
                        },
                        &mut declared_here,
                        own_path,
                        references,
                    )?);
                }
                Declaration::Raw(raw) => {
                    statements.push(bluejs::Stmt::Expr(
                        ExpressionLowerer::for_module(module, &raw.tokens).parse()?,
                    ));
                }
                Declaration::TypeAlias(_) | Declaration::Interface(_) => {}
                Declaration::Variable(_) | Declaration::Function(_) => {}
                other => {
                    return Err(unsupported(
                        other.span().clone(),
                        "this declaration cannot appear in a namespace body lowered directly",
                    ));
                }
            }
        }
        Ok(statements)
    }
}
