// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Predicate declaration validity, separately from runtime result checking.

use super::*;
use crate::parser::{BindingPattern, TypePredicate};

impl ModuleChecker<'_> {
    pub(in crate::checker::module) fn check_predicate_position(
        &mut self,
        predicate: &TypePredicate,
    ) {
        if predicate.return_position {
            return;
        }
        if predicate.asserts {
            self.typescript_type_error(
                &predicate.span,
                "a type predicate requires a function return position".into(),
                DiagnosticCode::TypeMismatch,
                1228,
                Vec::new(),
            );
        } else {
            let value = self.scopes.as_ref().and_then(|scopes| {
                scopes.query_type(&predicate.parameter, predicate.parameter_span.start)
            });
            if value.is_some() && !self.types.contains_key(&predicate.parameter) {
                self.typescript_type_error(
                    &predicate.parameter_span,
                    format!(
                        "value `{}` is used as a predicate type",
                        predicate.parameter
                    ),
                    DiagnosticCode::UnknownType,
                    2749,
                    vec![predicate.parameter.clone()],
                );
            } else {
                self.typescript_type_error(
                    predicate.is_span.as_ref().unwrap_or(&predicate.span),
                    "a type predicate requires a function return position".into(),
                    DiagnosticCode::TypeMismatch,
                    1005,
                    vec![";".into()],
                );
            }
        }
    }

    pub(in crate::checker::module) fn check_predicate_signature(
        &mut self,
        result: &Type,
        parameters: &[Parameter],
    ) {
        let Type::Predicate(predicate) = result else {
            return;
        };
        if !predicate.return_position {
            return;
        }
        let parameter = parameters
            .iter()
            .find(|parameter| parameter.name == predicate.parameter && parameter.pattern.is_none());
        let expected = if predicate.parameter == "this" {
            self.scopes
                .as_ref()
                .and_then(|scopes| scopes.query_type("this", predicate.parameter_span.start))
        } else if let Some(parameter) = parameter {
            if parameter.rest {
                self.predicate_parameter_error(predicate, 1229, Vec::new());
                return;
            }
            let annotation = parameter.annotation.clone().unwrap_or(Type::Any);
            Some(if parameter.optional && parameter.default.is_none() {
                Type::Union(vec![annotation, Type::Undefined])
            } else {
                annotation
            })
        } else {
            let destructured = parameters.iter().any(|parameter| match &parameter.pattern {
                Some(BindingPattern::Object(bindings)) => bindings
                    .iter()
                    .any(|binding| binding.name == predicate.parameter),
                Some(BindingPattern::Array(bindings)) => bindings
                    .iter()
                    .flatten()
                    .any(|binding| binding.name == predicate.parameter),
                None => false,
            });
            self.predicate_parameter_error(
                predicate,
                if destructured { 1230 } else { 1225 },
                vec![predicate.parameter.clone()],
            );
            return;
        };
        let Some(target) = &predicate.target else {
            return;
        };
        self.check_type(
            target,
            predicate.target_span.as_ref().unwrap_or(&predicate.span),
        );
        let Some(expected) = expected else {
            self.predicate_parameter_error(predicate, 2526, Vec::new());
            return;
        };
        let mut substitutions = BTreeMap::new();
        substitutions.insert("this".into(), expected.clone());
        let target = substitute_type(target, &substitutions);
        let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
        budget.checking = self.checking;
        let assignable = is_assignable(
            &target,
            &expected,
            &self.types,
            &mut HashSet::new(),
            &mut budget,
        );
        if budget.exhausted {
            self.type_error(
                &predicate.span,
                "predicate compatibility exceeds its generic-expansion limit".into(),
                DiagnosticCode::ResourceLimit,
            );
        } else if !assignable {
            self.typescript_type_error(
                predicate.target_span.as_ref().unwrap_or(&predicate.span),
                format!(
                    "predicate type `{}` is not assignable to parameter type `{}`",
                    type_label(&target),
                    type_label(&expected)
                ),
                DiagnosticCode::TypeMismatch,
                2677,
                Vec::new(),
            );
            let detail = crate::diagnostic::type_text::detail(&target, &expected, self.project);
            if let Some(counterpart) = self
                .diagnostics
                .last_mut()
                .and_then(|diagnostic| diagnostic.typescript.as_mut())
            {
                counterpart.message.push_str(&format!(
                    "\n  Type '{}' is not assignable to type '{}'.{}",
                    crate::diagnostic::type_text::render_in(&target, self.project),
                    crate::diagnostic::type_text::render_in(&expected, self.project),
                    detail.replace('\n', "\n  ")
                ));
            }
        }
    }

    fn predicate_parameter_error(
        &mut self,
        predicate: &TypePredicate,
        code: u32,
        arguments: Vec<String>,
    ) {
        self.typescript_type_error(
            &predicate.parameter_span,
            format!("invalid predicate parameter `{}`", predicate.parameter),
            DiagnosticCode::TypeMismatch,
            code,
            arguments,
        );
    }
}
