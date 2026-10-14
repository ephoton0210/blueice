// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! NamedEvaluation survives a wrapper without shadowing the enclosing binding.

use super::*;
use crate::parser::{
    FunctionBodyItem, FunctionElseBranch, NestedFunctionBody, Parameter, VariableDeclaration,
};

pub(super) fn contextual_name(
    module: &Module,
    expression: &ClassExpression,
    tokens: &[Token],
) -> String {
    let quote = |name: &str| serde_json::to_string(name).expect("a name is a JSON string");
    for (declaration, _) in super::super::super::runtime_declarations(&module.declarations) {
        match declaration {
            Declaration::Variable(variable)
                if initializer(module, expression, &variable.initializer) =>
            {
                return quote(&variable.name);
            }
            Declaration::Function(function) => {
                if let Some(name) = parameter_name(module, expression, &function.parameters)
                    .or_else(|| variable_name(module, expression, &function.locals))
                {
                    return quote(&name);
                }
            }
            _ => {}
        }
    }
    for class in module.classes() {
        for member in &class.members {
            if let Some(field) = &member.field {
                if field
                    .initializer
                    .as_ref()
                    .is_some_and(|value| initializer(module, expression, value))
                {
                    if member.key.first().is_some_and(|token| token.is("[")) {
                        let key = super::super::super::computed_fields::captured_key_name(
                            tokens,
                            member.key[0].start,
                        );
                        return format!("(typeof {key} === \"symbol\" ? ({key}.description === undefined ? \"\" : \"[\" + {key}.description + \"]\") : String({key}))");
                    }
                    return quote(&field.name);
                }
            }
            let (parameters, body) = if let Some(method) = &member.method {
                (method.parameters.as_slice(), method.body.as_deref())
            } else if let Some(constructor) = &member.constructor {
                (
                    constructor.parameters.as_slice(),
                    constructor.body.as_deref(),
                )
            } else if let Some(accessor) = &member.accessor {
                (
                    accessor.parameters.as_slice(),
                    Some(accessor.body.as_slice()),
                )
            } else if let Some(block) = &member.static_block {
                (&[][..], Some(block.body.as_slice()))
            } else {
                continue;
            };
            if let Some(name) = parameter_name(module, expression, parameters)
                .or_else(|| body.and_then(|body| body_name(module, expression, body)))
            {
                return quote(&name);
            }
        }
    }
    for function in module.nested_functions.values() {
        if let Some(name) = parameter_name(module, expression, &function.parameters) {
            return quote(&name);
        }
        if let NestedFunctionBody::Block { locals, .. } = &function.body {
            if let Some(name) = variable_name(module, expression, locals) {
                return quote(&name);
            }
        }
    }
    let end = tokens.partition_point(|token| token.start < expression.class.span.start);
    if end >= 2
        && tokens[end - 1].is("=")
        && tokens[end - 2].kind == crate::TokenKind::Identifier
        && (end < 3 || !tokens[end - 3].is("."))
    {
        return quote(&tokens[end - 2].text);
    }
    quote("")
}

fn initializer(module: &Module, expression: &ClassExpression, tokens: &[Token]) -> bool {
    let tokens = tokens
        .iter()
        .filter(|token| {
            !module.edits.iter().any(|edit| {
                edit.replacement.is_empty() && edit.start <= token.start && edit.end >= token.end
            })
        })
        .collect::<Vec<_>>();
    let mut tokens = tokens.as_slice();
    while tokens.first().is_some_and(|token| token.is("("))
        && tokens.last().is_some_and(|token| token.is(")"))
    {
        tokens = &tokens[1..tokens.len() - 1];
    }
    tokens
        .first()
        .is_some_and(|token| token.start == expression.class.span.start)
        && tokens
            .last()
            .is_some_and(|token| token.end == expression.class.span.end)
}

fn variable_name(
    module: &Module,
    expression: &ClassExpression,
    variables: &[VariableDeclaration],
) -> Option<String> {
    variables
        .iter()
        .find(|variable| initializer(module, expression, &variable.initializer))
        .map(|variable| variable.name.clone())
}

fn parameter_name(
    module: &Module,
    expression: &ClassExpression,
    parameters: &[Parameter],
) -> Option<String> {
    parameters
        .iter()
        .find(|parameter| {
            parameter
                .default
                .as_ref()
                .is_some_and(|value| initializer(module, expression, value))
        })
        .map(|parameter| parameter.name.clone())
}

fn body_name(
    module: &Module,
    expression: &ClassExpression,
    body: &[FunctionBodyItem],
) -> Option<String> {
    body.iter().find_map(|item| match item {
        FunctionBodyItem::Variable(variable) => {
            variable_name(module, expression, std::slice::from_ref(variable))
        }
        FunctionBodyItem::Function(function) => {
            parameter_name(module, expression, &function.parameters)
                .or_else(|| variable_name(module, expression, &function.locals))
        }
        FunctionBodyItem::If(statement) => if_name(module, expression, statement),
        FunctionBodyItem::While(statement) => body_name(module, expression, &statement.body),
        FunctionBodyItem::Try(statement) => body_name(module, expression, &statement.block)
            .or_else(|| {
                statement
                    .handler
                    .as_ref()
                    .and_then(|handler| body_name(module, expression, &handler.body))
            })
            .or_else(|| {
                statement
                    .finalizer
                    .as_ref()
                    .and_then(|body| body_name(module, expression, body))
            }),
        _ => None,
    })
}

fn if_name(
    module: &Module,
    expression: &ClassExpression,
    statement: &crate::parser::FunctionIfStatement,
) -> Option<String> {
    body_name(module, expression, &statement.consequent).or_else(|| {
        statement
            .alternate
            .as_ref()
            .and_then(|branch| match branch {
                FunctionElseBranch::Braced(body) => body_name(module, expression, body),
                FunctionElseBranch::ElseIf(branch) => if_name(module, expression, branch),
            })
    })
}
