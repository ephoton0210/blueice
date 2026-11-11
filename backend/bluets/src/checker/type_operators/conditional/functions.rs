// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Receiver inference is separate from the positional runtime argument list.

use super::*;

pub(super) fn receiver(
    actual: &[Parameter],
    pattern: &[Parameter],
    bindings: &mut BTreeMap<String, Type>,
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
        super::matches(
            actual.unwrap_or(&Type::StrictUnknown),
            expected,
            bindings,
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
    bindings: &mut BTreeMap<String, Type>,
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
                if !super::matches(
                    &tuple,
                    &Type::Infer(parameter.clone()),
                    bindings,
                    aliases,
                    budget,
                ) {
                    return false;
                }
            }
            return super::matches(result, expected, bindings, aliases, budget);
        }
    }
    actual.len() == pattern.len()
        && actual.iter().zip(pattern).all(|(actual, pattern)| {
            super::matches(
                actual.annotation.as_ref().unwrap_or(&Type::Any),
                pattern.annotation.as_ref().unwrap_or(&Type::Any),
                bindings,
                aliases,
                budget,
            )
        })
        && super::matches(result, expected, bindings, aliases, budget)
}
