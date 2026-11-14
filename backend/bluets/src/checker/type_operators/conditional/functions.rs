// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Receiver inference is separate from the positional runtime argument list.

use super::*;

pub(super) fn receiver(
    actual: &[Parameter],
    pattern: &[Parameter],
    bindings: &mut MatchBindings,
    aliases: &BTreeMap<String, TypeDefinition>,
    budget: &mut TypeExpansionBudget,
) -> bool {
    let Some(expected) = pattern.iter().find(|parameter| parameter.name == "this") else {
        return true;
    };
    let expected = expected.annotation.as_ref().unwrap_or(&Type::Any);
    let actual = actual
        .iter()
        .find(|parameter| parameter.name == "this")
        .and_then(|parameter| parameter.annotation.as_ref());
    if !expected.infer_parameters().is_empty() {
        // An absent receiver contributes no evidence: Native infers unknown.
        // It is not a positional parameter and must not shift argument labels.
        bindings.parameter(
            actual.unwrap_or(&Type::StrictUnknown),
            expected,
            aliases,
            budget,
        )
    } else {
        actual.is_none_or(|actual| {
            is_assignable(expected, actual, aliases, &mut HashSet::new(), budget)
        })
    }
}

pub(super) fn matches(
    actual: &[Parameter],
    result: &Type,
    pattern: &[Parameter],
    expected: &Type,
    bindings: &mut MatchBindings,
    aliases: &BTreeMap<String, TypeDefinition>,
    budget: &mut TypeExpansionBudget,
) -> bool {
    if !receiver(actual, pattern, bindings, aliases, budget) {
        return false;
    }
    let actual = actual
        .iter()
        .filter(|parameter| parameter.name != "this")
        .collect::<Vec<_>>();
    let pattern = pattern
        .iter()
        .filter(|parameter| parameter.name != "this")
        .collect::<Vec<_>>();
    if let [rest] = pattern.as_slice() {
        if rest.rest {
            if let Some(Type::Infer(parameter)) = &rest.annotation {
                let tuple = Type::Tuple(
                    actual
                        .iter()
                        .map(|parameter| TupleTypeElement {
                            annotation: parameter.annotation.clone().unwrap_or(Type::Any),
                            optional: parameter.optional,
                            label: Some(parameter.name.clone()),
                            rest: parameter.rest,
                        })
                        .collect(),
                );
                if !bindings.parameter(&tuple, &Type::Infer(parameter.clone()), aliases, budget) {
                    return false;
                }
            }
            return returns(result, expected, bindings, aliases, budget);
        }
    }
    if let Some(rest_index) = pattern.iter().position(|parameter| parameter.rest) {
        let positional = actual
            .iter()
            .map(|parameter| TupleTypeElement {
                annotation: parameter.annotation.clone().unwrap_or(Type::Any),
                optional: parameter.optional,
                label: Some(parameter.name.clone()),
                rest: parameter.rest,
            })
            .collect::<Vec<_>>();
        let Ok(positional) =
            expand_concrete_tuple_spreads(&positional, aliases, &mut HashSet::new(), budget)
        else {
            return false;
        };
        let mut consumed = 0;
        for parameter in &pattern[..rest_index] {
            let Some(actual) = positional.get(consumed) else {
                // A shorter function can ignore the caller's extra arguments.
                // resolve supplies defaults for infer slots with no evidence.
                continue;
            };
            let annotation = if actual.rest {
                match &actual.annotation {
                    Type::Array(item) => (**item).clone(),
                    _ => return false,
                }
            } else {
                consumed += 1;
                if actual.optional && budget.checking.strict_null_checks {
                    union([actual.annotation.clone(), Type::Undefined])
                } else {
                    actual.annotation.clone()
                }
            };
            if !bindings.parameter(
                &annotation,
                parameter.annotation.as_ref().unwrap_or(&Type::Any),
                aliases,
                budget,
            ) {
                return false;
            }
        }
        let tail = Type::Tuple(positional[consumed..].to_vec());
        return bindings.parameter(
            &tail,
            pattern[rest_index]
                .annotation
                .as_ref()
                .unwrap_or(&Type::Any),
            aliases,
            budget,
        ) && returns(result, expected, bindings, aliases, budget);
    }
    actual.iter().zip(pattern).all(|(actual, pattern)| {
        let mut annotation = actual.annotation.clone().unwrap_or(Type::Any);
        if actual.optional && budget.checking.strict_null_checks {
            annotation = union([annotation, Type::Undefined]);
        }
        bindings.parameter(
            &annotation,
            pattern.annotation.as_ref().unwrap_or(&Type::Any),
            aliases,
            budget,
        )
    }) && returns(result, expected, bindings, aliases, budget)
}

fn returns(
    actual: &Type,
    pattern: &Type,
    bindings: &mut MatchBindings,
    aliases: &BTreeMap<String, TypeDefinition>,
    budget: &mut TypeExpansionBudget,
) -> bool {
    // A void-returning callback permits a value that its caller ignores.
    *pattern == Type::Void || super::matches(actual, pattern, bindings, aliases, budget)
}
