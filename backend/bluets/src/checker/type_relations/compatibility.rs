// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Callable objects and weak records retain structural member rules.
use super::*;

/// Instantiate a generic source constructor against a concrete target using the
/// same bounded candidates and constraints as a contextual generic call.
pub(in crate::checker) fn constructor_relation_type(
    source: &crate::parser::TypeSignature,
    target: &crate::parser::TypeSignature,
    aliases: &BTreeMap<String, TypeDefinition>,
    budget: &mut TypeExpansionBudget,
) -> (Type, bool) {
    if !source.construct || source.type_parameters.is_empty() || !target.type_parameters.is_empty()
    {
        return (source.function_type(), true);
    }
    let signature = FunctionSignature {
        parameters: source.parameters.clone(),
        type_parameters: source.type_parameters.clone(),
        return_type: source.result.clone(),
    };
    let actuals = target
        .parameters
        .iter()
        .map(|parameter| {
            let value = parameter.annotation.clone().unwrap_or(Type::Any);
            if parameter.rest {
                match value {
                    Type::Array(element) => *element,
                    other => other,
                }
            } else {
                value
            }
        })
        .collect::<Vec<_>>();
    let inference = crate::checker::inference::infer_contextual_result(
        &signature,
        &actuals,
        aliases,
        budget,
        Some(&target.result),
    );
    let function = substitute_type(
        &Type::Function {
            parameters: source.parameters.clone(),
            result: Box::new(source.result.clone()),
        },
        &inference.substitutions,
    );
    (
        function,
        inference.rejected_constraint.is_none() && !budget.exhausted,
    )
}

pub(super) fn weak_pair(actual: &[TypeField], expected: &[TypeField]) -> bool {
    !actual.is_empty()
        && !expected.is_empty()
        && expected.iter().all(|field| field.optional)
        && !actual
            .iter()
            .any(|field| expected.iter().any(|target| target.name == field.name))
}

pub(in crate::checker) fn record_fields_assignable(
    actual: &[TypeField],
    expected: &[TypeField],
    aliases: &BTreeMap<String, TypeDefinition>,
    visited: &mut HashSet<String>,
    budget: &mut TypeExpansionBudget,
    check_weak: bool,
) -> bool {
    if check_weak && weak_pair(actual, expected) {
        return false;
    }
    expected.iter().all(|expected| {
        actual
            .iter()
            .find(|field| field.name == expected.name)
            .map_or(expected.optional, |actual| {
                if actual.optional && !expected.optional {
                    return false;
                }
                let target = if expected.optional && !budget.checking.exact_optional_property_types
                {
                    union([expected.value.clone(), Type::Undefined])
                } else {
                    expected.value.clone()
                };
                let source = if actual.optional
                    && budget.checking.strict_null_checks
                    && !budget.checking.exact_optional_property_types
                {
                    union([actual.value.clone(), Type::Undefined])
                } else {
                    actual.value.clone()
                };
                let checking = budget.checking;
                if expected.method {
                    budget.checking.strict_function_types = false;
                }
                let result = is_assignable(&source, &target, aliases, &mut visited.clone(), budget);
                budget.checking = checking;
                result
            })
    })
}

pub(super) fn assignable(
    actual: &Type,
    expected: &Type,
    aliases: &BTreeMap<String, TypeDefinition>,
    visited: &mut HashSet<String>,
    budget: &mut TypeExpansionBudget,
) -> Option<bool> {
    if let (
        Type::IndexedRecord {
            object: actual_object,
            indices: actual_indices,
        },
        Type::IndexedRecord {
            object: expected_object,
            indices: expected_indices,
        },
    ) = (actual, expected)
    {
        let objects = match (actual_object.as_ref(), expected_object.as_ref()) {
            (Type::Record(a), Type::Record(e)) => {
                record_fields_assignable(a, e, aliases, &mut visited.clone(), budget, false)
            }
            _ => is_assignable(
                actual_object,
                expected_object,
                aliases,
                &mut visited.clone(),
                budget,
            ),
        };
        return Some(
            objects
                && expected_indices.iter().all(|expected| {
                    actual_indices.iter().any(|actual| {
                        (actual.key == expected.key
                            || (actual.key == Type::String && expected.key == Type::Number))
                            && is_assignable(
                                &actual.value,
                                &expected.value,
                                aliases,
                                &mut visited.clone(),
                                budget,
                            )
                    })
                }),
        );
    }
    match (actual, expected) {
        (
            Type::CallableRecord {
                fields: a,
                signatures: ac,
            },
            Type::CallableRecord {
                fields: e,
                signatures: ec,
            },
        ) => Some(
            record_fields_assignable(a, e, aliases, visited, budget, false)
                && ec.iter().all(|expected| {
                    ac.iter().any(|actual| {
                        actual.construct == expected.construct
                            && (!actual.abstract_constructor || expected.abstract_constructor)
                            && (!actual.construct
                                || actual.constructor_visibility == expected.constructor_visibility)
                            && {
                                let (function, valid) =
                                    constructor_relation_type(actual, expected, aliases, budget);
                                valid
                                    && is_assignable(
                                        &function,
                                        &expected.function_type(),
                                        aliases,
                                        &mut visited.clone(),
                                        budget,
                                    )
                            }
                    })
                }),
        ),
        (
            Type::Function { .. } | Type::GenericFunction { .. },
            Type::CallableRecord { fields, signatures },
        ) => Some(
            fields.iter().all(|field| field.optional)
                && signatures.iter().all(|signature| {
                    !signature.construct
                        && is_assignable(
                            actual,
                            &signature.function_type(),
                            aliases,
                            &mut visited.clone(),
                            budget,
                        )
                }),
        ),
        (
            Type::CallableRecord { signatures, .. },
            Type::Function { .. } | Type::GenericFunction { .. },
        ) => Some(signatures.iter().any(|signature| {
            !signature.construct
                && is_assignable(
                    &signature.function_type(),
                    expected,
                    aliases,
                    &mut visited.clone(),
                    budget,
                )
        })),
        (Type::CallableRecord { fields, .. }, Type::Record(expected)) => Some(
            record_fields_assignable(fields, expected, aliases, visited, budget, true),
        ),
        _ => None,
    }
}
