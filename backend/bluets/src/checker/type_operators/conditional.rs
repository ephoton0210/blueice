// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Conditional matching binds inferred types before selecting the true branch.
use super::*;

mod barriers;
mod callables;
mod functions;

#[derive(Clone, Default)]
pub(super) struct MatchBindings {
    values: BTreeMap<String, Type>,
    saw_barrier: bool,
}

impl std::ops::Deref for MatchBindings {
    type Target = BTreeMap<String, Type>;
    fn deref(&self) -> &Self::Target {
        &self.values
    }
}

impl std::ops::DerefMut for MatchBindings {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.values
    }
}

pub(super) fn resolve(
    value: &ConditionalType,
    aliases: &BTreeMap<String, TypeDefinition>,
    _visited: &mut HashSet<String>,
    budget: &mut TypeExpansionBudget,
) -> Option<Type> {
    let actual = &value.check;
    let mut bindings = MatchBindings::default();
    let mut matched = matches(actual, &value.extends, &mut bindings, aliases, budget);
    if matched && bindings.saw_barrier {
        for parameter in value.extends.infer_parameters() {
            let fallback = parameter.constraint.unwrap_or(Type::StrictUnknown);
            bindings.values.entry(parameter.name).or_insert(fallback);
        }
        let pattern = barriers::instantiate(&value.extends, &bindings.values, budget);
        matched = is_assignable(actual, &pattern, aliases, &mut HashSet::new(), budget);
    }
    if budget.exhausted {
        return None;
    }
    Some(if matched {
        substitute_type(&value.when_true, &bindings.values)
    } else {
        value.when_false.clone()
    })
}

