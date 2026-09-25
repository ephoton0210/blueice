// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Bounded-type inference for a supported computed receiver.

use super::*;

/// Preserve every declared possibility rather than losing readonly types in
/// an inferred local alias. A canonical index selects one tuple element;
/// otherwise a heterogeneous container yields a union of its value types.
pub(super) fn indexed_value_type(
    value: &Type,
    index: Option<usize>,
    aliases: &BTreeMap<String, TypeDefinition>,
    visited: &mut HashSet<String>,
    budget: &mut TypeExpansionBudget,
) -> Type {
    match value {
        Type::Array(element) => (**element).clone(),
        Type::Tuple(values) => index.map_or_else(
            || alternatives_type(values.to_vec()),
            |index| values.get(index).cloned().unwrap_or(Type::Unknown),
        ),
        Type::Record(fields) if index.is_none() => alternatives_type(
            fields
                .iter()
                .map(|field| {
                    if field.optional {
                        Type::Union(vec![field.value.clone(), Type::Undefined])
                    } else {
                        field.value.clone()
                    }
                })
                .collect(),
        ),
        Type::Union(parts) => alternatives_type(
            parts
                .iter()
                .map(|part| indexed_value_type(part, index, aliases, &mut visited.clone(), budget))
                .collect(),
        ),
        Type::Named { .. } => {
            match instantiate_named(value, aliases, visited, budget, "computed receiver") {
                Some(instantiated) => {
                    indexed_value_type(&instantiated, index, aliases, visited, budget)
                }
                None => Type::Unknown,
            }
        }
        _ => Type::Unknown,
    }
}

pub(super) fn canonical_index_key(tokens: &[Token]) -> Option<usize> {
    let [key] = tokens else { return None };
    let text = match key.kind {
        TokenKind::Number => key.text.as_str(),
        TokenKind::String => unescaped_property_name(&key.text)?,
        _ => return None,
    };
    let index = text.parse::<u32>().ok()?;
    (index != u32::MAX && index.to_string() == text).then_some(index as usize)
}

fn alternatives_type(values: Vec<Type>) -> Type {
    let Some(first) = values.first() else {
        return Type::Unknown;
    };
    if values.iter().all(|value| value == first) {
        first.clone()
    } else {
        Type::Union(values)
    }
}
