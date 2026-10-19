// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Resolve array aliases before checking rest arguments, without losing binders.

use super::*;

impl ModuleChecker<'_> {
    pub(in crate::checker::module) fn canonical_rest_signature(
        &self,
        mut signature: FunctionSignature,
    ) -> FunctionSignature {
        for parameter in &mut signature.parameters {
            if !parameter.rest {
                continue;
            }
            let Some(mut annotation) = parameter.annotation.clone() else {
                continue;
            };
            if matches!(&annotation, Type::Named { name, .. } if signature.type_parameters.iter().any(|parameter| &parameter.name == name))
            {
                continue;
            }
            let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
            let mut visited = HashSet::new();
            while let Some(expanded) = instantiate_named(
                &annotation,
                &self.types,
                &mut visited,
                &mut budget,
                "rest parameter",
            ) {
                annotation = expanded;
            }
            if matches!(annotation, Type::Array(_)) && !budget.exhausted {
                parameter.annotation = Some(annotation);
            }
        }
        signature
    }

    pub(super) fn canonicalize_rest_signatures(&mut self) {
        let functions = std::mem::take(&mut self.functions);
        self.functions = functions
            .into_iter()
            .map(|(name, signatures)| {
                (
                    name,
                    signatures
                        .into_iter()
                        .map(|signature| self.canonical_rest_signature(signature))
                        .collect(),
                )
            })
            .collect();
    }
}
