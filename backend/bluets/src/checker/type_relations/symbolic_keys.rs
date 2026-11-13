// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Key sets retain their declaring parameter and directed constraint evidence.
use super::*;

fn parameter(value: &Type, aliases: &BTreeMap<String, TypeDefinition>) -> Option<String> {
    let Type::Named { name, arguments } = canonical_parameter_references(value, aliases) else {
        return None;
    };
    (arguments.is_empty()
        && aliases
            .get(&name)
            .is_some_and(|d| d.kind == TypeDefinitionKind::Parameter))
    .then_some(name)
}

fn key_parameter(value: &Type, aliases: &BTreeMap<String, TypeDefinition>) -> Option<String> {
    let Type::KeyOf(object) = value else {
        return None;
    };
    parameter(object, aliases)
}

pub(super) fn assignable(
    actual: &Type,
    expected: &Type,
    aliases: &BTreeMap<String, TypeDefinition>,
    visited: &mut HashSet<String>,
    budget: &mut TypeExpansionBudget,
) -> Option<bool> {
    let source = key_parameter(actual, aliases);
    let target = key_parameter(expected, aliases);
    if source.is_none() && target.is_none() {
        return None;
    }
    // Let the existing rigid-parameter and alias rules expose a source bound.
    // A target parameter remains rigid, rather than becoming its constraint.
    if source.is_none() && matches!(actual, Type::Named { .. }) {
        return None;
    }
    if !budget.consume() {
        return Some(false);
    }
    if let (Some(source), Some(target)) = (&source, &target) {
        if source == target {
            return Some(true);
        }
        return Some(object_constraint_implies(
            &Type::Named {
                name: target.clone(),
                arguments: Vec::new(),
            },
            source,
            aliases,
            &mut HashSet::new(),
            budget,
        ));
    }
    if matches!(actual, Type::Any | Type::Never) {
        return Some(true);
    }
    if matches!(expected, Type::Any | Type::Unknown | Type::StrictUnknown) {
        return Some(true);
    }
    if let Type::Union(parts) = actual {
        return Some(
            parts
                .iter()
                .all(|part| is_assignable(part, expected, aliases, &mut visited.clone(), budget)),
        );
    }
    if let Type::Intersection(parts) = actual {
        return Some(
            parts
                .iter()
                .any(|part| is_assignable(part, expected, aliases, &mut visited.clone(), budget)),
        );
    }
    if let Type::Union(parts) = expected {
        if source.is_some()
            && is_assignable(
                &Type::Union(vec![Type::String, Type::Number, Type::Symbol]),
                expected,
                aliases,
                &mut visited.clone(),
                budget,
            )
        {
            return Some(true);
        }
        return Some(
            parts
                .iter()
                .any(|part| is_assignable(actual, part, aliases, &mut visited.clone(), budget)),
        );
    }
    if let Type::Intersection(parts) = expected {
        return Some(
            parts
                .iter()
                .all(|part| is_assignable(actual, part, aliases, &mut visited.clone(), budget)),
        );
    }
    for (value, actual_side) in [(actual, true), (expected, false)] {
        if matches!(value, Type::Named { name, .. } if aliases.get(name).is_some_and(|d| d.kind == TypeDefinitionKind::Parameter))
        {
            continue;
        }
        let mut path = visited.clone();
        if let Some(expanded) =
            instantiate_named(value, aliases, &mut path, budget, "symbolic key relation")
        {
            if expanded != *value {
                return Some(if actual_side {
                    is_assignable(&expanded, expected, aliases, &mut path, budget)
                } else {
                    is_assignable(actual, &expanded, aliases, &mut path, budget)
                });
            }
        }
    }
    if source.is_some() {
        // A generic object can have keys beyond its constraint. Its key set's
        // upper bound is the full domain, not just the constraint's known keys.
        let domain = Type::Union(vec![Type::String, Type::Number, Type::Symbol]);
        return Some(is_assignable(
            &domain,
            expected,
            aliases,
            &mut visited.clone(),
            budget,
        ));
    }
    if matches!(actual, Type::Unknown | Type::StrictUnknown) {
        return Some(false);
    }
    let target = target?;
    let key = format!("key-minimum:{target}:{}", type_identity(actual));
    if !visited.insert(key) {
        return Some(false);
    }
    // A declared object constraint supplies a lower bound on available keys.
    // This proves a required literal key without identifying two binders.
    let constraint = &aliases.get(&target)?.value;
    Some(is_assignable(
        actual,
        &Type::KeyOf(Box::new(constraint.clone())),
        aliases,
        visited,
        budget,
    ))
}

fn object_constraint_implies(
    value: &Type,
    target: &str,
    aliases: &BTreeMap<String, TypeDefinition>,
    active: &mut HashSet<String>,
    budget: &mut TypeExpansionBudget,
) -> bool {
    if !budget.consume() {
        return false;
    }
    if let Some(name) = parameter(value, aliases) {
        if name == target {
            return true;
        }
        if !active.insert(name.clone()) {
            return false;
        }
        return object_constraint_implies(&aliases[&name].value, target, aliases, active, budget);
    }
    match value {
        Type::Intersection(parts) => parts.iter().any(|part| {
            object_constraint_implies(part, target, aliases, &mut active.clone(), budget)
        }),
        Type::Union(parts) => parts.iter().all(|part| {
            object_constraint_implies(part, target, aliases, &mut active.clone(), budget)
        }),
        Type::Named { name, .. }
            if aliases
                .get(name)
                .is_some_and(|d| d.kind == TypeDefinitionKind::Alias) =>
        {
            instantiate_named(
                value,
                aliases,
                active,
                budget,
                "object constraint key proof",
            )
            .is_some_and(|value| object_constraint_implies(&value, target, aliases, active, budget))
        }
        _ => false,
    }
}
