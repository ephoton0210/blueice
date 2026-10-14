// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Checks for retained calls and nullable subjects in compound expressions.

use super::*;

impl ModuleChecker<'_> {
    pub(in crate::checker::module) fn check_flow_initializers(
        &mut self,
        function: &FunctionDeclaration,
        scope: &BTreeMap<String, Type>,
    ) {
        let Some(scopes) = &self.scopes else {
            return;
        };
        let execution = scopes.flow_execution(function.span.start);
        let variables = self
            .module
            .expression_variable_types
            .iter()
            .filter_map(|(start, annotation)| {
                (function.span.start <= *start
                    && *start < function.span.end
                    && scopes.flow_execution(*start) == execution
                    && !function
                        .locals
                        .iter()
                        .any(|local| local.span.start <= *start && *start < local.span.end))
                .then(|| scopes.flow_initializer(*start, annotation.clone()))
                .flatten()
            })
            .collect::<Vec<_>>();
        for variable in variables {
            self.check_variable_in_scope(&variable, scope);
        }
    }
    pub(in crate::checker::module) fn check_flow_calls(
        &mut self,
        tokens: &[Token],
        scope: &BTreeMap<String, Type>,
        span: &SourceSpan,
    ) {
        if direct_call_parts(tokens).is_some()
            && tokens
                .iter()
                .position(|token| token.is("("))
                .and_then(|open| super::super::super::scopes::targets::close(tokens, open))
                == tokens.len().checked_sub(1)
        {
            return;
        }
        let execution = self
            .scopes
            .as_ref()
            .map(|scopes| scopes.flow_execution(span.start));
        for (index, token) in tokens.iter().enumerate() {
            if token.kind != TokenKind::Identifier
                || !tokens.get(index + 1).is_some_and(|token| token.is("("))
                || index.checked_sub(1).is_some_and(|previous| {
                    tokens[previous].is(".")
                        || tokens[previous].is("?.")
                        || tokens[previous].is("new")
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
            let call_span = SourceSpan::new(&span.module, token.start, tokens[end].end);
            self.check_function_call(&tokens[index..=end], scope, &call_span);
        }
    }

    pub(in crate::checker::module) fn check_flow_in_subjects(
        &mut self,
        tokens: &[Token],
        scope: &BTreeMap<String, Type>,
    ) {
        for pair in tokens.windows(2) {
            if pair[0].is("in") && pair[1].kind == TokenKind::Identifier {
                let value = self.infer_expression(&pair[1..], scope);
                self.flow_nullish_error(&pair[1], &value);
            }
        }
    }

    pub(in crate::checker::module) fn flow_nullish_error(
        &mut self,
        token: &Token,
        value: &Type,
    ) -> bool {
        if !self.checking.strict_null_checks {
            return false;
        }
        let parts = match value {
            Type::Union(parts) => parts.as_slice(),
            value => std::slice::from_ref(value),
        };
        let null = parts.contains(&Type::Null);
        let undefined = parts.contains(&Type::Undefined);
        if !null && !undefined {
            return false;
        }
        self.typescript_type_error(
            &token.span(&self.module.id),
            format!("`{}` is possibly nullish", token.text),
            DiagnosticCode::TypeMismatch,
            if null && undefined {
                18049
            } else if null {
                18047
            } else {
                18048
            },
            vec![token.text.clone()],
        );
        true
    }
}
