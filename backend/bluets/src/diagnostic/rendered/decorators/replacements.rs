// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Explain already-rejected replacements and context arguments from owned AST.
use super::*;
use crate::diagnostic::{type_text, TypeScriptRelatedInformation};
use crate::SourceSpan;

fn line(depth: usize, message: String) -> String {
    format!("\n{}{message}", "  ".repeat(depth))
}

fn mismatch(actual: &Type, expected: &Type, project: &Project, depth: usize) -> String {
    line(
        depth,
        format!(
            "Type '{}' is not assignable to type '{}'.",
            type_text::render_in(actual, project),
            type_text::render_in(expected, project)
        ),
    )
}

fn reason(actual: &Type, expected: &Type, project: &Project, depth: usize) -> String {
    let without_receiver = |value: &Type| match value {
        Type::Function { parameters, result } => Type::Function {
            parameters: parameters
                .iter()
                .filter(|p| p.name != "this")
                .cloned()
                .collect(),
            result: result.clone(),
        },
        other => other.clone(),
    };
    type_text::detail(
        &without_receiver(actual),
        &without_receiver(expected),
        project,
    )
    .replace('\n', &format!("\n{}", "  ".repeat(depth - 1)))
}

pub(super) fn replacement(
    project: &Project,
    diagnostic: &mut Diagnostic,
    class: &ClassDeclaration,
    member: Option<&ClassMemberShell>,
    function: &crate::FunctionDeclaration,
) {
    let module = project.modules.get(&class.span.module);
    let legacy = diagnostic.typescript.as_ref().is_some_and(|counterpart| {
        counterpart
            .arguments
            .get(1)
            .is_some_and(|expected| expected.contains("TypedPropertyDescriptor"))
    });
    let Some(expected) = crate::checker::decorator_replacement_type(class, member, legacy) else {
        return;
    };
    let actual = function.return_type.as_ref().unwrap_or(&Type::Any);
    let mut detail = mismatch(actual, &expected, project, 1);
    if member.is_none() {
        if let Type::Named { name, .. } = actual {
            if let Some(name) = name.strip_prefix("typeof ") {
                let name = name.rsplit_once('@').map_or(name, |(name, _)| name);
                let other = module.and_then(|module| {
                    module.declarations.iter().find_map(|item| match item {
                        Declaration::Class(other) if other.name == name => Some(other),
                        _ => None,
                    })
                });
                if let Some(other) = other {
                    if let Some(missing) = class.members.iter().find(|target| {
                        target.field.as_ref().is_some_and(|field| !field.is_static)
                            && !other
                                .members
                                .iter()
                                .any(|source| source.name == target.name)
                    }) {
                        if let (Some(name), Some(token)) = (&missing.name, missing.key.first()) {
                            detail += &line(2, format!("Property '{name}' is missing in type '{}' but required in type '{}'.", other.name, class.name));
                            if let Some(counterpart) = diagnostic.typescript.as_mut() {
                                counterpart.related_information.push(
                                    TypeScriptRelatedInformation {
                                        code: 2728,
                                        message: format!("'{name}' is declared here."),
                                        span: SourceSpan::new(
                                            &class.span.module,
                                            token.start,
                                            token.end,
                                        ),
                                        position: None,
                                    },
                                );
                            }
                        }
                    }
                }
            }
        }
    } else if let (
        Type::Named {
            name: actual_name,
            arguments: actual_args,
        },
        Type::Named {
            name: expected_name,
            arguments: expected_args,
        },
    ) = (actual, &expected)
    {
        if actual_name == "TypedPropertyDescriptor" && expected_name == actual_name {
            if let (Some(actual), Some(expected)) = (actual_args.first(), expected_args.first()) {
                let actual_optional = Type::Union(vec![actual.clone(), Type::Undefined]);
                let expected_optional = Type::Union(vec![expected.clone(), Type::Undefined]);
                detail += &line(2, "Types of property 'value' are incompatible.".into());
                detail += &mismatch(&actual_optional, &expected_optional, project, 3);
                detail += &mismatch(actual, expected, project, 4);
                detail += &reason(actual, expected, project, 5);
            }
        }
    } else if let (Type::Record(fields), Type::Named { name, arguments }) = (actual, &expected) {
        if name == "ClassAccessorDecoratorResult" {
            if let (Some(field), Some(expected)) =
                (fields.iter().find(|f| f.name == "get"), arguments.get(1))
            {
                if let Type::Function { result, .. } = &field.value {
                    detail += &line(
                        2,
                        "The types returned by 'get()' are incompatible between these types."
                            .into(),
                    );
                    detail += &mismatch(result, expected, project, 3);
                }
            }
        }
    } else {
        detail += &reason(actual, &expected, project, 2);
    }
    let target = type_text::render_in(&expected, project);
    let target = if matches!(
        expected,
        Type::Function { .. } | Type::GenericFunction { .. }
    ) {
        format!("void | ({target})")
    } else {
        format!("void | {target}")
    };
    let arguments = vec![type_text::render_in(actual, project), target];
    if let Some(counterpart) = diagnostic.typescript.as_mut() {
        counterpart.message = format!(
            "Decorator function return type '{}' is not assignable to type '{}'.{detail}",
            arguments[0], arguments[1]
        );
        counterpart.arguments = arguments;
    }
}

pub(super) fn context(
    project: &Project,
    diagnostic: &mut Diagnostic,
    member: Option<&ClassMemberShell>,
    function: &crate::FunctionDeclaration,
    decorator: &Decorator,
) {
    let actual_kind = member.map_or("class", |member| {
        if member.method.is_some() {
            "method"
        } else if let Some(accessor) = &member.accessor {
            if accessor.getter {
                "getter"
            } else {
                "setter"
            }
        } else if member.field.as_ref().is_some_and(|field| field.accessor) {
            "accessor"
        } else {
            "field"
        }
    });
    let expected_kind = function
        .parameters
        .get(1)
        .and_then(|parameter| parameter.annotation.as_ref())
        .and_then(|value| {
            if let Type::Named { name, .. } = value {
                name.strip_prefix("Class")
                    .and_then(|name| name.strip_suffix("DecoratorContext"))
            } else {
                None
            }
        })
        .map(str::to_lowercase);
    let Some(counterpart) = diagnostic.typescript.as_mut() else {
        return;
    };
    if counterpart.arguments.len() != 2 {
        return;
    }
    let mut message = counterpart.message_template.to_string();
    message += &line(
        1,
        format!(
            "Argument of type '{}' is not assignable to parameter of type '{}'.",
            counterpart.arguments[0], counterpart.arguments[1]
        ),
    );
    if let Some(expected_kind) = expected_kind {
        message += &line(2, "Types of property 'kind' are incompatible.".into());
        message += &line(
            3,
            format!("Type '\"{actual_kind}\"' is not assignable to type '\"{expected_kind}\"'."),
        );
    }
    counterpart.message = message;
    if let (Some(first), Some(last)) = (decorator.tokens.first(), decorator.tokens.last()) {
        counterpart.span = SourceSpan::new(&counterpart.span.module, first.start, last.end);
        counterpart.position = project.source(&counterpart.span.module).and_then(|source| {
            crate::diagnostic::positions::from_source(source, &counterpart.span)
        });
    }
}
