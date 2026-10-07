// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Explain rejected overload candidates without selecting or accepting a call.
use super::*;

impl ModuleChecker<'_> {
    pub(super) fn overload_details(
        &self,
        signatures: &[FunctionSignature],
        actuals: &[Type],
    ) -> String {
        let mut details = String::new();
        let mut ordinal = 0;
        for signature in signatures {
            if !function_signature_accepts_argument_count(signature, actuals.len()) {
                continue;
            }
            let mut inference_budget = TypeExpansionBudget::new(self.max_type_expansions);
            inference_budget.checking = self.checking;
            let substitutions =
                infer_call_substitutions(signature, actuals, &self.types, &mut inference_budget);
            let mismatch = actuals.iter().enumerate().find_map(|(index, actual)| {
                let parameter = function_parameter_for_argument(signature, index)?;
                let expected = call_parameter_expected_type(parameter, &substitutions);
                let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
                budget.checking = self.checking;
                if is_assignable(
                    actual,
                    &expected,
                    &self.types,
                    &mut HashSet::new(),
                    &mut budget,
                ) {
                    return None;
                }
                let expected = if parameter.optional || parameter.default.is_some() {
                    if let Type::Union(values) = &expected {
                        let mut values = values
                            .iter()
                            .filter(|value| !matches!(value, Type::Undefined | Type::Null))
                            .cloned()
                            .collect::<Vec<_>>();
                        if values.len() == 1 {
                            values.pop().unwrap()
                        } else {
                            Type::Union(values)
                        }
                    } else {
                        expected
                    }
                } else {
                    expected
                };
                if let (
                    Type::Function {
                        parameters: actual_parameters,
                        result: actual_result,
                    },
                    Type::Function {
                        parameters: expected_parameters,
                        result: expected_result,
                    },
                ) = (actual, &expected)
                {
                    if actual_parameters
                        .iter()
                        .zip(expected_parameters)
                        .all(|(actual, expected)| actual.annotation == expected.annotation)
                        && **expected_result != Type::Void
                        && actual_result != expected_result
                    {
                        return Some(format!(
                            "Type '{}' is not assignable to type '{}'.",
                            crate::diagnostic::type_text::render_in(actual_result, self.project),
                            crate::diagnostic::type_text::render_in(expected_result, self.project)
                        ));
                    }
                }
                Some(format!(
                    "Argument of type '{}' is not assignable to parameter of type '{}'.{}",
                    crate::diagnostic::type_text::argument(actual, &expected),
                    crate::diagnostic::type_text::render_in(&expected, self.project),
                    crate::diagnostic::type_text::detail(actual, &expected, self.project)
                ))
            });
            let Some(mismatch) = mismatch else { continue };
            ordinal += 1;
            let parameters = crate::diagnostic::type_text::parameter_list_in(
                &signature.parameters,
                Some(self.project),
            );
            let result =
                crate::diagnostic::type_text::render_in(&signature.return_type, self.project);
            details.push_str(&format!("\n  Overload {ordinal} of {}, '({parameters}): {result}', gave the following error.",signatures.len()));
            for line in mismatch.lines() {
                details.push_str(&format!("\n    {line}"));
            }
        }
        details
    }
}

impl ModuleChecker<'_> {
    pub(in crate::checker::module) fn present_callback_return_error(
        &mut self,
        signatures: &[FunctionSignature],
        actuals: &[Type],
        arguments: &[&[Token]],
        callee: &Token,
    ) {
        for signature in signatures {
            let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
            budget.checking = self.checking;
            let substitutions =
                infer_call_substitutions(signature, actuals, &self.types, &mut budget);
            for (index, actual) in actuals.iter().enumerate() {
                let Some(parameter) = function_parameter_for_argument(signature, index) else {
                    break;
                };
                let expected = call_parameter_expected_type(parameter, &substitutions);
                if is_assignable(
                    actual,
                    &expected,
                    &self.types,
                    &mut HashSet::new(),
                    &mut budget,
                ) {
                    continue;
                }
                let (
                    Type::Function {
                        result: actual_result,
                        ..
                    },
                    Type::Function {
                        result: expected_result,
                        ..
                    },
                ) = (actual, &expected)
                else {
                    break;
                };
                if is_assignable(
                    actual_result,
                    expected_result,
                    &self.types,
                    &mut HashSet::new(),
                    &mut budget,
                ) {
                    break;
                }
                let Some(argument) = arguments.get(index) else {
                    break;
                };
                let Some(function) = argument
                    .first()
                    .and_then(|token| self.module.nested_functions.get(&token.start))
                else {
                    break;
                };
                if function.parameters.is_empty() {
                    break;
                }
                let start = callee.start;
                let end = callee.end;
                let origin = self
                    .project
                    .source(&parameter.span.module)
                    .and_then(|source| crate::syntax::lex(&parameter.span.module, source).ok())
                    .and_then(|tokens| {
                        let selected = tokens
                            .iter()
                            .filter(|token| {
                                parameter.span.start <= token.start
                                    && token.end <= parameter.span.end
                            })
                            .collect::<Vec<_>>();
                        let colon = selected.iter().position(|token| token.is(":"))?;
                        Some(SourceSpan::new(
                            &parameter.span.module,
                            selected.get(colon + 1)?.start,
                            selected.last()?.end,
                        ))
                    });
                if let Some(counterpart) = self
                    .diagnostics
                    .last_mut()
                    .and_then(|diagnostic| diagnostic.typescript.as_mut())
                {
                    counterpart.span = SourceSpan::new(&self.module.id, start, end);
                    if let Some(origin) = origin {
                        let hint = Diagnostic::error(DiagnosticCode::TypeMismatch, origin, "")
                            .with_typescript(6502, Vec::new())
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
                return;
            }
        }
    }
}
