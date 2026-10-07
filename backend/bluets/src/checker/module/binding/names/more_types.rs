// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Index declarations and unique symbol positions retain native origins.
use super::*;

impl ModuleChecker<'_> {
    pub(super) fn check_additional_type(&mut self, value: &Type, span: &SourceSpan) {
        match value {
            Type::Readonly(inner) if !matches!(inner.as_ref(), Type::Array(_) | Type::Tuple(_)) => {
                let tokens =
                    crate::syntax::lex(&self.module.id, &self.module.source).unwrap_or_default();
                if let Some(keyword) = tokens.iter().find(|token| {
                    span.start <= token.start && token.end <= span.end && token.is("readonly")
                }) {
                    self.additional_type_error(keyword.span(&self.module.id), 1354, vec![]);
                }
            }
            Type::UniqueSymbol(origin) => {
                let declaration =
                    self.module
                        .declarations
                        .iter()
                        .find_map(|declaration| match declaration {
                            Declaration::Variable(variable)
                                if variable.annotation.as_ref() == Some(value) =>
                            {
                                Some(variable)
                            }
                            _ => None,
                        });
                let (span, code) = if let Some(variable) = declaration {
                    if variable.kind == crate::parser::VariableKind::Const {
                        return;
                    }
                    let tokens = crate::syntax::lex(&self.module.id, &self.module.source)
                        .unwrap_or_default();
                    let Some(name) = tokens.iter().find(|token| {
                        variable.span.start <= token.start
                            && token.end <= variable.span.end
                            && token.is(&variable.name)
                    }) else {
                        return;
                    };
                    (name.span(&self.module.id), 1332)
                } else {
                    (origin.clone(), 1335)
                };
                self.additional_type_error(span, code, vec![]);
            }
            Type::IndexedRecord { object, indices } => {
                let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
                budget.checking = self.checking;
                for index in indices {
                    if !matches!(
                        index.key,
                        Type::String | Type::Number | Type::Symbol | Type::TemplateLiteral(_)
                    ) {
                        self.additional_type_error(
                            SourceSpan::new(
                                &self.module.id,
                                index.span.start + 1,
                                index.span.start + 1 + index.name.len(),
                            ),
                            1268,
                            vec![],
                        );
                        continue;
                    }
                    if let Type::Record(fields) | Type::CallableRecord { fields, .. } =
                        object.as_ref()
                    {
                        for field in fields {
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
                                self.additional_type_error(
                                    SourceSpan::new(
                                        &self.module.id,
                                        field.span.start,
                                        field.span.start + field.name.len(),
                                    ),
                                    2411,
                                    vec![
                                        field.name.clone(),
                                        crate::diagnostic::type_text::render(&field.value),
                                        crate::diagnostic::type_text::render(&index.key),
                                        crate::diagnostic::type_text::render(&index.value),
                                    ],
                                );
                            }
                        }
                    }
                    if index.key == Type::Number {
                        if let Some(string) = indices.iter().find(|other| other.key == Type::String)
                        {
                            if !is_assignable(
                                &index.value,
                                &string.value,
                                &self.types,
                                &mut HashSet::new(),
                                &mut budget,
                            ) {
                                self.additional_type_error(
                                    index.span.clone(),
                                    2413,
                                    vec![
                                        "number".into(),
                                        crate::diagnostic::type_text::render(&index.value),
                                        "string".into(),
                                        crate::diagnostic::type_text::render(&string.value),
                                    ],
                                );
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn additional_type_error(&mut self, span: SourceSpan, code: u32, arguments: Vec<String>) {
        self.diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::TypeMismatch,
                span,
                "invalid static type declaration",
            )
            .with_typescript(code, arguments)
            .with_source_position(&self.module.source),
        );
    }
}
