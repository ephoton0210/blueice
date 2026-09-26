// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

pub(super) fn is_assignable(
    actual: &Type,
    expected: &Type,
    aliases: &BTreeMap<String, TypeDefinition>,
    visited: &mut HashSet<String>,
    budget: &mut TypeExpansionBudget,
) -> bool {
    if matches!(actual, Type::Any | Type::Unknown) || matches!(expected, Type::Any | Type::Unknown)
    {
        return true;
    }
    if actual == expected {
        return true;
    }
    if let Some(expanded) = instantiate_named(actual, aliases, visited, budget, "actual") {
        return is_assignable(&expanded, expected, aliases, visited, budget);
    }
    if let Some(expanded) = instantiate_named(expected, aliases, visited, budget, "expected") {
        return is_assignable(actual, &expanded, aliases, visited, budget);
    }
    if let Type::Union(options) = actual {
        return options
            .iter()
            .all(|option| is_assignable(option, expected, aliases, &mut visited.clone(), budget));
    }
    if let Type::Union(options) = expected {
        return options
            .iter()
            .any(|option| is_assignable(actual, option, aliases, &mut visited.clone(), budget));
    }
    if let Type::Intersection(parts) = expected {
        return parts
            .iter()
            .all(|part| is_assignable(actual, part, aliases, &mut visited.clone(), budget));
    }
    if let (Type::Intersection(_), Type::Record(expected_fields)) = (actual, expected) {
        return expected_fields.iter().all(|expected_field| {
            match property_type(actual, &expected_field.name, aliases, visited, budget) {
                PropertyType::Found { value: actual, .. } => is_assignable(
                    &actual,
                    &expected_field.value,
                    aliases,
                    &mut visited.clone(),
                    budget,
                ),
                PropertyType::Missing => expected_field.optional,
                PropertyType::Indeterminate => true,
                PropertyType::Exhausted => false,
            }
        });
    }
    if let Type::Intersection(parts) = actual {
        return parts
            .iter()
            .any(|part| is_assignable(part, expected, aliases, &mut visited.clone(), budget));
    }
    match (actual, expected) {
        (Type::Literal(value), Type::String) => value.starts_with('\'') || value.starts_with('\"'),
        (Type::Literal(value), Type::Number) => value.parse::<f64>().is_ok(),
        (Type::Literal(value), Type::Boolean) => matches!(value.as_str(), "true" | "false"),
        (Type::Array(actual), Type::Array(expected)) => {
            is_assignable(actual, expected, aliases, visited, budget)
        }
        (
            Type::Function {
                parameters: actual_parameters,
                result: actual_result,
            },
            Type::Function {
                parameters: expected_parameters,
                result: expected_result,
            },
        ) if actual_parameters.len() == expected_parameters.len() => {
            expected_parameters
                .iter()
                .zip(actual_parameters)
                .all(|(expected, actual)| {
                    expected.optional == actual.optional
                        && is_assignable(
                            expected.annotation.as_ref().unwrap_or(&Type::Unknown),
                            actual.annotation.as_ref().unwrap_or(&Type::Unknown),
                            aliases,
                            &mut visited.clone(),
                            budget,
                        )
                })
                && is_assignable(actual_result, expected_result, aliases, visited, budget)
        }
        (Type::Tuple(actual), Type::Tuple(expected)) if actual.len() == expected.len() => {
            actual.iter().zip(expected).all(|(actual, expected)| {
                is_assignable(actual, expected, aliases, &mut visited.clone(), budget)
            })
        }
        (Type::Record(actual), Type::Record(expected)) => expected.iter().all(|expected_field| {
            actual
                .iter()
                .find(|actual_field| actual_field.name == expected_field.name)
                .map(|actual_field| {
                    (actual_field.optional == expected_field.optional || expected_field.optional)
                        && is_assignable(
                            &actual_field.value,
                            &expected_field.value,
                            aliases,
                            &mut visited.clone(),
                            budget,
                        )
                })
                .unwrap_or(expected_field.optional)
        }),
        _ => actual == expected,
    }
}

pub(super) fn instantiate_named(
    value: &Type,
    aliases: &BTreeMap<String, TypeDefinition>,
    visited: &mut HashSet<String>,
    budget: &mut TypeExpansionBudget,
    side: &str,
) -> Option<Type> {
    let Type::Named { name, arguments } = value else {
        return None;
    };
    let definition = aliases.get(name)?;
    let arguments = complete_type_arguments(&definition.parameters, arguments)?;
    let key = format!("{side}:{}", type_identity(value));
    if !visited.insert(key) {
        return None;
    }
    if !budget.consume() {
        return None;
    }
    let substitutions = definition
        .parameters
        .iter()
        .map(|parameter| parameter.name.clone())
        .zip(arguments)
        .collect();
    Some(substitute_type(&definition.value, &substitutions))
}

