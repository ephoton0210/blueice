// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Class context explains an existing override error without changing its verdict.
use super::*;
use crate::{ClassDeclaration, ClassMemberShell, Parameter, Type};

pub(super) fn refine(project: &Project, diagnostic: &mut Diagnostic) {
    if diagnostic.message.starts_with("implemented member ") {
        return;
    }
    let Some(counterpart) = &diagnostic.typescript else {
        return;
    };
    if !matches!(counterpart.code, 2415 | 2416 | 2417 | 2741) {
        return;
    }
    let mut classes = Vec::new();
    fn collect<'a>(items: &'a [Declaration], classes: &mut Vec<&'a ClassDeclaration>) {
        for item in items {
            match item {
                Declaration::Class(class) => classes.push(class),
                Declaration::Namespace(ns) => collect(&ns.body, classes),
                _ => {}
            }
        }
    }
    for module in project.modules.values() {
        collect(&module.declarations, &mut classes);
    }
    if counterpart.code == 2741 {
        if let Some(name) = counterpart.arguments.get(1) {
            if let Some(base) = classes
                .iter()
                .find(|class| class.name == *name && class.members.is_empty())
                .and_then(|class| class.extends_name.as_ref())
            {
                let args = vec![
                    counterpart.arguments[0].clone(),
                    base.clone(),
                    counterpart.arguments[2].clone(),
                ];
                let rendered = super::super::mapping::build(
                    2741,
                    &counterpart.span,
                    args.clone(),
                    &diagnostic.message,
                )
                .unwrap();
                let counterpart = diagnostic.typescript.as_mut().unwrap();
                counterpart.arguments = args;
                counterpart.message = rendered.message;
            }
        }
        return;
    }
    let Some(class) = classes.iter().find(|class| {
        class.span.module == counterpart.span.module
            && class.span.start <= counterpart.span.start
            && class.span.end >= counterpart.span.end
    }) else {
        return;
    };
    let Some(base_name) = class.extends_name.as_ref() else {
        return;
    };
    let resolved_name = project
        .modules
        .get(&class.span.module)
        .and_then(|module| {
            module.declarations.iter().find_map(|item| match item {
                Declaration::Import(import) => import
                    .bindings
                    .iter()
                    .find(|binding| binding.local == *base_name)
                    .map(|binding| binding.imported.as_str()),
                _ => None,
            })
        })
        .unwrap_or(base_name);
    let Some(base) = find_class(&classes, resolved_name) else {
        return;
    };
    if counterpart.code == 2415 {
        let detail = super::super::type_text::detail(
            &Type::Named {
                name: class.name.clone(),
                arguments: Vec::new(),
            },
            &Type::Named {
                name: base.name.clone(),
                arguments: Vec::new(),
            },
            project,
        );
        diagnostic
            .typescript
            .as_mut()
            .unwrap()
            .message
            .push_str(&detail);
        return;
    }
    let member = class
        .members
        .iter()
        .find(|member| {
            member.span.start <= diagnostic.span.start && member.span.end >= diagnostic.span.start
        })
        .or_else(|| {
            let name = diagnostic.message.split('`').nth(1)?;
            class
                .members
                .iter()
                .find(|member| member.name.as_deref() == Some(name))
        });
    let name = member
        .and_then(|member| member.name.as_ref())
        .or_else(|| counterpart.arguments.first());
    let Some(name) = name else { return };
    let static_side = counterpart.code == 2417;
    let own = members(class, name, static_side);
    let mut parent = base;
    let mut arguments = class.extends_arguments.clone();
    let mut inherited = members(parent, name, static_side);
    for _ in 0..64 {
        if !inherited.is_empty() {
            break;
        }
        let Some(next) = parent
            .extends_name
            .as_deref()
            .and_then(|name| find_class(&classes, name))
        else {
            break;
        };
        let substitutions = class_substitutions(parent, &arguments);
        arguments = parent
            .extends_arguments
            .iter()
            .map(|value| crate::checker::substitute_type(value, &substitutions))
            .collect();
        parent = next;
        inherited = members(parent, name, static_side);
    }
    if own.is_empty() || inherited.is_empty() {
        return;
    }
    let actual = member_type(own[0]);
    let substitutions = if static_side {
        Default::default()
    } else {
        class_substitutions(parent, &arguments)
    };
    let expected = inherited
        .iter()
        .filter_map(|member| member_type(member))
        .map(|value| crate::checker::substitute_type(&value, &substitutions))
        .find(|expected| {
            actual.as_ref().is_some_and(|actual| {
                !super::super::type_text::detail(actual, expected, project).is_empty()
            })
        })
        .or_else(|| {
            member_type(inherited[0])
                .map(|value| crate::checker::substitute_type(&value, &substitutions))
        });
    let (Some(actual), Some(expected)) = (actual, expected) else {
        return;
    };
    let args = if static_side {
        vec![
            format!("typeof {}", class.name),
            format!("typeof {}", base.name),
        ]
    } else {
        vec![
            name.clone(),
            class.name.clone(),
            super::super::type_text::render_in(
                &Type::Named {
                    name: base_name.clone(),
                    arguments: class.extends_arguments.clone(),
                },
                project,
            ),
        ]
    };
    let header = super::super::mapping::build(
        counterpart.code,
        &counterpart.span,
        args.clone(),
        &diagnostic.message,
    )
    .unwrap()
    .message;
    let actual_text = signature_text(&own, project)
        .unwrap_or_else(|| super::super::type_text::render_in(&actual, project));
    let expected_text = signature_text_specialized(&inherited, project, &substitutions)
        .unwrap_or_else(|| super::super::type_text::render_in(&expected, project));
    let detail = super::super::type_text::detail(&actual, &expected, project);
    let result_only = matches!((&actual,&expected),(Type::Function{parameters:a,result:ar},Type::Function{parameters:e,result:er}) if a.len()==e.len() && a.iter().zip(e).all(|(a,e)|a.annotation==e.annotation && a.optional==e.optional && a.rest==e.rest) && ar!=er);
    let message = if static_side && result_only {
        let (
            Type::Function { result: actual, .. },
            Type::Function {
                result: expected, ..
            },
        ) = (&actual, &expected)
        else {
            unreachable!()
        };
        format!("{header}\n  The types returned by '{name}(...)' are incompatible between these types.\n    Type '{}' is not assignable to type '{}'.",super::super::type_text::render_in(actual,project),super::super::type_text::render_in(expected,project))
    } else {
        let prefix = if static_side {
            format!("\n  Types of property '{name}' are incompatible.")
        } else {
            String::new()
        };
        let indent = if static_side { "    " } else { "  " };
        let detail = detail
            .lines()
            .skip(1)
            .map(|line| format!("\n{indent}{line}"))
            .collect::<String>();
        format!("{header}{prefix}\n{indent}Type '{actual_text}' is not assignable to type '{expected_text}'.{detail}")
    };
    let counterpart = diagnostic.typescript.as_mut().unwrap();
    counterpart.arguments = args;
    counterpart.message = message;
}
fn find_class<'a>(classes: &[&'a ClassDeclaration], name: &str) -> Option<&'a ClassDeclaration> {
    let name = crate::parser::source_type_name(name);
    classes.iter().copied().find(|class| class.name == name)
}
fn members<'a>(
    class: &'a ClassDeclaration,
    name: &str,
    static_side: bool,
) -> Vec<&'a ClassMemberShell> {
    let mut members = class
        .members
        .iter()
        .filter(|member| {
            member.name.as_deref() == Some(name)
                && member
                    .method
                    .as_ref()
                    .map(|method| method.is_static)
                    .or_else(|| member.field.as_ref().map(|field| field.is_static))
                    .or_else(|| member.accessor.as_ref().map(|accessor| accessor.is_static))
                    .unwrap_or(false)
                    == static_side
        })
        .collect::<Vec<_>>();
    if members.iter().any(|member| {
        member
            .method
            .as_ref()
            .is_some_and(|method| method.body.is_none())
    }) {
        members.retain(|member| {
            member
                .method
                .as_ref()
                .is_some_and(|method| method.body.is_none())
        });
    }
    members
}
fn member_type(member: &ClassMemberShell) -> Option<Type> {
    if let Some(field) = &member.field {
        let value = field.declared_type()?;
        return Some(if field.optional {
            Type::Union(vec![value, Type::Undefined])
        } else {
            value
        });
    }
    if let Some(accessor) = &member.accessor {
        return accessor.return_type.clone().or_else(|| {
            accessor
                .parameters
                .first()
                .and_then(|parameter| parameter.annotation.clone())
        });
    }
    let method = member.method.as_ref()?;
    Some(Type::Function {
        parameters: method.parameters.clone(),
        result: Box::new(method.return_type.clone().unwrap_or(Type::Any)),
    })
}
fn signature_text(members: &[&ClassMemberShell], project: &Project) -> Option<String> {
    signature_text_specialized(members, project, &Default::default())
}
fn signature_text_specialized(
    members: &[&ClassMemberShell],
    project: &Project,
    substitutions: &std::collections::BTreeMap<String, Type>,
) -> Option<String> {
    let signatures = members
        .iter()
        .map(|member| {
            member.method.as_ref()?;
            let value = crate::checker::substitute_type(&member_type(member)?, substitutions);
            let (Type::Function { parameters, result }
            | Type::GenericFunction {
                parameters, result, ..
            }) = value
            else {
                return None;
            };
            let parameters = parameter_text(&parameters, project);
            let result = super::super::type_text::render_in(&result, project);
            Some((parameters, result))
        })
        .collect::<Option<Vec<_>>>()?;
    if let [(parameters, result)] = signatures.as_slice() {
        Some(format!("({parameters}) => {result}"))
    } else {
        Some(format!(
            "{{ {} }}",
            signatures
                .iter()
                .map(|(parameters, result)| format!("({parameters}): {result};"))
                .collect::<Vec<_>>()
                .join(" ")
        ))
    }
}
fn parameter_text(parameters: &[Parameter], project: &Project) -> String {
    super::super::type_text::parameter_list_in(parameters, Some(project))
}

fn class_substitutions(
    class: &ClassDeclaration,
    arguments: &[Type],
) -> std::collections::BTreeMap<String, Type> {
    let arguments = crate::checker::complete_type_arguments(&class.type_parameters, arguments)
        .unwrap_or_default();
    class
        .type_parameters
        .iter()
        .map(|parameter| parameter.name.clone())
        .zip(arguments)
        .collect()
}
