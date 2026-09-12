// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

/// Performs the grammar's lexical PrivateEnvironment checks without lowering
/// the program.  The parser uses this for parse-only Test262 cases; the
/// compiler repeats the lookup while assigning hidden owner bindings.
pub(crate) fn validate_private_early_errors(program: &Program) -> Result<(), CompileError> {
    validate_private_statements(&program.body, &HashSet::new())
}

fn missing_private_name() -> CompileError {
    CompileError::InvalidSyntax("private name is not declared in an enclosing class")
}

fn validate_private_name(name: &str, names: &HashSet<String>) -> Result<(), CompileError> {
    names
        .contains(name)
        .then_some(())
        .ok_or_else(missing_private_name)
}

fn validate_private_statements(
    statements: &[Stmt],
    names: &HashSet<String>,
) -> Result<(), CompileError> {
    for statement in statements {
        validate_private_statement(statement, names)?;
    }
    Ok(())
}

fn validate_private_statement(
    statement: &Stmt,
    names: &HashSet<String>,
) -> Result<(), CompileError> {
    match statement {
        Stmt::Empty | Stmt::Break(_) | Stmt::Continue(_) | Stmt::ClassPrivateBrand(_) => Ok(()),
        Stmt::Expr(expr) | Stmt::Throw(expr) => validate_private_expression(expr, names),
        Stmt::Block(statements) => validate_private_statements(statements, names),
        Stmt::VarDecl(_, declarations) => {
            for declaration in declarations {
                validate_private_pattern(&declaration.pattern, names)?;
                if let Some(initializer) = &declaration.init {
                    validate_private_expression(initializer, names)?;
                }
            }
            Ok(())
        }
        Stmt::If {
            test,
            consequent,
            alternate,
        } => {
            validate_private_expression(test, names)?;
            validate_private_statement(consequent, names)?;
            if let Some(alternate) = alternate {
                validate_private_statement(alternate, names)?;
            }
            Ok(())
        }
        Stmt::For {
            init,
            test,
            update,
            body,
        } => {
            if let Some(init) = init {
                validate_private_for_init(init, names)?;
            }
            for expression in [test.as_ref(), update.as_ref()].into_iter().flatten() {
                validate_private_expression(expression, names)?;
            }
            validate_private_statement(body, names)
        }
        Stmt::ForIn { left, right, body }
        | Stmt::ForOf {
            left, right, body, ..
        } => {
            validate_private_for_head(left, names)?;
            validate_private_expression(right, names)?;
            validate_private_statement(body, names)
        }
        Stmt::While { test, body } | Stmt::DoWhile { body, test } => {
            validate_private_expression(test, names)?;
            validate_private_statement(body, names)
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            validate_private_expression(discriminant, names)?;
            for case in cases {
                if let Some(test) = &case.test {
                    validate_private_expression(test, names)?;
                }
                validate_private_statements(&case.consequent, names)?;
            }
            Ok(())
        }
        Stmt::Labelled { item, .. } => validate_private_statement(item, names),
        Stmt::Return(value) => value
            .as_ref()
            .map_or(Ok(()), |value| validate_private_expression(value, names)),
        Stmt::Try {
            block,
            handler,
            finalizer,
        } => {
            validate_private_statements(block, names)?;
            if let Some(handler) = handler {
                if let Some(param) = &handler.param {
                    validate_private_pattern(param, names)?;
                }
                validate_private_statements(&handler.body, names)?;
            }
            if let Some(finalizer) = finalizer {
                validate_private_statements(finalizer, names)?;
            }
            Ok(())
        }
        Stmt::With { object, body } => {
            validate_private_expression(object, names)?;
            validate_private_statement(body, names)
        }
        Stmt::FunctionDecl(function) | Stmt::ModuleDefaultFunction { function, .. } => {
            validate_private_function(function, names)
        }
        Stmt::ClassDecl(class) => validate_private_class(class, names),
        Stmt::ClassField(statement) => validate_private_statement(statement, names),
    }
}

fn validate_private_for_init(init: &ForInit, names: &HashSet<String>) -> Result<(), CompileError> {
    match init {
        ForInit::Expr(expression) => validate_private_expression(expression, names),
        ForInit::VarDecl(_, declarations) => declarations.iter().try_for_each(|declaration| {
            validate_private_pattern(&declaration.pattern, names)?;
            declaration.init.as_ref().map_or(Ok(()), |expression| {
                validate_private_expression(expression, names)
            })
        }),
    }
}

