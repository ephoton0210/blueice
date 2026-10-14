// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Opt-in function diagnostics do not change emitted bodies.

use super::*;

impl ModuleChecker<'_> {
    pub(super) fn check_function_flags(
        &mut self,
        function: &FunctionDeclaration,
        scope: &BTreeMap<String, Type>,
    ) {
        if !self.explicit_checking || function.declared || function.overload {
            return;
        }
        if self.checking.no_implicit_any {
            for parameter in &function.parameters {
                if parameter.annotation.is_none()
                    && parameter.default.is_none()
                    && !self
                        .return_inference
                        .parameters
                        .borrow()
                        .contains_key(&parameter.span.start)
                {
                    self.type_error(
                        &parameter.span,
                        format!(
                            "parameter `{}` implicitly has an `any` type",
                            parameter.name
                        ),
                        DiagnosticCode::TypeMismatch,
                    );
                }
            }
        }
        if self.checking.no_implicit_this
            && !scope.contains_key("this")
            && !function.parameters.iter().any(|p| p.name == "this")
        {
            let execution = self
                .scopes
                .as_ref()
                .map(|scopes| scopes.flow_execution(function.span.end.saturating_sub(1)));
            let tokens = crate::syntax::lex_with_limits(
                &self.module.id,
                &self.module.source[function.span.start..function.span.end],
                function.span.end - function.span.start,
                function.span.end - function.span.start + 1,
            )
            .unwrap_or_default();
            let positions: Vec<_> = tokens
                .iter()
                .filter(|t| t.is("this"))
                .filter(|token| {
                    let position = function.span.start + token.start;
                    !self.module.class_expressions().any(|expression| {
                        expression.class.span.start <= position
                            && position < expression.class.span.end
                    }) && self
                        .scopes
                        .as_ref()
                        .is_none_or(|scopes| Some(scopes.flow_execution(position)) == execution)
                })
                .map(|token| (token.start, token.end))
                .collect();
            for (start, end) in positions {
                self.type_error(
                    &SourceSpan::new(
                        &self.module.id,
                        function.span.start + start,
                        function.span.start + end,
                    ),
                    "`this` implicitly has an `any` type".to_string(),
                    DiagnosticCode::TypeMismatch,
                );
            }
        }
        let returns = return_inference::has_return(&function.body);
        let completes = self.inference_body_completes(&function.body, scope);
        if self.checking.no_implicit_returns
            && function.returns.iter().any(|tokens| !tokens.is_empty())
            && completes
            && function
                .return_type
                .as_ref()
                .is_none_or(|t| !matches!(t, Type::Void | Type::Any | Type::Undefined))
        {
            self.type_error(
                &function.span,
                "not all code paths return a value".to_string(),
                DiagnosticCode::ReturnTypeMismatch,
            );
        }
        if !self.checking.strict_null_checks
            && !returns
            && completes
            && function.return_type.as_ref().is_some_and(|t| {
                !matches!(t, Type::Any | Type::Unknown | Type::Void | Type::Undefined)
            })
        {
            self.type_error(
                &function.span,
                "a function with this declared return type must return a value".to_string(),
                DiagnosticCode::ReturnTypeMismatch,
            );
        }
    }

    pub(super) fn contextualize_call_arguments(
        &self,
        tokens: &[Token],
        scope: &BTreeMap<String, Type>,
    ) {
        for range in member_call_ranges(tokens, |start| {
            self.module.generic_call_type_arguments.contains_key(&start)
        }) {
            let Some(call) = member_call_parts(&tokens[range], |start| {
                self.module.generic_call_type_arguments.contains_key(&start)
            }) else {
                continue;
            };
            let base = self.infer_expression(call.receiver, scope);
            let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
            let PropertyType::Found { value, .. } = property_type(
                &base,
                &call.member.text,
                &self.types,
                &mut HashSet::new(),
                &mut budget,
            ) else {
                continue;
            };
            let Some(signatures) =
                super::super::expressions::signatures::callable_signatures(&value, false)
            else {
                continue;
            };
            if let Some(arguments) = split_call_arguments(call.arguments) {
                let _ = self.expanded_call_argument_types_for(&arguments, scope, &signatures);
            }
        }
        let execution = tokens.first().and_then(|token| {
            self.scopes
                .as_ref()
                .map(|scopes| scopes.flow_execution(token.start))
        });
        for (index, token) in tokens.iter().enumerate() {
            if token.kind != TokenKind::Identifier
                || !tokens.get(index + 1).is_some_and(|token| token.is("("))
                || index.checked_sub(1).is_some_and(|previous| {
                    matches!(tokens[previous].text.as_str(), "." | "?." | "new")
                })
                || self
                    .scopes
                    .as_ref()
                    .is_some_and(|scopes| Some(scopes.flow_execution(token.start)) != execution)
            {
                continue;
            }
            let Some(end) = super::super::super::scopes::targets::close(tokens, index + 1) else {
                continue;
            };
            let Some(call) = direct_call_parts(&tokens[index..=end]) else {
                continue;
            };
            let signatures = self
                .function_value_signatures(&call.callee.text, scope, false)
                .or_else(|| self.functions.get(&call.callee.text).cloned());
            let Some(signatures) = signatures else {
                continue;
            };
            let Some(arguments) = split_call_arguments(call.arguments) else {
                continue;
            };
            let _ = self.expanded_call_argument_types_for(&arguments, scope, &signatures);
        }
    }
}
