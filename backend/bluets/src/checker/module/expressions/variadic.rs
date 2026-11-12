// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Forwarding a rigid argument pack requires the same lexical parameter.
use super::*;

impl ModuleChecker<'_> {
    pub(super) fn check_resolved_rest_pack(
        &mut self,
        signature: &FunctionSignature,
        substitutions: &BTreeMap<String, Type>,
        actuals: &[Type],
        sources: (&[&[Token]], &[Token]),
        span: &SourceSpan,
    ) -> Option<usize> {
        let (arguments, argument_tokens) = sources;
        let parameters = signature
            .parameters
            .iter()
            .filter(|parameter| parameter.name != "this")
            .collect::<Vec<_>>();
        let index = parameters.iter().position(|parameter| parameter.rest)?;
        if actuals.get(index).is_some_and(|actual| {
            self.correlated_argument_pack(actual, index, std::slice::from_ref(signature))
        }) {
            return None;
        }
        let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
        budget.checking = self.checking;
        let whole = parameter_expected_type(parameters[index], substitutions);
        let expected = crate::checker::type_operators::expanded(
            &whole,
            &self.types,
            &mut HashSet::new(),
            &mut budget,
        );
        if budget.exhausted {
            self.type_error(
                span,
                "rest parameter expansion exceeds its limit".into(),
                DiagnosticCode::ResourceLimit,
            );
            return Some(index);
        }
        if !matches!(expected, Type::Tuple(_)) {
            return None;
        }
        let mut values = actuals.get(index..).unwrap_or_default().to_vec();
        for (value, argument) in values.iter_mut().zip(arguments.iter().skip(index)) {
            if let [token] = strip_outer_parentheses(argument) {
                if matches!(token.kind, TokenKind::Number | TokenKind::String)
                    || token.is("true")
                    || token.is("false")
                {
                    *value = Type::Literal(token.text.clone());
                }
            }
        }
        let actual = Type::Tuple(values.into_iter().map(TupleTypeElement::required).collect());
        if !self.is_assignable_bounded(&actual, &expected, span) {
            let tokens = arguments
                .get(index)
                .and_then(|argument| argument.first())
                .map_or(argument_tokens, |first| {
                    let start = argument_tokens
                        .iter()
                        .position(|token| token.start == first.start)
                        .unwrap_or(0);
                    let end = arguments
                        .last()
                        .and_then(|argument| argument.last())
                        .and_then(|last| {
                            argument_tokens
                                .iter()
                                .position(|token| token.end == last.end)
                        })
                        .map_or(argument_tokens.len(), |index| index + 1);
                    &argument_tokens[start..end]
                });
            self.call_argument_error(
                span,
                "rest arguments do not satisfy the instantiated tuple".into(),
                &actual,
                &expected,
                tokens,
                false,
            );
        }
        Some(index)
    }

    pub(super) fn correlated_argument_pack(
        &self,
        actual: &Type,
        index: usize,
        signatures: &[FunctionSignature],
    ) -> bool {
        let actual = canonical_parameter_references(actual, &self.types);
        let Type::Named { name, arguments } = &actual else {
            return false;
        };
        if !arguments.is_empty()
            || !self.types.get(name).is_some_and(|definition| {
                definition.kind == TypeDefinitionKind::Parameter
                    && matches!(definition.value, Type::Tuple(_) | Type::Array(_))
            })
        {
            return false;
        }
        !signatures.is_empty()
            && signatures.iter().all(|signature| {
                let parameters = signature
                    .parameters
                    .iter()
                    .filter(|p| p.name != "this")
                    .collect::<Vec<_>>();
                signature.type_parameters.is_empty()
                    && parameters.len() == index + 1
                    && parameters.last().is_some_and(|parameter| {
                        parameter.rest
                            && parameter.annotation.as_ref().is_some_and(|expected| {
                                canonical_parameter_references(expected, &self.types) == actual
                            })
                    })
            })
    }
}