fn validate_private_for_head(head: &ForHead, names: &HashSet<String>) -> Result<(), CompileError> {
    match head {
        ForHead::Decl(_, pattern) => validate_private_pattern(pattern, names),
        ForHead::AnnexBVarInit(pattern, initializer) => {
            validate_private_pattern(pattern, names)?;
            validate_private_expression(initializer, names)
        }
        ForHead::Assignment(pattern) => validate_private_assignment_pattern(pattern, names),
        ForHead::Expr(expression) => validate_private_expression(expression, names),
    }
}

fn validate_private_class(class: &Class, names: &HashSet<String>) -> Result<(), CompileError> {
    // ClassHeritage is evaluated in the *outer* PrivateEnvironment.  The
    // class's own names become visible only after this point.
    if let Some(base) = &class.extends {
        validate_private_expression(base, names)?;
    }
    let declarations = class_private_declarations(class)?;
    let mut class_names = names.clone();
    class_names.extend(declarations.into_iter().map(|(name, _)| name));
    for element in &class.elements {
        match element {
            ClassElement::Method { key, function, .. }
            | ClassElement::Accessor { key, function, .. } => {
                validate_private_key(key, &class_names)?;
                validate_private_function(function, &class_names)?;
            }
            ClassElement::Field {
                key, initializer, ..
            } => {
                validate_private_key(key, &class_names)?;
                if let Some(initializer) = initializer {
                    validate_private_expression(initializer, &class_names)?;
                }
            }
            ClassElement::StaticBlock(statements) => {
                validate_private_statements(statements, &class_names)?;
            }
        }
    }
    Ok(())
}

fn validate_private_function(
    function: &Function,
    names: &HashSet<String>,
) -> Result<(), CompileError> {
    for parameter in &function.params {
        validate_private_pattern(&parameter.pattern, names)?;
        if let Some(default) = &parameter.default {
            validate_private_expression(default, names)?;
        }
    }
    validate_private_statements(&function.body, names)
}

fn validate_private_pattern(
    pattern: &Pattern,
    names: &HashSet<String>,
) -> Result<(), CompileError> {
    match pattern {
        Pattern::Identifier(_) => Ok(()),
        Pattern::Array(elements) => elements.iter().flatten().try_for_each(|element| {
            validate_private_pattern(&element.pattern, names)?;
            element.default.as_ref().map_or(Ok(()), |expression| {
                validate_private_expression(expression, names)
            })
        }),
        Pattern::Object(properties) => properties.iter().try_for_each(|property| match property {
            ObjectPatternProp::KeyValue {
                key,
                value,
                default,
            } => {
                validate_private_key(key, names)?;
                validate_private_pattern(value, names)?;
                default.as_ref().map_or(Ok(()), |expression| {
                    validate_private_expression(expression, names)
                })
            }
            ObjectPatternProp::Rest(pattern) => validate_private_pattern(pattern, names),
        }),
    }
}

fn validate_private_assignment_pattern(
    pattern: &AssignmentPattern,
    names: &HashSet<String>,
) -> Result<(), CompileError> {
    match pattern {
        AssignmentPattern::Target(expression) => validate_private_expression(expression, names),
        AssignmentPattern::Array(elements) => elements.iter().flatten().try_for_each(|element| {
            validate_private_assignment_pattern(&element.pattern, names)?;
            element.default.as_ref().map_or(Ok(()), |expression| {
                validate_private_expression(expression, names)
            })
        }),
        AssignmentPattern::Object(properties) => {
            properties.iter().try_for_each(|property| match property {
                AssignmentPatternProp::KeyValue {
                    key,
                    value,
                    default,
                } => {
                    validate_private_key(key, names)?;
                    validate_private_assignment_pattern(value, names)?;
                    default.as_ref().map_or(Ok(()), |expression| {
                        validate_private_expression(expression, names)
                    })
                }
                AssignmentPatternProp::Rest(pattern) => {
                    validate_private_assignment_pattern(pattern, names)
                }
            })
        }
    }
}

fn validate_private_key(key: &PropertyKey, names: &HashSet<String>) -> Result<(), CompileError> {
    if let PropertyKey::Computed(expression) = key {
        validate_private_expression(expression, names)
    } else {
        Ok(())
    }
}

