// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A predicate must prove both implications for the same unmodified parameter.

use super::*;
use crate::parser::TypePredicate;

impl ScopeModel<'_> {
    pub(in crate::checker) fn inferred_predicate(
        &self,
        parameters: &[Parameter],
        span: &SourceSpan,
    ) -> Option<Type> {
        let graph = graph::execution(self, self.flow_execution(span.start))?;
        if graph.returns.len() != 1 || !graph.normal_exits.is_empty() {
            return None;
        }
        let tokens = &graph.nodes[graph.returns[0]].tokens;
        let tokens = if tokens.last().is_some_and(|token| token.is(";")) {
            &tokens[..tokens.len() - 1]
        } else {
            tokens
        };
        for parameter in parameters {
            let Some(declared) = parameter.annotation.as_ref() else {
                continue;
            };
            if parameter.pattern.is_some()
                || parameter.rest
                || matches!(declared, Type::Any | Type::Boolean)
            {
                continue;
            }
            let id = self.flow_binding(&parameter.name, parameter.span.start)?;
            if self.mutations.iter().any(|mutation| {
                span.start <= mutation.operator.start
                    && mutation.operator.start < span.end
                    && self
                        .flow_binding(&parameter.name, mutation.operator.start)
                        .as_ref()
                        == Some(&id)
                    && mutation
                        .target
                        .first()
                        .is_some_and(|token| token.text == parameter.name)
            }) {
                continue;
            }
            let Some(positive) = predicates::narrow(self, &State::new(), tokens, true) else {
                continue;
            };
            let narrowed = current(self, &positive, &id);
            if narrowed == *declared || narrowed == Type::Never {
                continue;
            }
            let Some(negative) = predicates::narrow(self, &positive, tokens, false) else {
                continue;
            };
            if current(self, &negative, &id) != Type::Never {
                continue;
            }
            return Some(Type::Predicate(Box::new(TypePredicate {
                parameter: parameter.name.clone(),
                asserts: false,
                target: Some(Box::new(narrowed)),
                span: span.clone(),
                parameter_span: parameter.span.clone(),
                is_span: None,
                target_span: None,
                return_position: true,
            })));
        }
        None
    }
}
