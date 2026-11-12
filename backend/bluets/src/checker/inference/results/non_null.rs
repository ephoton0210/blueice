// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Preserve alias surfaces when a reduced empty-record intersection is a union.
use super::*;

pub(super) fn materialize(
    value: &Type,
    aliases: &BTreeMap<String, TypeDefinition>,
    budget: &mut TypeExpansionBudget,
) -> Option<Type> {
    let Type::Named { name, arguments } = value else {
        return None;
    };
    if !crate::checker::type_relations::non_null_alias(name, aliases) {
        return None;
    }
    let mut operand = value.clone();
    let mut active = HashSet::new();
    while matches!(&operand, Type::Named { name, .. } if aliases.get(name).is_some_and(|d| d.kind == TypeDefinitionKind::Alias))
    {
        let Some(next) = instantiate_named(
            &operand,
            aliases,
            &mut active,
            budget,
            "inferred intersection",
        ) else {
            break;
        };
        if next == operand {
            break;
        }
        operand = next;
    }
    let reduced = crate::checker::type_relations::normalize_intersection(&operand, aliases, budget)
        .unwrap_or(operand);
    if budget.exhausted {
        return Some(value.clone());
    }
    let same_intersection_argument = matches!(reduced, Type::Intersection(_))
        && arguments.len() == 1
        && crate::checker::type_operators::normalize_nullable_union(&arguments[0], budget.checking)
            .as_ref()
            .unwrap_or(&arguments[0])
            == &reduced;
    if matches!(&reduced, Type::Union(parts) if parts.len() > 1) || same_intersection_argument {
        let arguments = arguments
            .iter()
            .map(|argument| {
                // An alias argument retains its union allocation order. Finish
                // each constituent without sorting the complete authored union.
                let argument = if let Type::Union(parts) = argument {
                    crate::checker::type_operators::union(
                        parts
                            .iter()
                            .map(|part| super::finish(part.clone(), aliases, budget)),
                    )
                } else {
                    super::finish(argument.clone(), aliases, budget)
                };
                match argument {
                    Type::Union(parts) if !budget.checking.strict_null_checks => {
                        crate::checker::type_operators::union(
                            parts
                                .into_iter()
                                .filter(|part| !matches!(part, Type::Null | Type::Undefined)),
                        )
                    }
                    value => value,
                }
            })
            .collect();
        Some(Type::Named {
            name: name.clone(),
            arguments,
        })
    } else if reduced == *value {
        Some(value.clone())
    } else {
        Some(super::finish(reduced, aliases, budget))
    }
}
