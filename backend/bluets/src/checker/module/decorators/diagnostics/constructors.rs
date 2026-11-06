// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Generic construct diagnostics compare returns under shared lexical binders.
use super::*;
use crate::parser::TypeSignature;

pub(super) fn signature_text(signature: &TypeSignature, project: &Project) -> String {
    let binders = signature
        .type_parameters
        .iter()
        .map(|parameter| {
            let mut name = crate::parser::source_type_name(&parameter.name).to_string();
            if let Some(value) = &parameter.constraint {
                name += &format!(" extends {}", type_text::render_in(value, project));
            }
            name
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "new <{binders}>({}) => {}",
        type_text::parameter_list_in(&signature.parameters, Some(project)),
        type_text::render_in(&signature.result, project)
    )
}

impl ModuleChecker<'_> {
    fn shared_constructor_returns(
        &self,
        source: &TypeSignature,
        target: &TypeSignature,
        budget: &mut TypeExpansionBudget,
    ) -> Option<(Type, Type)> {
        if source.type_parameters.is_empty()
            || source.type_parameters.len() != target.type_parameters.len()
        {
            return None;
        }
        let mut substitutions = BTreeMap::new();
        for (source, target) in source.type_parameters.iter().zip(&target.type_parameters) {
            let value = Type::Named {
                name: target.name.clone(),
                arguments: Vec::new(),
            };
            substitutions.insert(source.name.clone(), value.clone());
            substitutions.insert(crate::parser::type_parameter_identity(source), value);
        }
        let source = substitute_type(&source.result, &substitutions);
        let target = target.result.clone();
        (!self.relation_matches(&source, &target, budget)).then_some((source, target))
    }

    pub(super) fn constructor_return_reason(
        &self,
        source: &TypeSignature,
        target: &TypeSignature,
        depth: usize,
        budget: &mut TypeExpansionBudget,
        related: &mut Vec<TypeScriptRelatedInformation>,
    ) -> Option<String> {
        let (actual, expected) = self.shared_constructor_returns(source, target, budget)?;
        Some(
            line(
                depth,
                format!(
                    "Construct signature return types '{}' and '{}' are incompatible.",
                    type_text::render_in(&actual, self.project),
                    type_text::render_in(&expected, self.project)
                ),
            ) + &self.decorator_reason(&actual, &expected, depth + 1, budget, related),
        )
    }

    pub(in crate::checker::module::decorators) fn replacement_constructor_return_failure(
        &self,
        actual: &Type,
        expected: &Type,
    ) -> bool {
        let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
        budget.checking = self.checking;
        let a = self.relation_expanded(actual, &mut budget);
        let e = self.relation_expanded(expected, &mut budget);
        let (
            Type::CallableRecord { signatures: a, .. },
            Type::CallableRecord { signatures: e, .. },
        ) = (a, e)
        else {
            return false;
        };
        let (Some(a), Some(e)) = (
            a.iter().find(|s| s.construct),
            e.iter().find(|s| s.construct),
        ) else {
            return false;
        };
        a.type_parameters
            .iter()
            .zip(&e.type_parameters)
            .all(|(a, e)| {
                self.relation_matches(
                    e.constraint.as_ref().unwrap_or(&Type::StrictUnknown),
                    a.constraint.as_ref().unwrap_or(&Type::StrictUnknown),
                    &mut budget,
                )
            })
            && self.shared_constructor_returns(a, e, &mut budget).is_some()
    }
}
