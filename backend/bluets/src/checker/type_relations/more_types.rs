// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Additional primitive and container relations share the expansion budget.
use super::*;

pub(in crate::checker) fn normalized(
    value: &Type,
    aliases: &BTreeMap<String, TypeDefinition>,
) -> Option<Type> {
    match value {
        Type::Named { name, arguments }
            if arguments.len() == 1
                && matches!(name.as_str(), "Array" | "ReadonlyArray")
                && aliases.get(name).is_some_and(|definition| {
                    definition.kind == TypeDefinitionKind::LibraryInterface
                }) =>
        {
            let array = Type::Array(Box::new(arguments[0].clone()));
            Some(if name == "ReadonlyArray" {
                Type::Readonly(Box::new(array))
            } else {
                array
            })
        }
        Type::Union(parts) if parts.contains(&Type::Any) => Some(Type::Any),
        Type::Union(parts) if parts.contains(&Type::StrictUnknown) => Some(Type::StrictUnknown),
        Type::Intersection(parts) if parts.contains(&Type::Never) => Some(Type::Never),
        Type::Intersection(parts) if parts.contains(&Type::StrictUnknown) => {
            let mut parts = parts
                .iter()
                .filter(|part| **part != Type::StrictUnknown)
                .cloned()
                .collect::<Vec<_>>();
            Some(match parts.len() {
                0 => Type::StrictUnknown,
                1 => parts.pop().expect("one constituent"),
                _ => Type::Intersection(parts),
            })
        }
        _ => None,
    }
}

pub(super) fn assignable(
    actual: &Type,
    expected: &Type,
    aliases: &BTreeMap<String, TypeDefinition>,
    visited: &mut HashSet<String>,
    budget: &mut TypeExpansionBudget,
) -> Option<bool> {
    if let Some(value) = normalized(actual, aliases) {
        return Some(is_assignable(&value, expected, aliases, visited, budget));
    }
    if let Some(value) = normalized(expected, aliases) {
        return Some(is_assignable(actual, &value, aliases, visited, budget));
    }
    if *expected == Type::StrictUnknown {
        return Some(true);
    }
    if *actual == Type::StrictUnknown
        && !matches!(
            expected,
            Type::Union(_) | Type::Intersection(_) | Type::Named { .. }
        )
    {
        return Some(matches!(expected, Type::Any | Type::Unknown));
    }
    match (actual, expected) {
        (Type::UniqueSymbol(_), Type::Symbol) => Some(true),
        (Type::UniqueSymbol(a), Type::UniqueSymbol(b)) => Some(a == b),
        (Type::Literal(value), Type::BigInt) => Some(value.ends_with('n')),
        (Type::Readonly(actual), Type::Readonly(expected)) => {
            Some(is_assignable(actual, expected, aliases, visited, budget))
        }
        (_, Type::Readonly(expected)) => {
            Some(is_assignable(actual, expected, aliases, visited, budget))
        }
        (Type::Readonly(_), Type::Array(_) | Type::Tuple(_)) => Some(false),
        (_, Type::IndexedRecord { object, indices }) => {
            let object_matches = match (actual, object.as_ref()) {
                (Type::Record(actual), Type::Record(expected)) => record_fields_assignable(
                    actual,
                    expected,
                    aliases,
                    &mut visited.clone(),
                    budget,
                    false,
                ),
                _ => is_assignable(actual, object, aliases, &mut visited.clone(), budget),
            };
            if !object_matches {
                return Some(false);
            }
            let fields = match actual {
                Type::Record(fields) | Type::CallableRecord { fields, .. } => fields,
                _ => return None,
            };
            Some(indices.iter().all(|index| {
                fields.iter().all(|field| {
                    if index.key == Type::Number && field.name.parse::<f64>().is_err() {
                        return true;
                    }
                    if !budget.consume() {
                        return false;
                    }
                    is_assignable(
                        &field.value,
                        &index.value,
                        aliases,
                        &mut visited.clone(),
                        budget,
                    )
                })
            }))
        }
        (Type::IndexedRecord { object, .. }, _) if !matches!(expected, Type::Named { .. }) => {
            Some(is_assignable(object, expected, aliases, visited, budget))
        }
        _ => None,
    }
}
