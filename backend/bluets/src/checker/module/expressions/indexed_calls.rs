// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Indexed callees reuse the same signatures, arguments and receiver checks.

use super::*;

impl ModuleChecker<'_> {
    pub(in crate::checker::module) fn infer_indexed_call(
        &self,
        tokens: &[Token],
        scope: &BTreeMap<String, Type>,
    ) -> Option<Type> {
        let call = indexed_call_parts(tokens, |start| {
            self.module.generic_call_type_arguments.contains_key(&start)
        })?;
        let callee = self.infer_expression(call.callee, scope);
        if callee == Type::Any {
            return Some(Type::Any);
        }
        let name = format!("#indexed@{}", call.callee[0].start);
        let mut local = scope.clone();
        local.insert(name.clone(), callee);
        Some(
            self.function_value_signatures(&name, &local, false)
                .map_or(Type::Unknown, |signatures| {
                    self.infer_function_call(&signatures, call.arguments, scope, None)
                }),
        )
    }

    pub(in crate::checker::module) fn check_indexed_calls_in_expression(
        &mut self,
        tokens: &[Token],
        scope: &BTreeMap<String, Type>,
        span: &SourceSpan,
    ) {
        for range in indexed_call_ranges(tokens, |start| {
            self.module.generic_call_type_arguments.contains_key(&start)
        }) {
            let Some(call) = indexed_call_parts(&tokens[range], |start| {
                self.module.generic_call_type_arguments.contains_key(&start)
            }) else {
                continue;
            };
            let Some((receiver, _)) = member_access_target(call.callee) else {
                continue;
            };
            let value = self.infer_expression(call.callee, scope);
            let name = format!("#indexed@{}", call.callee[0].start);
            let mut local = scope.clone();
            local.insert(name.clone(), value.clone());
            let first = &call.callee[0];
            let last = call.callee.last().expect("indexed callee is nonempty");
            let callee_span = SourceSpan::new(&span.module, first.start, last.end);
            if self
                .function_value_signatures(&name, &local, false)
                .is_none()
            {
                if !matches!(value, Type::Any | Type::Unknown) {
                    let label = match &value {
                        Type::Number => "Number".to_string(),
                        Type::String => "String".to_string(),
                        Type::Boolean => "Boolean".to_string(),
                        _ => crate::diagnostic::type_text::render_in(&value, self.project),
                    };
                    let message = format!(
                        "This expression is not callable.\n  Type '{label}' has no call signatures."
                    );
                    let before = self.diagnostics.len();
                    self.typescript_type_error(
                        &callee_span,
                        message.clone(),
                        DiagnosticCode::TypeMismatch,
                        2349,
                        Vec::new(),
                    );
                    if self.diagnostics.len() > before {
                        if let Some(counterpart) = self
                            .diagnostics
                            .last_mut()
                            .and_then(|diagnostic| diagnostic.typescript.as_mut())
                        {
                            counterpart.message = message;
                        }
                    }
                }
                continue;
            }
            let mut qualified = vec![
                Token {
                    kind: TokenKind::Identifier,
                    text: name.clone(),
                    start: first.start,
                    end: last.end,
                },
                Token {
                    kind: TokenKind::Punct,
                    text: "(".to_string(),
                    start: last.end,
                    end: last.end,
                },
            ];
            qualified.extend_from_slice(call.arguments);
            let before = self.diagnostics.len();
            self.check_function_call_with_receiver(&qualified, &local, span, Some(receiver));
            let original = self
                .module
                .source
                .get(first.start..last.end)
                .unwrap_or("indexed value");
            for diagnostic in &mut self.diagnostics[before..] {
                diagnostic.message = diagnostic.message.replace(&name, original);
            }
        }
    }
}
