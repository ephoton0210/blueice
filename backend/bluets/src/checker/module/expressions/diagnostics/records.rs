// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Nested object assignment failures retain the exact property and its owner.

use super::*;

impl ModuleChecker<'_> {
    pub(super) fn nested_record_assignment_error(
        &mut self,
        span: &SourceSpan,
        message: &str,
        code: DiagnosticCode,
        actual: &Type,
        expected: &Type,
    ) -> bool {
        let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
        budget.checking = self.checking;
        let Some((actual, expected, owner)) =
            self.nested_mismatch(actual, expected, span, &mut budget, 0)
        else {
            return false;
        };
        if budget.exhausted {
            return false;
        }
        let point = crate::syntax::lex(&self.module.id, &self.module.source)
            .ok()
            .and_then(|tokens| {
                tokens
                    .into_iter()
                    .find(|token| token.start == actual.span.start)
            })
            .map(|token| token.span(&self.module.id));
        let Some(point) = point else {
            return false;
        };
        self.typescript_type_error(
            &point,
            message.into(),
            code,
            2322,
            vec![
                crate::diagnostic::type_text::render_in(&actual.value, self.project),
                crate::diagnostic::type_text::render_in(&expected.value, self.project),
            ],
        );
        let origin = self
            .project
            .source(&expected.span.module)
            .and_then(|source| crate::syntax::lex(&expected.span.module, source).ok())
            .and_then(|tokens| {
                tokens.into_iter().find(|token| {
                    token.start >= expected.span.start
                        && token.end <= expected.span.end
                        && token.is(&expected.name)
                })
            })
            .map(|token| token.span(&expected.span.module))
            .unwrap_or_else(|| expected.span.clone());
        if let Some(counterpart) = self
            .diagnostics
            .last_mut()
            .and_then(|diagnostic| diagnostic.typescript.as_mut())
        {
            counterpart.related_information.push(crate::TypeScriptRelatedInformation {
                code: 6500,
                message: format!("The expected type comes from property '{}' which is declared here on type '{}'", expected.name, crate::diagnostic::type_text::render_in(&owner, self.project)),
                span: origin,
                position: None,
            });
        }
        true
    }

    fn nested_mismatch(
        &self,
        actual: &Type,
        expected: &Type,
        span: &SourceSpan,
        budget: &mut TypeExpansionBudget,
        depth: usize,
    ) -> Option<(TypeField, TypeField, Type)> {
        if depth >= 128 || !budget.consume() {
            return None;
        }
        let Type::Record(actual) = actual else {
            return None;
        };
        let shape = crate::checker::type_operators::expanded(
            expected,
            &self.types,
            &mut HashSet::new(),
            budget,
        );
        let Type::Record(fields) = shape else {
            return None;
        };
        if fields
            .iter()
            .any(|field| !field.optional && !actual.iter().any(|item| item.name == field.name))
        {
            return None;
        }
        for item in actual {
            let field = fields.iter().find(|field| field.name == item.name)?;
            if is_assignable(
                &item.value,
                &field.value,
                &self.types,
                &mut HashSet::new(),
                budget,
            ) {
                continue;
            }
            if item.span.module != "<inferred>"
                || item.span.start < span.start
                || item.span.end > span.end
            {
                return None;
            }
            if let Some(nested) =
                self.nested_mismatch(&item.value, &field.value, span, budget, depth + 1)
            {
                return Some(nested);
            }
            return (depth > 0).then(|| (item.clone(), field.clone(), expected.clone()));
        }
        None
    }
}
