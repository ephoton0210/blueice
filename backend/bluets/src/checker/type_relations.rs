// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;
mod substitution;
pub(crate) use substitution::type_identity;
pub(crate) use substitution::{complete_type_arguments, substitute_type, type_label};
mod compatibility;
mod identity_shapes;
mod intersections;
mod more_types;
mod parameters;
mod string_mapping;
mod surfaces;
mod variance;
use crate::checker::type_operators::union;
pub(super) use compatibility::constructor_relation_type;
pub(super) use compatibility::record_fields_assignable;
pub(super) use intersections::has_alias as non_null_alias;
pub(super) use intersections::normalized as normalize_intersection;
pub(super) use more_types::normalized as normalize_more_type;
pub(super) use parameters::canonical_parameter_references;

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
            Some(TypeDefinition {
                kind: TypeDefinitionKind::Parameter,
                ..
            }) => Err(TupleSpreadError::Unresolved),
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
    if let Type::InferenceBarrier(base) = actual {
        return budget.consume() && is_assignable(base, expected, aliases, visited, budget);
    }
    if let Type::InferenceBarrier(base) = expected {
        return budget.consume() && is_assignable(actual, base, aliases, visited, budget);
    }
    if let (
        Type::Named {
            name: actual_name, ..
        },
        Type::Named {
            name: expected_name,
            ..
        },
    ) = (actual, expected)
    {
        if actual_name != expected_name
            && [actual_name, expected_name].iter().all(|name| {
                aliases.get(*name).is_some_and(|definition| {
                    definition.kind == TypeDefinitionKind::LibraryInterface
                })
            })
        {
            let pair = format!(
                "library-relation:{}:{}",
                type_identity(actual),
                type_identity(expected)
            );
            if !visited.insert(pair.clone()) {
                return true;
            }
            let result = assignable_inner(actual, expected, aliases, visited, budget);
            visited.remove(&pair);
            return result;
        }
    }
    assignable_inner(actual, expected, aliases, visited, budget)
}

