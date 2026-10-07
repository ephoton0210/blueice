// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Bounded property lookup shared by expression inference and mutation checks.

use super::{instantiate_named, Type, TypeDefinition, TypeDefinitionKind};
use std::collections::{BTreeMap, HashSet};

pub(super) struct TypeExpansionBudget {
    pub(super) checking: crate::CheckingOptions,
    remaining: usize,
    pub(super) exhausted: bool,
}

impl TypeExpansionBudget {
    pub(super) fn new(limit: usize) -> Self {
        Self {
            checking: crate::CheckingOptions::legacy(),
            remaining: limit,
            exhausted: false,
        }
    }

    pub(super) fn consume(&mut self) -> bool {
        if let Some(remaining) = self.remaining.checked_sub(1) {
            self.remaining = remaining;
            true
        } else {
            self.exhausted = true;
            false
        }
    }
}

pub(super) enum PropertyType {
    Found {
        value: Type,
        readonly: bool,
    },
    Missing,
    /// The checker has no property semantics for this expression; do not
    /// invent a definite value type from an opaque branch.
    Indeterminate,
    Exhausted,
}

pub(super) fn property_type(
    value: &Type,
    property: &str,
    aliases: &BTreeMap<String, TypeDefinition>,
    visited: &mut HashSet<String>,
    budget: &mut TypeExpansionBudget,
) -> PropertyType {
    // Enum literals retain nominal identity for assignment. Property lookup
    // uses their underlying primitive without changing that identity.
    if let Type::Literal(name) = value {
        if let Some(definition) = aliases
            .get(name)
            .filter(|definition| definition.kind == TypeDefinitionKind::EnumMember)
        {
            if !budget.consume() {
                return PropertyType::Exhausted;
            }
            return property_type(&definition.value, property, aliases, visited, budget);
        }
    }
    let boxed = match value {
        Type::String => Some(("String", Vec::new())),
        Type::Number => Some(("Number", Vec::new())),
        Type::Boolean => Some(("Boolean", Vec::new())),
        Type::Function { .. } => Some(("Function", Vec::new())),
        Type::Literal(text) if text.starts_with(['\'', '"', '`']) => Some(("String", Vec::new())),
        Type::Literal(text) if text.parse::<f64>().is_ok() => Some(("Number", Vec::new())),
        Type::Literal(text) if matches!(text.as_str(), "true" | "false") => {
            Some(("Boolean", Vec::new()))
        }
        Type::Array(element) => Some(("Array", vec![(**element).clone()])),
        Type::Tuple(elements) => Some((
            "Array",
            vec![Type::Union(
                elements
                    .iter()
                    .map(|element| element.indexed_type())
                    .collect(),
            )],
        )),
        _ => None,
    };
    if let Some((name, arguments)) = boxed.filter(|(name, _)| aliases.contains_key(*name)) {
        return property_type(
            &Type::Named {
                name: name.to_string(),
                arguments,
            },
            property,
            aliases,
            visited,
            budget,
        );
    }
    match value {
        Type::Record(fields) | Type::CallableRecord { fields, .. } => {
            let mut values = Vec::new();
            let mut readonly = false;
            for field in fields.iter().filter(|field| field.name == property) {
                // Keep every same-named signature in declaration order. The
                // first field preserves the existing simple-lookup budget.
                if !values.is_empty() && !budget.consume() {
                    return PropertyType::Exhausted;
                }
                values.push(if field.optional {
                    Type::Union(vec![field.value.clone(), Type::Undefined])
                } else {
                    field.value.clone()
                });
                readonly |= field.readonly;
            }
            match values.len() {
                0 => PropertyType::Missing,
                1 => PropertyType::Found {
                    value: values.pop().expect("one matching field"),
                    readonly,
                },
                _ => PropertyType::Found {
                    value: Type::Intersection(values),
                    readonly,
                },
            }
        }
        Type::Named { .. }
        | Type::Conditional(_)
        | Type::Mapped(_)
        | Type::IndexedAccess { .. }
        | Type::KeyOf(_)
        | Type::TemplateLiteral(_) => {
            match instantiate_named(value, aliases, visited, budget, "property") {
                Some(value) => property_type(&value, property, aliases, visited, budget),
                None if budget.exhausted => PropertyType::Exhausted,
                None => PropertyType::Indeterminate,
            }
        }
        Type::Union(parts) => {
            let mut values = Vec::new();
            let mut readonly = false;
            let mut indeterminate = false;
            for part in parts {
                if !budget.consume() {
                    return PropertyType::Exhausted;
                }
                match property_type(part, property, aliases, &mut visited.clone(), budget) {
                    PropertyType::Found {
                        value,
                        readonly: part_readonly,
                    } => {
                        values.push(value);
                        readonly |= part_readonly;
                    }
                    PropertyType::Missing => return PropertyType::Missing,
                    PropertyType::Indeterminate => indeterminate = true,
                    PropertyType::Exhausted => return PropertyType::Exhausted,
                }
            }
            if indeterminate && !readonly {
                return PropertyType::Indeterminate;
            }
            if values.is_empty() {
                return PropertyType::Indeterminate;
            }
            let value = if indeterminate {
                Type::Unknown
            } else if values.iter().all(|value| value == &values[0]) {
                values.swap_remove(0)
            } else {
                Type::Union(values)
            };
            PropertyType::Found { value, readonly }
        }
        Type::Intersection(parts) => {
            let mut indeterminate = false;
            let mut found = Vec::new();
            let mut readonly = false;
            for part in parts {
                match property_type(part, property, aliases, visited, budget) {
                    PropertyType::Found {
                        value,
                        readonly: part_readonly,
                    } => {
                        if !found.is_empty() && !budget.consume() {
                            return PropertyType::Exhausted;
                        }
                        found.push(value);
                        readonly |= part_readonly;
                    }
                    PropertyType::Missing => {}
                    PropertyType::Indeterminate => indeterminate = true,
                    PropertyType::Exhausted => return PropertyType::Exhausted,
                }
            }
            if !found.is_empty() {
                let value = if found.len() == 1 {
                    found.pop().expect("one matching property")
                } else {
                    Type::Intersection(found)
                };
                PropertyType::Found { value, readonly }
            } else if indeterminate {
                PropertyType::Indeterminate
            } else {
                PropertyType::Missing
            }
        }
        Type::Any | Type::Unknown => PropertyType::Indeterminate,
        _ => PropertyType::Missing,
    }
}

