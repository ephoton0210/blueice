// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Signature spelling and diagnostic-only expansion of concrete rest tuples.
use super::*;
use crate::{Declaration, Project};

pub(super) fn normalize(value: &Type, project: Option<&Project>, depth: usize) -> Type {
    if depth >= 32 {
        return value.clone();
    }
    match value {
        Type::Named { name, arguments } => {
            fn alias<'a>(
                items: &'a [Declaration],
                name: &str,
            ) -> Option<&'a crate::TypeAliasDeclaration> {
                items.iter().find_map(|item| match item {
                    Declaration::TypeAlias(alias) if alias.name == name => Some(alias),
                    Declaration::Namespace(ns) => alias(&ns.body, name),
                    _ => None,
                })
            }
            let Some(alias) = project.and_then(|project| {
                project.modules.values().find_map(|module| {
                    alias(&module.declarations, crate::parser::source_type_name(name))
                })
            }) else {
                return value.clone();
            };
            fn substitute(
                value: &Type,
                arguments: &std::collections::BTreeMap<String, Type>,
            ) -> Type {
                match value {
                    Type::Named {
                        name,
                        arguments: args,
                    } if args.is_empty() && arguments.contains_key(name) => arguments[name].clone(),
                    Type::Named {
                        name,
                        arguments: args,
                    } => Type::Named {
                        name: name.clone(),
                        arguments: args
                            .iter()
                            .map(|value| substitute(value, arguments))
                            .collect(),
                    },
                    Type::Array(value) => Type::Array(Box::new(substitute(value, arguments))),
                    Type::Tuple(elements) => Type::Tuple(
                        elements
                            .iter()
                            .map(|element| {
                                let mut element = element.clone();
                                element.annotation = substitute(&element.annotation, arguments);
                                element
                            })
                            .collect(),
                    ),
                    _ => value.clone(),
                }
            }
            let substitutions = alias
                .type_parameters
                .iter()
                .zip(arguments)
                .map(|(parameter, value)| (parameter.name.clone(), value.clone()))
                .collect();
            normalize(
                &substitute(&alias.value, &substitutions),
                project,
                depth + 1,
            )
        }
        Type::Tuple(elements) => {
            let mut result = Vec::new();
            for element in elements {
                let mut element = element.clone();
                if element.rest {
                    element.annotation = normalize(&element.annotation, project, depth + 1);
                    if let Type::Tuple(elements) = &element.annotation {
                        result.extend(elements.clone());
                        continue;
                    }
                }
                result.push(element);
            }
            Type::Tuple(result)
        }
        _ => value.clone(),
    }
}

pub(super) fn expanded(
    parameters: &[Parameter],
    project: Option<&Project>,
    split_middle: bool,
) -> Vec<Parameter> {
    let mut result = Vec::new();
    for parameter in parameters {
        let mut parameter = parameter.clone();
        let mut value = parameter.annotation.clone().unwrap_or(Type::Any);
        if parameter.rest {
            value = normalize(&value, project, 0);
            if let Type::Tuple(elements) = &value {
                let middle = elements
                    .iter()
                    .position(|element| element.rest)
                    .filter(|index| *index + 1 < elements.len());
                if middle.is_none() || split_middle {
                    for (index, element) in elements.iter().enumerate() {
                        let mut expanded = parameter.clone();
                        expanded.name = element.label.clone().unwrap_or_else(|| {
                            if element.rest {
                                parameter.name.clone()
                            } else {
                                format!("{}_{index}", parameter.name)
                            }
                        });
                        expanded.optional = element.optional;
                        expanded.default = None;
                        expanded.rest = element.rest;
                        expanded.annotation = Some(element.annotation.clone());
                        if middle == Some(index) {
                            expanded.annotation = Some(Type::Tuple(elements[index..].to_vec()));
                            result.push(expanded);
                            break;
                        }
                        result.push(expanded);
                    }
                    continue;
                }
            }
        }
        parameter.annotation = Some(value);
        result.push(parameter);
    }
    result
}

pub(super) fn optional(value: &Type) -> Type {
    if matches!(value, Type::Any | Type::Unknown)
        || matches!(value,Type::Union(values) if values.contains(&Type::Undefined))
    {
        return value.clone();
    }
    match value {
        Type::Union(values) => {
            let mut values = values.clone();
            values.push(Type::Undefined);
            Type::Union(values)
        }
        _ => Type::Union(vec![value.clone(), Type::Undefined]),
    }
}

pub(crate) fn list(parameters: &[Parameter], project: Option<&Project>) -> String {
    expanded(parameters, project, false)
        .iter()
        .map(|parameter| {
            let value = parameter.annotation.as_ref().unwrap_or(&Type::Any);
            let value = if parameter.optional || parameter.default.is_some() {
                optional(value)
            } else {
                value.clone()
            };
            format!(
                "{}{}{}: {}",
                if parameter.rest { "..." } else { "" },
                parameter.name,
                if parameter.optional || parameter.default.is_some() {
                    "?"
                } else {
                    ""
                },
                text(&value, 0, project)
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}
