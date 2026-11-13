// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Object intersections distribute before key projection under the work limit.
use super::*;

pub(super) enum Objects {
    Simple(Vec<Type>),
    Distributed(Vec<Type>),
}

pub(super) fn order_objects(parts: &mut [Type], aliases: &BTreeMap<String, TypeDefinition>) {
    fn origin<'a>(
        value: &Type,
        aliases: &'a BTreeMap<String, TypeDefinition>,
    ) -> Option<(&'a str, usize)> {
        let Type::Named { name, arguments } = canonical_parameter_references(value, aliases) else {
            return None;
        };
        if !arguments.is_empty() {
            return None;
        }
        let (name, definition) = aliases.get_key_value(&name)?;
        if definition.kind != TypeDefinitionKind::Parameter {
            return None;
        }
        crate::parser::type_parameter_origin(name)
    }
    fn generic(value: &Type, aliases: &BTreeMap<String, TypeDefinition>) -> bool {
        match value {
            Type::Union(parts) | Type::Intersection(parts) => {
                parts.iter().all(|part| generic(part, aliases))
            }
            value => origin(value, aliases).is_some(),
        }
    }
    if !parts.iter().all(|part| generic(part, aliases)) {
        return;
    }
    parts.sort_by(
        |left, right| match (origin(left, aliases), origin(right, aliases)) {
            (Some((lm, lo)), Some((rm, ro))) if lm == rm => lo.cmp(&ro),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            _ => std::cmp::Ordering::Equal,
        },
    );
}

pub(super) fn key_intersection(values: Vec<Type>) -> Type {
    let mut parts = Vec::new();
    for value in values {
        if value == Type::Never {
            return Type::Never;
        }
        if !parts.contains(&value) {
            parts.push(value);
        }
    }
    match parts.len() {
        0 => Type::Never,
        1 => parts.pop().unwrap(),
        n if n >= 3 && parts.iter().any(|p| matches!(p, Type::Union(_))) => {
            let right = parts.split_off(n / 2);
            Type::Intersection(vec![key_intersection(parts), key_intersection(right)])
        }
        _ => Type::Intersection(parts),
    }
}

pub(super) fn intersection(
    parts: Vec<Type>,
    aliases: &BTreeMap<String, TypeDefinition>,
    visited: &HashSet<String>,
    budget: &mut TypeExpansionBudget,
) -> Option<Objects> {
    let mut pending = parts.into_iter().rev().collect::<Vec<_>>();
    let mut normalized = Vec::new();
    while let Some(part) = pending.pop() {
        if !budget.consume() {
            return None;
        }
        let (part, _) = operand::normalize(&part, aliases, &mut visited.clone(), budget)?;
        match part {
            Type::Intersection(parts) => pending.extend(parts.into_iter().rev()),
            part => normalized.push(part),
        }
    }
    if !normalized.iter().any(|part| matches!(part, Type::Union(_))) {
        return Some(Objects::Simple(normalized));
    }
    let mut branches = vec![Vec::new()];
    for part in normalized {
        let mut options = match part {
            Type::Union(parts) => parts,
            part => vec![part],
        };
        order_objects(&mut options, aliases);
        let mut next = Vec::new();
        for branch in &branches {
            for option in &options {
                if !budget.consume() {
                    return None;
                }
                let mut product = branch.clone();
                product.push(option.clone());
                next.push(product);
            }
        }
        branches = next;
    }
    Some(Objects::Distributed(
        branches.into_iter().map(Type::Intersection).collect(),
    ))
}

pub(super) fn ordered_union(values: Vec<Type>, order: &[Type]) -> Type {
    let mut value = union(values);
    if let Type::Union(parts) = &mut value {
        // Deferred keys are interned when each projected object is visited.
        // Keep that encounter order when combining subsequent object branches.
        if parts.iter().all(|part| matches!(part, Type::KeyOf(_))) {
            parts.sort_by_key(|part| {
                order
                    .iter()
                    .position(|known| known == part)
                    .unwrap_or(usize::MAX)
            });
        }
    }
    value
}
