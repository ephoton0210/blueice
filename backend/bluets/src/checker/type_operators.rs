// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Bounded type operators share the structural relation's expansion budget.

use super::*;
use crate::parser::{ConditionalType, MappedModifier, MappedType, TemplateLiteralType};

mod conditional;
mod keys;
mod literal_order;
mod mapped;
mod templates;

pub(super) fn expanded(
    value: &Type,
    aliases: &BTreeMap<String, TypeDefinition>,
    visited: &mut HashSet<String>,
    budget: &mut TypeExpansionBudget,
) -> Type {
    let mut value = value.clone();
    loop {
        if let Some(next) = super::type_relations::normalize_more_type(&value, aliases) {
            if !budget.consume() {
                break;
            }
            value = next;
            continue;
        }
        let Some(next) = instantiate_named(&value, aliases, visited, budget, "operator operand")
        else {
            break;
        };
        if next == value {
            break;
        }
        value = next;
    }
    value
}

fn union(values: impl IntoIterator<Item = Type>) -> Type {
    fn add(value: Type, into: &mut Vec<Type>) {
        match value {
            Type::Never => {}
            Type::Union(values) => values.into_iter().for_each(|value| add(value, into)),
            value => {
                if !into.contains(&value) {
                    into.push(value);
                }
            }
        }
    }
    let mut parts = Vec::new();
    values.into_iter().for_each(|value| add(value, &mut parts));
    match parts.len() {
        0 => Type::Never,
        1 => parts.pop().expect("one union constituent"),
        _ => Type::Union(parts),
    }
}

pub(super) fn resolve(
    value: &Type,
    aliases: &BTreeMap<String, TypeDefinition>,
    visited: &mut HashSet<String>,
    budget: &mut TypeExpansionBudget,
) -> Option<Type> {
    if !budget.consume() {
        return None;
    }
    match value {
        Type::KeyOf(value) => keys::keyof(value, aliases, visited, budget),
        Type::IndexedAccess { object, index, .. } => {
            keys::indexed(object, index, aliases, visited, budget)
        }
        Type::Conditional(value) => conditional::resolve(value, aliases, visited, budget),
        Type::Mapped(value) => mapped::resolve(value, aliases, visited, budget),
        Type::TemplateLiteral(value) => templates::resolve(value, aliases, visited, budget),
        _ => None,
    }
}

pub(super) fn template_matches(
    actual: &Type,
    expected: &TemplateLiteralType,
    aliases: &BTreeMap<String, TypeDefinition>,
    budget: &mut TypeExpansionBudget,
) -> bool {
    templates::matches(actual, expected, &mut BTreeMap::new(), aliases, budget)
}

/// Diagnostic aliases retain their name for records and ordinary unions;
/// reduced operator scalars and literal products display the selected result.
pub(super) fn diagnostic_type(
    value: &Type,
    aliases: &BTreeMap<String, TypeDefinition>,
    budget: &mut TypeExpansionBudget,
) -> Type {
    let original = value.clone();
    let mut value = value.clone();
    let mut visited = HashSet::new();
    let mut operator = false;
    let mut indexed = false;
    loop {
        if let Type::KeyOf(operand) = &value {
            let operand = expanded(operand, aliases, &mut HashSet::new(), budget);
            if matches!(operand, Type::Array(_) | Type::Tuple(_)) {
                return Type::KeyOf(Box::new(operand));
            }
        }
        operator |= matches!(
            value,
            Type::Conditional(_)
                | Type::KeyOf(_)
                | Type::IndexedAccess { .. }
                | Type::TemplateLiteral(_)
        );
        indexed |= matches!(value, Type::IndexedAccess { .. });
        if let Type::Named { name, .. } = &value {
            let Some(definition) = aliases.get(name) else {
                break;
            };
            if !operator
                && matches!(
                    definition.value,
                    Type::Union(_)
                        | Type::Record(_)
                        | Type::CallableRecord { .. }
                        | Type::Mapped(_)
                )
            {
                return original;
            }
        }
        let Some(next) =
            instantiate_named(&value, aliases, &mut visited, budget, "diagnostic operator")
        else {
            break;
        };
        if next == value {
            break;
        }
        value = next;
    }
    if let Type::Union(parts) = value {
        let parts = parts
            .into_iter()
            .map(|part| diagnostic_type(&part, aliases, budget))
            .collect::<Vec<_>>();
        value = union(parts);
    }
    if matches!(
        value,
        Type::Record(_) | Type::CallableRecord { .. } | Type::Mapped(_)
    ) || (indexed && matches!(value, Type::Union(_)))
    {
        original
    } else {
        value
    }
}

/// Inspect alias syntax without changing ordinary literal-union inference.
pub(super) fn is_operator_context(
    value: &Type,
    aliases: &BTreeMap<String, TypeDefinition>,
) -> bool {
    let mut value = value;
    let mut visited = HashSet::new();
    loop {
        if value.operator_children().is_some()
            || matches!(value, Type::KeyOf(_) | Type::IndexedAccess { .. })
        {
            return true;
        }
        let Type::Named { name, .. } = value else {
            return false;
        };
        if !visited.insert(name) {
            return false;
        }
        let Some(definition) = aliases.get(name) else {
            return false;
        };
        value = &definition.value;
    }
}
