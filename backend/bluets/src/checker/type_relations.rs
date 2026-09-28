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
        (Type::Tuple(actual), Type::Array(expected)) => actual.iter().all(|element| {
            is_assignable(
                &element.indexed_type(),
                expected,
                aliases,
                &mut visited.clone(),
                budget,
            )
        }),
        (Type::Array(actual), Type::Tuple(expected))
            if expected.last().is_some_and(|element| element.rest) =>
        {
            let fixed = &expected[..expected.len() - 1];
            fixed.iter().all(|element| element.optional)
                && expected.iter().all(|element| {
                    is_assignable(
                        actual,
                        &element.indexed_type(),
                        aliases,
                        &mut visited.clone(),
                        budget,
                    )
                })
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
        (Type::Tuple(actual), Type::Tuple(expected)) => {
            if actual.iter().any(|element| element.rest)
                || expected.iter().any(|element| element.rest)
            {
                return trailing_rest_tuple_assignable(actual, expected, aliases, visited, budget);
            }
            let actual_required = actual.iter().filter(|element| !element.optional).count();
            let expected_required = expected.iter().filter(|element| !element.optional).count();
            actual_required >= expected_required
                && actual.len() <= expected.len()
                && actual.iter().zip(expected).all(|(actual, expected)| {
                    if actual.rest != expected.rest || (actual.optional && !expected.optional) {
                        return false;
                    }
                    let expected_type = if expected.optional {
                        Type::Union(vec![expected.annotation.clone(), Type::Undefined])
                    } else {
                        expected.annotation.clone()
                    };
                    is_assignable(
                        &actual.annotation,
                        &expected_type,
                        aliases,
                        &mut visited.clone(),
                        budget,
                    )
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

fn trailing_rest_tuple_assignable(
    actual: &[crate::parser::TupleTypeElement],
    expected: &[crate::parser::TupleTypeElement],
    aliases: &BTreeMap<String, TypeDefinition>,
    visited: &mut HashSet<String>,
    budget: &mut TypeExpansionBudget,
) -> bool {
    let actual_rest = actual.last().filter(|element| element.rest);
    let expected_rest = expected.last().filter(|element| element.rest);
    if actual
        .iter()
        .take(actual.len().saturating_sub(1))
        .any(|element| element.rest)
        || expected
            .iter()
            .take(expected.len().saturating_sub(1))
            .any(|element| element.rest)
        || (actual_rest.is_some() && expected_rest.is_none())
    {
        return false;
    }
    let actual_fixed = actual.len() - usize::from(actual_rest.is_some());
    let expected_fixed = expected.len() - usize::from(expected_rest.is_some());
    let actual_required = actual[..actual_fixed]
        .iter()
        .filter(|element| !element.optional)
        .count();
    let expected_required = expected[..expected_fixed]
        .iter()
        .filter(|element| !element.optional)
        .count();
    if actual_required < expected_required
        || (expected_rest.is_none() && actual_fixed > expected_fixed)
    {
        return false;
    }
    let positions = if actual_rest.is_some() {
        actual_fixed.max(expected_fixed)
    } else {
        actual_fixed
    };
    for index in 0..positions {
        let Some(actual_type) = tuple_position_type(actual, index) else {
            continue;
        };
        let Some(expected_type) = tuple_position_type(expected, index) else {
            return false;
        };
        if !is_assignable(
            &actual_type,
            &expected_type,
            aliases,
            &mut visited.clone(),
            budget,
        ) {
            return false;
        }
    }
    if let (Some(Type::Array(actual_tail)), Some(Type::Array(expected_tail))) = (
        actual_rest.map(|element| &element.annotation),
        expected_rest.map(|element| &element.annotation),
    ) {
        is_assignable(actual_tail, expected_tail, aliases, visited, budget)
    } else {
        actual_rest.is_none()
    }
}

fn tuple_position_type(elements: &[crate::parser::TupleTypeElement], index: usize) -> Option<Type> {
    elements
        .get(index)
        .or_else(|| elements.last().filter(|element| element.rest))
        .map(|element| element.indexed_type())
}

/// Catch bindings have TypeScript's strict `unknown` semantics. Other
/// inference paths still use `Unknown` as a permissive bounded fallback.
pub(super) fn accepts_strict_unknown(
    expected: &Type,
    aliases: &BTreeMap<String, TypeDefinition>,
    visited: &mut HashSet<String>,
    budget: &mut TypeExpansionBudget,
) -> bool {
    match expected {
        Type::Any | Type::Unknown => true,
        Type::Union(options) => options
            .iter()
            .any(|option| accepts_strict_unknown(option, aliases, &mut visited.clone(), budget)),
        Type::Intersection(parts) => parts
            .iter()
            .all(|part| accepts_strict_unknown(part, aliases, &mut visited.clone(), budget)),
        Type::Named { .. } => instantiate_named(expected, aliases, visited, budget, "expected")
            .is_some_and(|expanded| accepts_strict_unknown(&expanded, aliases, visited, budget)),
        _ => false,
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
                .map(|value| crate::parser::TupleTypeElement {
                    annotation: substitute_type(&value.annotation, substitutions),
                    optional: value.optional,
                    label: value.label.clone(),
                    rest: value.rest,
                })
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
                .map(|value| {
                    format!(
                        "{}{}{}",
                        if value.rest { "..." } else { "" },
                        type_identity(&value.annotation),
                        if value.optional { "?" } else { "" }
                    )
                })
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
            values
                .iter()
                .map(|value| {
                    format!(
                        "{}{}{}",
                        if value.rest { "..." } else { "" },
                        type_label(&value.annotation),
                        if value.optional { "?" } else { "" }
                    )
                })
                .collect::<Vec<_>>()
                .join(", ")
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
