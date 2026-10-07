// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Decorator call and replacement messages from retained syntax.
use super::*;
use crate::{ClassDeclaration, ClassMemberShell, Decorator, Type};

pub(super) fn refine(project: &Project, diagnostic: &mut Diagnostic) {
    let Some(counterpart) = &diagnostic.typescript else {
        return;
    };
    if !matches!(counterpart.code, 1238 | 1239 | 1240 | 1241 | 1270) {
        return;
    }
    let Some(module) = project.modules.get(&counterpart.span.module) else {
        return;
    };
    fn owner<'a>(
        items: &'a [Declaration],
        span: &crate::SourceSpan,
    ) -> Option<(
        &'a ClassDeclaration,
        Option<&'a ClassMemberShell>,
        &'a Decorator,
    )> {
        let matches = |decorator: &Decorator| {
            decorator.span.start <= span.start && decorator.span.end >= span.end
        };
        for item in items {
            match item {
                Declaration::Class(class) => {
                    if let Some(decorator) =
                        class.decorators.iter().find(|decorator| matches(decorator))
                    {
                        return Some((class, None, decorator));
                    }
                    for member in &class.members {
                        if let Some(decorator) = member
                            .decorators
                            .iter()
                            .find(|decorator| matches(decorator))
                        {
                            return Some((class, Some(member), decorator));
                        }
                        let parameters = member
                            .method
                            .as_ref()
                            .map(|method| &method.parameters)
                            .or_else(|| member.constructor.as_ref().map(|ctor| &ctor.parameters))
                            .or_else(|| {
                                member
                                    .accessor
                                    .as_ref()
                                    .map(|accessor| &accessor.parameters)
                            });
                        if let Some(decorator) = parameters
                            .into_iter()
                            .flatten()
                            .flat_map(|parameter| &parameter.decorators)
                            .find(|decorator| matches(decorator))
                        {
                            return Some((class, Some(member), decorator));
                        }
                    }
                }
                Declaration::Namespace(ns) => {
                    if let Some(owner) = owner(&ns.body, span) {
                        return Some(owner);
                    }
                }
                _ => {}
            }
        }
        None
    }
    let Some((class, member, decorator)) = owner(&module.declarations, &counterpart.span) else {
        return;
    };
    let Some(name) = decorator.tokens.first().map(|token| token.text.as_str()) else {
        return;
    };
    let function = module.declarations.iter().find_map(|item| match item {
        Declaration::Function(function) if function.name == name => Some(function),
        _ => None,
    });
    if counterpart.code == 1270 {
        let Some(function) = function else { return };
        let result = function.return_type.as_ref().unwrap_or(&Type::Any);
        let expected = if let Some(member) = member {
            if let Some(field) = &member.field {
                let value = super::super::type_text::render_in(
                    &field.declared_type().unwrap_or(Type::Any),
                    project,
                );
                if field.accessor {
                    format!(
                        "void | ClassAccessorDecoratorResult<{}, {value}>",
                        class.name
                    )
                } else {
                    format!("void | ((this: {}, value: {value}) => {value})", class.name)
                }
            } else if let Some(method) = &member.method {
                format!(
                    "void | (({}) => {})",
                    super::super::type_text::parameter_list_in(&method.parameters, Some(project)),
                    super::super::type_text::render_in(
                        method.return_type.as_ref().unwrap_or(&Type::Void),
                        project
                    )
                )
            } else {
                return;
            }
        } else {
            format!("void | typeof {}", class.name)
        };
        let args = vec![
            super::super::type_text::render_in(result, project),
            expected,
        ];
        let message = super::super::mapping::build(
            1270,
            &counterpart.span,
            args.clone(),
            &diagnostic.message,
        )
        .unwrap()
        .message;
        let counterpart = diagnostic.typescript.as_mut().unwrap();
        counterpart.arguments = args;
        counterpart.message = message;
        return;
    }
    let detail = if let Some(function) = function {
        let runtime = if diagnostic
            .message
            .starts_with("a decorator is called with 2")
        {
            2
        } else if counterpart.code == 1238 {
            1
        } else if counterpart.code == 1239
            || member
                .is_some_and(|member| member.field.as_ref().is_some_and(|field| field.accessor))
        {
            3
        } else {
            2
        };
        format!("\n  The runtime will invoke the decorator with {runtime} arguments, but the decorator expects {}.",function.parameters.len())
    } else {
        let ty = module.declarations.iter().find_map(|item| match item {
            Declaration::Variable(value) if value.name == name => {
                value.annotation.clone().or_else(|| {
                    value.initializer.first().map(|token| {
                        if token.text.parse::<f64>().is_ok() {
                            Type::Number
                        } else {
                            Type::String
                        }
                    })
                })
            }
            _ => None,
        });
        let Some(ty) = ty else { return };
        let value = match ty {
            Type::Number => "Number".into(),
            Type::String => "String".into(),
            Type::Boolean => "Boolean".into(),
            _ => super::super::type_text::render_in(&ty, project),
        };
        format!("\n  This expression is not callable.\n    Type '{value}' has no call signatures.")
    };
    diagnostic
        .typescript
        .as_mut()
        .unwrap()
        .message
        .push_str(&detail);
}
