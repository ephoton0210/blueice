// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Bounded property lookup shared by expression inference and mutation checks.

use super::{instantiate_named, Type, TypeDefinition};
use std::collections::{BTreeMap, HashSet};

pub(super) struct TypeExpansionBudget {
    remaining: usize,
    pub(super) exhausted: bool,
}

impl TypeExpansionBudget {
    pub(super) fn new(limit: usize) -> Self {
        Self {
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
    match value {
        Type::Record(fields) => fields
            .iter()
            .find(|field| field.name == property)
            .map(|field| PropertyType::Found {
                value: if field.optional {
                    Type::Union(vec![field.value.clone(), Type::Undefined])
                } else {
                    field.value.clone()
                },
                readonly: field.readonly,
            })
            .unwrap_or(PropertyType::Missing),
        Type::Named { .. } => {
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
            let mut found: Option<(Type, bool)> = None;
            for part in parts {
                match property_type(part, property, aliases, visited, budget) {
                    PropertyType::Found { value, readonly } => {
                        if let Some((_, found_readonly)) = &mut found {
                            *found_readonly |= readonly;
                        } else {
                            found = Some((value, readonly));
                        }
                    }
                    PropertyType::Missing => {}
                    PropertyType::Indeterminate => indeterminate = true,
                    PropertyType::Exhausted => return PropertyType::Exhausted,
                }
            }
            if let Some((value, readonly)) = found {
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
