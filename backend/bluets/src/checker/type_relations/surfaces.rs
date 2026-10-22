// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Array and primitive syntax expose the same parsed members as property reads.
use super::*;

pub(super) fn assignable(
    actual: &Type,
    expected: &Type,
    aliases: &BTreeMap<String, TypeDefinition>,
    visited: &mut HashSet<String>,
    budget: &mut TypeExpansionBudget,
) -> Option<bool> {
    if !matches!(
        actual,
        Type::Array(_)
            | Type::Tuple(_)
            | Type::String
            | Type::Number
            | Type::Boolean
            | Type::BigInt
            | Type::Symbol
            | Type::UniqueSymbol(_)
            | Type::Literal(_)
            | Type::Function { .. }
            | Type::GenericFunction { .. }
    ) {
        return None;
    }
    let Type::Record(fields) = expected else {
        return None;
    };
    let mut source = Vec::new();
    for field in fields {
        match property_type(actual, &field.name, aliases, &mut visited.clone(), budget) {
            PropertyType::Found { value, readonly } => source.push(TypeField {
                value,
                readonly,
                name: field.name.clone(),
                optional: false,
                method: field.method,
                accessor_write_type: None,
                span: field.span.clone(),
            }),
            PropertyType::Missing => {}
            PropertyType::Indeterminate => return None,
            PropertyType::Exhausted => return Some(false),
        }
    }
    if source.is_empty() && !fields.is_empty() && fields.iter().all(|field| field.optional) {
        return Some(false);
    }
    Some(compatibility::record_fields_assignable(
        &source, fields, aliases, visited, budget, true,
    ))
}
