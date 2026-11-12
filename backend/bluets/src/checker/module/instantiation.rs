// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Transport semantic instantiation failures without inventing a recovery type.

use super::*;
use std::cell::RefCell;

#[derive(Default)]
pub(super) struct Failures {
    current_expression: RefCell<Option<SourceSpan>>,
    pending: RefCell<Vec<Failure>>,
    diagnosed: RefCell<BTreeSet<String>>,
}

struct Failure {
    requested: Type,
    repeated: Type,
    span: SourceSpan,
}

impl Failures {
    pub(super) fn contains(&self, value: &Type) -> bool {
        if self.diagnosed.borrow().is_empty() && self.pending.borrow().is_empty() {
            return false;
        }
        let identity = type_relations::type_identity(value);
        self.diagnosed.borrow().contains(&identity)
            || self.pending.borrow().iter().any(|failure| {
                type_relations::type_identity(&failure.requested) == identity
                    || type_relations::type_identity(&failure.repeated) == identity
            })
    }
}

impl ModuleChecker<'_> {
    pub(in crate::checker::module) fn infer_expression(
        &self,
        tokens: &[Token],
        scope: &BTreeMap<String, Type>,
    ) -> Type {
        let span = tokens
            .first()
            .zip(tokens.last())
            .map(|(first, last)| SourceSpan::new(&self.module.id, first.start, last.end));
        let previous = self.instantiation.current_expression.replace(span);
        let value = self.infer_expression_inner(tokens, scope);
        self.instantiation.current_expression.replace(previous);
        value
    }

    pub(in crate::checker::module) fn materialize_result(
        &self,
        value: &Type,
        substitutions: &BTreeMap<String, Type>,
        budget: &mut TypeExpansionBudget,
    ) -> Type {
        let span = self.instantiation.current_expression.borrow().clone();
        self.materialize_result_at(value, substitutions, budget, span)
    }

    pub(in crate::checker::module) fn materialize_result_at(
        &self,
        value: &Type,
        substitutions: &BTreeMap<String, Type>,
        budget: &mut TypeExpansionBudget,
        span: Option<SourceSpan>,
    ) -> Type {
        let result =
            crate::checker::inference::result_type(value, substitutions, &self.types, budget);
        if budget.exhausted && self.enforce_types {
            if let Some(span) = &span {
                self.generic_inference_failure.set(Some(span.clone()));
            }
        }
        if let (Some(repeated), Some(span)) = (&budget.instantiation_failure, span) {
            self.instantiation.pending.borrow_mut().push(Failure {
                requested: substitute_type(value, substitutions),
                repeated: repeated.clone(),
                span,
            });
        }
        result
    }

    pub(in crate::checker::module) fn check_demanded_type(
        &mut self,
        value: &Type,
        span: &SourceSpan,
    ) {
        if !crate::checker::type_operators::is_operator_context(value, &self.types) {
            return;
        }
        let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
        budget.checking = self.checking;
        crate::checker::inference::result_type(value, &BTreeMap::new(), &self.types, &mut budget);
        if let Some(repeated) = budget.instantiation_failure {
            if self.publish_instantiation_failure(value, &repeated, span) {
                if let Type::Named { name, .. } = value {
                    self.point_last_named_type(name, span);
                }
            }
        } else if budget.exhausted {
            self.type_error(
                span,
                "demanded type instantiation exceeds its generic-expansion limit".into(),
                DiagnosticCode::ResourceLimit,
            );
        }
    }

    fn publish_instantiation_failure(
        &mut self,
        requested: &Type,
        repeated: &Type,
        span: &SourceSpan,
    ) -> bool {
        let requested = type_relations::type_identity(requested);
        let repeated = type_relations::type_identity(repeated);
        let mut diagnosed = self.instantiation.diagnosed.borrow_mut();
        let reported = diagnosed.contains(&requested) || diagnosed.contains(&repeated);
        diagnosed.insert(requested);
        diagnosed.insert(repeated);
        drop(diagnosed);
        if !reported {
            self.typescript_type_error(
                span,
                "Type instantiation is excessively deep and possibly infinite.".into(),
                DiagnosticCode::TypeMismatch,
                2589,
                Vec::new(),
            );
        }
        !reported
    }

    pub(in crate::checker::module) fn report_instantiation_failures(&mut self) {
        let pending = self.instantiation.pending.take();
        for failure in pending {
            self.publish_instantiation_failure(
                &failure.requested,
                &failure.repeated,
                &failure.span,
            );
        }
    }
}
