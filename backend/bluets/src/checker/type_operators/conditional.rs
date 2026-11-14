// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Conditional matching binds inferred types before selecting the true branch.
use super::*;

mod alias_variance;
mod barriers;
mod callables;
mod candidates;
mod containers;
mod functions;
pub(super) use candidates::MatchBindings;

pub(super) fn resolve(
    value: &ConditionalType,
    aliases: &BTreeMap<String, TypeDefinition>,
    _visited: &mut HashSet<String>,
    budget: &mut TypeExpansionBudget,
) -> Option<Type> {
    if value.extends.infer_parameters().is_empty() {
        return predicate(value, aliases, budget);
    }
    let actual = &value.check;
    let mut bindings = MatchBindings::default();
    let collected = matches(actual, &value.extends, &mut bindings, aliases, budget);
    let may_be_any = matches!(
        actual,
        Type::Any
            | Type::Union(_)
            | Type::Conditional(_)
            | Type::IndexedAccess { .. }
            | Type::InferenceBarrier(_)
            | Type::StringMapping(_)
    ) || matches!(actual, Type::Named {name, ..} if aliases.get(name)
            .is_some_and(|definition| definition.kind == TypeDefinitionKind::Alias));
    let is_any = may_be_any && expanded(actual, aliases, &mut HashSet::new(), budget) == Type::Any;
    if !collected && !is_any {
        return (!budget.exhausted).then(|| value.when_false.clone());
    }
    let parameters = value.extends.infer_parameters();
    let pattern = if parameters.is_empty() {
        value.extends.clone()
    } else {
        bindings.finish(&parameters, aliases, budget);
        barriers::instantiate(&value.extends, &bindings.values, budget)
    };
    if is_any
        && !matches!(
            expanded(&pattern, aliases, &mut HashSet::new(), budget),
            Type::Any | Type::StrictUnknown
        )
    {
        if budget.exhausted {
            return None;
        }
        let when_true = substitute_type(&value.when_true, &bindings.values);
        return Some(if when_true == Type::Any || value.when_false == Type::Any {
            Type::Any
        } else {
            union([when_true, value.when_false.clone()])
        });
    }
    let source = expanded(actual, aliases, &mut HashSet::new(), budget);
    let checking_source = callables::checking_source(&source, &pattern, aliases, budget)?;
    let matched = collected
        && is_assignable(
            &checking_source,
            &pattern,
            aliases,
            &mut HashSet::new(),
            budget,
        );
    if budget.exhausted {
        return None;
    }
    Some(if matched {
        substitute_type(&value.when_true, &bindings.values)
    } else {
        value.when_false.clone()
    })
}

fn predicate(
    value: &ConditionalType,
    aliases: &BTreeMap<String, TypeDefinition>,
    budget: &mut TypeExpansionBudget,
) -> Option<Type> {
    // A conditional without inference needs one complete relation, not both
    // candidate collection and validation. In particular, replaying an alias
    // target can traverse an entire distributed key set several times.
    let source = expanded(&value.check, aliases, &mut HashSet::new(), budget);
    if source == Type::Any
        && !matches!(
            expanded(&value.extends, aliases, &mut HashSet::new(), budget),
            Type::Any | Type::StrictUnknown
        )
    {
        if budget.exhausted {
            return None;
        }
        return Some(
            if value.when_true == Type::Any || value.when_false == Type::Any {
                Type::Any
            } else {
                union([value.when_true.clone(), value.when_false.clone()])
            },
        );
    }
    let matched = is_assignable(
        &source,
        &value.extends,
        aliases,
        &mut HashSet::new(),
        budget,
    );
    (!budget.exhausted).then(|| {
        if matched {
            value.when_true.clone()
        } else {
            value.when_false.clone()
        }
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
        let mut ignored = bindings.clone();
        return matches(actual, base, &mut ignored, aliases, budget);
    }
    if let Type::Intersection(parts) = pattern {
        let mut candidate = bindings.clone();
        if parts
            .iter()
            .all(|part| matches(actual, part, &mut candidate, aliases, budget))
        {
            *bindings = candidate;
            return true;
        }
        return false;
    }
    if let Some(result) = alias_variance::matching(actual, pattern, bindings, aliases, budget) {
        return result;
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
        let normalized = super::normalize_nullable_union(actual, budget.checking);
        let actual = normalized.as_ref().unwrap_or(actual);
        return bindings.record(&parameter.name, actual, aliases, budget);
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
        (_, Type::Readonly(expected)) => {
            let source = match &actual {
                Type::Readonly(source) => source.as_ref(),
                source => source,
            };
            matches(source, expected, bindings, aliases, budget)
        }
        (_, Type::TemplateLiteral(template)) => {
            templates::matches(&actual, template, bindings, aliases, budget)
        }
        (Type::Array(actual), Type::Array(pattern)) => {
            matches(actual, pattern, bindings, aliases, budget)
        }
        (Type::Tuple(actual), Type::Array(pattern)) => actual
            .iter()
            .all(|element| matches(&element.indexed_type(), pattern, bindings, aliases, budget)),
        (Type::Tuple(actual), Type::Tuple(pattern)) => {
            containers::tuple(actual, pattern, bindings, aliases, budget)
        }
        (Type::Record(fields), Type::Record(pattern)) => pattern.iter().all(|field| {
            if fields
                .iter()
                .any(|actual| actual.name == field.name && actual.optional)
                && !field.optional
            {
                return false;
            }
            match property_type(&actual, &field.name, aliases, &mut HashSet::new(), budget) {
                PropertyType::Found { value, .. } => {
                    bindings.field(&value, &field.value, field.method, aliases, budget)
                }
                _ => field.optional,
            }
        }),
        (Type::Intersection(_) | Type::CallableRecord { .. }, Type::Record(pattern)) => {
            pattern.iter().all(|field| {
                match property_type(&actual, &field.name, aliases, &mut HashSet::new(), budget) {
                    PropertyType::Found { value, .. } => {
                        bindings.field(&value, &field.value, field.method, aliases, budget)
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
        _ if bindings.contravariant => {
            is_assignable(pattern, &actual, aliases, &mut HashSet::new(), budget)
        }
        _ => is_assignable(&actual, pattern, aliases, &mut HashSet::new(), budget),
    }
}
