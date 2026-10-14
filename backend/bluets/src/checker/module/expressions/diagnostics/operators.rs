// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Assignment presentation for mapped shapes and literal operator products.
use super::*;
use crate::checker::type_operators;

impl ModuleChecker<'_> {
    pub(super) fn operator_assignment_error(
        &mut self,
        span: &SourceSpan,
        message: &str,
        code: DiagnosticCode,
        actual: &Type,
        expected: &Type,
    ) -> bool {
        let mut target = expected.clone();
        let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
        let mut visited = HashSet::new();
        while matches!(target, Type::Named { .. }) {
            let Some(next) = instantiate_named(
                &target,
                &self.types,
                &mut visited,
                &mut budget,
                "assignment shape",
            ) else {
                break;
            };
            target = next;
        }
        let literal_initializer =
            self.module
                .source
                .get(span.start..span.end)
                .is_some_and(|source| {
                    source
                        .split_once('=')
                        .is_some_and(|(_, value)| value.trim_start().starts_with('{'))
                });
        if matches!(target, Type::Mapped(_)) && literal_initializer {
            let (Some(actual_fields), false) = self.expanded_record_fields(actual.clone()) else {
                return false;
            };
            let (Some(expected_fields), false) = self.expanded_record_fields(expected.clone())
            else {
                return false;
            };
            if let Some(field) = actual_fields.iter().find(|field| {
                !expected_fields
                    .iter()
                    .any(|expected| expected.name == field.name)
            }) {
                self.typescript_type_error(
                    span,
                    message.into(),
                    code,
                    2353,
                    vec![
                        field.name.clone(),
                        crate::diagnostic::type_text::render_in(expected, self.project),
                    ],
                );
                self.point_operator_field(&field.name, &field.span);
                return true;
            }
        }
        if literal_initializer {
            if let (Type::Record(actual), Type::Union(parts)) = (actual, &target) {
                for part in parts {
                    let (Some(fields), false) = self.expanded_record_fields(part.clone()) else {
                        continue;
                    };
                    for field in actual {
                        let Some(expected) =
                            fields.iter().find(|expected| expected.name == field.name)
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
                                message.into(),
                                code,
                                2322,
                                vec![
                                    crate::diagnostic::type_text::render_in(
                                        &field.value,
                                        self.project,
                                    ),
                                    crate::diagnostic::type_text::render_in(
                                        &expected.value,
                                        self.project,
                                    ),
                                ],
                            );
                            self.point_operator_field(&field.name, &field.span);
                            return true;
                        }
                    }
                }
            }
        }
        let Type::TemplateLiteral(_) = target else {
            return false;
        };
        let display = type_operators::diagnostic_type(expected, &self.types, &mut budget);
        let (Type::Literal(actual), Type::Union(parts)) = (actual, &display) else {
            return false;
        };
        let actual_name = actual.trim_matches(['\'', '"']);
        let names = parts
            .iter()
            .filter_map(|value| {
                if let Type::Literal(name) = value {
                    Some(name.trim_matches(['\'', '"']))
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        let Some(suggestion) =
            crate::diagnostic::spelling::literal_suggestion(actual_name, names.iter().copied())
        else {
            return false;
        };
        self.typescript_type_error(
            span,
            message.into(),
            code,
            2820,
            vec![
                format!("{actual_name:?}"),
                crate::diagnostic::type_text::render_in(&display, self.project),
                format!("{suggestion:?}"),
            ],
        );
        true
    }

    fn point_operator_field(&mut self, name: &str, span: &SourceSpan) {
        let Ok(tokens) = crate::syntax::lex(&self.module.id, &self.module.source) else {
            return;
        };
        if let Some(token) = tokens
            .iter()
            .find(|token| span.start <= token.start && token.end <= span.end && token.is(name))
        {
            self.point_last_typescript(std::slice::from_ref(token));
        }
    }
}
