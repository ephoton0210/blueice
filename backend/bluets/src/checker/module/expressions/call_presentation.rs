// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Explain rejected overload candidates without selecting or accepting a call.
use super::*;

impl ModuleChecker<'_> {
    pub(super) fn overload_details(
        &self,
        signatures: &[FunctionSignature],
        actuals: &[Type],
    ) -> String {
        let mut details = String::new();
        let mut ordinal = 0;
        for signature in signatures {
            if !function_signature_accepts_argument_count(signature, actuals.len()) {
                continue;
            }
            let mut inference_budget = TypeExpansionBudget::new(self.max_type_expansions);
            inference_budget.checking = self.checking;
            let substitutions =
                infer_call_substitutions(signature, actuals, &self.types, &mut inference_budget);
            let mismatch = actuals.iter().enumerate().find_map(|(index, actual)| {
                let parameter = function_parameter_for_argument(signature, index)?;
                let expected = call_parameter_expected_type(parameter, &substitutions);
                let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
                budget.checking = self.checking;
                if is_assignable(
                    actual,
                    &expected,
                    &self.types,
                    &mut HashSet::new(),
                    &mut budget,
                ) {
                    return None;
                }
                let expected = if parameter.optional || parameter.default.is_some() {
                    if let Type::Union(values) = &expected {
                        let mut values = values
                            .iter()
                            .filter(|value| !matches!(value, Type::Undefined | Type::Null))
                            .cloned()
                            .collect::<Vec<_>>();
                        if values.len() == 1 {
                            values.pop().unwrap()
                        } else {
                            Type::Union(values)
                        }
                    } else {
                        expected
                    }
                } else {
                    expected
                };
                Some(format!(
                    "Argument of type '{}' is not assignable to parameter of type '{}'.{}",
                    crate::diagnostic::type_text::argument(actual, &expected),
                    crate::diagnostic::type_text::render_in(&expected, self.project),
                    crate::diagnostic::type_text::detail(actual, &expected, self.project)
                ))
            });
            let Some(mismatch) = mismatch else { continue };
            ordinal += 1;
            let parameters = crate::diagnostic::type_text::parameter_list_in(
                &signature.parameters,
                Some(self.project),
            );
            let result =
                crate::diagnostic::type_text::render_in(&signature.return_type, self.project);
            details.push_str(&format!("\n  Overload {ordinal} of {}, '({parameters}): {result}', gave the following error.",signatures.len()));
            for line in mismatch.lines() {
                details.push_str(&format!("\n    {line}"));
            }
        }
        details
    }
}
