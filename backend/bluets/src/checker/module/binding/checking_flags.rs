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
            for token in crate::syntax::lex_with_limits(
                &self.module.id,
                &self.module.source[function.span.start..function.span.end],
                function.span.end - function.span.start,
                function.span.end - function.span.start + 1,
            )
            .unwrap_or_default()
            .iter()
            .filter(|t| t.is("this"))
            {
                self.type_error(
                    &SourceSpan::new(
                        &self.module.id,
                        function.span.start + token.start,
                        function.span.start + token.end,
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
        if !self.explicit_checking {
            return;
        }
        let Some(call) = direct_call_parts(tokens) else {
            return;
        };
        let signatures = self
            .function_value_signature(&call.callee.text, scope)
            .map(|s| vec![s])
            .or_else(|| self.functions.get(&call.callee.text).cloned());
        let Some(signature) = signatures.as_ref().and_then(|s| s.last()) else {
            return;
        };
        let Some(arguments) = split_call_arguments(call.arguments) else {
            return;
        };
        for (argument, parameter) in arguments.iter().zip(&signature.parameters) {
            if let Some(expected) = &parameter.annotation {
                self.infer_in_context(argument, scope, expected);
            }
        }
    }
}
