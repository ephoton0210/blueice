// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TupleSpreadError {
    Unresolved,
    Unsupported,
    Cyclic,
    Exhausted,
}

pub(super) fn expand_concrete_tuple_spreads(
    elements: &[crate::parser::TupleTypeElement],
    aliases: &BTreeMap<String, TypeDefinition>,
    active: &mut HashSet<String>,
    budget: &mut TypeExpansionBudget,
) -> Result<Vec<crate::parser::TupleTypeElement>, TupleSpreadError> {
    let mut expanded = Vec::new();
    for element in elements {
        if !element.rest || matches!(element.annotation, Type::Array(_)) {
            if !budget.consume() {
                return Err(TupleSpreadError::Exhausted);
            }
            expanded.push(element.clone());
            continue;
        }
        if let Type::Tuple(items) = &element.annotation {
            expanded.extend(expand_concrete_tuple_spreads(
                items, aliases, active, budget,
            )?);
            continue;
        }
        let Type::Named { name, arguments } = &element.annotation else {
            return Err(TupleSpreadError::Unsupported);
        };
        if !active.insert(name.clone()) {
            return Err(TupleSpreadError::Cyclic);
        }
        if !budget.consume() {
            active.remove(name);
            return Err(TupleSpreadError::Exhausted);
        }
        let replacement = match aliases.get(name) {
            Some(TypeDefinition {
                kind: TypeDefinitionKind::Alias,
                parameters,
                value,
            }) => {
                let specialized = complete_type_arguments(parameters, arguments).map(|completed| {
                    let substitutions = parameters
                        .iter()
                        .map(|parameter| parameter.name.clone())
                        .zip(completed)
                        .collect();
                    substitute_type(value, &substitutions)
                });
                match specialized {
                    Some(Type::Tuple(items)) => {
                        expand_concrete_tuple_spreads(&items, aliases, active, budget)
                    }
                    Some(annotation @ Type::Array(_)) => {
                        Ok(vec![crate::parser::TupleTypeElement {
                            annotation,
                            optional: false,
                            label: None,
                            rest: true,
                        }])
                    }
                    Some(annotation @ Type::Named { .. }) => expand_concrete_tuple_spreads(
                        &[crate::parser::TupleTypeElement {
                            annotation,
                            optional: false,
                            label: None,
                            rest: true,
                        }],
                        aliases,
                        active,
                        budget,
                    ),
                    _ => Err(TupleSpreadError::Unsupported),
                }
            }
            Some(_) => Err(TupleSpreadError::Unsupported),
            None => Err(TupleSpreadError::Unresolved),
        };
        active.remove(name);
        expanded.extend(replacement?);
    }
    if expanded.iter().filter(|element| element.rest).count() > 1
        || expanded.iter().enumerate().any(|(index, element)| {
            element.rest
                && expanded[index + 1..]
                    .iter()
                    .any(|following| following.optional)
        })
    {
        return Err(TupleSpreadError::Unsupported);
    }
    crate::parser::require_tuple_positions_before_suffix(&mut expanded);
    Ok(expanded)
}

