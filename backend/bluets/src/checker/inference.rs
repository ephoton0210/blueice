// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Bounded structural candidates for one generic call instantiation.

use super::*;

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
            collect(
                annotation,
                actual,
                &parameters,
                &mut substitutions,
                aliases,
                budget,
            );
        }
    }
    if let Some(context) = context {
        let mut contextual = BTreeMap::new();
        collect(
            &signature.return_type,
            context,
            &parameters,
            &mut contextual,
            aliases,
            budget,
        );
        for (name, value) in contextual {
            substitutions.entry(name).or_insert(value);
        }
    }
    let naked = match &signature.return_type {
        Type::Named { name, arguments } if arguments.is_empty() && parameters.contains(name) => {
            Some(name.as_str())
        }
        _ => None,
    };
    for (name, value) in &mut substitutions {
        if naked != Some(name.as_str())
            || context.is_some_and(|value| !matches!(value, Type::Literal(_)))
        {
            *value = widen(value);
        }
    }
    for parameter in &signature.type_parameters {
        let default = parameter
            .default
            .as_ref()
            .map(|value| substitute_type(value, &substitutions))
            .unwrap_or(Type::Unknown);
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
            if value == Type::Unknown
                || !is_assignable(&value, &constraint, aliases, &mut HashSet::new(), budget)
            {
                if value != Type::Unknown && rejected_constraint.is_none() {
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

fn collect(
    template: &Type,
    actual: &Type,
    parameters: &BTreeSet<String>,
    substitutions: &mut BTreeMap<String, Type>,
    aliases: &BTreeMap<String, TypeDefinition>,
    budget: &mut TypeExpansionBudget,
) {
    if !budget.consume() {
        return;
    }
    if let Type::Named { name, arguments } = template {
        if arguments.is_empty() && parameters.contains(name) {
            match substitutions.get(name) {
                Some(previous) if previous != actual && widen(previous) == widen(actual) => {
                    substitutions.insert(name.clone(), widen(previous));
                }
                None => {
                    substitutions.insert(name.clone(), actual.clone());
                }
                _ => {}
            }
            return;
        }
    }
    if let Some(expanded) = instantiate_named(
        template,
        aliases,
        &mut HashSet::new(),
        budget,
        "inference template",
    ) {
        collect(
            &expanded,
            actual,
            parameters,
            substitutions,
            aliases,
            budget,
        );
        return;
    }
    if let Some(expanded) = instantiate_named(
        actual,
        aliases,
        &mut HashSet::new(),
        budget,
        "inference argument",
    ) {
        collect(
            template,
            &expanded,
            parameters,
            substitutions,
            aliases,
            budget,
        );
        return;
    }
    match (template, actual) {
        (Type::Array(template), Type::Array(actual)) => {
            collect(
                template,
                &widen(actual),
                parameters,
                substitutions,
                aliases,
                budget,
            );
        }
        (Type::Tuple(templates), Type::Tuple(actuals)) => {
            for (template, actual) in templates.iter().zip(actuals) {
                collect(
                    &template.annotation,
                    &widen(&actual.annotation),
                    parameters,
                    substitutions,
                    aliases,
                    budget,
                );
            }
        }
        (Type::Record(templates), Type::Record(actuals)) => {
            for template in templates {
                if let Some(actual) = actuals.iter().find(|actual| actual.name == template.name) {
                    collect(
                        &template.value,
                        &widen(&actual.value),
                        parameters,
                        substitutions,
                        aliases,
                        budget,
                    );
                }
            }
        }
        (
            Type::Function {
                parameters: templates,
                result: template,
            },
            Type::Function {
                parameters: actuals,
                result: actual,
            },
        ) => {
            for (template, actual) in templates.iter().zip(actuals) {
                if let (Some(template), Some(actual)) = (&template.annotation, &actual.annotation) {
                    let mut contravariant = BTreeMap::new();
                    collect(
                        template,
                        actual,
                        parameters,
                        &mut contravariant,
                        aliases,
                        budget,
                    );
                    substitutions.extend(contravariant);
                }
            }
            collect(template, actual, parameters, substitutions, aliases, budget);
        }
        (Type::Union(templates), actual) => {
            for template in templates {
                if !matches!(template, Type::Undefined | Type::Null) {
                    collect(template, actual, parameters, substitutions, aliases, budget);
                }
            }
        }
        _ => {}
    }
}