pub(super) fn contains_readonly_member(
    value: &Type,
    aliases: &BTreeMap<String, TypeDefinition>,
    visited: &mut HashSet<String>,
    budget: &mut TypeExpansionBudget,
) -> Result<bool, ()> {
    match value {
        Type::Record(fields) | Type::CallableRecord { fields, .. } => {
            Ok(fields.iter().any(|field| field.readonly))
        }
        Type::Named { .. }
        | Type::Conditional(_)
        | Type::Mapped(_)
        | Type::IndexedAccess { .. }
        | Type::KeyOf(_)
        | Type::TemplateLiteral(_) => {
            match instantiate_named(value, aliases, visited, budget, "readonly property") {
                Some(value) => contains_readonly_member(&value, aliases, visited, budget),
                None if budget.exhausted => Err(()),
                None => Ok(false),
            }
        }
        Type::Union(parts) | Type::Intersection(parts) => {
            for part in parts {
                if contains_readonly_member(part, aliases, visited, budget)? {
                    return Ok(true);
                }
            }
            Ok(false)
        }
        _ => Ok(false),
    }
}

/// Mutability also applies to private/protected marker fields and to a
/// readonly alternative of a union. Accessibility is checked separately.
pub(super) fn readonly_property(
    owner: &Type,
    property: &str,
    aliases: &BTreeMap<String, TypeDefinition>,
    visited: &mut HashSet<String>,
    budget: &mut TypeExpansionBudget,
    private_owner: Option<&str>,
    depth: usize,
) -> bool {
    if depth > 128 {
        budget.exhausted = true;
        return false;
    }
    if !budget.consume() {
        return false;
    }
    match owner {
        Type::Record(fields) | Type::CallableRecord { fields, .. } => {
            if fields.iter().any(|field| field.name == property) {
                fields
                    .iter()
                    .any(|field| field.name == property && field.readonly)
            } else {
                fields
                    .iter()
                    .find(|field| mutation_name_matches(&field.name, property, private_owner))
                    .is_some_and(|field| field.readonly)
            }
        }
        Type::Named { .. }
        | Type::Conditional(_)
        | Type::Mapped(_)
        | Type::IndexedAccess { .. }
        | Type::KeyOf(_)
        | Type::TemplateLiteral(_) => {
            instantiate_named(owner, aliases, visited, budget, "readonly mutation").is_some_and(
                |value| {
                    readonly_property(
                        &value,
                        property,
                        aliases,
                        visited,
                        budget,
                        private_owner,
                        depth + 1,
                    )
                },
            )
        }
        Type::Union(parts) | Type::Intersection(parts) => parts.iter().any(|part| {
            readonly_property(
                part,
                property,
                aliases,
                &mut visited.clone(),
                budget,
                private_owner,
                depth + 1,
            )
        }),
        _ => false,
    }
}

