// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Never assertions cannot erase the existing runtime readonly/opaque policy.
use super::*;

impl ModuleChecker<'_> {
    pub(super) fn assertion_preserves_runtime_qualifiers(&self, value: &Type) -> bool {
        let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
        budget.checking = self.checking;
        qualified(value, &self.types, &mut HashSet::new(), &mut budget, 0)
    }
}

fn qualified(
    value: &Type,
    aliases: &BTreeMap<String, TypeDefinition>,
    visited: &mut HashSet<String>,
    budget: &mut TypeExpansionBudget,
    depth: usize,
) -> bool {
    if matches!(
        value,
        Type::Any
            | Type::Never
            | Type::Void
            | Type::Undefined
            | Type::Null
            | Type::Boolean
            | Type::Number
            | Type::String
            | Type::BigInt
            | Type::Symbol
            | Type::UniqueSymbol(_)
            | Type::Literal(_)
            | Type::Function { .. }
            | Type::GenericFunction { .. }
    ) {
        return false;
    }
    if depth > super::readonly::MAX_READONLY_TAINT_DEPTH || !budget.consume() {
        return true;
    }
    match value {
        Type::Unknown | Type::StrictUnknown | Type::Readonly(_) => true,
        Type::Record(fields) | Type::CallableRecord { fields, .. } => fields.iter().any(|field| {
            field.readonly
                || qualified(
                    &field.value,
                    aliases,
                    &mut visited.clone(),
                    budget,
                    depth + 1,
                )
        }),
        Type::IndexedRecord { object, indices } => {
            qualified(object, aliases, &mut visited.clone(), budget, depth + 1)
                || indices.iter().any(|index| {
                    index.readonly
                        || qualified(
                            &index.value,
                            aliases,
                            &mut visited.clone(),
                            budget,
                            depth + 1,
                        )
                })
        }
        Type::Array(element) => qualified(element, aliases, visited, budget, depth + 1),
        Type::Tuple(elements) => elements.iter().any(|element| {
            qualified(
                &element.annotation,
                aliases,
                &mut visited.clone(),
                budget,
                depth + 1,
            )
        }),
        Type::Union(parts) | Type::Intersection(parts) => parts
            .iter()
            .any(|part| qualified(part, aliases, &mut visited.clone(), budget, depth + 1)),
        Type::Named { .. }
        | Type::Conditional(_)
        | Type::Mapped(_)
        | Type::IndexedAccess { .. }
        | Type::KeyOf(_)
        | Type::TemplateLiteral(_) => {
            match instantiate_named(
                value,
                aliases,
                visited,
                budget,
                "assertion runtime qualifier",
            ) {
                Some(value) => qualified(&value, aliases, visited, budget, depth + 1),
                None => budget.exhausted,
            }
        }
        _ => false,
    }
}
