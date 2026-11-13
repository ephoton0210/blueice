// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Bounded structural candidates for one generic call instantiation.

use super::*;
mod barriers;
mod candidates;
pub(super) use barriers::{checking_type, diagnostic_type};
mod collect;
mod dependencies;
mod indexed;
mod mapped;
mod result_parameters;
mod results;
mod surfaces;

pub(crate) const VERSION: &str = "bluets-generic-inference/10";

pub(in crate::checker) fn direct_result_parameter(
    signature: &FunctionSignature,
    aliases: &BTreeMap<String, TypeDefinition>,
    budget: &mut TypeExpansionBudget,
) -> Option<String> {
    let parameters = signature
        .type_parameters
        .iter()
        .map(|p| p.name.clone())
        .collect();
    result_parameters::naked(
        &signature.return_type,
        &parameters,
        aliases,
        &mut HashSet::new(),
        budget,
    )
}

pub(super) fn symbolic_indexed_access(
    value: &Type,
    aliases: &BTreeMap<String, TypeDefinition>,
) -> bool {
    let Type::IndexedAccess { object, .. } = value else {
        return false;
    };
    let object = canonical_parameter_references(object, aliases);
    matches!(&object, Type::Named { name, .. } if aliases.get(name).is_some_and(|definition| definition.kind == TypeDefinitionKind::Parameter))
}

pub(super) fn result_type(
    value: &Type,
    substitutions: &BTreeMap<String, Type>,
    aliases: &BTreeMap<String, TypeDefinition>,
    budget: &mut TypeExpansionBudget,
) -> Type {
    let value = substitute_type(value, substitutions).runtime_result();
    if symbolic_indexed_access(&value, aliases) {
        return value;
    }
    results::finish(value, aliases, budget)
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
        dependencies: BTreeMap::new(),
        evidence: 0,
    };
    for (index, actual) in actuals.iter().enumerate() {
        let Some(parameter) = function_parameter_for_argument(signature, index) else {
            break;
        };
        let annotation = if parameter.rest {
            if let Some(annotation @ Type::Named { name, arguments }) = &parameter.annotation {
                if arguments.is_empty() && signature.type_parameters.iter().any(|p| p.name == *name)
                {
                    let mut value = Type::Tuple(
                        actuals[index..]
                            .iter()
                            .cloned()
                            .map(crate::parser::TupleTypeElement::required)
                            .collect(),
                    );
                    if signature.type_parameters.iter().any(|p| {
                        p.name == *name
                            && p.is_const
                            && matches!(p.constraint, Some(Type::Readonly(_)))
                    }) {
                        value = Type::Readonly(Box::new(value));
                    }
                    collector.collect(annotation, &value, false);
                    break;
                }
            }
            rest_parameter_element_annotation(parameter)
        } else {
            parameter.annotation.as_ref()
        };
        if let Some(annotation) = annotation {
            collector.collect(annotation, actual, false);
        }
    }
    let naked = result_parameters::naked(
        &signature.return_type,
        &parameters,
        aliases,
        &mut HashSet::new(),
        collector.budget,
    );
    for parameter in &signature.type_parameters {
        let literal_constraint = parameter.constraint.as_ref().is_some_and(|constraint| {
            if matches!(constraint, Type::KeyOf(_) | Type::Literal(_)) {
                return true;
            }
            let constraint = super::type_operators::expanded(
                constraint,
                aliases,
                &mut HashSet::new(),
                collector.budget,
            );
            let primitive = |value: &Type| {
                matches!(
                    value,
                    Type::String
                        | Type::Number
                        | Type::BigInt
                        | Type::Boolean
                        | Type::Symbol
                        | Type::UniqueSymbol(_)
                        | Type::Literal(_)
                )
            };
            primitive(&constraint)
                || matches!(&constraint, Type::Union(parts) if parts.iter().all(primitive))
        });
        let preserve = parameter.is_const
            || literal_constraint
            || (naked.as_deref() == Some(parameter.name.as_str())
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
            dependencies: BTreeMap::new(),
            evidence: 0,
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
        substitutions
            .entry(parameter.name.clone())
            .or_insert(default);
    }
    // Constraints may refer to a later parameter. Populate all arguments
    // before validating any of them, rather than replacing valid earlier
    // evidence with an unresolved forward constraint.
    for parameter in &signature.type_parameters {
        let value = substitutions[&parameter.name].clone();
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
                    // A primitive constraint preserves successful literal inference,
                    // but a rejected ordinary candidate retains the widened type
                    // reported by the public checker before that inference support.
                    let candidate = if parameter.is_const {
                        value
                    } else {
                        widen(&value)
                    };
                    rejected_constraint =
                        Some((parameter.name.clone(), candidate, constraint.clone()));
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
        Type::StringLiteral(_) => Type::String,
        Type::Literal(text) if text.starts_with(['\'', '"', '`']) => Type::String,
        Type::Literal(text) if text.parse::<f64>().is_ok() => Type::Number,
        Type::Literal(text) if matches!(text.as_str(), "true" | "false") => Type::Boolean,
        value => value.clone(),
    }
}
