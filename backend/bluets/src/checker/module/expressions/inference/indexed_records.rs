// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Index-signature contexts guide the actual named properties of a literal.

use super::*;

impl ModuleChecker<'_> {
    pub(super) fn infer_indexed_record_in_context(
        &self,
        tokens: &[Token],
        scope: &BTreeMap<String, Type>,
        expected: &Type,
    ) -> Option<Type> {
        let Type::Record(mut fields) = self.infer_record(tokens, scope) else {
            return None;
        };
        let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
        budget.checking = self.checking;
        let expected = crate::checker::type_operators::expanded(
            expected,
            &self.types,
            &mut HashSet::new(),
            &mut budget,
        );
        if budget.exhausted {
            return None;
        }
        for field in &mut fields {
            match property_type(
                &expected,
                &field.name,
                &self.types,
                &mut HashSet::new(),
                &mut budget,
            ) {
                PropertyType::Found { value, .. } => field.value = value,
                PropertyType::Exhausted => return None,
                _ => {}
            }
        }
        let result = self.infer_record_in_context(tokens, scope, Some(&fields));
        Some(result)
    }
}
