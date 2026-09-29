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
                && !tokens[start].is("this")
                && !tokens[start].is("super")
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
        let member_type = match member_type {
            PropertyType::Found { value, .. } => value,
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
                self.type_error(
                    diagnostic_span,
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
            Type::Intersection(overloads) if matches!(call.receiver, [receiver] if self.is_bound_class_constructor_value(&receiver.text, scope) || self.is_bound_class_static_this(receiver, scope)) =>
            {
                let Some(signatures) = method_overload_signatures(&overloads) else {
                    return;
                };
                match self.select_function_signature(&signatures, &actuals, None) {
                    Ok(Some(_)) => {}
                    Ok(None) => self.type_error(
                        &call_span,
                        format!(
                            "no overload of static method {} matches the supplied argument types",
                            call.member.text
                        ),
                        DiagnosticCode::TypeMismatch,
                    ),
                    Err(()) => self.type_error(
                        &call_span,
                        format!(
                            "static method overload selection exceeds the {} generic-expansion limit",
                            self.max_type_expansions
                        ),
                        DiagnosticCode::ResourceLimit,
                    ),
                }
                return;
            }
            Type::Intersection(overloads) if self.is_bound_class_instance_type(&base) => {
                let Some(signatures) = method_overload_signatures(&overloads) else {
                    return;
                };
                match self.select_function_signature(&signatures, &actuals, None) {
                    Ok(Some(_)) => {}
                    Ok(None) => self.type_error(
                        &call_span,
                        format!(
                            "no overload of instance method {} matches the supplied argument types",
                            call.member.text
                        ),
                        DiagnosticCode::TypeMismatch,
                    ),
                    Err(()) => self.type_error(
                        &call_span,
                        format!(
                            "instance method overload selection exceeds the {} generic-expansion limit",
                            self.max_type_expansions
                        ),
                        DiagnosticCode::ResourceLimit,
                    ),
                }
                return;
            }
            Type::Intersection(overloads) => {
                if !supports_callback_method_receiver(&base, &self.types, self.max_type_expansions)
                {
                    self.type_error(
                        &call_span,
                        format!("unsupported overload set for method {}", call.member.text),
                        DiagnosticCode::TypeMismatch,
                    );
                    return;
                }
                match select_callback_method_overload(&overloads, &actuals) {
                    Ok(Type::Function { parameters, .. }) => {
                        let named_callback = matches!(arguments.get(1), Some([callback]) if callback.kind == TokenKind::Identifier);
                        if matches!(actuals.get(1), Some(Type::Any | Type::Unknown))
                            || (matches!(actuals.get(1), Some(Type::Function { .. }))
                                && !named_callback)
                        {
                            self.type_error(
                                &call_span,
                                format!(
                                    "method {} requires a named function callback",
                                    call.member.text
                                ),
                                DiagnosticCode::TypeMismatch,
                            );
                            return;
                        }
                        (parameters.clone(), true)
                    }
                    Ok(_) => unreachable!("the selected overload is a function"),
                    Err(MethodOverloadError::Ambiguous) => {
                        self.type_error(
                            &call_span,
                            format!(
                                "ambiguous overload of method {} for first argument type `{}`",
                                call.member.text,
                                type_label(&actuals[0])
                            ),
                            DiagnosticCode::TypeMismatch,
                        );
                        return;
                    }
                    Err(MethodOverloadError::NoMatch) => {
                        self.type_error(
                            &call_span,
                            format!(
                                "no overload of method {} matches the supplied argument types",
                                call.member.text
                            ),
                            DiagnosticCode::TypeMismatch,
                        );
                        return;
                    }
                    Err(MethodOverloadError::Unsupported) => {
                        self.type_error(
                            &call_span,
                            format!("unsupported overload set for method {}", call.member.text),
                            DiagnosticCode::TypeMismatch,
                        );
                        return;
                    }
                }
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
                self.type_error(
                    argument_span,
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
