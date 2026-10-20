// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Satisfies errors in nested literal tuples point at the incompatible element.
use super::*;

impl ModuleChecker<'_> {
    pub(super) fn literal_composition_error(
        &mut self,
        tokens: &[Token],
        expected: &Type,
        scope: &BTreeMap<String, Type>,
        depth: usize,
    ) -> bool {
        if depth >= MAX_LITERAL_INFERENCE_CONTAINERS {
            return false;
        }
        let mut tokens = strip_outer_parentheses(tokens);
        while let Some(operand) = erased_assertion_operand(tokens) {
            tokens = strip_outer_parentheses(operand);
        }
        let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
        budget.checking = self.checking;
        let mut expected = crate::checker::type_operators::expanded(
            expected,
            &self.types,
            &mut HashSet::new(),
            &mut budget,
        );
        if let Type::Readonly(inner) = expected {
            expected = *inner;
        }
        match expected {
            Type::Record(fields) if tokens.first().is_some_and(|token| token.is("{")) => {
                let Type::Record(actual) = self.infer_expression(tokens, scope) else {
                    return false;
                };
                let parent = Type::Record(fields.clone());
                let mut reported = false;
                for field in actual {
                    let Some(expected) = fields.iter().find(|expected| expected.name == field.name)
                    else {
                        continue;
                    };
                    let start = tokens.partition_point(|token| token.start < field.span.start);
                    let end = tokens.partition_point(|token| token.end <= field.span.end);
                    let piece = &tokens[start..end];
                    if piece.get(1).is_some_and(|token| token.is(":")) {
                        let value = strip_outer_parentheses(&piece[2..]);
                        if erased_assertion_operand(value).is_some() {
                            if !is_assignable(
                                &field.value,
                                &expected.value,
                                &self.types,
                                &mut HashSet::new(),
                                &mut budget,
                            ) && !budget.exhausted
                            {
                                self.asserted_property_error(&field, expected, &parent);
                                reported = true;
                            }
                            let before = self.diagnostics.len();
                            self.check_additional_expression(value, scope, &field.span);
                            reported |= self.diagnostics.len() != before;
                            continue;
                        }
                    }
                    if piece.get(1).is_some_and(|token| token.is(":"))
                        && self.literal_composition_error(
                            &piece[2..],
                            &expected.value,
                            scope,
                            depth + 1,
                        )
                    {
                        reported = true;
                    }
                }
                return reported;
            }
            Type::Tuple(elements) => {
                let Some(parts) = Self::literal_elements(tokens) else {
                    return false;
                };
                let Ok(elements) = expand_concrete_tuple_spreads(
                    &elements,
                    &self.types,
                    &mut HashSet::new(),
                    &mut budget,
                ) else {
                    return false;
                };
                for (index, part) in parts.iter().enumerate() {
                    let candidates = tuple_indexed_candidates(&elements, Some(index));
                    let Some(expected) = candidates.first() else {
                        continue;
                    };
                    if self.literal_composition_error(part, expected, scope, depth + 1) {
                        return true;
                    }
                    let actual = self.infer_in_context(part, scope, expected);
                    if !is_assignable(
                        &actual,
                        expected,
                        &self.types,
                        &mut HashSet::new(),
                        &mut budget,
                    ) && !budget.exhausted
                    {
                        let (Some(first), Some(last)) = (part.first(), part.last()) else {
                            continue;
                        };
                        let point = SourceSpan::new(&self.module.id, first.start, last.end);
                        self.typescript_type_error(
                            &point,
                            "nested tuple element is incompatible".into(),
                            DiagnosticCode::TypeMismatch,
                            2322,
                            vec![
                                crate::diagnostic::type_text::render_in(&actual, self.project),
                                crate::diagnostic::type_text::render_in(expected, self.project),
                            ],
                        );
                        return true;
                    }
                }
            }
            _ => {}
        }
        false
    }

    fn asserted_property_error(&mut self, actual: &TypeField, expected: &TypeField, parent: &Type) {
        let point = SourceSpan::new(
            &self.module.id,
            actual.span.start,
            actual.span.start + actual.name.len(),
        );
        self.typescript_type_error(
            &point,
            "asserted property is incompatible".into(),
            DiagnosticCode::TypeMismatch,
            2322,
            vec![
                crate::diagnostic::type_text::render_in(&actual.value, self.project),
                crate::diagnostic::type_text::render_in(&expected.value, self.project),
            ],
        );
        let target = match &expected.value {
            Type::Readonly(inner) => inner.as_ref(),
            other => other,
        };
        self.explain_last_type_pair(&actual.value, target);
        if let Some(counterpart) = self
            .diagnostics
            .last_mut()
            .and_then(|diagnostic| diagnostic.typescript.as_mut())
        {
            counterpart.related_information.push(crate::TypeScriptRelatedInformation {
                code: 6500,
                message: format!("The expected type comes from property '{}' which is declared here on type '{}'", expected.name, crate::diagnostic::type_text::render_in(parent, self.project)),
                span: SourceSpan::new(&expected.span.module, expected.span.start, expected.span.start + expected.name.len()),
                position: None,
            });
        }
    }
}
