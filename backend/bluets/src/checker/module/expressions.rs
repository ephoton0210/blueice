// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Expression inference and direct-runtime checks for one module.

use super::*;

mod call_presentation;
mod diagnostics;
mod flow_checks;
mod freshness;
mod function_methods;
mod generic_inference;
mod implementation_origin;
mod indexing;
mod inference;
mod library_presentation;
mod more_types;
mod optional_property;
mod property_diagnostics;
mod readonly;
pub(super) mod signatures;

pub(super) use indexing::indexed_value_type;
use indexing::{canonical_index_key, tuple_indexed_candidates};
fn method_overload_signatures(overloads: &[Type]) -> Option<Vec<FunctionSignature>> {
    signatures::callable_signatures(&Type::Intersection(overloads.to_vec()), false)
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
        if let Some(alternatives) = self.optional_spread_scopes(&arguments, scope) {
            let mut results = alternatives.iter().map(|alternative| {
                self.infer_function_call(signatures, tokens, alternative, explicit_type_arguments)
            });
            let first = results.next().unwrap_or(Type::Unknown);
            return if results.all(|result| result == first) {
                first
            } else {
                Type::Unknown
            };
        }
        let Ok(actuals) = self.expanded_call_argument_types_for(&arguments, scope, signatures)
        else {
            return Type::Unknown;
        };
        let Ok(Some(signature)) =
            self.select_function_signature(signatures, &actuals, explicit_type_arguments)
        else {
            return Type::Unknown;
        };
        let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
        budget.checking = self.checking;
        let substitutions = function_call_substitutions(
            signature,
            &actuals,
            explicit_type_arguments,
            &self.types,
            &mut budget,
        )
        .expect("selected function signature has valid substitutions");
        substitute_type(&signature.return_type, &substitutions).runtime_result()
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
        let value_signature = self.function_value_signatures(&call.callee.text, scope, false);
        let Some(signatures) =
            value_signature.or_else(|| self.functions.get(&call.callee.text).cloned())
        else {
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
        if let Some(alternatives) = self.optional_spread_scopes(&arguments, scope) {
            let before = self.diagnostics.len();
            for alternative in &alternatives {
                self.check_function_call(tokens, alternative, span);
            }
            self.dedupe_diagnostics_since(before);
            return;
        }
        let actuals = match self.expanded_call_argument_types_for(&arguments, scope, &signatures) {
            Ok(actuals) => actuals,
            Err(()) => {
                let message = format!(
                    "a spread argument for function {} must have a fixed-length tuple type",
                    call.callee.text,
                );
                if signatures
                    .iter()
                    .any(|signature| signature.parameters.last().is_some_and(|p| p.rest))
                {
                    self.blue_only_type_error(
                        span, message, DiagnosticCode::TypeMismatch,
                        "BlueTSC's call spread subset requires a fixed tuple even when TypeScript permits an array passed to a rest parameter.",
                    );
                } else {
                    self.typescript_type_error(
                        span,
                        message,
                        DiagnosticCode::TypeMismatch,
                        2556,
                        Vec::new(),
                    );
                }
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
            self.rejected_call_error(
                span,
                format!(
                    "no overload of function {} accepts the supplied argument types",
                    call.callee.text
                ),
                &signatures,
                &actuals,
                false,
            );
            self.present_callback_return_error(&signatures, &actuals, &arguments, call.callee);
            self.present_hidden_implementation(&call.callee.text, &actuals, false);
            if self
                .diagnostics
                .last()
                .and_then(|d| d.typescript.as_ref())
                .is_some_and(|d| d.code == 2554)
            {
                self.point_last_typescript(std::slice::from_ref(call.callee));
            }
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
            self.point_last_typescript(std::slice::from_ref(call.callee));
            return;
        }
        let mut rejected_constraint = None;
        let substitutions = if let Some(explicit) = explicit {
            let Some(substitutions) = self.check_explicit_function_type_arguments(
                &signature,
                explicit,
                span,
                call.callee.start,
            ) else {
                return;
            };
            substitutions
        } else {
            let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
            budget.checking = self.checking;
            let inferred = crate::checker::inference::infer_contextual_result(
                &signature,
                &actuals,
                &self.types,
                &mut budget,
                None,
            );
            if budget.exhausted {
                self.type_error(
                    span,
                    "generic inference exceeds its expansion limit".into(),
                    DiagnosticCode::ResourceLimit,
                );
                return;
            }
            rejected_constraint = inferred.rejected_constraint;
            inferred.substitutions
        };
        for (index, actual) in actuals.iter().enumerate() {
            let parameter = function_parameter_for_argument(&signature, index)
                .expect("an accepted function call has a parameter for every argument");
            let expected = call_parameter_expected_type(parameter, &substitutions);
            if self.check_fresh_properties(
                arguments.get(index).copied().unwrap_or(&[]),
                &expected,
                scope,
                span,
            ) {
                continue;
            }

            if !self.is_assignable_bounded(actual, &expected, span) {
                let displayed_expected =
                    crate::diagnostic::type_text::default_parameter(&expected, parameter);
                let overloaded =
                    call.callee
                        .text
                        .split_once('.')
                        .is_some_and(|(receiver, member)| {
                            self.library_values.contains(receiver)
                                && crate::diagnostic::templates::member_overloads(
                                    &format!("{receiver}Constructor"),
                                    member,
                                ) > 1
                        });
                let message = rejected_constraint.as_ref().filter(|(_, candidate, _)| {
                    matches!(candidate, Type::Number | Type::String | Type::Boolean | Type::Literal(_))
                }).map_or_else(
                    || format!(
                        "argument {} has type `{}`, which is not assignable to parameter `{}` of type `{}`",
                        index + 1, type_label(actual), parameter.name, type_label(&expected)
                    ),
                    |(name, candidate, constraint)| format!(
                        "inferred type `{}` does not satisfy constraint `{}` for `{name}`",
                        type_label(candidate), type_label(constraint)
                    ),
                );
                self.call_argument_error(
                    span,
                    message,
                    actual,
                    &displayed_expected,
                    arguments.get(index).copied().unwrap_or(&[]),
                    overloaded,
                );
                self.present_hidden_implementation(&call.callee.text, &actuals, false);
                if overloaded {
                    if let Some((receiver, member)) = call.callee.text.split_once('.') {
                        self.present_library_overloads(
                            &format!("{receiver}Constructor.{member}"),
                            &actuals,
                            &substitutions,
                        );
                    }
                }
            }
        }
    }

    /// When the last call argument spreads a variable whose tuple type ends
    /// in optional elements, the scopes to check: one per possible spread
    /// length, with the variable replaced by that fixed-length prefix. The
    /// call is valid only if every length is, so no position is invented.
    /// `None` when there is no such spread, or when the variable is also used
    /// by another argument (replacing it would change that argument's type).
    pub(in crate::checker::module) fn optional_spread_scopes(
        &self,
        arguments: &[&[Token]],
        scope: &BTreeMap<String, Type>,
    ) -> Option<Vec<BTreeMap<String, Type>>> {
        let (last, others) = arguments.split_last()?;
        let [dots, name] = *last else {
            return None;
        };
        if !dots.is("...") || name.kind != TokenKind::Identifier {
            return None;
        }
        let Some(Type::Tuple(values)) = scope.get(&name.text) else {
            return None;
        };
        if values.iter().any(|value| value.rest) || !values.iter().any(|value| value.optional) {
            return None;
        }
        if others
            .iter()
            .any(|argument| argument.iter().any(|token| token.text == name.text))
        {
            return None;
        }
        let required = values.iter().filter(|value| !value.optional).count();
        Some(
            (required..=values.len())
                .map(|length| {
                    let mut alternative = scope.clone();
                    alternative.insert(
                        name.text.clone(),
                        Type::Tuple(
                            values[..length]
                                .iter()
                                .map(|value| TupleTypeElement {
                                    optional: false,
                                    ..value.clone()
                                })
                                .collect(),
                        ),
                    );
                    alternative
                })
                .collect(),
        )
    }

    /// Drops repeated diagnostics added since `before`, which the per-length
    /// re-checks of an optional spread can produce.
    pub(in crate::checker::module) fn dedupe_diagnostics_since(&mut self, before: usize) {
        let mut seen = Vec::new();
        let mut index = before;
        while index < self.diagnostics.len() {
            let diagnostic = &self.diagnostics[index];
            let key = (
                diagnostic.code,
                diagnostic.span.clone(),
                diagnostic.message.clone(),
            );
            if seen.contains(&key) {
                self.diagnostics.remove(index);
            } else {
                seen.push(key);
                index += 1;
            }
        }
    }

    /// Argument types where a bracketed literal takes its type from the
    /// parameter it is passed to. Each candidate signature is tried in order
    /// and the first parameter type that accepts the literal, read against
    /// that type, is used; otherwise the literal keeps its plain inferred
    /// type, so context can only turn a rejection into an acceptance. After a
    /// spread argument, later positions have no fixed parameter and use plain
    /// inference.
    pub(super) fn expanded_call_argument_types_for(
        &self,
        arguments: &[&[Token]],
        scope: &BTreeMap<String, Type>,
        signatures: &[FunctionSignature],
    ) -> Result<Vec<Type>, ()> {
        let mut ordered = signatures.to_vec();
        ordered.sort_by_key(|signature| {
            !signature.parameters.iter().any(|parameter| {
                matches!(parameter.annotation, Some(Type::Literal(_) | Type::Null))
            })
        });
        let signatures = ordered.as_slice();
        let mut actuals = Vec::new();
        let mut positions_known = true;
        for argument in arguments {
            if argument.first().is_some_and(|token| token.is("...")) {
                positions_known = false;
                actuals.extend(self.expanded_call_argument_types(&[argument], scope)?);
                continue;
            }
            if positions_known {
                if let [token] = strip_outer_parentheses(argument) {
                    if matches!(token.kind, TokenKind::Number | TokenKind::String)
                        || token.is("true")
                        || token.is("false")
                    {
                        let preserve = signatures.iter().any(|signature| {
                            let Some(parameter) = function_parameter_for_argument(signature, actuals.len()) else { return false };
                            let annotation = if parameter.rest { rest_parameter_element_annotation(parameter) } else { parameter.annotation.as_ref() };
                            matches!(annotation, Some(Type::Literal(_))) || (matches!(annotation, Some(Type::Named { name, arguments }) if arguments.is_empty() && signature.type_parameters.iter().any(|p| p.name == *name))
                                && matches!(&signature.return_type, Type::Named { name, arguments } if arguments.is_empty() && signature.type_parameters.iter().any(|p| p.name == *name)))
                        });
                        if preserve {
                            actuals.push(Type::Literal(token.text.clone()));
                            continue;
                        }
                    }
                }
            }
            // A number literal passed where an enum is expected stays a literal.
            if positions_known {
                let enum_literal = signatures.iter().find_map(|signature| {
                    let parameter = function_parameter_for_argument(signature, actuals.len())?;
                    let expected = call_parameter_expected_type(parameter, &BTreeMap::new());
                    self.enum_literal_for(argument, &expected)
                });
                if let Some(literal) = enum_literal {
                    actuals.push(literal);
                    continue;
                }
            }
            let contextual = (positions_known
                && argument.first().is_some_and(|token| {
                    token.is("[")
                        || token.is("{")
                        || self.module.nested_functions.contains_key(&token.start)
                }))
            .then(|| self.contextual_argument_type(argument, scope, &actuals, signatures))
            .flatten();
            match contextual {
                Some(actual) => actuals.push(actual),
                None => actuals.extend(self.expanded_call_argument_types(&[argument], scope)?),
            }
        }
        Ok(actuals)
    }

    fn contextual_argument_type(
        &self,
        argument: &[Token],
        scope: &BTreeMap<String, Type>,
        actuals: &[Type],
        signatures: &[FunctionSignature],
    ) -> Option<Type> {
        let mut rejected = None;
        for signature in signatures {
            let Some(parameter) = function_parameter_for_argument(signature, actuals.len()) else {
                continue;
            };
            let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
            budget.checking = self.checking;
            let substitutions =
                infer_call_substitutions(signature, actuals, &self.types, &mut budget);
            if actuals.iter().enumerate().any(|(index, actual)| {
                let Some(parameter) = function_parameter_for_argument(signature, index) else {
                    return true;
                };
                !is_assignable(
                    actual,
                    &call_parameter_expected_type(parameter, &substitutions),
                    &self.types,
                    &mut HashSet::new(),
                    &mut budget,
                )
            }) {
                continue;
            }
            let expected = call_parameter_expected_type(parameter, &substitutions);
            let actual = self.infer_in_context(argument, scope, &expected);
            if is_assignable(
                &actual,
                &expected,
                &self.types,
                &mut HashSet::new(),
                &mut budget,
            ) {
                return Some(actual);
            }
            if rejected.is_none() {
                rejected = Some(actual);
            }
        }
        rejected
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
                if values.iter().any(|value| value.optional || value.rest) {
                    return Err(());
                }
                actuals.extend(values.into_iter().map(|value| value.annotation));
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
        let mut candidates = signatures.iter().collect::<Vec<_>>();
        candidates.sort_by_key(|signature| {
            !signature.parameters.iter().any(|parameter| {
                matches!(parameter.annotation, Some(Type::Literal(_) | Type::Null))
            })
        });
        for signature in candidates {
            if function_signature_matches(
                signature,
                actuals,
                explicit_type_arguments,
                &self.types,
                self.max_type_expansions,
                self.checking,
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
        let actual = canonical_parameter_references(actual, &self.types);
        let expected = canonical_parameter_references(expected, &self.types);
        let (actual, expected) = (&actual, &expected);
        let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
        budget.checking = self.checking;
        let assignable = if self.strict_catch_unknown && matches!(actual, Type::Unknown) {
            accepts_strict_unknown(expected, &self.types, &mut HashSet::new(), &mut budget)
        } else {
            let direct = is_assignable(
                actual,
                expected,
                &self.types,
                &mut HashSet::new(),
                &mut budget,
            );
            // A symbolic tuple spread is assignable to a wider target through
            // its constraint; only the source side may widen, so distinct
            // parameters are never treated as equal.
            direct
                || (!budget.exhausted
                    && !self.allowed_tuple_spread_parameters.is_empty()
                    && is_assignable(
                        &substitute_type(actual, &self.allowed_tuple_spread_parameters),
                        expected,
                        &self.types,
                        &mut HashSet::new(),
                        &mut budget,
                    ))
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
        argument_span: &SourceSpan,
        actual_description: &str,
    ) {
        for (index, parameter) in signature.type_parameters.iter().enumerate() {
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
                self.point_last_type_argument(argument_span, Some(index));
            }
        }
    }

    pub(super) fn check_explicit_function_type_arguments(
        &mut self,
        signature: &FunctionSignature,
        arguments: &[Type],
        span: &SourceSpan,
        call_start: usize,
    ) -> Option<BTreeMap<String, Type>> {
        let argument_span = SourceSpan::new(&span.module, call_start, span.end);
        let required = signature
            .type_parameters
            .iter()
            .filter(|parameter| parameter.default.is_none())
            .count();
        if arguments.len() < required || arguments.len() > signature.type_parameters.len() {
            self.typescript_type_error(
                span,
                format!(
                    "function type arguments require {required} to {} argument(s), got {}",
                    signature.type_parameters.len(),
                    arguments.len()
                ),
                DiagnosticCode::TypeMismatch,
                2558,
                vec![
                    if required == signature.type_parameters.len() {
                        required.to_string()
                    } else {
                        format!("{required}-{}", signature.type_parameters.len())
                    },
                    arguments.len().to_string(),
                ],
            );
            self.point_last_type_argument(&argument_span, None);
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
            &argument_span,
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
mod enum_access;
mod mutation;

use mutation::{member_access_target, unescaped_property_name};
