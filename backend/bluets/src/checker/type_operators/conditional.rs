// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Conditional matching binds inferred types before selecting the true branch.
use super::*;

pub(super) fn resolve(
    value: &ConditionalType,
    aliases: &BTreeMap<String, TypeDefinition>,
    _visited: &mut HashSet<String>,
    budget: &mut TypeExpansionBudget,
) -> Option<Type> {
    let actual = &value.check;
    let mut substitutions = BTreeMap::new();
    let matched = matches(actual, &value.extends, &mut substitutions, aliases, budget);
    if budget.exhausted {
        return None;
    }
    Some(if matched {
        substitute_type(&value.when_true, &substitutions)
    } else {
        value.when_false.clone()
    })
}

pub(super) fn matches(
    actual: &Type,
    pattern: &Type,
    bindings: &mut BTreeMap<String, Type>,
    aliases: &BTreeMap<String, TypeDefinition>,
    budget: &mut TypeExpansionBudget,
) -> bool {
    if !budget.consume() {
        return false;
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
    let actual = expanded(actual, aliases, &mut HashSet::new(), budget);
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
        } if type_parameters.is_empty() => Some(Type::Function {
            parameters: parameters.clone(),
            result: result.clone(),
        }),
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
    if let Type::Infer(parameter) = pattern {
        if parameter.constraint.as_ref().is_some_and(|constraint| {
            !is_assignable(&actual, constraint, aliases, &mut HashSet::new(), budget)
        }) {
            return false;
        }
        let value = match bindings.get(&parameter.name) {
            Some(previous) => union([previous.clone(), actual]),
            None => actual,
        };
        bindings.insert(parameter.name.clone(), value);
        return true;
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
                Some(actual) => matches(&actual.value, &field.value, bindings, aliases, budget),
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
        ) => {
            if pattern.len() == 1 && pattern[0].rest {
                if let Some(Type::Infer(parameter)) = &pattern[0].annotation {
                    let tuple = Type::Tuple(
                        actual
                            .iter()
                            .filter(|p| p.name != "this")
                            .map(|p| TupleTypeElement {
                                annotation: p.annotation.clone().unwrap_or(Type::Any),
                                optional: p.optional,
                                label: Some(p.name.clone()),
                                rest: p.rest,
                            })
                            .collect(),
                    );
                    if !matches(
                        &tuple,
                        &Type::Infer(parameter.clone()),
                        bindings,
                        aliases,
                        budget,
                    ) {
                        return false;
                    }
                }
                return matches(result, expected, bindings, aliases, budget);
            }
            actual.len() == pattern.len()
                && actual.iter().zip(pattern).all(|(actual, pattern)| {
                    matches(
                        actual.annotation.as_ref().unwrap_or(&Type::Any),
                        pattern.annotation.as_ref().unwrap_or(&Type::Any),
                        bindings,
                        aliases,
                        budget,
                    )
                })
                && matches(result, expected, bindings, aliases, budget)
        }
        _ => is_assignable(&actual, pattern, aliases, &mut HashSet::new(), budget),
    }
}
