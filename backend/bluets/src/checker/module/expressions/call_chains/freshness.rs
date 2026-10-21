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
        let Type::Named {
            name,
            arguments: types,
        } = &signature.return_type
        else {
            return Some(false);
        };
        if !types.is_empty()
            || !signature
                .type_parameters
                .iter()
                .any(|p| p.name == *name && !p.is_const && p.constraint.is_none())
        {
            return Some(false);
        }
        Some(signature.parameters.iter().zip(arguments).any(|(parameter,argument)| {
            matches!(&parameter.annotation,Some(Type::Named {name:n,arguments}) if n == name && arguments.is_empty())
                && self.fresh_array_element(argument,scope,&mut seen.clone(),depth+1)
        }))
    }
}
