// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Public/protected members participate; private implementation details do not.
use super::*;
use crate::parser::{ClassDeclaration, Visibility};

pub(super) fn check(module: &Module, class: &ClassDeclaration, out: &mut Vec<Diagnostic>) {
    for member in &class.members {
        if member.key.first().is_some_and(|t| t.is("["))
            && member
                .key
                .get(1)
                .is_some_and(|t| t.kind != TokenKind::String && t.kind != TokenKind::Number)
        {
            out.push(
                Diagnostic::error(
                    DiagnosticCode::TypeMismatch,
                    span(module, &member.key),
                    "isolated computed names require an explicit declaration identity",
                )
                .with_typescript(9038, Vec::new()),
            );
        }
        if let Some(field) = &member.field {
            if field.visibility != Visibility::Private
                && !field.name.starts_with('#')
                && field.annotation.is_none()
            {
                if let Some(initializer) = &field.initializer {
                    if let Some((code, location)) = literals::failure(module, initializer) {
                        let target = Target::new(field.name_span.clone(), &field.name, 9012, 9029);
                        out.push(issue(
                            target,
                            if code == 9013 {
                                field.name_span.clone()
                            } else {
                                location
                            },
                            if code == 9013 { 9012 } else { code },
                        ));
                    }
                }
            }
        }
        if let Some(constructor) = &member.constructor {
            parameters(module, &constructor.parameters, out);
        }
        if let Some(method) = &member.method {
            if method.visibility == Visibility::Private || method.name.starts_with('#') {
                continue;
            }
            parameters(module, &method.parameters, out);
            let Some(body) = method.body.as_ref() else {
                continue;
            };
            if method.return_type.is_none()
                && !class.method_groups.iter().any(|group| {
                    group.name == method.name
                        && group.is_static == method.is_static
                        && !group.signature_member_indices.is_empty()
                })
            {
                returns(
                    module,
                    &returned(body),
                    Target::new(method.name_span.clone(), &method.name, 9008, 9034),
                    out,
                );
            }
        }
        if let Some(accessor) = &member.accessor {
            if accessor.visibility == Visibility::Private || !accessor.getter {
                continue;
            }
            let annotated = class
                .members
                .iter()
                .filter_map(|member| member.accessor.as_ref())
                .any(|other| {
                    other.name == accessor.name
                        && other.is_static == accessor.is_static
                        && (other.return_type.is_some()
                            || other.parameters.iter().any(|p| p.annotation.is_some()))
                });
            if !annotated {
                returns(
                    module,
                    &returned(&accessor.body),
                    Target::new(accessor.name_span.clone(), &accessor.name, 9009, 9032),
                    out,
                );
            }
        }
    }
}

fn returned(body: &[crate::parser::FunctionBodyItem]) -> Vec<Vec<Token>> {
    use crate::parser::{
        FunctionBodyItem as Item, FunctionElseBranch as Else, FunctionIfStatement,
    };
    enum Work<'a> {
        Body(&'a [Item]),
        If(&'a FunctionIfStatement),
    }
    let mut work = vec![Work::Body(body)];
    let mut returned = Vec::new();
    while let Some(item) = work.pop() {
        match item {
            Work::If(branch) => {
                if let Some(alternate) = &branch.alternate {
                    match alternate {
                        Else::Braced(body) => work.push(Work::Body(body)),
                        Else::ElseIf(branch) => work.push(Work::If(branch)),
                    }
                }
                work.push(Work::Body(&branch.consequent));
            }
            Work::Body(body) => {
                for item in body {
                    match item {
                        Item::Return { tokens, .. } => returned.push(tokens.clone()),
                        Item::If(branch) => work.push(Work::If(branch)),
                        Item::While(loop_) => work.push(Work::Body(&loop_.body)),
                        Item::Try(statement) => {
                            work.push(Work::Body(&statement.block));
                            if let Some(handler) = &statement.handler {
                                work.push(Work::Body(&handler.body));
                            }
                            if let Some(finalizer) = &statement.finalizer {
                                work.push(Work::Body(finalizer));
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }
    returned
}