fn mutation_name_matches(name: &str, property: &str, private_owner: Option<&str>) -> bool {
    let marker = name.split_once(' ').and_then(|(visibility, rest)| {
        matches!(visibility, "private" | "protected")
            .then(|| rest.split_once(' '))
            .flatten()
    });
    name == property
        || marker.is_some_and(|(owner, name)| {
            name == property && (!property.starts_with('#') || private_owner == Some(owner))
        })
}

/// A restricted member's type can carry a readonly target further down a
/// receiver chain. This lookup serves mutation checks only; the ordinary
/// checker still applies accessibility before permitting member access.
pub(super) fn mutation_field_type(
    owner: &Type,
    property: &str,
    aliases: &BTreeMap<String, TypeDefinition>,
    visited: &mut HashSet<String>,
    budget: &mut TypeExpansionBudget,
    private_owner: Option<&str>,
    depth: usize,
) -> Option<Type> {
    if depth > 128 {
        budget.exhausted = true;
        return None;
    }
    if !budget.consume() {
        return None;
    }
    match owner {
        Type::Record(fields) | Type::CallableRecord { fields, .. } => fields
            .iter()
            .find(|field| mutation_name_matches(&field.name, property, private_owner))
            .map(|field| {
                if field.optional {
                    Type::Union(vec![field.value.clone(), Type::Undefined])
                } else {
                    field.value.clone()
                }
            }),
        Type::Named { .. }
        | Type::Conditional(_)
        | Type::Mapped(_)
        | Type::IndexedAccess { .. }
        | Type::KeyOf(_)
        | Type::TemplateLiteral(_) => {
            instantiate_named(owner, aliases, visited, budget, "mutation receiver").and_then(
                |value| {
                    mutation_field_type(
                        &value,
                        property,
                        aliases,
                        visited,
                        budget,
                        private_owner,
                        depth + 1,
                    )
                },
            )
        }
        Type::Union(parts) | Type::Intersection(parts) => {
            let values: Vec<_> = parts
                .iter()
                .filter_map(|part| {
                    mutation_field_type(
                        part,
                        property,
                        aliases,
                        &mut visited.clone(),
                        budget,
                        private_owner,
                        depth + 1,
                    )
                })
                .collect();
            match values.as_slice() {
                [] => None,
                [value] => Some(value.clone()),
                _ => Some(Type::Union(values)),
            }
        }
        _ => None,
    }
}
