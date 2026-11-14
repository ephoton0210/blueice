// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Tuple candidates preserve fixed endpoints and the inferred middle sequence.

use super::*;

pub(super) fn tuple(
    actual: &[TupleTypeElement],
    pattern: &[TupleTypeElement],
    bindings: &mut MatchBindings,
    aliases: &BTreeMap<String, TypeDefinition>,
    budget: &mut TypeExpansionBudget,
) -> bool {
    let rest = pattern.iter().position(|element| element.rest);
    let prefix = rest.unwrap_or(pattern.len());
    let suffix = rest.map_or(0, |index| pattern.len() - index - 1);
    let mut required = 0;
    for element in pattern {
        if !budget.consume() {
            return false;
        }
        required += usize::from(!element.optional && !element.rest);
    }
    if actual.len() < required || (rest.is_none() && actual.len() > pattern.len()) {
        return false;
    }
    let available_prefix = actual.len().saturating_sub(suffix);
    let mut absent_optional = false;
    for (index, expected) in pattern[..prefix].iter().enumerate() {
        let source = actual.get(index).filter(|_| index < available_prefix);
        let Some(source) = source else {
            if !expected.optional {
                return false;
            }
            absent_optional = true;
            continue;
        };
        if (source.optional && !expected.optional)
            || !super::matches(
                &source.annotation,
                &expected.annotation,
                bindings,
                aliases,
                budget,
            )
        {
            return false;
        }
    }
    let Some(rest_index) = rest else {
        return true;
    };
    for offset in 0..suffix {
        let source = &actual[actual.len() - suffix + offset];
        let expected = &pattern[rest_index + 1 + offset];
        if !super::matches(
            &source.annotation,
            &expected.annotation,
            bindings,
            aliases,
            budget,
        ) {
            return false;
        }
    }
    let middle = &actual[prefix.min(available_prefix)..available_prefix];
    let rest_type = &pattern[rest_index].annotation;
    if absent_optional && middle.is_empty() {
        if let Type::Infer(parameter) = rest_type {
            // No optional source position supplied evidence for the rest slot.
            // Its implicit array constraint remains unknown[], rather than [].
            bindings.missing_rest(&parameter.name);
            return true;
        }
    }
    match rest_type {
        Type::Array(element) => middle.iter().all(|source| {
            super::matches(&source.indexed_type(), element, bindings, aliases, budget)
        }),
        expected => {
            let source = if let [element] = middle {
                if element.rest {
                    element.annotation.clone()
                } else {
                    Type::Tuple(middle.to_vec())
                }
            } else {
                Type::Tuple(middle.to_vec())
            };
            super::matches(&source, expected, bindings, aliases, budget)
        }
    }
}
