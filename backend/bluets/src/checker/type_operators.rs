// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Concrete record keys and projections needed by generic constraints.

use super::*;

fn expanded(
    value: &Type,
    aliases: &BTreeMap<String, TypeDefinition>,
    visited: &mut HashSet<String>,
    budget: &mut TypeExpansionBudget,
) -> Type {
    let mut value = value.clone();
    while let Some(next) = instantiate_named(&value, aliases, visited, budget, "operator operand") {
        value = next;
    }
    value
}

pub(super) fn resolve(
    value: &Type,
    aliases: &BTreeMap<String, TypeDefinition>,
    visited: &mut HashSet<String>,
    budget: &mut TypeExpansionBudget,
) -> Option<Type> {
    match value {
        Type::KeyOf(operand) => {
            if !budget.consume() {
                return None;
            }
            let Type::Record(fields) = expanded(operand, aliases, visited, budget) else {
                return None;
            };
            let keys = fields
                .iter()
                .map(|field| Type::Literal(format!("{:?}", field.name)))
                .collect::<Vec<_>>();
            Some(match keys.len() {
                0 => Type::Never,
                1 => keys.into_iter().next()?,
                _ => Type::Union(keys),
            })
        }
        Type::IndexedAccess { object, index } => {
            if !budget.consume() {
                return None;
            }
            let object = expanded(object, aliases, visited, budget);
            let index = expanded(index, aliases, visited, budget);
            let Type::Literal(key) = index else {
                return None;
            };
            let key = key.trim_matches(['\'', '"']);
            match object {
                Type::Record(fields) => {
                    let field = fields.into_iter().find(|field| field.name == key)?;
                    Some(if field.optional {
                        Type::Union(vec![field.value, Type::Undefined])
                    } else {
                        field.value
                    })
                }
                Type::Array(item) if key.parse::<usize>().is_ok() => Some(*item),
                Type::Tuple(items) => items
                    .get(key.parse::<usize>().ok()?)
                    .map(TupleTypeElement::indexed_type),
                _ => None,
            }
        }
        _ => None,
    }
}
