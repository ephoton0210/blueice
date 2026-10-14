// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Property facts stay separate from the type of their owning object.

use super::*;

pub(super) fn names(path: &[(String, bool)]) -> Vec<String> {
    path.iter().map(|(name, _)| name.clone()).collect()
}

pub(super) fn read(
    scopes: &ScopeModel<'_>,
    state: &State,
    id: &BindingId,
    path: &[(String, bool)],
) -> Option<Type> {
    state
        .properties
        .get(&(id.clone(), names(path)))
        .cloned()
        .or_else(|| project(scopes, &current(scopes, state, id), path))
}

pub(super) fn project(
    scopes: &ScopeModel<'_>,
    value: &Type,
    path: &[(String, bool)],
) -> Option<Type> {
    let mut result = value.clone();
    let mut short_circuit = false;
    let mut budget = TypeExpansionBudget::new(scopes.max_type_expansions);
    for (key, optional) in path {
        let mut values = Vec::new();
        for part in predicates::parts(scopes, &result) {
            if matches!(part, Type::Null | Type::Undefined)
                && (*optional || (short_circuit && part == Type::Undefined))
            {
                values.push(Type::Undefined);
                short_circuit = true;
                continue;
            }
            match property_type(
                &part,
                key,
                &scopes.type_definitions,
                &mut HashSet::new(),
                &mut budget,
            ) {
                PropertyType::Found { value, .. } => values.push(value),
                PropertyType::Missing => {}
                PropertyType::Indeterminate => values.push(Type::Unknown),
                PropertyType::Exhausted => {
                    scopes.flow_limit(
                        &[],
                        "flow property lookup exceeds its generic-expansion limit",
                    );
                    return Some(Type::Unknown);
                }
            }
        }
        result = union(values);
    }
    (result != Type::Never).then_some(result)
}

/// A setter call does not establish the value subsequently returned by its getter.
pub(super) fn accessor_target(
    scopes: &ScopeModel<'_>,
    value: &Type,
    path: &[(String, bool)],
) -> bool {
    let Some((last, prefix)) = path.split_last() else {
        return false;
    };
    let owner = if prefix.is_empty() {
        Some(value.clone())
    } else {
        project(scopes, value, prefix)
    };
    owner.is_some_and(|owner| predicates::parts(scopes, &owner).iter().any(|part| {
        matches!(predicates::expand(scopes, part).object_type(), Type::Record(fields) | Type::CallableRecord { fields, .. }
            if fields.iter().any(|field| field.name == last.0 && field.accessor_write_type.is_some()))
    }))
}

pub(super) fn derives(scopes: &ScopeModel<'_>, name: &str, base: &str) -> bool {
    let mut current = name;
    for _ in 0..scopes.max_type_expansions {
        if current == base {
            return true;
        }
        let Some(parent) =
            scopes
                .module
                .declarations
                .iter()
                .find_map(|declaration| match declaration {
                    Declaration::Class(class) if class.name == current => {
                        class.extends_name.as_deref()
                    }
                    _ => None,
                })
        else {
            return false;
        };
        current = parent;
    }
    scopes.flow_limit(
        &[],
        "flow inheritance lookup exceeds its generic-expansion limit",
    );
    false
}
