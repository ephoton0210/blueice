// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Primitive literal-array assignment errors retain the actual element cause.

use super::*;

impl ModuleChecker<'_> {
    pub(super) fn check_array_element_assignment(
        &mut self,
        tokens: &[Token],
        annotation: &Type,
        inferred: &Type,
        scope: &BTreeMap<String, Type>,
    ) -> bool {
        let Some(elements) = Self::literal_elements(tokens) else {
            return false;
        };
        let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
        let mut visited = HashSet::new();
        let mut expected = annotation.clone();
        while let Some(value) = instantiate_named(
            &expected,
            &self.types,
            &mut visited,
            &mut budget,
            "array assignment",
        ) {
            expected = value;
        }
        let Type::Array(expected) = expected else {
            return false;
        };
        let mut expected = *expected;
        visited.clear();
        while let Some(value) = instantiate_named(
            &expected,
            &self.types,
            &mut visited,
            &mut budget,
            "array element assignment",
        ) {
            expected = value;
        }
        if !matches!(expected, Type::Number | Type::String | Type::Boolean) {
            return false;
        }
        for element in elements {
            let (Some(first), Some(last)) = (element.first(), element.last()) else {
                continue;
            };
            let actual = match crate::checker::module::return_inference::widen(
                self.infer_in_context(element, scope, &expected),
            ) {
                value @ (Type::Number | Type::String | Type::Boolean) => value,
                _ => continue,
            };
            let span = SourceSpan::new(&self.module.id, first.start, last.end);
            if !self.is_assignable_bounded(&actual, &expected, &span) {
                self.assignment_error(
                    &span,
                    format!(
                        "initializer has type `{}`, which is not assignable to `{}`",
                        type_label(inferred),
                        type_label(annotation)
                    ),
                    DiagnosticCode::TypeMismatch,
                    &actual,
                    &expected,
                );
                return true;
            }
        }
        false
    }
}