pub(super) fn is_assignable(
    actual: &Type,
    expected: &Type,
    aliases: &BTreeMap<String, TypeDefinition>,
    visited: &mut HashSet<String>,
    budget: &mut TypeExpansionBudget,
) -> bool {
    if let Type::Tuple(elements) = actual {
        if elements.iter().any(|element| {
            element.rest && matches!(element.annotation, Type::Named { .. } | Type::Tuple(_))
        }) {
            match expand_concrete_tuple_spreads(elements, aliases, &mut HashSet::new(), budget) {
                Ok(expanded) => {
                    return is_assignable(
                        &Type::Tuple(expanded),
                        expected,
                        aliases,
                        visited,
                        budget,
                    )
                }
                // An unresolved spread name is a symbolic type parameter that
                // annotation validation already accepted; keep it opaque so
                // only an identical tail compares equal.
                Err(TupleSpreadError::Unresolved) => {}
                Err(_) => return false,
            }
        }
    }
    if let Type::Tuple(elements) = expected {
        if elements.iter().any(|element| {
            element.rest && matches!(element.annotation, Type::Named { .. } | Type::Tuple(_))
        }) {
            match expand_concrete_tuple_spreads(elements, aliases, &mut HashSet::new(), budget) {
                Ok(expanded) => {
                    return is_assignable(actual, &Type::Tuple(expanded), aliases, visited, budget)
                }
                Err(TupleSpreadError::Unresolved) => {}
                Err(_) => return false,
            }
        }
    }
    if matches!(actual, Type::Any | Type::Unknown) || matches!(expected, Type::Any | Type::Unknown)
    {
        return true;
    }
    if actual == expected {
        return true;
    }
    if let Some(result) = enum_assignability(actual, expected, aliases) {
        return result;
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
        (Type::Literal(value), Type::String) => {
            value.starts_with('\'')
                || value.starts_with('\"')
                || enum_member_value(aliases, value).is_some_and(
                    |member| matches!(member, Type::Literal(text) if text.starts_with('"')),
                )
        }
        (Type::Literal(value), Type::Number) => {
            value.parse::<f64>().is_ok()
                || enum_member_value(aliases, value).is_some_and(
                    |member| matches!(member, Type::Literal(text) if text.parse::<f64>().is_ok()),
                )
        }
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
                // A function type whose result is `void` accepts a function that
                // returns anything: its result is simply ignored.
                && (matches!(**expected_result, Type::Void)
                    || is_assignable(actual_result, expected_result, aliases, visited, budget))
        }
        (Type::Tuple(actual), Type::Tuple(expected)) => {
            if actual.iter().any(|element| element.rest)
                || expected.iter().any(|element| element.rest)
            {
                if has_nontrailing_rest(actual) || has_nontrailing_rest(expected) {
                    return middle_rest_tuple_assignable(
                        actual, expected, aliases, visited, budget,
                    );
                }
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

/// The constant a member type `E.A` stands for, as a literal type.
fn enum_member_value<'a>(
    aliases: &'a BTreeMap<String, TypeDefinition>,
    literal: &str,
) -> Option<&'a Type> {
    aliases
        .get(literal)
        .filter(|definition| definition.kind == TypeDefinitionKind::EnumMember)
        .map(|definition| &definition.value)
}

/// What a number (or a number literal) may be assigned to an enum: a numeric
/// enum accepts any `number`, and a number literal only when it is the value
/// of one of its members. A string enum accepts no string literal; only its
/// own members, which are handled by ordinary union assignability.
///
/// `None` leaves the question to the general rules.
fn enum_assignability(
    actual: &Type,
    expected: &Type,
    aliases: &BTreeMap<String, TypeDefinition>,
) -> Option<bool> {
    let Type::Named { name, .. } = expected else {
        return None;
    };
    let definition = aliases.get(name)?;
    if definition.kind != TypeDefinitionKind::Enum {
        return None;
    }
    let members: Vec<&Type> = match &definition.value {
        Type::Union(options) => options
            .iter()
            .filter_map(|option| match option {
                Type::Literal(text) => enum_member_value(aliases, text),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    };
    let numeric =
        |value: &&Type| matches!(value, Type::Literal(text) if text.parse::<f64>().is_ok());
    let all_numeric = !members.is_empty() && members.iter().all(numeric);
    // An enum with a computed member is typed as a plain number or string.
    let open_number = matches!(definition.value, Type::Number);
    match actual {
        Type::Number if all_numeric || open_number => Some(true),
        Type::Literal(text) if text.parse::<f64>().is_ok() => {
            if open_number {
                return Some(true);
            }
            let value = text.parse::<f64>().ok()?;
            Some(members.iter().any(|member| {
                matches!(member, Type::Literal(candidate) if candidate.parse::<f64>().ok() == Some(value))
            }))
        }
        Type::Literal(text) if text.starts_with('"') || text.starts_with('\'') => Some(false),
        Type::String => Some(false),
        _ => None,
    }
}

fn has_nontrailing_rest(elements: &[crate::parser::TupleTypeElement]) -> bool {
    elements
        .iter()
        .position(|element| element.rest)
        .is_some_and(|index| index + 1 < elements.len())
}

fn middle_rest_tuple_assignable(
    actual: &[crate::parser::TupleTypeElement],
    expected: &[crate::parser::TupleTypeElement],
    aliases: &BTreeMap<String, TypeDefinition>,
    visited: &mut HashSet<String>,
    budget: &mut TypeExpansionBudget,
) -> bool {
    let actual_rest = actual.iter().position(|element| element.rest);
    let expected_rest = expected.iter().position(|element| element.rest);
    if actual_rest.is_some() && expected_rest.is_none() {
        return false;
    }
    let actual_required = actual
        .iter()
        .filter(|element| !element.optional && !element.rest)
        .count();
    let expected_required = expected
        .iter()
        .filter(|element| !element.optional && !element.rest)
        .count();
    if actual_required < expected_required {
        return false;
    }
    let maximum_sample = if actual_rest.is_some() {
        actual.len() + expected.len() + 1
    } else {
        actual.len()
    };
    for length in actual_required..=maximum_sample {
        if expected_rest.is_none() && length > expected.len() {
            return false;
        }
        for index in 0..length {
            if !budget.consume() {
                return false;
            }
            let Some(actual_type) = tuple_type_at_length(actual, length, index) else {
                return false;
            };
            let Some(expected_type) = tuple_type_at_length(expected, length, index) else {
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
    }
    true
}

pub(in crate::checker) fn tuple_type_at_length(
    elements: &[crate::parser::TupleTypeElement],
    length: usize,
    index: usize,
) -> Option<Type> {
    let Some(rest_index) = elements.iter().position(|element| element.rest) else {
        return elements.get(index).map(|element| element.value_type());
    };
    if index < rest_index {
        return Some(elements[index].value_type());
    }
    let suffix_len = elements.len() - rest_index - 1;
    let suffix_start = length.checked_sub(suffix_len)?;
    if index >= suffix_start {
        return elements
            .get(rest_index + 1 + index - suffix_start)
            .map(|element| element.value_type());
    }
    Some(elements[rest_index].indexed_type())
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
    if definition.kind == TypeDefinitionKind::EnumMember {
        // A member used as a type is the enum literal type, not its value.
        return Some(Type::Literal(name.clone()));
    }
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
        Type::Array(value) => value.array_element_text(type_identity),
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
        Type::Array(value) => value.array_element_text(type_label),
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
