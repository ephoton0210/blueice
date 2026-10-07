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
        for range in member_call_ranges(tokens, |start| {
            self.module.generic_call_type_arguments.contains_key(&start)
        }) {
            // Constructor validation owns `new receiver.Type(...)`.
            let call = &tokens[range];
            if constructor_call_parts(call).is_none() {
                self.check_member_call(call, scope, span);
            }
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
        let Some(call) = member_call_parts(tokens, |start| {
            self.module.generic_call_type_arguments.contains_key(&start)
        }) else {
            return;
        };
        if let Some(signatures) =
            self.module_member_signatures(call.receiver, &call.member.text, scope)
        {
            let callee = Token {
                kind: TokenKind::Identifier,
                text: format!("{}.{}", call.receiver[0].text, call.member.text),
                start: if call.generic {
                    call.member.start
                } else {
                    call.receiver[0].start
                },
                end: call.member.end,
            };
            let mut qualified = vec![
                callee,
                Token {
                    kind: TokenKind::Punct,
                    text: "(".to_string(),
                    start: call.member.end,
                    end: call.member.end,
                },
            ];
            if call.generic {
                let member = tokens
                    .iter()
                    .position(|token| token.start == call.member.start)
                    .unwrap();
                let close = explicit_generic_call_close(tokens, member).unwrap();
                qualified.splice(1..2, tokens[member + 1..=close + 1].iter().cloned());
            }
            qualified.extend_from_slice(call.arguments);
            // The qualified binding retains generic parameters and every overload.
            debug_assert!(!signatures.is_empty());
            self.check_function_call(&qualified, scope, span);
            return;
        }
        if self.check_function_method(
            call.receiver,
            &call.member.text,
            call.arguments,
            scope,
            span,
        ) {
            return;
        }

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
        let member_type = match member_type {
            PropertyType::Found { value, .. } => value,
            PropertyType::Missing if self.hidden_member(&base, &call.member.text).is_some() => {
                // Diagnosed, with its accessibility, by the restricted-member
                // scan over the whole expression.
                return;
            }
            PropertyType::Missing => {
                let call_span = SourceSpan::new(
                    &span.module,
                    tokens.first().expect("member call has a receiver").start,
                    tokens.last().expect("member call has a closing token").end,
                );
                let diagnostic_span = if let [receiver] = call.receiver {
                    if self.is_bound_class_constructor_value(&receiver.text, scope)
                        || self.is_bound_class_instance_type(&base)
                        || self.is_bound_class_static_this(receiver, scope)
                    {
                        &call_span
                    } else {
                        span
                    }
                } else {
                    span
                };
                self.missing_property_error(
                    diagnostic_span,
                    &base,
                    &call.member.text,
                    call.receiver
                        .first()
                        .filter(|_| call.receiver.len() == 1)
                        .map(|token| token.text.as_str()),
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
        if let Some(alternatives) = self.optional_spread_scopes(&arguments, scope) {
            let before = self.diagnostics.len();
            for alternative in &alternatives {
                self.check_member_call(tokens, alternative, span);
            }
            self.dedupe_diagnostics_since(before);
            return;
        }
        let context_signatures = match &member_type {
            Type::Function { parameters, result } => vec![FunctionSignature {
                parameters: parameters.clone(),
                type_parameters: Vec::new(),
                return_type: (**result).clone(),
            }],
            Type::Intersection(overloads) => {
                method_overload_signatures(overloads).unwrap_or_default()
            }
            _ => Vec::new(),
        };
        let Ok(actuals) =
            self.expanded_call_argument_types_for(&arguments, scope, &context_signatures)
        else {
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
        let call_span = SourceSpan::new(
            span.module.clone(),
            tokens.first().expect("member call has a receiver").start,
            tokens.last().expect("member call has a closing token").end,
        );
        let bound_class_member = self.is_bound_class_instance_type(&base)
            || matches!(call.receiver, [receiver] if self.is_bound_class_constructor_value(&receiver.text, scope) || self.is_bound_class_static_this(receiver, scope));
        let (parameters, selected_overload) = match member_type {
            Type::Function { parameters, .. } => (parameters, false),
            Type::Intersection(overloads) => {
                let Some(signatures) = method_overload_signatures(&overloads) else {
                    return;
                };
                match self.select_function_signature(&signatures, &actuals, None) {
                    Ok(Some(_)) => {}
                    Ok(None) => {
                        let kind = if bound_class_member {
                            "instance method"
                        } else {
                            "method"
                        };
                        self.rejected_call_error(
                            &call_span,
                            format!(
                                "no overload of {kind} {} matches the supplied argument types",
                                call.member.text
                            ),
                            &signatures,
                            &actuals,
                            true,
                        );
                        if let Some(argument) = arguments.first() {
                            self.point_last_typescript(argument);
                        }
                        self.present_callback_return_error(
                            &signatures,
                            &actuals,
                            &arguments,
                            call.member,
                        );
                    }
                    Err(()) => self.type_error(
                        &call_span,
                        format!(
                            "method overload selection exceeds the {} generic-expansion limit",
                            self.max_type_expansions
                        ),
                        DiagnosticCode::ResourceLimit,
                    ),
                }
                return;
            }
            _ => {
                self.type_error(
                    span,
                    format!("property `{}` is not callable", call.member.text),
                    DiagnosticCode::TypeMismatch,
                );
                return;
            }
        };
        let argument_span = if selected_overload || bound_class_member {
            &call_span
        } else {
            span
        };
        let required = parameters
            .iter()
            .filter(|parameter| !parameter.optional)
            .count();
        if actuals.len() < required || actuals.len() > parameters.len() {
            self.type_error(
                argument_span,
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
            if selected_overload && index == 0 {
                // The selector already compared the literal string values,
                // including equivalent single and double quote spellings.
                continue;
            }
            let expected = parameters[index]
                .annotation
                .as_ref()
                .expect("method signature parameters have annotations");
            if !self.is_assignable_bounded(actual, expected, argument_span) {
                let displayed_expected =
                    crate::diagnostic::type_text::default_parameter(expected, &parameters[index]);
                let library_receiver = match &base {
                    Type::Array(_) => Some("Array".to_string()),
                    Type::String => Some("String".to_string()),
                    Type::Named{name,..} if matches!(name.as_str(),"Promise"|"Generator"|"Iterator"|"IterableIterator")=>Some(name.clone()),
                    _ => matches!(call.receiver, [receiver] if self.library_values.contains(&receiver.text))
                        .then(|| format!("{}Constructor", call.receiver[0].text)),
                };
                let overloaded = library_receiver.as_ref().is_some_and(|receiver| {
                    crate::diagnostic::templates::member_overloads(receiver, &call.member.text) > 1
                });
                self.call_argument_error(
                    argument_span,
                    format!(
                        "argument {} has type `{}`, which is not assignable to method parameter `{}` of type `{}`",
                        index + 1,
                        type_label(actual),
                        parameters[index].name,
                        type_label(expected)
                    ),
                    actual,
                    &displayed_expected,
                    arguments.get(index).copied().unwrap_or(&[]),
                    overloaded,
                );
                if overloaded {
                    let mut substitutions = BTreeMap::new();
                    if let Type::Array(element) = &base {
                        substitutions.insert("T".into(), *element.clone());
                        substitutions.insert("S".into(), *element.clone());
                    }
                    self.present_library_overloads(
                        &format!("{}.{}", library_receiver.unwrap(), call.member.text),
                        &actuals.iter().zip(&arguments).map(|(actual,argument)|if matches!(*argument,[token] if token.is("true")||token.is("false")){Type::Literal(argument[0].text.clone())}else{actual.clone()}).collect::<Vec<_>>(),
                        &substitutions,
                    );
                } else if let Some(receiver) = library_receiver {
                    self.present_library_argument(
                        &format!("{receiver}.{}", call.member.text),
                        actual,
                        expected,
                        arguments.get(index).copied().unwrap_or(&[]),
                        &base,
                        index,
                    );
                }
                if overloaded && index + 1 == parameters.len() && matches!(expected, Type::Union(_))
                {
                    self.point_last_typescript(std::slice::from_ref(call.member));
                }
            }
        }
    }
}
