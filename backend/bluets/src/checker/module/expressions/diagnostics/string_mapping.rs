// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Explain rigid parameters through the actual string-mapping constraints.

use super::*;

impl ModuleChecker<'_> {
    fn mapping_base_constraint(
        &self,
        value: &Type,
        budget: &mut TypeExpansionBudget,
    ) -> Option<Type> {
        let canonical =
            crate::checker::type_relations::canonical_parameter_references(value, &self.types);
        let canonical = crate::checker::type_operators::expanded(
            &canonical,
            &self.types,
            &mut HashSet::new(),
            budget,
        );
        let Type::StringMapping(mapping) = canonical else {
            return None;
        };
        if !budget.consume() {
            return None;
        }
        if let Type::Named { name, .. } = &mapping.operand {
            if let Some(parameter) = self.types.get(name).filter(|definition| {
                definition.kind == TypeDefinitionKind::Parameter
                    && definition.value != Type::Unknown
                    && definition.value != Type::StrictUnknown
            }) {
                let application = Type::StringMapping(Box::new(crate::parser::StringMappingType {
                    operand: parameter.value.clone(),
                    ..*mapping
                }));
                return Some(crate::checker::type_operators::expanded(
                    &application,
                    &self.types,
                    &mut HashSet::new(),
                    budget,
                ));
            }
        }
        Some(Type::String)
    }

    pub(super) fn string_mapping_parameter_causes(
        &self,
        actual: &Type,
        parameter: &str,
        constraint: &Type,
    ) -> String {
        let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
        budget.checking = self.checking;
        let mut current = actual.clone();
        let mut seen = HashSet::new();
        let mut result = String::new();
        let mut indent = 4;
        while seen.insert(crate::checker::type_relations::type_identity(&current)) {
            let Some(next) = self.mapping_base_constraint(&current, &mut budget) else {
                break;
            };
            if budget.exhausted || next == current {
                break;
            }
            let source = crate::diagnostic::type_text::render_in(&next, self.project);
            let bound = crate::diagnostic::type_text::render_in(constraint, self.project);
            result.push_str(&format!(
                "\n{}Type '{source}' is not assignable to type '{parameter}'.\n{}'{source}' is assignable to the constraint of type '{parameter}', but '{parameter}' could be instantiated with a different subtype of constraint '{bound}'.",
                " ".repeat(indent), " ".repeat(indent + 2),
            ));
            current = next;
            indent += 4;
        }
        result
    }

    pub(super) fn string_mapping_source_constraint_detail(
        &self,
        actual: &Type,
        expected: &Type,
    ) -> Option<String> {
        if !matches!(expected, Type::StringMapping(_)) {
            return None;
        }
        let canonical =
            crate::checker::type_relations::canonical_parameter_references(actual, &self.types);
        let Type::Named { name, .. } = canonical else {
            return None;
        };
        let parameter = self.types.get(&name).filter(|definition| {
            definition.kind == TypeDefinitionKind::Parameter
                && definition.value != Type::Unknown
                && definition.value != Type::StrictUnknown
        })?;
        Some(format!(
            "\n  Type '{}' is not assignable to type '{}'.",
            crate::diagnostic::type_text::render_in(&parameter.value, self.project),
            crate::diagnostic::type_text::render_in(expected, self.project),
        ))
    }
}