pub(super) fn matches(
    actual: &Type,
    pattern: &Type,
    bindings: &mut MatchBindings,
    aliases: &BTreeMap<String, TypeDefinition>,
    budget: &mut TypeExpansionBudget,
) -> bool {
    if !budget.consume() {
        return false;
    }
    if let Type::InferenceBarrier(base) = pattern {
        bindings.saw_barrier = true;
        let mut ignored = bindings.clone();
        return matches(actual, base, &mut ignored, aliases, budget);
    }
    if let Type::Named {
        name: expected_name,
        ..
    } = pattern
    {
        if aliases
            .get(expected_name)
            .is_some_and(|definition| definition.kind == TypeDefinitionKind::LibraryInterface)
        {
            let mut operand = actual.clone();
            let mut visited = HashSet::new();
            while !matches!(&operand, Type::Named {name, ..} if name == expected_name) {
                let Some(next) = instantiate_named(
                    &operand,
                    aliases,
                    &mut visited,
                    budget,
                    "conditional identity",
                ) else {
                    break;
                };
                operand = next;
            }
            if operand != *actual
                && matches!(&operand, Type::Named {name, ..} if name == expected_name)
            {
                return matches(&operand, pattern, bindings, aliases, budget);
            }
        }
    }
    if let Type::Infer(parameter) = pattern {
        if parameter.constraint.as_ref().is_some_and(|constraint| {
            !is_assignable(actual, constraint, aliases, &mut HashSet::new(), budget)
        }) {
            return false;
        }
        let value = match bindings.values.get(&parameter.name) {
            Some(previous) => union([previous.clone(), actual.clone()]),
            None => actual.clone(),
        };
        bindings.values.insert(parameter.name.clone(), value);
        return true;
    }
    if let (
        Type::Named { name, arguments },
        Type::Named {
            name: pattern_name,
            arguments: patterns,
        },
    ) = (actual, pattern)
    {
        if name == pattern_name
            && arguments.len() == patterns.len()
            && aliases
                .get(name)
                .is_some_and(|definition| definition.kind == TypeDefinitionKind::LibraryInterface)
        {
            return arguments
                .iter()
                .zip(patterns)
                .all(|(actual, pattern)| matches(actual, pattern, bindings, aliases, budget));
        }
    }
    if matches!(pattern, Type::Named { .. }) && !pattern.infer_parameters().is_empty() {
        if let Some(expanded) = instantiate_named(
            pattern,
            aliases,
            &mut HashSet::new(),
            budget,
            "conditional pattern",
        ) {
            if expanded != *pattern {
                return matches(actual, &expanded, bindings, aliases, budget);
            }
        }
    }
    let actual = expanded(actual, aliases, &mut HashSet::new(), budget);
    if let Some(matched) = callables::constructor(&actual, pattern, bindings, aliases, budget) {
        return matched;
    }
    if actual == Type::Any {
        if let Some(matched) = callables::any_signature(pattern, bindings, aliases, budget) {
            return matched;
        }
    }
    let callable = match &actual {
        Type::CallableRecord { signatures, .. } => signatures
            .iter()
            .rev()
            .find(|s| !s.construct)
            .map(|s| s.function_type()),
        Type::Intersection(parts) => parts
            .iter()
            .rev()
            .find(|part| {
                matches!(
                    part,
                    Type::Function { .. }
                        | Type::GenericFunction { .. }
                        | Type::CallableRecord { .. }
                )
            })
            .cloned(),
        Type::GenericFunction {
            type_parameters,
            parameters,
            result,
            ..
        } => callables::erase(type_parameters, parameters, result, budget),
        _ => None,
    };
    if matches!(
        pattern,
        Type::Function { .. } | Type::GenericFunction { .. }
    ) {
        if let Some(callable) = callable {
            return matches(&callable, pattern, bindings, aliases, budget);
        }
    }

    match (&actual, pattern) {
        (_, Type::TemplateLiteral(template)) => {
            templates::matches(&actual, template, bindings, aliases, budget)
        }
        (Type::Array(actual), Type::Array(pattern)) => {
            matches(actual, pattern, bindings, aliases, budget)
        }
        (Type::Tuple(actual), Type::Tuple(pattern)) => {
            let fixed = pattern.iter().take_while(|element| !element.rest).count();
            if actual.len() < fixed || (fixed == pattern.len() && actual.len() != fixed) {
                return false;
            }
            for (actual, pattern) in actual.iter().zip(pattern.iter()).take(fixed) {
                if !matches(
                    &actual.annotation,
                    &pattern.annotation,
                    bindings,
                    aliases,
                    budget,
                ) {
                    return false;
                }
            }
            if let Some(rest) = pattern.get(fixed) {
                let tail = Type::Tuple(actual[fixed..].to_vec());
                match &rest.annotation {
                    Type::Array(item) => actual[fixed..]
                        .iter()
                        .all(|value| matches(&value.annotation, item, bindings, aliases, budget)),
                    value => matches(&tail, value, bindings, aliases, budget),
                }
            } else {
                true
            }
        }
        (Type::Record(actual), Type::Record(pattern)) => pattern.iter().all(|field| {
            match actual.iter().find(|actual| actual.name == field.name) {
                Some(actual) => {
                    (!actual.optional || field.optional)
                        && matches(&actual.value, &field.value, bindings, aliases, budget)
                }
                None => field.optional,
            }
        }),
        (Type::Intersection(_) | Type::CallableRecord { .. }, Type::Record(pattern)) => {
            pattern.iter().all(|field| {
                match property_type(&actual, &field.name, aliases, &mut HashSet::new(), budget) {
                    PropertyType::Found { value, .. } => {
                        matches(&value, &field.value, bindings, aliases, budget)
                    }
                    _ => field.optional,
                }
            })
        }
        (
            Type::Function {
                parameters: actual,
                result,
            },
            Type::Function {
                parameters: pattern,
                result: expected,
            },
        ) => functions::matches(actual, result, pattern, expected, bindings, aliases, budget),
        _ => is_assignable(&actual, pattern, aliases, &mut HashSet::new(), budget),
    }
}
