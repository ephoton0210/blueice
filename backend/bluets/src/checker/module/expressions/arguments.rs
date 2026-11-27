// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Static regions come from the parser; relational operators remain runtime.

use super::*;

impl ModuleChecker<'_> {
    pub(super) fn expression_binary_parts<'t>(
        &self,
        tokens: &'t [Token],
        operators: &[&str],
        generic: impl Fn(usize) -> bool,
    ) -> Option<(&'t [Token], &'t Token, &'t [Token])> {
        top_level_binary_parts_ignoring(tokens, operators, generic, |token| {
            self.erased_expression_token(token)
        })
    }

    pub(super) fn expression_shift_parts<'t>(
        &self,
        tokens: &'t [Token],
        generic: impl Fn(usize) -> bool,
    ) -> Option<(&'t [Token], ShiftOperator, &'t [Token])> {
        top_level_shift_parts_ignoring(tokens, generic, |token| self.erased_expression_token(token))
    }

    pub(super) fn point_argument_count_error(
        &mut self,
        callee: &Token,
        arguments: &[&[Token]],
        signature: &FunctionSignature,
        actual_count: usize,
    ) {
        let maximum = signature
            .parameters
            .iter()
            .filter(|p| p.name != "this")
            .count();
        let extra = (arguments.len() == actual_count && actual_count > maximum)
            .then(|| {
                arguments
                    .get(maximum)?
                    .first()
                    .zip(arguments.last()?.last())
            })
            .flatten();
        if let Some((first, last)) = extra {
            self.point_last_typescript(&[first.clone(), last.clone()]);
        } else {
            self.point_last_typescript(std::slice::from_ref(callee));
        }
    }

    pub(in crate::checker::module) fn call_arguments<'t>(
        &self,
        tokens: &'t [Token],
    ) -> Option<Vec<&'t [Token]>> {
        split_call_arguments_ignoring(tokens, |token| self.erased_expression_token(token))
    }

    fn erased_expression_token(&self, token: &Token) -> bool {
        self.scopes.as_ref().map_or_else(
            || {
                // Early binding inference precedes lexical scope indexing.
                self.module.edits.iter().any(|edit| {
                    edit.replacement.is_empty()
                        && edit.start <= token.start
                        && token.end <= edit.end
                })
            },
            |scopes| scopes.erased(token),
        )
    }
}
