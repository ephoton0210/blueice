// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Additional container diagnostics use retained types and original operands.
use super::*;

impl ModuleChecker<'_> {
    pub(super) fn additional_assignment_error(
        &mut self,
        span: &SourceSpan,
        message: &str,
        code: DiagnosticCode,
        actual: &Type,
        expected: &Type,
    ) -> bool {
        let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
        budget.checking = self.checking;
        let expanded = |value: &Type, budget: &mut TypeExpansionBudget| {
            crate::checker::type_operators::expanded(
                value,
                &self.types,
                &mut HashSet::new(),
                budget,
            )
        };
        let source = expanded(actual, &mut budget);
        let target = expanded(expected, &mut budget);
        if budget.exhausted {
            return false;
        }
        if source == Type::StrictUnknown
            && *actual != Type::StrictUnknown
            && self.bound_parameter_identity(actual).is_none()
        {
            self.typescript_type_error(
                span,
                message.into(),
                code,
                2322,
                vec![
                    "unknown".into(),
                    crate::diagnostic::type_text::render(expected),
                ],
            );
            return true;
        }
        let tokens = crate::syntax::lex(&self.module.id, &self.module.source).unwrap_or_default();
        let selected = tokens
            .iter()
            .filter(|token| span.start <= token.start && token.end <= span.end)
            .cloned()
            .collect::<Vec<_>>();
        let name_span = selected
            .windows(2)
            .find(|pair| matches!(pair[0].text.as_str(), "const" | "let" | "var"))
            .map(|pair| pair[1].span(&self.module.id))
            .unwrap_or_else(|| span.clone());
        if matches!(source, Type::Readonly(_)) && matches!(target, Type::Array(_) | Type::Tuple(_))
        {
            self.additional_assignment_diagnostic(
                (span, message, code),
                4104,
                vec![
                    crate::diagnostic::type_text::render(&source),
                    crate::diagnostic::type_text::render(&target),
                ],
                &name_span,
                None,
            );
            return true;
        }
        let Some(equal) = selected.iter().position(|token| token.is("=")) else {
            return false;
        };
        let operand = &selected[equal + 1..];
        let operand = if operand.last().is_some_and(|token| token.is(";")) {
            &operand[..operand.len() - 1]
        } else {
            operand
        };
        if target == Type::Never
            || matches!(&target, Type::Intersection(parts) if parts.contains(&Type::Never))
        {
            let display = match operand {
                [literal] if matches!(literal.kind, TokenKind::Number | TokenKind::String) => {
                    Type::Literal(literal.text.clone())
                }
                _ => actual.clone(),
            };
            self.additional_assignment_diagnostic(
                (span, message, code),
                2322,
                vec![
                    crate::diagnostic::type_text::render(&display),
                    "never".into(),
                ],
                &name_span,
                None,
            );
            return true;
        }
        if let (Type::Record(fields), Type::IndexedRecord { indices, .. }) = (&source, &target) {
            for field in fields {
                for index in indices {
                    if index.key == Type::Number && field.name.parse::<f64>().is_err() {
                        continue;
                    }
                    if !is_assignable(
                        &field.value,
                        &index.value,
                        &self.types,
                        &mut HashSet::new(),
                        &mut budget,
                    ) {
                        let origin = crate::TypeScriptRelatedInformation {
                            code: 6501,
                            message: "The expected type comes from this index signature.".into(),
                            span: index.span.clone(),
                            position: None,
                        };
                        let point = SourceSpan::new(
                            &self.module.id,
                            field.span.start,
                            field.span.start + field.name.len(),
                        );
                        self.additional_assignment_diagnostic(
                            (span, message, code),
                            2322,
                            vec![
                                crate::diagnostic::type_text::render(&field.value),
                                crate::diagnostic::type_text::render(&index.value),
                            ],
                            &point,
                            Some(origin),
                        );
                        return true;
                    }
                }
            }
        }
        let container = match &target {
            Type::Readonly(value) => value.as_ref(),
            value => value,
        };
        let special = matches!(target, Type::Readonly(_))
            || crate::checker::type_operators::is_operator_context(expected, &self.types)
            || matches!(container, Type::Tuple(items) if items.iter().any(|item| item.label().is_some()));
        if !special || !operand.first().is_some_and(|token| token.is("[")) {
            return false;
        }
        let Some(parts) = Self::literal_elements(operand) else {
            return false;
        };
        if let Type::Tuple(items) = container {
            if items.iter().any(|item| item.rest) {
                self.additional_assignment_diagnostic(
                    (span, message, code),
                    2322,
                    vec![
                        crate::diagnostic::type_text::render(&source),
                        crate::diagnostic::type_text::render(&target),
                    ],
                    &name_span,
                    None,
                );
                self.explain_last_type_pair(&source, &target);
                return true;
            }
        }
        for (position, part) in parts.iter().enumerate() {
            let expected = match container {
                Type::Array(value) => value.as_ref(),
                Type::Tuple(items) => match items.get(position) {
                    Some(item) => item.annotation(),
                    None => continue,
                },
                _ => return false,
            };
            let scope = self.values.clone();
            let actual = self.infer_in_context(part, &scope, expected);
            if !is_assignable(
                &actual,
                expected,
                &self.types,
                &mut HashSet::new(),
                &mut budget,
            ) {
                let point = SourceSpan::new(
                    &self.module.id,
                    part.first().map_or(span.start, |token| token.start),
                    part.last().map_or(span.end, |token| token.end),
                );
                self.additional_assignment_diagnostic(
                    (span, message, code),
                    2322,
                    vec![
                        crate::diagnostic::type_text::render(&actual),
                        crate::diagnostic::type_text::render(expected),
                    ],
                    &point,
                    None,
                );
                return true;
            }
        }
        false
    }

    fn additional_assignment_diagnostic(
        &mut self,
        (span, message, code): (&SourceSpan, &str, DiagnosticCode),
        native: u32,
        arguments: Vec<String>,
        point: &SourceSpan,
        related: Option<crate::TypeScriptRelatedInformation>,
    ) {
        let mut diagnostic =
            Diagnostic::error(code, span.clone(), message).with_typescript(native, arguments);
        if let Some(counterpart) = &mut diagnostic.typescript {
            counterpart.span = point.clone();
            counterpart.position =
                crate::diagnostic::positions::from_source(&self.module.source, point);
            counterpart.related_information.extend(related);
        }
        self.diagnostics.push(diagnostic);
    }
}
