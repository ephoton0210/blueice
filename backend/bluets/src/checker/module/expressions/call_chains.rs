// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Optional and asserted calls reuse the ordinary overload and receiver checker.
use super::*;
use crate::checker::type_operators::union;
mod callee;
mod freshness;
mod syntax;

struct ResolvedCall<'a> {
    call: syntax::Call<'a>,
    callee: callee::Callee,
    signatures: Vec<FunctionSignature>,
    unsafe_call: bool,
}

impl ModuleChecker<'_> {
    fn resolved_chain<'a>(
        &self,
        tokens: &'a [Token],
        scope: &BTreeMap<String, Type>,
    ) -> Option<ResolvedCall<'a>> {
        let call = syntax::call(tokens)?;
        if !call.optional
            && !call.callee.iter().any(|t| t.is("?.") || t.is("!"))
            && !call.callee.first().is_some_and(|t| t.is("("))
            && call.callee.iter().any(|t| t.is("("))
        {
            return None;
        }
        let mut callee = self.chain_callee(call.callee, scope, 0)?;
        let (non_nullable, nullable) = self.chain_non_nullable(&callee.value)?;
        let special = call.optional
            || call.callee.iter().any(|t| t.is("?.") || t.is("!"))
            || call.callee.first().is_some_and(|t| t.is("("))
            || nullable;
        if !special {
            return None;
        }
        let mut local = scope.clone();
        local.insert("#chain".into(), non_nullable);
        let signatures = self.function_value_signatures("#chain", &local, false)?;
        let unsafe_call = nullable && !call.optional && self.checking.strict_null_checks;
        if call.optional {
            callee.short_circuit |= nullable;
        }
        callee.short_circuit &= self.checking.strict_null_checks;
        Some(ResolvedCall {
            call,
            callee,
            signatures,
            unsafe_call,
        })
    }

    pub(in crate::checker::module) fn infer_call_chain(
        &self,
        tokens: &[Token],
        scope: &BTreeMap<String, Type>,
    ) -> Option<Type> {
        let ResolvedCall {
            call,
            callee,
            signatures,
            ..
        } = self.resolved_chain(tokens, scope)?;
        let explicit = call
            .explicit
            .and_then(|key| self.module.generic_call_type_arguments.get(&key))
            .map(Vec::as_slice);
        let result = self.infer_function_call(&signatures, call.arguments, scope, explicit);
        Some(if callee.short_circuit {
            union([result, Type::Undefined])
        } else {
            result
        })
    }

    pub(in crate::checker::module) fn check_call_chain(
        &mut self,
        tokens: &[Token],
        scope: &BTreeMap<String, Type>,
        span: &SourceSpan,
    ) -> bool {
        let tokens = strip_outer_parentheses(tokens);
        let tokens = erased_assertion_operand(tokens)
            .map(strip_outer_parentheses)
            .unwrap_or(tokens);
        let tokens = if tokens.last().is_some_and(|t| t.is("!")) {
            &tokens[..tokens.len() - 1]
        } else {
            tokens
        };
        let Some(ResolvedCall {
            call,
            callee,
            signatures,
            unsafe_call,
        }) = self.resolved_chain(tokens, scope)
        else {
            return false;
        };
        let first = &call.callee[0];
        let last = call.callee.last().unwrap();
        let point = SourceSpan::new(&self.module.id, first.start, last.end);
        if unsafe_call {
            let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
            budget.checking = self.checking;
            let expanded = crate::checker::type_operators::expanded(
                &callee.value,
                &self.types,
                &mut HashSet::new(),
                &mut budget,
            );
            let parts = if let Type::Union(parts) = &expanded {
                parts.as_slice()
            } else {
                std::slice::from_ref(&expanded)
            };
            let null = parts.contains(&Type::Null);
            let undefined = parts.contains(&Type::Undefined);
            let (invoke, flow) = if null && undefined {
                (2723, 18049)
            } else if null {
                (2721, 18047)
            } else {
                (2722, 18048)
            };
            self.typescript_type_error(
                &point,
                "nullable callable requires an optional call or an assertion".into(),
                DiagnosticCode::TypeMismatch,
                invoke,
                Vec::new(),
            );
            if call.callee.len() == 1 {
                self.typescript_type_error(
                    &point,
                    "nullable callable is used without a guard".into(),
                    DiagnosticCode::TypeMismatch,
                    flow,
                    vec![first.text.clone()],
                );
            }
            return true;
        }
        let value = Type::Intersection(
            signatures
                .into_iter()
                .map(|s| {
                    let result = if callee.short_circuit {
                        union([s.return_type, Type::Undefined])
                    } else {
                        s.return_type
                    };
                    if s.type_parameters.is_empty() {
                        Type::Function {
                            parameters: s.parameters,
                            result: Box::new(result),
                        }
                    } else {
                        Type::GenericFunction {
                            parameters: s.parameters,
                            type_parameters: s.type_parameters,
                            result: Box::new(result),
                            span: point.clone(),
                        }
                    }
                })
                .collect(),
        );
        let mut local = scope.clone();
        local.insert("#chain".into(), value);
        let receiver = callee.receiver.map(|value| {
            local.insert("#chain-receiver".into(), value);
            vec![Token {
                kind: TokenKind::Identifier,
                text: "#chain-receiver".into(),
                start: first.start,
                end: last.end,
            }]
        });
        let mut qualified = vec![Token {
            kind: TokenKind::Identifier,
            text: "#chain".into(),
            start: first.start,
            end: last.end,
        }];
        if let Some(key) = call.explicit {
            if !self.module.generic_call_type_arguments.contains_key(&key) {
                return false;
            }
            // The original source key is retained for explicit type arguments.
            qualified[0].start = key;
            let Some(open) = tokens.iter().position(|t| t.is("<")) else {
                return false;
            };
            let Some(close) = matching_call_angle_bracket(tokens, open) else {
                return false;
            };
            qualified.extend_from_slice(&tokens[open..=close]);
        }
        qualified.push(Token {
            kind: TokenKind::Punct,
            text: "(".into(),
            start: last.end,
            end: last.end,
        });
        qualified.extend_from_slice(call.arguments);
        let before = self.diagnostics.len();
        self.check_function_call_with_receiver(&qualified, &local, span, receiver.as_deref());
        let original = &self.module.source[first.start..last.end];
        for diagnostic in &mut self.diagnostics[before..] {
            diagnostic.message = diagnostic.message.replace("#chain", original);
        }
        true
    }
}
