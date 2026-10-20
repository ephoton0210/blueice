// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Bounded structural candidates for one generic call instantiation.

use super::*;
mod candidates;
mod collect;
mod mapped;

pub(crate) const VERSION: &str = "bluets-generic-inference/2";

pub(super) fn result_type(
    value: &Type,
    substitutions: &BTreeMap<String, Type>,
    aliases: &BTreeMap<String, TypeDefinition>,
    budget: &mut TypeExpansionBudget,
) -> Type {
    let value = substitute_type(value, substitutions).runtime_result();
    if matches!(
        value,
        Type::KeyOf(_)
            | Type::IndexedAccess { .. }
            | Type::Conditional(_)
            | Type::Mapped(_)
            | Type::TemplateLiteral(_)
    ) {
        super::type_operators::expanded(&value, aliases, &mut HashSet::new(), budget)
    } else {
        value
    }
}

pub(super) fn infer_call_substitutions(
    signature: &FunctionSignature,
    actuals: &[Type],
    aliases: &BTreeMap<String, TypeDefinition>,
    budget: &mut TypeExpansionBudget,
) -> BTreeMap<String, Type> {
    infer_contextual_substitutions(signature, actuals, aliases, budget, None)
}

pub(super) fn infer_contextual_substitutions(
    signature: &FunctionSignature,
    actuals: &[Type],
    aliases: &BTreeMap<String, TypeDefinition>,
    budget: &mut TypeExpansionBudget,
    context: Option<&Type>,
) -> BTreeMap<String, Type> {
    infer_contextual_result(signature, actuals, aliases, budget, context).substitutions
}

pub(super) struct InferenceResult {
    pub(super) substitutions: BTreeMap<String, Type>,
    pub(super) rejected_constraint: Option<(String, Type, Type)>,
}

pub(super) fn infer_contextual_result(
    signature: &FunctionSignature,
    actuals: &[Type],
    aliases: &BTreeMap<String, TypeDefinition>,
    budget: &mut TypeExpansionBudget,
    context: Option<&Type>,
) -> InferenceResult {
    let mut substitutions = BTreeMap::new();
    let mut rejected_constraint = None;
    if signature.type_parameters.is_empty() {
        return InferenceResult {
            substitutions,
            rejected_constraint,
        };
    }
    let parameters = signature
        .type_parameters
        .iter()
        .map(|p| p.name.clone())
        .collect();
    let mut collector = collect::Collector {
        parameters: &parameters,
        candidates: BTreeMap::new(),
        aliases,
        budget,
        active: HashSet::new(),
    };
    for (index, actual) in actuals.iter().enumerate() {
        let Some(parameter) = function_parameter_for_argument(signature, index) else {
            break;
        };
        let annotation = if parameter.rest {
            rest_parameter_element_annotation(parameter)
        } else {
            parameter.annotation.as_ref()
        };
        if let Some(annotation) = annotation {
            collector.collect(annotation, actual, false);
        }
    }
    let naked = match &signature.return_type {
        Type::Named { name, arguments } if arguments.is_empty() && parameters.contains(name) => {
            Some(name.as_str())
        }
        _ => None,
    };
    for parameter in &signature.type_parameters {
        let literal_constraint = parameter.constraint.as_ref().is_some_and(|constraint| {
            matches!(constraint, Type::KeyOf(_) | Type::Literal(_))
                || matches!(constraint, Type::Union(parts) if parts.iter().all(|p| matches!(p, Type::Literal(_))))
        });
        let preserve = literal_constraint
            || (naked == Some(parameter.name.as_str())
                && context.is_none_or(|value| matches!(value, Type::Literal(_))));
        if let Some(value) = collector
            .candidates
            .get(&parameter.name)
            .and_then(|candidates| candidates.solve(preserve, aliases, collector.budget))
        {
            substitutions.insert(parameter.name.clone(), value);
        }
    }
    drop(collector);
    if let Some(context) = context {
        let mut collector = collect::Collector {
            parameters: &parameters,
            candidates: BTreeMap::new(),
            aliases,
            budget,
            active: HashSet::new(),
        };
        collector.collect(&signature.return_type, context, false);
        for (name, candidates) in &collector.candidates {
            if let Some(value) = candidates.solve(true, aliases, collector.budget) {
                substitutions.entry(name.clone()).or_insert(value);
            }
        }
    }
    for parameter in &signature.type_parameters {
        let default = parameter
            .default
            .as_ref()
            .map(|value| substitute_type(value, &substitutions))
            .unwrap_or(Type::StrictUnknown);
        let value = substitutions
            .entry(parameter.name.clone())
            .or_insert(default)
            .clone();
        if let Some(constraint) = &parameter.constraint {
            let mut constraint = substitute_type(constraint, &substitutions);
            if let Some(resolved) =
                super::type_operators::resolve(&constraint, aliases, &mut HashSet::new(), budget)
            {
                constraint = resolved;
            }
            if matches!(value, Type::Unknown | Type::StrictUnknown)
                || !is_assignable(&value, &constraint, aliases, &mut HashSet::new(), budget)
            {
                if !matches!(value, Type::Unknown | Type::StrictUnknown)
                    && rejected_constraint.is_none()
                {
                    rejected_constraint = Some((parameter.name.clone(), value, constraint.clone()));
                }
                substitutions.insert(parameter.name.clone(), constraint);
            }
        }
    }
    for parameter in &signature.type_parameters {
        if let Some(value) = substitutions.get(&parameter.name).cloned() {
            substitutions.insert(crate::parser::type_parameter_identity(parameter), value);
        }
    }
    InferenceResult {
        substitutions,
        rejected_constraint,
    }
}

fn widen(value: &Type) -> Type {
    match value {
        Type::Literal(text) if text.starts_with(['\'', '"', '`']) => Type::String,
        Type::Literal(text) if text.parse::<f64>().is_ok() => Type::Number,
        Type::Literal(text) if matches!(text.as_str(), "true" | "false") => Type::Boolean,
        value => value.clone(),
    }
}
