// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Symbolic relations compare definitions and compose mappings in order.

use super::*;

pub(super) fn assignable(
    actual: &Type,
    expected: &Type,
    aliases: &BTreeMap<String, TypeDefinition>,
    visited: &mut HashSet<String>,
    budget: &mut TypeExpansionBudget,
) -> Option<bool> {
    if !matches!(actual, Type::StringMapping(_)) && !matches!(expected, Type::StringMapping(_)) {
        return None;
    }
    for (value, actual_side) in [(actual, true), (expected, false)] {
        if !matches!(value, Type::StringMapping(_)) {
            continue;
        }
        let expanded = type_operators::expanded(value, aliases, &mut visited.clone(), budget);
        if budget.exhausted {
            return Some(false);
        }
        if expanded != *value {
            return Some(if actual_side {
                is_assignable(&expanded, expected, aliases, visited, budget)
            } else {
                is_assignable(actual, &expanded, aliases, visited, budget)
            });
        }
    }
    if let (Type::StringMapping(actual), Type::StringMapping(expected)) = (actual, expected) {
        return Some(
            actual.origin == expected.origin
                && actual.operation == expected.operation
                && is_assignable(&actual.operand, &expected.operand, aliases, visited, budget),
        );
    }
    if let Type::StringMapping(target) = expected {
        // Ordinary union/intersection and named-type routing retains its own
        // semantics. It will return here for each selected concrete source.
        if matches!(
            actual,
            Type::Named { .. } | Type::Union(_) | Type::Intersection(_)
        ) {
            return None;
        }
        let mut stack = vec![target.as_ref()];
        let mut base = &target.operand;
        while let Type::StringMapping(next) = base {
            if !budget.consume() {
                return Some(false);
            }
            stack.push(next);
            base = &next.operand;
        }
        let mut mapped = actual.clone();
        for operation in stack.into_iter().rev() {
            let application = Type::StringMapping(Box::new(crate::parser::StringMappingType {
                operand: mapped,
                ..operation.clone()
            }));
            mapped = type_operators::expanded(&application, aliases, &mut visited.clone(), budget);
            if budget.exhausted {
                return Some(false);
            }
        }
        return Some(
            type_identity(&mapped) == type_identity(actual)
                && is_assignable(actual, base, aliases, visited, budget),
        );
    }
    if let Type::StringMapping(source) = actual {
        if matches!(expected, Type::Named { .. } | Type::Intersection(_)) {
            return None;
        }
        if let Type::Named { name, .. } = &source.operand {
            if let Some(parameter) = aliases.get(name).filter(|definition| {
                definition.kind == TypeDefinitionKind::Parameter
                    && !matches!(definition.value, Type::Unknown | Type::StrictUnknown)
            }) {
                let application = Type::StringMapping(Box::new(crate::parser::StringMappingType {
                    operand: parameter.value.clone(),
                    ..(**source).clone()
                }));
                let constrained = type_operators::expanded(&application, aliases, visited, budget);
                return Some(is_assignable(
                    &constrained,
                    expected,
                    aliases,
                    visited,
                    budget,
                ));
            }
        }
        return Some(is_assignable(
            &Type::String,
            expected,
            aliases,
            visited,
            budget,
        ));
    }
    None
}
