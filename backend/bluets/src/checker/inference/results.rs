// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Materialize concrete library operator arguments without expanding interfaces.
use super::*;
mod non_null;

fn conditional_alias(name: &str, aliases: &BTreeMap<String, TypeDefinition>) -> bool {
    let mut name = name;
    let mut active = HashSet::new();
    while active.insert(name) {
        let Some(definition) = aliases
            .get(name)
            .filter(|d| d.kind == TypeDefinitionKind::Alias)
        else {
            return false;
        };
        match &definition.value {
            Type::Conditional(_) => return true,
            Type::Named { name: next, .. } => name = next,
            _ => return false,
        }
    }
    false
}

pub(super) fn finish(
    value: Type,
    aliases: &BTreeMap<String, TypeDefinition>,
    budget: &mut TypeExpansionBudget,
) -> Type {
    let operation = match &value {
        Type::Named { name, .. } => {
            super::barriers::alias_has_barrier(name, aliases)
                || crate::checker::type_relations::non_null_alias(name, aliases)
                || conditional_alias(name, aliases)
        }
        Type::Mapped(_)
        | Type::Conditional(_)
        | Type::KeyOf(_)
        | Type::IndexedAccess { .. }
        | Type::TemplateLiteral(_) => true,
        _ => false,
    };
    if !operation {
        return finish_inner(value, aliases, budget);
    }
    let Some(key) = budget.enter_instantiation(&value) else {
        return value;
    };
    let result = finish_inner(value.clone(), aliases, budget);
    budget.leave_instantiation(&key);
    if budget.instantiation_failure.is_some() {
        value
    } else {
        result
    }
}

fn finish_inner(
    value: Type,
    aliases: &BTreeMap<String, TypeDefinition>,
    budget: &mut TypeExpansionBudget,
) -> Type {
    if super::symbolic_indexed_access(&value, aliases) {
        return value;
    }
    match value {
        // Copying a function value retains its declared signature. Operations
        // inside parameter constraints/annotations are demanded by the call or
        // relation that uses them, preserving diagnostic names such as keyof E.
        value @ (Type::Function { .. } | Type::GenericFunction { .. }) => value,
        Type::InferenceBarrier(value) => Type::inference_barrier(finish(*value, aliases, budget)),
        value @ (Type::Mapped(_)
        | Type::Conditional(_)
        | Type::KeyOf(_)
        | Type::IndexedAccess { .. }
        | Type::TemplateLiteral(_)) => {
            let expanded = instantiate_named(
                &value,
                aliases,
                &mut HashSet::new(),
                budget,
                "inferred operator",
            )
            .unwrap_or_else(|| value.clone());
            if expanded == value || budget.exhausted {
                value
            } else {
                finish(expanded, aliases, budget)
            }
        }
        Type::Named { name, arguments } => {
            if super::barriers::alias_has_barrier(&name, aliases)
                || crate::checker::type_relations::non_null_alias(&name, aliases)
                || conditional_alias(&name, aliases)
            {
                let parameters = aliases
                    .iter()
                    .filter(|(_, definition)| definition.kind == TypeDefinitionKind::Parameter)
                    .map(|(name, _)| name.clone())
                    .collect();
                let open = arguments.iter().any(|argument| {
                    super::dependencies::contains(
                        argument,
                        &parameters,
                        aliases,
                        &mut HashSet::new(),
                        budget,
                    )
                });
                if !open {
                    let value = Type::Named { name, arguments };
                    if let Some(value) = non_null::materialize(&value, aliases, budget) {
                        return value;
                    }
                    let expanded = instantiate_named(
                        &value,
                        aliases,
                        &mut HashSet::new(),
                        budget,
                        "inferred closed conditional",
                    )
                    .unwrap_or_else(|| value.clone());
                    return if expanded == value || budget.exhausted {
                        value
                    } else {
                        finish(expanded, aliases, budget)
                    };
                }
            }
            let mut arguments: Vec<_> = arguments
                .into_iter()
                .map(|value| finish(value, aliases, budget))
                .collect();
            if let Some(definition) = aliases.get(&name) {
                if definition.kind == TypeDefinitionKind::LibraryInterface {
                    let substitutions: BTreeMap<_, _> = definition
                        .parameters
                        .iter()
                        .zip(&arguments)
                        .map(|(p, value)| (p.name.clone(), value.clone()))
                        .collect();
                    while let Some(parameter) = definition
                        .parameters
                        .get(arguments.len().saturating_sub(1))
                        .filter(|_| !arguments.is_empty())
                    {
                        if parameter
                            .default
                            .as_ref()
                            .map(|default| substitute_type(default, &substitutions))
                            .as_ref()
                            != arguments.last()
                        {
                            break;
                        }
                        arguments.pop();
                    }
                }
            }
            Type::Named { name, arguments }
        }
        Type::Array(value) => Type::Array(Box::new(finish(*value, aliases, budget))),
        Type::Readonly(value) => Type::Readonly(Box::new(finish(*value, aliases, budget))),
        Type::Union(values) => {
            let mut values: Vec<_> = values
                .into_iter()
                .map(|v| finish(v, aliases, budget))
                .collect();
            if values.iter().all(|value| {
                matches!(
                    value,
                    Type::String
                        | Type::Number
                        | Type::BigInt
                        | Type::Boolean
                        | Type::Symbol
                        | Type::Null
                        | Type::Undefined
                )
            }) && values
                .iter()
                .any(|value| !matches!(value, Type::Null | Type::Undefined))
            {
                values.sort_by_key(|value| match value {
                    Type::String => 0,
                    Type::Number => 1,
                    Type::BigInt => 2,
                    Type::Boolean => 3,
                    Type::Symbol => 4,
                    Type::Undefined => 5,
                    _ => 6,
                });
            }
            if values.contains(&Type::Any) {
                Type::Any
            } else if values.contains(&Type::StrictUnknown) {
                Type::StrictUnknown
            } else {
                super::super::type_operators::union(values)
            }
        }
        Type::Tuple(mut values) => {
            for value in &mut values {
                value.annotation = finish(value.annotation.clone(), aliases, budget);
            }
            Type::Tuple(values)
        }
        value => value.map_children(|child, _| finish(child.clone(), aliases, budget)),
    }
}