/// Resolves omitted trailing generic arguments through their declaration-site
/// defaults. The parser/checker reports invalid argument counts and constraint
/// violations separately; this helper is also used by structural expansion,
/// where `None` simply means the named type cannot be expanded safely.
pub(super) fn complete_type_arguments(
    parameters: &[TypeParameter],
    supplied: &[Type],
) -> Option<Vec<Type>> {
    if supplied.len() > parameters.len() {
        return None;
    }
    let mut substitutions = BTreeMap::new();
    let mut arguments = Vec::with_capacity(parameters.len());
    for (index, parameter) in parameters.iter().enumerate() {
        let value = supplied.get(index).cloned().or_else(|| {
            parameter
                .default
                .as_ref()
                .map(|value| substitute_type(value, &substitutions))
        })?;
        substitutions.insert(parameter.name.clone(), value.clone());
        arguments.push(value);
    }
    Some(arguments)
}

pub(super) fn substitute_type(value: &Type, substitutions: &BTreeMap<String, Type>) -> Type {
    match value {
        Type::Named { name, arguments } if arguments.is_empty() => substitutions
            .get(name)
            .cloned()
            .unwrap_or_else(|| value.clone()),
        Type::Named { name, arguments } => Type::Named {
            name: name.clone(),
            arguments: arguments
                .iter()
                .map(|argument| substitute_type(argument, substitutions))
                .collect(),
        },
        Type::Array(value) => Type::Array(Box::new(substitute_type(value, substitutions))),
        Type::Tuple(values) => Type::Tuple(
            values
                .iter()
                .map(|value| substitute_type(value, substitutions))
                .collect(),
        ),
        Type::Record(fields) => Type::Record(
            fields
                .iter()
                .map(|field| TypeField {
                    name: field.name.clone(),
                    readonly: field.readonly,
                    optional: field.optional,
                    value: substitute_type(&field.value, substitutions),
                    span: field.span.clone(),
                })
                .collect(),
        ),
        Type::Function { parameters, result } => Type::Function {
            parameters: parameters
                .iter()
                .map(|parameter| Parameter {
                    annotation: parameter
                        .annotation
                        .as_ref()
                        .map(|value| substitute_type(value, substitutions)),
                    ..parameter.clone()
                })
                .collect(),
            result: Box::new(substitute_type(result, substitutions)),
        },
        Type::Union(values) => Type::Union(
            values
                .iter()
                .map(|value| substitute_type(value, substitutions))
                .collect(),
        ),
        Type::Intersection(values) => Type::Intersection(
            values
                .iter()
                .map(|value| substitute_type(value, substitutions))
                .collect(),
        ),
        _ => value.clone(),
    }
}

pub(super) fn type_identity(value: &Type) -> String {
    match value {
        Type::Named { name, arguments } => format!(
            "{name}<{}>",
            arguments
                .iter()
                .map(type_identity)
                .collect::<Vec<_>>()
                .join(",")
        ),
        Type::Array(value) => format!("{}[]", type_identity(value)),
        Type::Tuple(values) => format!(
            "[{}]",
            values
                .iter()
                .map(type_identity)
                .collect::<Vec<_>>()
                .join(",")
        ),
        Type::Record(_) => "record".to_string(),
        Type::Function { .. } => "function".to_string(),
        Type::Union(values) => values
            .iter()
            .map(type_identity)
            .collect::<Vec<_>>()
            .join("|"),
        Type::Intersection(values) => values
            .iter()
            .map(type_identity)
            .collect::<Vec<_>>()
            .join("&"),
        _ => type_label(value),
    }
}

pub(crate) fn type_label(value: &Type) -> String {
    match value {
        Type::Any => "any".to_string(),
        Type::Unknown => "unknown".to_string(),
        Type::Never => "never".to_string(),
        Type::Void => "void".to_string(),
        Type::Null => "null".to_string(),
        Type::Undefined => "undefined".to_string(),
        Type::Boolean => "boolean".to_string(),
        Type::Number => "number".to_string(),
        Type::String => "string".to_string(),
        Type::Literal(value) => value.clone(),
        Type::Named { name, .. } => name.clone(),
        Type::Array(value) => format!("{}[]", type_label(value)),
        Type::Tuple(values) => format!(
            "[{}]",
            values.iter().map(type_label).collect::<Vec<_>>().join(", ")
        ),
        Type::Record(_) => "record".to_string(),
        Type::Function { .. } => "function".to_string(),
        Type::Union(values) => values
            .iter()
            .map(type_label)
            .collect::<Vec<_>>()
            .join(" | "),
        Type::Intersection(values) => values
            .iter()
            .map(type_label)
            .collect::<Vec<_>>()
            .join(" & "),
    }
}