fn assignable_inner(
    actual: &Type,
    expected: &Type,
    aliases: &BTreeMap<String, TypeDefinition>,
    visited: &mut HashSet<String>,
    budget: &mut TypeExpansionBudget,
) -> bool {
    if *expected == Type::Never {
        return type_operators::expanded(actual, aliases, visited, budget) == Type::Never;
    }
    if actual == expected {
        return true;
    }
    // Preserve a symbolic key set's declaration identity before approximation
    // as broad property keys can expand the parameter's constraint.
    if variance::same_keyof_parameter(actual, expected, aliases) {
        return budget.consume();
    }
    // Explicit any constituents retain their TypeScript union/intersection
    // meaning even against a rigid parameter. Normalize that existing evidence
    // before rigidity; unresolved/unknown types never acquire any here.
    let compound_normalization = match actual {
        Type::Union(_) | Type::Intersection(_) => more_types::normalized(actual, aliases),
        _ => None,
    };
    if let Some(value @ (Type::Any | Type::Never)) = compound_normalization {
        return is_assignable(&value, expected, aliases, visited, budget);
    }
    if let Type::Conditional(value) = actual {
        if (matches!(value.when_true, Type::Never) || matches!(value.when_false, Type::Never))
            && matches!(expected, Type::Named { name, arguments } if arguments.is_empty()
                && aliases.get(name).is_some_and(|definition| definition.kind == TypeDefinitionKind::Parameter))
            && budget.consume()
            && is_assignable(
                &value.when_true,
                expected,
                aliases,
                &mut visited.clone(),
                budget,
            )
            && is_assignable(
                &value.when_false,
                expected,
                aliases,
                &mut visited.clone(),
                budget,
            )
        {
            return true;
        }
    }
    if let (
        Type::IndexedAccess {
            object: actual_object,
            index: actual_index,
            ..
        },
        Type::IndexedAccess {
            object: expected_object,
            index: expected_index,
            ..
        },
    ) = (actual, expected)
    {
        // Index spans locate diagnostics; they do not distinguish types.
        // For one object, a narrower set of keys selects a subtype of the
        // values selected by its wider set, even before T is instantiated.
        if type_identity(actual_object) == type_identity(expected_object) {
            if is_assignable(
                actual_index,
                expected_index,
                aliases,
                &mut visited.clone(),
                budget,
            ) {
                return true;
            }
            let object = canonical_parameter_references(actual_object, aliases);
            let index = canonical_parameter_references(expected_index, aliases);
            if [&object, &index].iter().all(|value| matches!(value, Type::Named { name, .. } if aliases.get(name).is_some_and(|definition| definition.kind == TypeDefinitionKind::Parameter))) {
                // A rigid key parameter can choose a narrower value type
                // than the enclosing object's constraint. Expanding that
                // constraint here loses the very distinction being checked.
                return false;
            }
        }
    }
    // Every symbolic keyof result is a property key, even before its operand
    // can be instantiated. Concrete, narrower comparisons still use expansion.
    if (matches!(actual, Type::KeyOf(_))
        || matches!(actual, Type::Intersection(parts) if parts.iter().any(|part| matches!(part, Type::KeyOf(_)))))
        && is_assignable(
            &Type::Union(vec![Type::String, Type::Number, Type::Symbol]),
            expected,
            aliases,
            &mut visited.clone(),
            budget,
        )
    {
        return true;
    }
    // A restriction of a rigid parameter must compare each component before
    // replacing the source parameter with its (potentially wider) constraint.
    if let Type::Named { name, .. } = expected {
        if intersections::has_alias(name, aliases) {
            let mut path = visited.clone();
            if let Some(expanded) = instantiate_named(
                expected,
                aliases,
                &mut path,
                budget,
                "expected non-null restriction",
            ) {
                return is_assignable(actual, &expanded, aliases, &mut path, budget);
            }
        }
    }
    if let Type::Intersection(parts) = expected {
        if parts.iter().any(intersections::empty_record) {
            if let Some(reduced) = intersections::normalized(expected, aliases, budget) {
                return is_assignable(actual, &reduced, aliases, visited, budget);
            }
            return budget.consume()
                && parts.iter().all(|part| {
                    is_assignable(actual, part, aliases, &mut visited.clone(), budget)
                });
        }
    }
    if let Some(result) = variance::assignable(actual, expected, aliases, visited, budget) {
        return result;
    }
    if let Some(result) =
        parameters::generic_functions_assignable(actual, expected, aliases, visited, budget)
    {
        return result;
    }
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
    if let (Some(actual), Some(expected)) = (actual.string_value(), expected.string_value()) {
        return actual == expected;
    }
    if let (Type::Literal(actual), Type::Literal(expected)) = (actual, expected) {
        if actual.starts_with(['\'', '"'])
            && expected.starts_with(['\'', '"'])
            && !actual.contains('\\')
            && !expected.contains('\\')
        {
            let token = |text: &String| Token {
                kind: TokenKind::String,
                text: text.clone(),
                start: 0,
                end: text.len(),
            };
            if let (Some(actual), Some(expected)) = (
                crate::syntax::string_contents(&token(actual)),
                crate::syntax::string_contents(&token(expected)),
            ) {
                return actual == expected;
            }
        }
    }
    if let Some(result) = compatibility::assignable(actual, expected, aliases, visited, budget) {
        return result;
    }
    if let Some(result) = more_types::assignable(actual, expected, aliases, visited, budget) {
        return result;
    }
    if let Some(result) = surfaces::assignable(actual, expected, aliases, visited, budget) {
        return result;
    }
    if matches!(actual, Type::Any | Type::Unknown) || matches!(expected, Type::Any | Type::Unknown)
    {
        return true;
    }
    if actual == &Type::Never
        || actual == expected
        || (actual == &Type::Undefined && expected == &Type::Void)
    {
        return true;
    }
    // Array syntax and the original Array/ReadonlyArray declarations describe
    // the same elements. Owner replacements retain ordinary structural rules.
    let library_arguments = |value: &Type, names: &[&str]| match value {
        Type::Named { name, arguments }
            if names.contains(&name.as_str())
                && arguments.len() == 1
                && aliases.get(name).is_some_and(|definition| {
                    definition.kind == TypeDefinitionKind::LibraryInterface
                }) =>
        {
            Some(arguments[0].clone())
        }
        _ => None,
    };
    if let Some(element) = library_arguments(expected, &["Array", "ReadonlyArray"]) {
        match actual {
            Type::Array(actual) => {
                return is_assignable(actual, &element, aliases, visited, budget)
            }
            Type::Tuple(actual) => {
                return actual.iter().all(|part| {
                    is_assignable(
                        &part.indexed_type(),
                        &element,
                        aliases,
                        &mut visited.clone(),
                        budget,
                    )
                })
            }
            _ => {}
        }
    }
    if let (Some(element), Type::Array(expected)) =
        (library_arguments(actual, &["Array"]), expected)
    {
        return is_assignable(&element, expected, aliases, visited, budget);
    }
    // Parsed intrinsic arrays already declare their yielded elements. Compare
    // that evidence directly instead of expanding the recursive iterator
    // protocol for each tuple position. Owner replacements remain structural.
    if let Some(element) = library_arguments(expected, &["Iterable"]) {
        let trusted = |name: &str| {
            aliases
                .get(name)
                .is_some_and(|definition| definition.kind == TypeDefinitionKind::LibraryInterface)
        };
        let (sequence, trusted) = match actual {
            Type::Readonly(sequence) => (sequence.as_ref(), trusted("ReadonlyArray")),
            sequence => (sequence, trusted("Array")),
        };
        if trusted {
            match sequence {
                Type::Array(actual) => {
                    return budget.consume()
                        && is_assignable(actual, &element, aliases, visited, budget)
                }
                Type::Tuple(actual) => {
                    return budget.consume()
                        && actual.iter().all(|part| {
                            is_assignable(
                                &part.indexed_type(),
                                &element,
                                aliases,
                                &mut visited.clone(),
                                budget,
                            )
                        })
                }
                _ => {}
            }
        }
    }
    if let (Some(actual), Some(expected)) = (
        library_arguments(actual, &["Iterable"]),
        library_arguments(expected, &["Iterable"]),
    ) {
        if !budget.consume() {
            return false;
        }
        return is_assignable(&actual, &expected, aliases, visited, budget);
    }
    if let Some(result) = enum_assignability(actual, expected, aliases) {
        return result;
    }
    // An alias can hide an entire union, so expose its top-level shape before
    // distributing constituents. Keep library and nominal declaration names
    // intact for their directed argument comparisons.
    for (value, actual_side) in [(actual, true), (expected, false)] {
        if matches!(value, Type::Named { name, .. } if aliases.get(name).is_some_and(|definition| definition.kind == TypeDefinitionKind::Alias))
        {
            let mut path = visited.clone();
            if let Some(expanded) = instantiate_named(
                value,
                aliases,
                &mut path,
                budget,
                if actual_side {
                    "actual alias"
                } else {
                    "expected alias"
                },
            ) {
                return if actual_side {
                    is_assignable(&expanded, expected, aliases, &mut path, budget)
                } else {
                    is_assignable(actual, &expanded, aliases, &mut path, budget)
                };
            }
        }
    }
    // Compare named union constituents before erasing their generic identity
    // into structural members. This retains variance and recursion guards.
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
    if let Some(result) = string_mapping::assignable(actual, expected, aliases, visited, budget) {
        return result;
    }
    if let Some(expanded) = instantiate_named(actual, aliases, visited, budget, "actual") {
        return is_assignable(&expanded, expected, aliases, visited, budget);
    }
    let mut expected_visited = visited.clone();
    let expected_side = if matches!(expected, Type::Named { .. }) {
        format!("expected for {}", type_identity(actual))
    } else {
        "expected".to_string()
    };
    if let Some(expanded) = instantiate_named(
        expected,
        aliases,
        &mut expected_visited,
        budget,
        &expected_side,
    ) {
        return is_assignable(actual, &expanded, aliases, &mut expected_visited, budget);
    }
    if !budget.checking.strict_null_checks
        && matches!(actual, Type::Null | Type::Undefined)
        && !matches!(expected, Type::Never)
    {
        return true;
    }
    if let Type::Intersection(parts) = expected {
        return parts
            .iter()
            .all(|part| is_assignable(actual, part, aliases, &mut visited.clone(), budget));
    }
    if let (Type::Intersection(_), Type::Record(expected_fields)) = (actual, expected) {
        return expected_fields.iter().all(|expected_field| {
            match property_type(
                actual,
                &expected_field.name,
                aliases,
                &mut visited.clone(),
                budget,
            ) {
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
    if let Type::TemplateLiteral(template) = expected {
        return super::type_operators::template_matches(actual, template, aliases, budget);
    }
    match (actual, expected) {
        (Type::Predicate(actual), Type::Predicate(expected)) => {
            actual.asserts == expected.asserts
                && match (actual.target.as_deref(), expected.target.as_deref()) {
                    (Some(actual), Some(expected)) => {
                        is_assignable(actual, expected, aliases, visited, budget)
                    }
                    (None, None) => true,
                    (Some(_), None) => expected.asserts,
                    _ => false,
                }
        }
        (Type::Predicate(actual), _) => {
            is_assignable(&actual.runtime_type(), expected, aliases, visited, budget)
        }
        (Type::StringLiteral(_) | Type::TemplateLiteral(_), Type::String) => true,
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
        ) => {
            // A callback may ignore trailing arguments supplied by its caller.
            // Every parameter it does consume must still be compatible.
            parameters::compatible(actual_parameters, expected_parameters, aliases, visited, budget)
                // A function type whose result is `void` accepts a function that
                // returns anything: its result is simply ignored.
                && match (actual_result.as_ref(), expected_result.as_ref()) {
                    (_, Type::Void) => true,
                    (_, Type::Predicate(predicate)) if predicate.asserts => true,
                    (Type::Predicate(actual), Type::Predicate(expected)) => {
                        let actual_index = actual_parameters
                            .iter()
                            .position(|parameter| parameter.name == actual.parameter);
                        let expected_index = expected_parameters
                            .iter()
                            .position(|parameter| parameter.name == expected.parameter);
                        actual_index == expected_index
                            && (actual_index.is_some()
                                || (actual.parameter == "this" && expected.parameter == "this"))
                            && is_assignable(
                                actual_result,
                                expected_result,
                                aliases,
                                visited,
                                budget,
                            )
                    }
                    _ => is_assignable(actual_result, expected_result, aliases, visited, budget),
                }
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
        (Type::Record(actual), Type::Record(expected)) => {
            record_fields_assignable(actual, expected, aliases, visited, budget, true)
        }
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
    if definition.kind == TypeDefinitionKind::Parameter {
        return None;
    }
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
        Type::Any | Type::Unknown | Type::StrictUnknown => true,
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
    if let Type::InferenceBarrier(base) = value {
        return budget.consume().then(|| (**base).clone());
    }
    if matches!(
        value,
        Type::Conditional(_)
            | Type::StringMapping(_)
            | Type::Infer(_)
            | Type::Mapped(_)
            | Type::TemplateLiteral(_)
            | Type::KeyOf(_)
            | Type::IndexedAccess { .. }
    ) {
        return super::type_operators::resolve(value, aliases, visited, budget)
            .filter(|resolved| !matches!(value, Type::StringMapping(_)) || resolved != value);
    }
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
    if let Type::Intrinsic(intrinsic) = &definition.value {
        use crate::parser::IntrinsicKind;
        return match intrinsic.kind {
            IntrinsicKind::StringMapping(operation) => {
                let [operand] = arguments.as_slice() else {
                    return None;
                };
                Some(Type::StringMapping(Box::new(
                    crate::parser::StringMappingType {
                        operation,
                        origin: intrinsic.origin.clone(),
                        operand: operand.clone(),
                    },
                )))
            }
            IntrinsicKind::NoInfer => {
                let [operand] = arguments.as_slice() else {
                    return None;
                };
                Some(Type::inference_barrier(operand.clone()))
            }
            IntrinsicKind::BuiltinIteratorReturn => {
                Some(if budget.checking.strict_builtin_iterator_return {
                    Type::Undefined
                } else {
                    Type::Any
                })
            }
        };
    }
    let arguments =
        super::type_operators::arguments::prepare(definition, arguments, aliases, visited, budget)?;
    let substitutions = definition
        .parameters
        .iter()
        .map(|parameter| parameter.name.clone())
        .zip(arguments)
        .map(|(name, value)| {
            let value = if definition.kind == TypeDefinitionKind::Class {
                canonical_parameter_references(&value, aliases)
            } else {
                value
            };
            (name, value)
        })
        .collect();
    crate::performance::instantiated();
    Some(substitute_type(&definition.value, &substitutions))
}
