// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Carry the selected identity call's literal freshness into declaration emit.
use super::*;

impl ModuleChecker<'_> {
    pub(in crate::checker::module) fn fresh_call_chain(
        &self,
        tokens: &[Token],
        scope: &BTreeMap<String, Type>,
        seen: &mut BTreeSet<String>,
        depth: usize,
    ) -> Option<bool> {
        let tokens = strip_outer_parentheses(tokens);
        let tokens = if tokens.last().is_some_and(|t| t.is("!")) {
            &tokens[..tokens.len() - 1]
        } else {
            tokens
        };
        let ResolvedCall {
            call, signatures, ..
        } = self.resolved_chain(tokens, scope)?;
        if call.explicit.is_some() {
            return Some(false);
        }
        let arguments = split_call_arguments(call.arguments)?;
        let actuals = self
            .expanded_call_argument_types_for(&arguments, scope, &signatures)
            .ok()?;
        let signature = self
            .select_function_signature(&signatures, &actuals, None)
            .ok()??;
        let Some(result_parameter) = crate::checker::inference::declared_parameter(
            &signature.return_type,
            &signature.type_parameters,
        ) else {
            return Some(false);
        };
        if result_parameter.is_const || result_parameter.constraint.is_some() {
            return Some(false);
        }
        Some(
            signature
                .parameters
                .iter()
                .zip(arguments)
                .any(|(parameter, argument)| {
                    parameter
                        .annotation
                        .as_ref()
                        .and_then(|value| {
                            crate::checker::inference::declared_parameter(
                                value,
                                &signature.type_parameters,
                            )
                        })
                        .is_some_and(|parameter| std::ptr::eq(parameter, result_parameter))
                        && self.fresh_array_element(argument, scope, &mut seen.clone(), depth + 1)
                }),
        )
    }
}
