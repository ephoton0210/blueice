// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Normalize an object operand one step at a time, stopping at real binders.
use super::*;

pub(super) fn normalize(
    value: &Type,
    aliases: &BTreeMap<String, TypeDefinition>,
    visited: &mut HashSet<String>,
    budget: &mut TypeExpansionBudget,
) -> Option<(Type, bool)> {
    let mut value = canonical_parameter_references(value, aliases);
    loop {
        if matches!(&value,Type::Named {name,arguments} if arguments.is_empty()
            && aliases.get(name).is_some_and(|d|d.kind==TypeDefinitionKind::Parameter))
        {
            return Some((value, true));
        }
        if let Type::Union(parts) = &value {
            let flattened = union(parts.clone());
            if flattened != value {
                if !budget.consume() {
                    return None;
                }
                value = flattened;
                continue;
            }
        }
        let normalized = super::super::normalize_nullable_union(&value, budget.checking)
            .or_else(|| crate::checker::type_relations::normalize_more_type(&value, aliases));
        if let Some(next) = normalized {
            if !budget.consume() {
                return None;
            }
            value = next;
            continue;
        }
        if let Some(next) =
            crate::checker::type_relations::normalize_intersection(&value, aliases, budget)
        {
            if next != value {
                value = next;
                continue;
            }
        }
        if let Type::Conditional(conditional) = &value {
            if crate::checker::inference::depends_on_parameter(&conditional.check, aliases, budget)
            {
                return Some((value, true));
            }
        }
        let Some(next) = instantiate_named(&value, aliases, visited, budget, "keyof operand")
        else {
            break;
        };
        if next == value {
            break;
        }
        value = canonical_parameter_references(&next, aliases);
    }
    Some((value, false))
}
