// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Primitive literal-array assignment errors retain the actual element cause.

use super::*;

impl ModuleChecker<'_> {
    pub(in crate::checker::module) fn check_array_element_assignment(
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
        let contexts: Vec<_> = match expected {
            Type::Array(value) => elements
                .into_iter()
                .map(|tokens| (tokens, (*value).clone()))
                .collect(),
            Type::Tuple(values) => elements
                .into_iter()
                .zip(values)
                .take_while(|(_, value)| !value.rest)
                .map(|(tokens, value)| (tokens, value.annotation))
                .collect(),
            _ => return false,
        };
        for (element, expected) in contexts {
            let (Some(first), Some(last)) = (element.first(), element.last()) else {
                continue;
            };
            let span = SourceSpan::new(&self.module.id, first.start, last.end);
            let expected = crate::checker::inference::result_type(
                &expected,
                &BTreeMap::new(),
                &self.types,
                &mut budget,
            );
            let expected = crate::checker::type_operators::diagnostic_type(
                &expected,
                &self.types,
                &mut budget,
            );
            let actual = self.infer_in_context(element, scope, &expected);
            let literal_context = matches!(expected, Type::Literal(_))
                || matches!(&expected, Type::Union(parts) if parts.iter().any(|part| matches!(part, Type::Literal(_))));
            let actual = if literal_context {
                actual
            } else {
                crate::checker::module::return_inference::widen(actual)
            };
            if self.is_assignable_bounded(&actual, &expected, &span) {
                continue;
            }
            if matches!(expected, Type::Record(_) | Type::Named { .. })
                && element.first().is_some_and(|token| token.is("{"))
            {
                self.call_argument_error(
                    &span,
                    "tuple element does not satisfy its contextual type".into(),
                    &actual,
                    &expected,
                    element,
                    false,
                );
                return true;
            }
            let expected = if let Type::Union(parts) = expected {
                crate::checker::type_operators::union(
                    parts
                        .into_iter()
                        .filter(|part| !matches!(part, Type::Null | Type::Undefined)),
                )
            } else {
                expected
            };
            if !matches!(
                expected,
                Type::Number | Type::String | Type::Boolean | Type::Literal(_) | Type::Union(_)
            ) || !matches!(
                actual,
                Type::Number | Type::String | Type::Boolean | Type::Literal(_)
            ) {
                continue;
            }
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
        false
    }
}
