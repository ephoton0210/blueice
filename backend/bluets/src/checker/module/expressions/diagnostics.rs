// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Diagnostic causes determined from the checked call signatures.

use super::*;
mod compatibility;
mod constructors;
mod more_types;
mod operators;

impl ModuleChecker<'_> {
    pub(in crate::checker::module) fn call_argument_error(
        &mut self,
        span: &SourceSpan,
        message: String,
        actual: &Type,
        expected: &Type,
        argument: &[Token],
        overloaded: bool,
    ) {
        if overloaded {
            self.typescript_type_error(
                span,
                message,
                DiagnosticCode::TypeMismatch,
                2769,
                Vec::new(),
            );
            self.point_last_typescript(argument);
            return;
        }
        if self.weak_compatibility_error(
            span,
            &message,
            DiagnosticCode::TypeMismatch,
            actual,
            expected,
        ) {
            self.point_last_typescript(argument);
            return;
        }
        if argument.first().is_some_and(|token| token.is("{")) {
            let (Some(actual_fields), false) = self.expanded_record_fields(actual.clone()) else {
                self.typescript_type_error(
                    span,
                    message,
                    DiagnosticCode::TypeMismatch,
                    2345,
                    vec![
                        crate::diagnostic::type_text::argument(actual, expected),
                        crate::diagnostic::type_text::render_in(expected, self.project),
                    ],
                );
                self.point_last_typescript(argument);
                return;
            };
            if let (Some(expected_fields), false) = self.expanded_record_fields(expected.clone()) {
                for field in actual_fields {
                    let Some(expected) = expected_fields
                        .iter()
                        .find(|expected| expected.name == field.name)
                    else {
                        continue;
                    };
                    let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
                    budget.checking = self.checking;
                    if !is_assignable(
                        &field.value,
                        &expected.value,
                        &self.types,
                        &mut HashSet::new(),
                        &mut budget,
                    ) && !budget.exhausted
                    {
                        self.typescript_type_error(
                            span,
                            message,
                            DiagnosticCode::TypeMismatch,
                            2322,
                            vec![
                                crate::diagnostic::type_text::render_in(&field.value, self.project),
                                crate::diagnostic::type_text::render_in(
                                    &expected.value,
                                    self.project,
                                ),
                            ],
                        );
                        if let Some(token) = argument.iter().find(|token| token.is(&field.name)) {
                            self.point_last_typescript(std::slice::from_ref(token));
                        }
                        return;
                    }
                }
            }
        }
        self.typescript_type_error(
            span,
            message,
            DiagnosticCode::TypeMismatch,
            2345,
            vec![
                crate::diagnostic::type_text::argument(actual, expected),
                crate::diagnostic::type_text::render_in(expected, self.project),
            ],
        );
        self.point_last_typescript(argument);
        self.explain_last_type_pair(actual, expected);
        if let Type::Function { result, .. } = actual {
            if let Some(body) = argument
                .windows(2)
                .find(|pair| pair[0].is("=>") && pair[1].is("{"))
            {
                if let Some(end) = argument.last().map(|token| token.end) {
                    if let Some(newline) = self
                        .module
                        .source
                        .get(body[1].start..end)
                        .and_then(|text| text.find(['\n', '\r', '\u{2028}', '\u{2029}']))
                    {
                        if let Some(counterpart) = self
                            .diagnostics
                            .last_mut()
                            .and_then(|diagnostic| diagnostic.typescript.as_mut())
                        {
                            counterpart.span.end = body[1].start + newline;
                        }
                    }
                }
            }
            let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
            budget.checking = self.checking;
            if is_assignable(
                result,
                expected,
                &self.types,
                &mut HashSet::new(),
                &mut budget,
            ) && !budget.exhausted
            {
                if let Some(counterpart) = self
                    .diagnostics
                    .last_mut()
                    .and_then(|diagnostic| diagnostic.typescript.as_mut())
                {
                    let hint = Diagnostic::error(
                        DiagnosticCode::TypeMismatch,
                        counterpart.span.clone(),
                        "",
                    )
                    .with_typescript(6212, Vec::new())
                    .typescript
                    .unwrap();
                    counterpart
                        .related_information
                        .push(crate::TypeScriptRelatedInformation {
                            code: hint.code,
                            message: hint.message,
                            span: hint.span,
                            position: None,
                        });
                }
            }
        }
    }

    pub(in crate::checker::module) fn point_last_typescript(&mut self, tokens: &[Token]) {
        if !self.enforce_types {
            return;
        }
        if let (Some(first), Some(last), Some(diagnostic)) =
            (tokens.first(), tokens.last(), self.diagnostics.last_mut())
        {
            if let Some(counterpart) = &mut diagnostic.typescript {
                counterpart.span = SourceSpan::new(&self.module.id, first.start, last.end);
            }
        }
    }

    pub(in crate::checker::module) fn assignment_error(
        &mut self,
        span: &SourceSpan,
        message: String,
        bts_code: DiagnosticCode,
        actual: &Type,
        expected: &Type,
    ) {
        if self.constructor_assignment_error(span, &message, bts_code, actual, expected) {
            return;
        }
        if self.compatibility_assignment_error(span, &message, bts_code, actual, expected) {
            return;
        }
        if self.additional_assignment_error(span, &message, bts_code, actual, expected) {
            return;
        }
        if self.operator_assignment_error(span, &message, bts_code, actual, expected) {
            return;
        }
        let (actual_fields, actual_exhausted) = self.expanded_record_fields(actual.clone());
        let literal_initializer =
            self.module
                .source
                .get(span.start..span.end)
                .is_some_and(|source| {
                    source
                        .split_once('=')
                        .is_some_and(|(_, value)| value.trim_start().starts_with('{'))
                });
        let (expected_fields, expected_exhausted) = self.expanded_record_fields(expected.clone());
        if !actual_exhausted && !expected_exhausted {
            if let (Some(actual_fields), Some(expected_fields)) = (actual_fields, expected_fields) {
                let missing = expected_fields
                    .iter()
                    .filter(|field| {
                        !field.optional
                            && !actual_fields.iter().any(|actual| actual.name == field.name)
                    })
                    .collect::<Vec<_>>();
                if let [field] = missing.as_slice() {
                    let name = field.name.rsplit(' ').next().unwrap_or(&field.name);
                    // A public/private mismatch or two private declarations
                    // is an incompatible identity, rather than an absent field.
                    let restricted = field.name.contains(' ') && !name.starts_with('#');
                    let conflicting = actual_fields
                        .iter()
                        .any(|actual| actual.name.rsplit(' ').next() == Some(name));
                    if !restricted && !conflicting {
                        self.typescript_type_error(
                            span,
                            message,
                            bts_code,
                            2741,
                            vec![
                                name.into(),
                                crate::diagnostic::type_text::render_in(actual, self.project),
                                crate::diagnostic::type_text::render_in(expected, self.project),
                            ],
                        );
                        return;
                    }
                }
                if self.checking.exact_optional_property_types
                    && expected_fields.iter().any(|field| {
                        field.optional
                            && actual_fields.iter().any(|actual| {
                                actual.name == field.name
                                    && actual.value == Type::Undefined
                                    && field.value != Type::Undefined
                            })
                    })
                {
                    self.typescript_type_error(
                        span,
                        message,
                        bts_code,
                        2375,
                        vec![
                            crate::diagnostic::type_text::render_in(actual, self.project),
                            crate::diagnostic::type_text::render_in(expected, self.project),
                        ],
                    );
                    self.explain_last_type_pair(
                        &Type::Record(actual_fields.clone()),
                        &Type::Record(expected_fields.clone()),
                    );
                    return;
                }
                for field in &actual_fields {
                    if !literal_initializer {
                        break;
                    }
                    let Some(expected_field) = expected_fields
                        .iter()
                        .find(|expected| expected.name == field.name)
                    else {
                        continue;
                    };
                    let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
                    budget.checking = self.checking;
                    if !is_assignable(
                        &field.value,
                        &expected_field.value,
                        &self.types,
                        &mut HashSet::new(),
                        &mut budget,
                    ) && !budget.exhausted
                    {
                        self.typescript_type_error(
                            span,
                            message,
                            bts_code,
                            2322,
                            vec![
                                crate::diagnostic::type_text::render_in(&field.value, self.project),
                                crate::diagnostic::type_text::render_in(
                                    &expected_field.value,
                                    self.project,
                                ),
                            ],
                        );
                        if let Ok(tokens) = crate::syntax::lex(&self.module.id, &self.module.source)
                        {
                            let selected = tokens
                                .iter()
                                .filter(|token| token.start >= span.start && token.end <= span.end)
                                .collect::<Vec<_>>();
                            let initializer = selected
                                .iter()
                                .position(|token| token.is("="))
                                .and_then(|index| selected.get(index + 1))
                                .filter(|token| token.is("{"));
                            if let Some(pair) = tokens
                                .windows(2)
                                .filter(|pair| {
                                    initializer.is_some_and(|start| pair[0].start > start.start)
                                        && pair[0].end <= span.end
                                })
                                .rfind(|pair| pair[0].is(&field.name) && pair[1].is(":"))
                            {
                                self.point_last_typescript(&pair[..1]);
                            }
                        }
                        return;
                    }
                }
            }
        }
        let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
        let display = super::super::super::type_operators::diagnostic_type(
            expected,
            &self.types,
            &mut budget,
        );
        self.typescript_type_error(
            span,
            message,
            bts_code,
            2322,
            vec![
                crate::diagnostic::type_text::render_in(actual, self.project),
                crate::diagnostic::type_text::render_in(&display, self.project),
            ],
        );
        self.explain_last_type_pair(actual, &display);
    }

    pub(in crate::checker::module) fn explain_last_type_pair(
        &mut self,
        actual: &Type,
        expected: &Type,
    ) {
        if self.enforce_types {
            let detail = self
                .compatibility_detail(actual, expected)
                .unwrap_or_else(|| {
                    crate::diagnostic::type_text::detail(actual, expected, self.project)
                });
            if let Some(counterpart) = self
                .diagnostics
                .last_mut()
                .and_then(|diagnostic| diagnostic.typescript.as_mut())
            {
                if matches!(counterpart.code, 2322 | 2345 | 2375) {
                    counterpart.message.push_str(&detail);
                }
            }
        }
    }

    pub(in crate::checker::module) fn rejected_call_error(
        &mut self,
        span: &SourceSpan,
        message: String,
        signatures: &[FunctionSignature],
        actuals: &[Type],
        typescript_overloads: bool,
    ) {
        let mut detail = String::new();
        let matching_arity = signatures
            .iter()
            .filter(|signature| function_signature_accepts_argument_count(signature, actuals.len()))
            .collect::<Vec<_>>();
        let (code, arguments) = if matching_arity.is_empty() && !signatures.is_empty() {
            let minimum = signatures
                .iter()
                .map(function_signature_required_arguments)
                .min()
                .unwrap();
            let maximum = signatures
                .iter()
                .map(|signature| signature.parameters.len())
                .max()
                .unwrap();
            let expected = if minimum == maximum {
                minimum.to_string()
            } else {
                format!("{minimum}-{maximum}")
            };
            (2554, vec![expected, actuals.len().to_string()])
        } else if signatures.len() > 1 || typescript_overloads {
            detail = self.overload_details(signatures, actuals);
            (2769, Vec::new())
        } else if let Some(signature) = matching_arity.first() {
            let mut inference_budget = TypeExpansionBudget::new(self.max_type_expansions);
            inference_budget.checking = self.checking;
            let substitutions =
                infer_call_substitutions(signature, actuals, &self.types, &mut inference_budget);
            let mismatch = actuals.iter().enumerate().find_map(|(index, actual)| {
                let parameter = function_parameter_for_argument(signature, index)?;
                let expected = call_parameter_expected_type(parameter, &substitutions);
                let displayed_expected = if parameter.default.is_some() {
                    match &expected {
                        Type::Union(values) => {
                            let mut values = values
                                .iter()
                                .filter(|value| **value != Type::Undefined)
                                .cloned()
                                .collect::<Vec<_>>();
                            if values.len() == 1 {
                                values.pop().unwrap()
                            } else {
                                Type::Union(values)
                            }
                        }
                        _ => expected.clone(),
                    }
                } else {
                    expected.clone()
                };
                let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
                budget.checking = self.checking;
                (!is_assignable(
                    actual,
                    &expected,
                    &self.types,
                    &mut HashSet::new(),
                    &mut budget,
                ))
                .then(|| {
                    detail = crate::diagnostic::type_text::detail(
                        actual,
                        &displayed_expected,
                        self.project,
                    );
                    vec![
                        crate::diagnostic::type_text::argument(actual, &displayed_expected),
                        crate::diagnostic::type_text::render_in(&displayed_expected, self.project),
                    ]
                })
            });
            (2345, mismatch.unwrap_or_default())
        } else {
            (2345, Vec::new())
        };
        self.typescript_type_error(span, message, DiagnosticCode::TypeMismatch, code, arguments);
        if self.enforce_types {
            if let Some(counterpart) = self
                .diagnostics
                .last_mut()
                .and_then(|diagnostic| diagnostic.typescript.as_mut())
            {
                counterpart.message.push_str(&detail);
            }
        }
    }
}
