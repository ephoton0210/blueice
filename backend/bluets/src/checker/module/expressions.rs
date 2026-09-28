// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Expression inference and direct-runtime checks for one module.

use super::*;

mod indexing;
mod inference;
mod method_overloads;
mod optional_property;
mod readonly;

use indexing::{canonical_index_key, indexed_value_type};
use method_overloads::{
    select_callback_method_overload, supports_callback_method_receiver, MethodOverloadError,
};
use optional_property::optional_property_type;

fn method_overload_signatures(overloads: &[Type]) -> Option<Vec<FunctionSignature>> {
    overloads
        .iter()
        .map(|overload| {
            let Type::Function { parameters, result } = overload else {
                return None;
            };
            Some(FunctionSignature {
                parameters: parameters.clone(),
                type_parameters: Vec::new(),
                return_type: *result.clone(),
            })
        })
        .collect()
}

impl<'a> ModuleChecker<'a> {
    pub(super) fn infer_function_call(
        &self,
        signatures: &[FunctionSignature],
        tokens: &[Token],
        scope: &BTreeMap<String, Type>,
        explicit_type_arguments: Option<&[Type]>,
    ) -> Type {
        let Some(arguments) = split_call_arguments(tokens) else {
            return Type::Unknown;
        };
        let Ok(actuals) = self.expanded_call_argument_types(&arguments, scope) else {
            return Type::Unknown;
        };
        let Ok(Some(signature)) =
            self.select_function_signature(signatures, &actuals, explicit_type_arguments)
        else {
            return Type::Unknown;
        };
        let substitutions =
            function_call_substitutions(signature, &actuals, explicit_type_arguments)
                .expect("selected function signature has valid substitutions");
        substitute_type(&signature.return_type, &substitutions)
    }

    pub(super) fn check_function_call(
        &mut self,
        tokens: &[Token],
        scope: &BTreeMap<String, Type>,
        span: &SourceSpan,
    ) {
        let Some(call) = direct_call_parts(tokens) else {
            return;
        };
        let Some(signatures) = self.functions.get(&call.callee.text).cloned() else {
            if self.require_declared_global_calls && !scope.contains_key(&call.callee.text) {
                self.type_error(
                    span,
                    format!(
                        "function {} is not declared by this page profile",
                        call.callee.text
                    ),
                    DiagnosticCode::UnknownName,
                );
            }
            return;
        };
        let Some(arguments) = split_call_arguments(call.arguments) else {
            return;
        };
        for argument in &arguments {
            let argument = if argument.first().is_some_and(|token| token.is("...")) {
                &argument[1..]
            } else {
                argument
            };
            self.check_direct_property_access(argument, scope, span);
        }
        let actuals = match self.expanded_call_argument_types(&arguments, scope) {
            Ok(actuals) => actuals,
            Err(()) => {
                self.type_error(
                    span,
                    format!(
                        "a spread argument for function {} must have a fixed-length tuple type",
                        call.callee.text
                    ),
                    DiagnosticCode::TypeMismatch,
                );
                return;
            }
        };
        let explicit = call.generic.then(|| {
            self.module
                .generic_call_type_arguments
                .get(&call.callee.start)
                .expect("parsed generic call has recorded type arguments")
                .as_slice()
        });
        let selected = match self.select_function_signature(&signatures, &actuals, explicit) {
            Ok(selected) => selected.cloned(),
            Err(()) => {
                self.type_error(
                    span,
                    format!(
                        "overload selection exceeds the {} generic-expansion limit",
                        self.max_type_expansions
                    ),
                    DiagnosticCode::ResourceLimit,
                );
                return;
            }
        };
        if signatures.len() > 1 && selected.is_none() {
            self.type_error(
                span,
                format!(
                    "no overload of function {} accepts the supplied argument types",
                    call.callee.text
                ),
                DiagnosticCode::TypeMismatch,
            );
            return;
        }
        let Some(signature) = selected.or_else(|| signatures.first().cloned()) else {
            return;
        };
        let required = function_signature_required_arguments(&signature);
        if !function_signature_accepts_argument_count(&signature, actuals.len()) {
            let expected = if signature
                .parameters
                .last()
                .is_some_and(|parameter| parameter.rest)
            {
                format!("at least {required}")
            } else {
                format!("{required} to {}", signature.parameters.len())
            };
            self.type_error(
                span,
                format!(
                    "function {} expects {} argument(s), got {}",
                    call.callee.text,
                    expected,
                    actuals.len()
                ),
                DiagnosticCode::TypeMismatch,
            );
            return;
        }
        let substitutions = if let Some(explicit) = explicit {
            let Some(substitutions) =
                self.check_explicit_function_type_arguments(&signature, explicit, span)
            else {
                return;
            };
            substitutions
        } else {
            let substitutions = infer_call_substitutions(&signature, &actuals);
            self.check_call_type_parameter_constraints(
                &signature,
                &substitutions,
                span,
                "inferred type",
            );
            substitutions
        };
        for (index, actual) in actuals.iter().enumerate() {
            let parameter = function_parameter_for_argument(&signature, index)
                .expect("an accepted function call has a parameter for every argument");
            let expected = call_parameter_expected_type(parameter, &substitutions);
            if !self.is_assignable_bounded(actual, &expected, span) {
                self.type_error(
                    span,
                    format!(
                        "argument {} has type `{}`, which is not assignable to parameter `{}` of type `{}`",
                        index + 1,
                        type_label(actual),
                        parameter.name,
                        type_label(&expected)
                    ),
                    DiagnosticCode::TypeMismatch,
                );
            }
        }
    }

