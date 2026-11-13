// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Empty-record intersections restrict values without relying on alias spelling.
use super::*;
mod primitives;

pub(in crate::checker) fn has_alias(
    name: &str,
    aliases: &BTreeMap<String, TypeDefinition>,
) -> bool {
    let mut name = name;
    let mut active = HashSet::new();
    while active.insert(name) {
        let Some(definition) = aliases
            .get(name)
            .filter(|d| d.kind == TypeDefinitionKind::Alias)
        else {
            return false;
        };
        match &definition.value {
            Type::Intersection(parts) => return parts.iter().any(empty_record),
            Type::Named { name: next, .. } => name = next,
            _ => return false,
        }
    }
    false
}

pub(in crate::checker) fn empty_record(value: &Type) -> bool {
    matches!(value, Type::Record(fields) if fields.is_empty())
}

pub(in crate::checker) fn normalized(
    value: &Type,
    aliases: &BTreeMap<String, TypeDefinition>,
    budget: &mut TypeExpansionBudget,
) -> Option<Type> {
    let Type::Intersection(parts) = value else {
        return None;
    };
    if let Some(result) = primitives::reduce(parts, budget) {
        return (result != *value && !budget.exhausted).then_some(result);
    }
    if !parts.iter().any(empty_record) {
        return None;
    }
    let result = restrict_parts(parts, aliases, &mut HashSet::new(), budget);
    (result != *value && !budget.exhausted).then_some(result)
}

fn restrict_parts(
    parts: &[Type],
    aliases: &BTreeMap<String, TypeDefinition>,
    active: &mut HashSet<String>,
    budget: &mut TypeExpansionBudget,
) -> Type {
    if !budget.consume() {
        return Type::Intersection(parts.to_vec());
    }
    if parts.contains(&Type::Never) {
        return Type::Never;
    }
    if parts.contains(&Type::Any) {
        return Type::Any;
    }
    let mut result = Vec::new();
    for part in parts {
        if empty_record(part) || *part == Type::StrictUnknown {
            continue;
        }
        let restricted = restrict(part, aliases, active, budget);
        if restricted == Type::Never {
            return Type::Never;
        }
        let values = match restricted {
            Type::Intersection(values) => values,
            value => vec![value],
        };
        for value in values {
            if !result.contains(&value) {
                result.push(value);
            }
        }
    }
    match result.len() {
        0 => Type::Record(Vec::new()),
        1 => result.pop().unwrap(),
        _ => Type::Intersection(result),
    }
}

fn restrict(
    value: &Type,
    aliases: &BTreeMap<String, TypeDefinition>,
    active: &mut HashSet<String>,
    budget: &mut TypeExpansionBudget,
) -> Type {
    if !budget.consume() {
        return value.clone();
    }
    match value {
        Type::Null | Type::Undefined | Type::Never => Type::Never,
        Type::StrictUnknown => Type::Record(Vec::new()),
        Type::Union(parts) => union(
            parts
                .iter()
                .map(|part| restrict(part, aliases, &mut active.clone(), budget)),
        ),
        Type::Intersection(parts) => restrict_parts(parts, aliases, active, budget),
        Type::Named { name, .. } => match aliases.get(name).map(|definition| definition.kind) {
            Some(
                TypeDefinitionKind::Interface
                | TypeDefinitionKind::LibraryInterface
                | TypeDefinitionKind::Class
                | TypeDefinitionKind::EnumMember,
            ) => value.clone(),
            Some(TypeDefinitionKind::Alias) => {
                if let Some(expanded) =
                    instantiate_named(value, aliases, active, budget, "empty-record intersection")
                {
                    let reduced = restrict(&expanded, aliases, active, budget);
                    if reduced == expanded
                        && !matches!(expanded, Type::Union(_) | Type::Intersection(_))
                    {
                        value.clone()
                    } else {
                        reduced
                    }
                } else {
                    Type::Intersection(vec![value.clone(), Type::Record(Vec::new())])
                }
            }
            _ => Type::Intersection(vec![value.clone(), Type::Record(Vec::new())]),
        },
        Type::Conditional(_) | Type::KeyOf(_) | Type::IndexedAccess { .. } | Type::Mapped(_) => {
            if let Some(expanded) =
                instantiate_named(value, aliases, active, budget, "intersection operator")
            {
                if expanded != *value {
                    return restrict(&expanded, aliases, active, budget);
                }
            }
            Type::Intersection(vec![value.clone(), Type::Record(Vec::new())])
        }
        Type::Unknown | Type::Infer(_) => {
            Type::Intersection(vec![value.clone(), Type::Record(Vec::new())])
        }
        Type::InferenceBarrier(inner) => {
            Type::inference_barrier(restrict(inner, aliases, active, budget))
        }
        _ => value.clone(),
    }
}
