// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Predicate inference reuses lexical identities and the bounded flow algebra.

use super::*;

impl ModuleChecker<'_> {
    pub(super) fn infer_result_predicate(
        &self,
        parameters: &[Parameter],
        span: &SourceSpan,
    ) -> Option<Type> {
        let owned;
        let scopes = match &self.scopes {
            Some(scopes) => scopes,
            None => {
                owned = scopes::ScopeModel::new(
                    self.project,
                    self.module,
                    &self.values,
                    &self.types,
                    self.ambient,
                    self.library_target,
                    self.max_type_expansions,
                );
                &owned
            }
        };
        scopes.inferred_predicate(parameters, span)
    }
}
