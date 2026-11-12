// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Contextually typed concise callbacks diagnose their incompatible return.

use super::*;

impl ModuleChecker<'_> {
    pub(super) fn contextual_callback_result_error(
        &mut self,
        actual: &Type,
        expected: &Type,
        argument: &[Token],
        parameter: &Parameter,
    ) -> bool {
        let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
        budget.checking = self.checking;
        let expected = crate::checker::type_operators::expanded(
            expected,
            &self.types,
            &mut HashSet::new(),
            &mut budget,
        );
        let (
            Type::Function {
                parameters: actual_parameters,
                result: actual_result,
            },
            Type::Function {
                parameters: expected_parameters,
                result: expected_result,
            },
        ) = (actual, expected)
        else {
            return false;
        };
        if *expected_result == Type::Void
            || is_assignable(
                actual_result,
                &expected_result,
                &self.types,
                &mut HashSet::new(),
                &mut budget,
            )
            || !is_assignable(
                &Type::Function {
                    parameters: actual_parameters.clone(),
                    result: Box::new(Type::Void),
                },
                &Type::Function {
                    parameters: expected_parameters,
                    result: Box::new(Type::Void),
                },
                &self.types,
                &mut HashSet::new(),
                &mut budget,
            )
            || budget.exhausted
        {
            return false;
        }
        let Some(function) = strip_outer_parentheses(argument)
            .first()
            .and_then(|token| self.module.nested_functions.get(&token.start))
            .filter(|function| {
                function.return_type.is_none() && function.type_parameters.is_empty()
            })
        else {
            return false;
        };
        let crate::parser::NestedFunctionBody::Expression(body) = &function.body else {
            return false;
        };
        let (Some(first), Some(last)) = (body.first(), body.last()) else {
            return false;
        };
        let point = SourceSpan::new(&self.module.id, first.start, last.end);
        // Only a source function signature contributes a declaration-site
        // return hint. A container or alias does not fabricate that origin.
        let origin = matches!(parameter.annotation, Some(Type::Function { .. }))
            .then(|| {
                let source = self.project.source(&parameter.span.module)?;
                let tokens = crate::syntax::lex(&parameter.span.module, source).ok()?;
                let tokens = tokens
                    .iter()
                    .filter(|token| {
                        parameter.span.start <= token.start && token.end <= parameter.span.end
                    })
                    .collect::<Vec<_>>();
                let colon = tokens.iter().position(|token| token.is(":"))?;
                Some(SourceSpan::new(
                    &parameter.span.module,
                    tokens.get(colon + 1)?.start,
                    tokens.last()?.end,
                ))
            })
            .flatten();
        self.typescript_type_error(
            &point,
            "contextual callback return type is incompatible".into(),
            DiagnosticCode::TypeMismatch,
            2322,
            vec![
                crate::diagnostic::type_text::render_in(actual_result, self.project),
                crate::diagnostic::type_text::render_in(&expected_result, self.project),
            ],
        );
        self.explain_last_type_pair(actual_result, &expected_result);
        if let Some(origin) = origin {
            let related = Diagnostic::error(DiagnosticCode::TypeMismatch, origin, "")
                .with_typescript(6502, Vec::new())
                .typescript
                .unwrap();
            if let Some(counterpart) = self
                .diagnostics
                .last_mut()
                .and_then(|diagnostic| diagnostic.typescript.as_mut())
            {
                counterpart
                    .related_information
                    .push(crate::TypeScriptRelatedInformation {
                        code: related.code,
                        message: related.message,
                        span: related.span,
                        position: None,
                    });
            }
        }
        true
    }
}