    pub(super) fn expanded_call_argument_types(
        &self,
        arguments: &[&[Token]],
        scope: &BTreeMap<String, Type>,
    ) -> Result<Vec<Type>, ()> {
        let mut actuals = Vec::new();
        for argument in arguments {
            if argument.first().is_some_and(|token| token.is("...")) {
                let Type::Tuple(values) = self.infer_expression(&argument[1..], scope) else {
                    return Err(());
                };
                actuals.extend(values);
            } else {
                actuals.push(match *argument {
                    [literal] if literal.kind == TokenKind::String => {
                        Type::Literal(literal.text.clone())
                    }
                    _ => self.infer_expression(argument, scope),
                });
            }
        }
        Ok(actuals)
    }

    pub(super) fn select_function_signature<'b>(
        &self,
        signatures: &'b [FunctionSignature],
        actuals: &[Type],
        explicit_type_arguments: Option<&[Type]>,
    ) -> Result<Option<&'b FunctionSignature>, ()> {
        for signature in signatures {
            if function_signature_matches(
                signature,
                actuals,
                explicit_type_arguments,
                &self.types,
                self.max_type_expansions,
            )? {
                return Ok(Some(signature));
            }
        }
        Ok(None)
    }

    pub(super) fn is_assignable_bounded(
        &mut self,
        actual: &Type,
        expected: &Type,
        span: &SourceSpan,
    ) -> bool {
        let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
        let assignable = if self.strict_catch_unknown && matches!(actual, Type::Unknown) {
            accepts_strict_unknown(expected, &self.types, &mut HashSet::new(), &mut budget)
        } else {
            is_assignable(
                actual,
                expected,
                &self.types,
                &mut HashSet::new(),
                &mut budget,
            )
        };
        if budget.exhausted {
            self.type_error(
                span,
                format!(
                    "type comparison exceeds the {} generic-expansion limit",
                    self.max_type_expansions
                ),
                DiagnosticCode::ResourceLimit,
            );
            true
        } else {
            assignable
        }
    }

    pub(super) fn check_call_type_parameter_constraints(
        &mut self,
        signature: &FunctionSignature,
        substitutions: &BTreeMap<String, Type>,
        span: &SourceSpan,
        actual_description: &str,
    ) {
        for parameter in &signature.type_parameters {
            let Some(constraint) = &parameter.constraint else {
                continue;
            };
            let actual = substitutions
                .get(&parameter.name)
                .expect("function substitutions contain every type parameter");
            let expected = substitute_type(constraint, substitutions);
            if !self.is_assignable_bounded(actual, &expected, span) {
                self.type_error(
                    span,
                    format!(
                        "{actual_description} `{}` does not satisfy constraint `{}` for `{}`",
                        type_label(actual),
                        type_label(&expected),
                        parameter.name,
                    ),
                    DiagnosticCode::TypeMismatch,
                );
            }
        }
    }

    pub(super) fn check_explicit_function_type_arguments(
        &mut self,
        signature: &FunctionSignature,
        arguments: &[Type],
        span: &SourceSpan,
    ) -> Option<BTreeMap<String, Type>> {
        let required = signature
            .type_parameters
            .iter()
            .filter(|parameter| parameter.default.is_none())
            .count();
        if arguments.len() < required || arguments.len() > signature.type_parameters.len() {
            self.type_error(
                span,
                format!(
                    "function type arguments require {required} to {} argument(s), got {}",
                    signature.type_parameters.len(),
                    arguments.len(),
                ),
                DiagnosticCode::TypeMismatch,
            );
            return None;
        }
        for argument in arguments {
            self.check_type(argument, span);
        }
        let completed = complete_type_arguments(&signature.type_parameters, arguments)?;
        let substitutions = type_parameter_substitutions(&signature.type_parameters, completed);
        self.check_call_type_parameter_constraints(
            signature,
            &substitutions,
            span,
            "type argument",
        );
        Some(substitutions)
    }
}

fn erased_assertion_operand(tokens: &[Token]) -> Option<&[Token]> {
    let mut depth = 0usize;
    let mut assertion = None;
    for (index, token) in tokens.iter().enumerate() {
        match token.text.as_str() {
            "(" | "[" | "{" => depth += 1,
            ")" | "]" | "}" if depth > 0 => depth -= 1,
            "as" | "satisfies"
                if depth == 0
                    && index > 0
                    && index + 1 < tokens.len()
                    && !tokens[index - 1].is(".") =>
            {
                assertion = Some(index);
            }
            _ => {}
        }
    }
    assertion.map(|index| &tokens[..index])
}

/// Exposes only the final member in an expression. A computed key is known
/// only when its string spelling needs no JavaScript escape decoding; all
/// other keys remain dynamic rather than guessed.
mod arithmetic;
mod calls;
mod mutation;

use mutation::{contains_readonly_member, member_access_target, unescaped_property_name};
