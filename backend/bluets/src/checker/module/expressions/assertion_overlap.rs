// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Assertions compare widened literal sources and overlapping union members.
use super::*;

impl ModuleChecker<'_> {
    pub(super) fn assertion_source_base_type(
        &self,
        value: &Type,
        budget: &mut TypeExpansionBudget,
    ) -> Type {
        self.assertion_source_base(value, budget, &mut HashSet::new())
    }

    fn assertion_source_base(
        &self,
        value: &Type,
        budget: &mut TypeExpansionBudget,
        active_aliases: &mut HashSet<String>,
    ) -> Type {
        if !budget.consume() {
            return value.clone();
        }
        let value = crate::checker::type_operators::primitive_alias(value, &self.types, budget)
            .unwrap_or_else(|| value.clone());
        let member = match &value {
            Type::Literal(name) | Type::Named { name, .. }
                if self.types.get(name).is_some_and(|definition| {
                    definition.kind == TypeDefinitionKind::EnumMember
                }) =>
            {
                Some(name)
            }
            _ => None,
        };
        if let Some(member) = member {
            let (mut bare, module) = member
                .split_once('@')
                .map_or((member.as_str(), None), |(name, module)| {
                    (name, Some(module))
                });
            while let Some((owner, _)) = bare.rsplit_once('.') {
                if !budget.consume() {
                    break;
                }
                let name =
                    module.map_or_else(|| owner.to_string(), |module| format!("{owner}@{module}"));
                if self
                    .types
                    .get(&name)
                    .is_some_and(|definition| definition.kind == TypeDefinitionKind::Enum)
                {
                    return Type::Named {
                        name,
                        arguments: Vec::new(),
                    };
                }
                bare = owner;
            }
        }
        if let Type::Named { name, .. } = &value {
            if self
                .types
                .get(name)
                .is_some_and(|definition| definition.kind == TypeDefinitionKind::Alias)
            {
                let identity = type_identity(&value);
                if !active_aliases.insert(identity.clone()) {
                    return value;
                }
                let expanded = crate::checker::type_operators::expanded(
                    &value,
                    &self.types,
                    &mut HashSet::new(),
                    budget,
                );
                let base = if expanded != value {
                    self.assertion_source_base(&expanded, budget, active_aliases)
                } else {
                    expanded.clone()
                };
                active_aliases.remove(&identity);
                // An unchanged object/array alias retains its diagnostic name;
                // a union whose literals widened has a new primitive base.
                return if base != expanded { base } else { value };
            }
        }
        match value {
            Type::Union(parts) => crate::checker::type_operators::union(
                parts
                    .iter()
                    .map(|part| self.assertion_source_base(part, budget, active_aliases)),
            ),
            Type::TemplateLiteral(_) | Type::StringMapping(_) => Type::String,
            Type::Literal(ref text)
                if text.ends_with('n')
                    && text
                        .trim_start_matches('-')
                        .starts_with(|c: char| c.is_ascii_digit()) =>
            {
                Type::BigInt
            }
            Type::Literal(ref text)
                if ["0x", "0X", "0o", "0O", "0b", "0B"]
                    .iter()
                    .any(|prefix| text.trim_start_matches('-').starts_with(prefix)) =>
            {
                Type::Number
            }
            value => crate::checker::module::return_inference::widen(value),
        }
    }

    pub(super) fn assertion_overlaps_constraint(
        &self,
        actual: &Type,
        expected: &Type,
        budget: &mut TypeExpansionBudget,
    ) -> bool {
        self.assertion_overlap(actual, expected, budget, &mut HashSet::new())
    }

    fn assertion_overlap(
        &self,
        actual: &Type,
        expected: &Type,
        budget: &mut TypeExpansionBudget,
        seen: &mut HashSet<(String, String)>,
    ) -> bool {
        if !budget.consume() || !seen.insert((type_identity(actual), type_identity(expected))) {
            return false;
        }
        let actual = crate::checker::type_operators::expanded(
            actual,
            &self.types,
            &mut HashSet::new(),
            budget,
        );
        let expected = crate::checker::type_operators::expanded(
            expected,
            &self.types,
            &mut HashSet::new(),
            budget,
        );
        if is_assignable(&actual, &expected, &self.types, &mut HashSet::new(), budget)
            || is_assignable(&expected, &actual, &self.types, &mut HashSet::new(), budget)
        {
            return true;
        }
        // A cast needs one comparable constituent. Assignment still requires
        // every source constituent, so its union rule cannot decide this gate.
        if let Type::Union(parts) = &actual {
            return parts
                .iter()
                .any(|part| self.assertion_overlap(part, &expected, budget, seen));
        }
        if let Type::Union(parts) = &expected {
            return parts
                .iter()
                .any(|part| self.assertion_overlap(&actual, part, budget, seen));
        }
        false
    }
}
