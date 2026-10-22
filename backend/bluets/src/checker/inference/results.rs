// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Materialize concrete library operator arguments without expanding interfaces.
use super::*;

pub(super) fn finish(
    value: Type,
    aliases: &BTreeMap<String, TypeDefinition>,
    budget: &mut TypeExpansionBudget,
) -> Type {
    match value {
        value @ (Type::Mapped(_)
        | Type::Conditional(_)
        | Type::KeyOf(_)
        | Type::IndexedAccess { .. }
        | Type::TemplateLiteral(_)) => {
            let expanded = super::super::type_operators::expanded(
                &value,
                aliases,
                &mut HashSet::new(),
                budget,
            );
            if expanded == value || budget.exhausted {
                value
            } else {
                finish(expanded, aliases, budget)
            }
        }
        Type::Named { name, arguments } => {
            if aliases.get(&name).is_some_and(|definition| matches!(&definition.value, Type::Conditional(value) if value.span.module.starts_with("<builtin:"))) {
                let value = Type::Named {name, arguments};
                let expanded = super::super::type_operators::expanded(&value, aliases, &mut HashSet::new(), budget);
                return if expanded == value || budget.exhausted {value} else {finish(expanded, aliases, budget)};
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
            }) {
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
        value => value,
    }
}
