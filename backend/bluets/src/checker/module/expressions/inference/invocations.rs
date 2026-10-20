// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Immediate nested-function calls share inferred signatures with delayed ones.

use super::*;

impl ModuleChecker<'_> {
    pub(super) fn infer_immediate_call(
        &self,
        tokens: &[Token],
        scope: &BTreeMap<String, Type>,
    ) -> Option<Type> {
        if !tokens.first()?.is("(") {
            return None;
        }
        let close = scopes::targets::close(tokens, 0)?;
        if !tokens.get(close + 1)?.is("(")
            || scopes::targets::close(tokens, close + 1)? != tokens.len() - 1
        {
            return None;
        }
        let function =
            self.nested_function_type(strip_outer_parentheses(&tokens[1..close]), scope)?;
        let signatures = signatures::callable_signatures(&function, false)?;
        // Like direct/member calls, argument tokens include the final `)`.
        Some(self.infer_function_call(&signatures, &tokens[close + 2..], scope, None))
    }
}
