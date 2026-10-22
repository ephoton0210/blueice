// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A return context guides expression inference; it never replaces its result.
use super::*;

impl ModuleChecker<'_> {
    pub(in crate::checker::module) fn contextual_return(&self, start: usize, expected: &Type) {
        self.return_inference
            .contexts
            .borrow_mut()
            .insert(start, expected.clone());
        self.return_inference.results.borrow_mut().remove(&start);
    }

    pub(super) fn infer_return_expression(
        &self,
        tokens: &[Token],
        scope: &BTreeMap<String, Type>,
    ) -> Type {
        let context = self
            .return_inference
            .active
            .borrow()
            .last()
            .and_then(|frame| {
                self.return_inference
                    .contexts
                    .borrow()
                    .get(&frame.start)
                    .cloned()
            });
        match context {
            Some(expected) => self.infer_in_context(tokens, scope, &expected),
            None => crate::parser::widen_literal_tokens(tokens, true)
                .unwrap_or_else(|| self.infer_expression(tokens, scope)),
        }
    }
}
