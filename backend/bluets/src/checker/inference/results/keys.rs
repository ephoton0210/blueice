// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Key aliases materialize symbolic set operations without enumerating arrays.
use super::*;

pub(super) fn alias(name: &str, aliases: &BTreeMap<String, TypeDefinition>) -> bool {
    let mut name = name;
    let mut visited = HashSet::new();
    while visited.insert(name) {
        let Some(definition) = aliases
            .get(name)
            .filter(|d| d.kind == TypeDefinitionKind::Alias)
        else {
            return false;
        };
        match &definition.value {
            Type::KeyOf(_) => return true,
            Type::Named { name: next, .. } => name = next,
            _ => return false,
        }
    }
    false
}

pub(super) fn retained(value: &Type, aliases: &BTreeMap<String, TypeDefinition>) -> Option<Type> {
    let Type::KeyOf(operand) = value else {
        return None;
    };
    let normalized = crate::checker::type_relations::normalize_more_type(operand, aliases)
        .unwrap_or_else(|| (**operand).clone());
    (matches!(normalized,Type::Array(_) | Type::Tuple(_))
        || matches!(&normalized,Type::Readonly(inner)if matches!(inner.as_ref(),Type::Array(_) | Type::Tuple(_))))
        .then(||Type::KeyOf(Box::new(normalized)))
}
