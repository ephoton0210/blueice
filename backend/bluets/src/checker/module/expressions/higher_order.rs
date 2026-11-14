// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Callable result instantiation retains parameters introduced by an argument.

use super::*;

pub(in crate::checker::module) struct ResultOrigin {
    diagnostic: Option<SourceSpan>,
    callable: Option<SourceSpan>,
}

impl ResultOrigin {
    pub(super) fn direct(callable: Option<SourceSpan>) -> Self {
        // The caller later attaches its complete call span to deferred errors.
        // The argument range still identifies a newly created callable value.
        Self {
            diagnostic: None,
            callable,
        }
    }

    pub(super) fn contextual(span: Option<SourceSpan>) -> Self {
        Self {
            diagnostic: span.clone(),
            callable: span,
        }
    }
}

impl ModuleChecker<'_> {
    pub(in crate::checker::module) fn infer_function_call(
        &self,
        signatures: &[FunctionSignature],
        tokens: &[Token],
        scope: &BTreeMap<String, Type>,
        explicit_type_arguments: Option<&[Type]>,
    ) -> Type {
        let Some(arguments) = split_call_arguments(tokens) else {
            return Type::Unknown;
        };
        if let Some(alternatives) = self.optional_spread_scopes(&arguments, scope) {
            let mut results = alternatives.iter().map(|alternative| {
                self.infer_function_call(signatures, tokens, alternative, explicit_type_arguments)
            });
            let first = results.next().unwrap_or(Type::Unknown);
            return if results.all(|result| result == first) {
                first
            } else {
                Type::Unknown
            };
        }
        let Ok(actuals) = self.explicit_call_argument_types(
            &arguments,
            scope,
            signatures,
            explicit_type_arguments,
        ) else {
            return Type::Unknown;
        };
        let Ok(Some(signature)) =
            self.select_function_signature(signatures, &actuals, explicit_type_arguments)
        else {
            return Type::Unknown;
        };
        let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
        budget.checking = self.checking;
        self.instantiate_signature_result(
            signature,
            &actuals,
            explicit_type_arguments,
            None,
            &mut budget,
            ResultOrigin::direct(
                tokens
                    .first()
                    .zip(tokens.last())
                    .map(|(first, last)| SourceSpan::new(&self.module.id, first.start, last.end)),
            ),
        )
    }

    pub(in crate::checker::module) fn instantiate_signature_result(
        &self,
        signature: &FunctionSignature,
        actuals: &[Type],
        explicit: Option<&[Type]>,
        context: Option<&Type>,
        budget: &mut TypeExpansionBudget,
        origin: ResultOrigin,
    ) -> Type {
        let inferred = explicit.is_none().then(|| {
            crate::checker::inference::infer_contextual_result(
                signature,
                actuals,
                &self.types,
                budget,
                context,
            )
        });
        let explicit_substitutions;
        let substitutions = if let Some(inferred) = &inferred {
            &inferred.substitutions
        } else {
            explicit_substitutions =
                function_call_substitutions(signature, actuals, explicit, &self.types, budget)
                    .expect("selected function signature has valid substitutions");
            &explicit_substitutions
        };
        let value = if let Some(span) = &origin.diagnostic {
            self.materialize_result_at(
                &signature.return_type,
                substitutions,
                budget,
                Some(span.clone()),
            )
        } else {
            self.materialize_result(&signature.return_type, substitutions, budget)
        };
        match inferred {
            Some(inferred) => {
                let value = crate::checker::inference::bind_result(
                    value,
                    &inferred,
                    budget,
                    origin.callable.as_ref(),
                );
                if budget.exhausted {
                    if origin.diagnostic.is_some() {
                        return self.materialize_result_at(
                            &value,
                            &BTreeMap::new(),
                            budget,
                            origin.diagnostic,
                        );
                    }
                    return self.materialize_result(&value, &BTreeMap::new(), budget);
                }
                value
            }
            None => value,
        }
    }
}