fn validate_private_expression(expr: &Expr, names: &HashSet<String>) -> Result<(), CompileError> {
    match expr {
        Expr::Number(_)
        | Expr::BigInt(_)
        | Expr::String(_)
        | Expr::Bool(_)
        | Expr::Null
        | Expr::This
        | Expr::Identifier(_)
        | Expr::RegExp { .. }
        | Expr::Super
        | Expr::NewTarget
        | Expr::ImportMeta => Ok(()),
        Expr::Parenthesized(expression)
        | Expr::Await(expression)
        | Expr::DynamicImport(expression)
        | Expr::Unary {
            arg: expression, ..
        }
        | Expr::Update {
            arg: expression, ..
        } => validate_private_expression(expression, names),
        Expr::Template { expressions, .. } => expressions
            .iter()
            .try_for_each(|expression| validate_private_expression(expression, names)),
        Expr::TaggedTemplate {
            tag, expressions, ..
        } => {
            validate_private_expression(tag, names)?;
            expressions
                .iter()
                .try_for_each(|expression| validate_private_expression(expression, names))
        }
        Expr::Array(elements) => elements
            .iter()
            .flatten()
            .try_for_each(|element| match element {
                ArrayElement::Normal(expression) | ArrayElement::Spread(expression) => {
                    validate_private_expression(expression, names)
                }
            }),
        Expr::Object(properties) => properties.iter().try_for_each(|property| match property {
            ObjectProp::KeyValue { key, value, .. } => {
                validate_private_key(key, names)?;
                validate_private_expression(value, names)
            }
            ObjectProp::Spread(expression) => validate_private_expression(expression, names),
            ObjectProp::Method { key, function } | ObjectProp::Accessor { key, function, .. } => {
                validate_private_key(key, names)?;
                validate_private_function(function, names)
            }
        }),
        Expr::Function(function) => validate_private_function(function, names),
        Expr::Class(class) => validate_private_class(class, names),
        Expr::Yield { value, .. } => value.as_deref().map_or(Ok(()), |expression| {
            validate_private_expression(expression, names)
        }),
        Expr::Arrow { params, body, .. } => {
            for parameter in params {
                validate_private_pattern(&parameter.pattern, names)?;
                if let Some(default) = &parameter.default {
                    validate_private_expression(default, names)?;
                }
            }
            match body {
                ArrowBody::Expr(expression) => validate_private_expression(expression, names),
                ArrowBody::Block(statements) => validate_private_statements(statements, names),
            }
        }
        Expr::Binary { left, right, .. } | Expr::Logical { left, right, .. } => {
            validate_private_expression(left, names)?;
            validate_private_expression(right, names)
        }
        Expr::Sequence(expressions) => expressions
            .iter()
            .try_for_each(|expression| validate_private_expression(expression, names)),
        Expr::Assign { target, value, .. } => {
            validate_private_expression(target, names)?;
            validate_private_expression(value, names)
        }
        Expr::DestructureAssign { pattern, value } => {
            validate_private_assignment_pattern(pattern, names)?;
            validate_private_expression(value, names)
        }
        Expr::Conditional {
            test,
            consequent,
            alternate,
        } => {
            validate_private_expression(test, names)?;
            validate_private_expression(consequent, names)?;
            validate_private_expression(alternate, names)
        }
        Expr::Call { callee, args }
        | Expr::OptionalCall { callee, args }
        | Expr::New { callee, args } => {
            validate_private_expression(callee, names)?;
            args.iter().try_for_each(|argument| match argument {
                Argument::Normal(expression) | Argument::Spread(expression) => {
                    validate_private_expression(expression, names)
                }
            })
        }
        Expr::Member {
            object,
            property,
            computed,
        }
        | Expr::OptionalMember {
            object,
            property,
            computed,
        } => {
            validate_private_expression(object, names)?;
            if *computed {
                validate_private_expression(property, names)?;
            }
            if !*computed {
                if let Expr::Identifier(name) = property.as_ref() {
                    if let Some(name) = name.strip_prefix('#') {
                        if matches!(object.as_ref(), Expr::Super) {
                            return Err(CompileError::InvalidSyntax(
                                "super cannot access a private element",
                            ));
                        }
                        validate_private_name(name, names)?;
                    }
                }
            }
            Ok(())
        }
        Expr::PrivateIn { name, object } => {
            validate_private_expression(object, names)?;
            validate_private_name(name, names)
        }
    }
}
