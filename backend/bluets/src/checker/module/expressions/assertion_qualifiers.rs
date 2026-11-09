// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Static assertions cannot erase the existing runtime readonly/opaque policy.
use super::*;

/// Matching mutable standard containers share their readonly API metadata;
/// their data arguments can still carry authority that an annotation must not
/// erase. Other projections retain the full receiver qualifier check.
pub(in crate::checker) fn projection_qualifiers(
    value: &Type,
    target: &Type,
    aliases: &BTreeMap<String, TypeDefinition>,
    budget: &mut TypeExpansionBudget,
) -> bool {
    fn container(
        value: &Type,
        aliases: &BTreeMap<String, TypeDefinition>,
        budget: &mut TypeExpansionBudget,
    ) -> Option<(String, Vec<Type>)> {
        let mut value = value.clone();
        let mut visited = HashSet::new();
        loop {
            match &value {
                Type::Array(element) => return Some(("Array".into(), vec![(**element).clone()])),
                Type::Named { name, arguments }
                    if matches!(name.as_str(), "Array" | "Map")
                        && aliases.get(name).is_some_and(|definition| {
                            definition.kind == TypeDefinitionKind::LibraryInterface
                        }) =>
                {
                    return Some((name.clone(), arguments.clone()));
                }
                Type::Named { name, .. }
                    if aliases
                        .get(name)
                        .is_some_and(|definition| definition.kind == TypeDefinitionKind::Alias) =>
                {
                    value = instantiate_named(
                        &value,
                        aliases,
                        &mut visited,
                        budget,
                        "flow container qualifier",
                    )?;
                }
                _ => return None,
            }
        }
    }
    if let (Some((source, arguments)), Some((destination, _))) = (
        container(value, aliases, budget),
        container(target, aliases, budget),
    ) {
        if source == destination {
            let result = arguments
                .iter()
                .any(|value| qualified(value, aliases, &mut HashSet::new(), budget, 0));
            return result;
        }
    }
    qualified(value, aliases, &mut HashSet::new(), budget, 0)
}

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
        Type::Named { name, arguments }
            if name == "Array"
                && arguments.len() == 1
                && aliases.get(name).is_some_and(|definition| {
                    definition.kind == TypeDefinitionKind::LibraryInterface
                }) =>
        {
            // The standard mutable container's library members include
            // readonly metadata. Its element can still carry runtime authority.
            qualified(&arguments[0], aliases, visited, budget, depth + 1)
        }
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
