// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Member call validation.

use super::*;

impl<'a> ModuleChecker<'a> {
    pub(in crate::checker::module) fn check_member_calls_in_expression(
        &mut self,
        tokens: &[Token],
        scope: &BTreeMap<String, Type>,
        span: &SourceSpan,
    ) {
        let mut open = Vec::new();
        let mut closes = vec![None; tokens.len()];
        for (index, token) in tokens.iter().enumerate() {
            if token.is("(") {
                open.push(index);
            } else if token.is(")") {
                if let Some(start) = open.pop() {
                    closes[start] = Some(index);
                }
            }
        }
        for start in 0..tokens.len().saturating_sub(3) {
            if tokens[start].kind != TokenKind::Identifier
                || !tokens[start + 1].is(".")
                || tokens[start + 2].kind != TokenKind::Identifier
                || !tokens[start + 3].is("(")
            {
                continue;
            }
            if let Some(end) = closes[start + 3] {
                self.check_member_call(&tokens[start..=end], scope, span);
            }
        }
        // A method on a call result has no identifier immediately before its
        // final dot (for example `document.getElementById('x')!.appendChild(y)`).
        // The direct-call scan above checks the inner call; check the complete
        // chain once for the outer receiver and its arguments.
        if member_call_parts(tokens).is_some_and(|call| call.receiver.len() > 1) {
            self.check_member_call(tokens, scope, span);
        }
    }

    fn check_member_call(
        &mut self,
        tokens: &[Token],
        scope: &BTreeMap<String, Type>,
        span: &SourceSpan,
    ) {
        let tokens = strip_outer_parentheses(tokens);
        let tokens = if tokens.len() > 1 && tokens.last().is_some_and(|token| token.is("!")) {
            &tokens[..tokens.len() - 1]
        } else {
            tokens
        };
        let Some(call) = member_call_parts(tokens) else {
            return;
        };
        if self.require_declared_global_calls
            && call.receiver.first().is_some_and(|base| {
                base.kind == TokenKind::Identifier && !scope.contains_key(&base.text)
            })
        {
            let base = &call.receiver[0];
            self.type_error(
                span,
                format!("object {} is not declared by this page profile", base.text),
                DiagnosticCode::UnknownName,
            );
            return;
        }
        let base = self.infer_expression(call.receiver, scope);
        let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
        let member_type = property_type(
            &base,
            &call.member.text,
            &self.types,
            &mut HashSet::new(),
            &mut budget,
        );
        let (parameters, _) = match member_type {
            PropertyType::Found {
                value: Type::Function { parameters, result },
                ..
            } => (parameters, result),
            PropertyType::Found { .. } => {
                self.type_error(
                    span,
                    format!("property `{}` is not callable", call.member.text),
                    DiagnosticCode::TypeMismatch,
                );
                return;
            }
            PropertyType::Missing
                if matches!(
                    base,
                    Type::String | Type::Number | Type::Boolean | Type::Array(_) | Type::Tuple(_)
                ) =>
            {
                // BlueTS does not yet model the JavaScript built-in method
                // catalogs. Preserve their runtime semantics while still
                // rejecting missing members on declared host interfaces.
                return;
            }
            PropertyType::Missing => {
                self.type_error(
                    span,
                    format!(
                        "property `{}` does not exist on type `{}`",
                        call.member.text,
                        type_label(&base)
                    ),
                    DiagnosticCode::TypeMismatch,
                );
                return;
            }
            PropertyType::Exhausted => {
                self.type_error(
                    span,
                    format!(
                        "property lookup exceeds the {} generic-expansion limit",
                        self.max_type_expansions
                    ),
                    DiagnosticCode::ResourceLimit,
                );
                return;
            }
            PropertyType::Indeterminate => return,
        };
        let Some(arguments) = split_call_arguments(call.arguments) else {
            return;
        };
        for argument in &arguments {
            self.check_function_call(argument, scope, span);
        }
        let Ok(actuals) = self.expanded_call_argument_types(&arguments, scope) else {
            self.type_error(
                span,
                format!(
                    "a spread argument for method {} must have a fixed-length tuple type",
                    call.member.text
                ),
                DiagnosticCode::TypeMismatch,
            );
            return;
        };
        let required = parameters
            .iter()
            .filter(|parameter| !parameter.optional)
            .count();
        if actuals.len() < required || actuals.len() > parameters.len() {
            self.type_error(
                span,
                format!(
                    "method {} expects {required} to {} argument(s), got {}",
                    call.member.text,
                    parameters.len(),
                    actuals.len()
                ),
                DiagnosticCode::TypeMismatch,
            );
            return;
        }
        for (index, actual) in actuals.iter().enumerate() {
            let expected = parameters[index]
                .annotation
                .as_ref()
                .expect("method signature parameters have annotations");
            if !self.is_assignable_bounded(actual, expected, span) {
                self.type_error(
                    span,
                    format!(
                        "argument {} has type `{}`, which is not assignable to method parameter `{}` of type `{}`",
                        index + 1,
                        type_label(actual),
                        parameters[index].name,
                        type_label(expected)
                    ),
                    DiagnosticCode::TypeMismatch,
                );
            }
        }
    }
}
