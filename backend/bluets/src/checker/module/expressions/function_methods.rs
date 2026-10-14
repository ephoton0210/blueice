// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Typed call/apply/bind adapters for bound function values.

use super::*;

impl ModuleChecker<'_> {
    fn method_signature(
        &self,
        receiver: &[Token],
        member: &str,
        scope: &BTreeMap<String, Type>,
    ) -> Option<FunctionSignature> {
        if !self.explicit_checking || !matches!(member, "call" | "apply" | "bind") {
            return None;
        }
        if let [name] = receiver {
            if let Some(signature) = self.function_value_signature(&name.text, scope) {
                return Some(signature);
            }
            if self.values.get(&name.text) == scope.get(&name.text) {
                if let Some(signatures) = self.functions.get(&name.text) {
                    return signatures.last().cloned();
                }
            }
        }
        match self.infer_expression(receiver, scope) {
            Type::Function { parameters, result } => Some(FunctionSignature {
                parameters,
                type_parameters: Vec::new(),
                return_type: *result,
            }),
            _ => None,
        }
    }

    pub(super) fn function_method_result(
        &self,
        receiver: &[Token],
        member: &str,
        arguments: &[Token],
        scope: &BTreeMap<String, Type>,
    ) -> Option<Type> {
        let signature = self.method_signature(receiver, member, scope)?;
        if !self.checking.strict_bind_call_apply {
            return Some(Type::Any);
        }
        if member == "bind" {
            let count = split_call_arguments(arguments)?.len().saturating_sub(1);
            return Some(Type::Function {
                parameters: signature
                    .parameters
                    .into_iter()
                    .filter(|p| p.name != "this")
                    .skip(count)
                    .collect(),
                result: Box::new(signature.return_type),
            });
        }
        Some(signature.return_type)
    }

    pub(super) fn check_function_method(
        &mut self,
        receiver: &[Token],
        member_token: &Token,
        arguments: &[Token],
        scope: &BTreeMap<String, Type>,
        span: &SourceSpan,
    ) -> bool {
        let member = member_token.text.as_str();
        let Some(signature) = self.method_signature(receiver, member, scope) else {
            return false;
        };
        let Some(arguments) = split_call_arguments(arguments) else {
            return true;
        };
        if !self.checking.strict_bind_call_apply {
            return true;
        }
        let Some((this_arg, arguments)) = arguments.split_first() else {
            self.typescript_type_error(
                span,
                format!("{member} requires a this argument"),
                DiagnosticCode::TypeMismatch,
                2555,
                vec!["1".into(), "0".into()],
            );
            self.point_last_typescript(std::slice::from_ref(member_token));
            return true;
        };
        if let Some(parameter) = signature.parameters.iter().find(|p| p.name == "this") {
            let actual = self.infer_expression(this_arg, scope);
            if parameter
                .annotation
                .as_ref()
                .is_some_and(|expected| !self.is_assignable_bounded(&actual, expected, span))
            {
                self.call_argument_error(
                    span,
                    "this argument has an incompatible type".to_string(),
                    &actual,
                    parameter.annotation.as_ref().unwrap(),
                    this_arg,
                    false,
                );
                let detail = self
                    .receiver_compatibility_detail(&actual, parameter.annotation.as_ref().unwrap());
                if let Some(counterpart) = self
                    .diagnostics
                    .last_mut()
                    .and_then(|d| d.typescript.as_mut())
                {
                    if counterpart.code == 2345 && !counterpart.message.contains('\n') {
                        counterpart.message.push_str(&detail);
                    }
                }
            }
        }
        let parameters = signature
            .parameters
            .iter()
            .filter(|p| p.name != "this")
            .collect::<Vec<_>>();
        let actuals = if member == "apply" {
            if arguments.len() != 1 {
                self.typescript_type_error(
                    span,
                    "apply requires one tuple of arguments".to_string(),
                    DiagnosticCode::TypeMismatch,
                    if arguments.is_empty() { 2684 } else { 2554 },
                    Vec::new(),
                );
                return true;
            }
            let actual = self.infer_expression(arguments[0], scope);
            match actual {
                Type::Tuple(elements) => elements.into_iter().map(|e| e.annotation).collect(),
                Type::Array(element) if parameters.iter().all(|p| p.rest) => vec![*element],
                Type::Any => return true,
                _ => {
                    let tokens = strip_outer_parentheses(arguments[0]);
                    if tokens.first().is_some_and(|t| t.is("["))
                        && tokens.last().is_some_and(|t| t.is("]"))
                    {
                        let mut items = tokens[1..].to_vec();
                        items.last_mut().expect("array has closing bracket").text = ")".to_string();
                        split_call_arguments(&items)
                            .unwrap_or_default()
                            .iter()
                            .map(|a| self.infer_expression(a, scope))
                            .collect()
                    } else {
                        self.type_error(
                            span,
                            "apply arguments must be a fixed-length tuple".to_string(),
                            DiagnosticCode::TypeMismatch,
                        );
                        return true;
                    }
                }
            }
        } else {
            arguments
                .iter()
                .map(|a| self.infer_expression(a, scope))
                .collect::<Vec<_>>()
        };
        let required = parameters.iter().filter(|p| !p.optional && !p.rest).count();
        if (member != "bind" && actuals.len() < required)
            || (actuals.len() > parameters.len() && !parameters.last().is_some_and(|p| p.rest))
        {
            self.typescript_type_error(
                span,
                format!("{member} argument count does not match the function"),
                DiagnosticCode::TypeMismatch,
                if member == "apply" { 2345 } else { 2554 },
                vec![
                    (parameters.len() + 1).to_string(),
                    (actuals.len() + 1).to_string(),
                ],
            );
        }
        for (index, actual) in actuals.iter().enumerate() {
            if let Some(parameter) = parameters
                .get(index)
                .or_else(|| parameters.last().filter(|p| p.rest))
            {
                if let Some(expected) = &parameter.annotation {
                    let expected = if parameter.rest {
                        match expected {
                            Type::Array(element) => &**element,
                            _ => expected,
                        }
                    } else {
                        expected
                    };
                    if !self.is_assignable_bounded(actual, expected, span) {
                        self.typescript_type_error(
                            span,
                            format!("{member} argument {} has an incompatible type", index + 1),
                            DiagnosticCode::TypeMismatch,
                            if member == "apply" { 2322 } else { 2345 },
                            vec![type_label(actual), type_label(expected)],
                        );
                        if member != "apply" {
                            if let Some(argument) = arguments.get(index) {
                                self.point_last_typescript(argument);
                            }
                        }
                    }
                }
            }
        }
        true
    }
}
