// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Named aliases retain declaration-site variance under the checking flags.

use super::*;

const OUTPUT: u8 = 1;
const INPUT: u8 = 2;
const BOTH: u8 = OUTPUT | INPUT;

pub(super) fn matching(
    actual: &Type,
    pattern: &Type,
    bindings: &mut MatchBindings,
    aliases: &BTreeMap<String, TypeDefinition>,
    budget: &mut TypeExpansionBudget,
) -> Option<bool> {
    let (
        Type::Named { name, arguments },
        Type::Named {
            name: target,
            arguments: patterns,
        },
    ) = (actual, pattern)
    else {
        return None;
    };
    let definition = aliases.get(name)?;
    if name != target
        || definition.kind != TypeDefinitionKind::Alias
        || definition.parameters.len() != arguments.len()
        || arguments.len() != patterns.len()
        || pattern.infer_parameters().is_empty()
    {
        return None;
    }
    // A scalar alias has no retained argument identity. Expanding its pattern
    // lets a genuine infer occurrence survive, while erased slots stay absent.
    if crate::checker::type_operators::primitive_alias(actual, aliases, budget).is_some() {
        return None;
    }
    let mut directions = Vec::new();
    for parameter in &definition.parameters {
        let direction = match parameter.variance {
            Some(crate::parser::Variance::In) => INPUT,
            Some(crate::parser::Variance::Out) => OUTPUT,
            Some(crate::parser::Variance::InOut) => BOTH,
            None => mask(
                &definition.value,
                &parameter.name,
                false,
                aliases,
                &mut HashSet::from([name.clone()]),
                budget,
            )?,
        };
        directions.push(direction);
    }
    Some(
        arguments
            .iter()
            .zip(patterns)
            .zip(directions)
            .all(|((source, target), direction)| {
                if direction == INPUT {
                    bindings.parameter(source, target, aliases, budget)
                } else {
                    super::matches(source, target, bindings, aliases, budget)
                }
            }),
    )
}

fn flip(direction: u8) -> u8 {
    ((direction & OUTPUT) << 1) | ((direction & INPUT) >> 1)
}

fn mask(
    value: &Type,
    parameter: &str,
    method: bool,
    aliases: &BTreeMap<String, TypeDefinition>,
    active: &mut HashSet<String>,
    budget: &mut TypeExpansionBudget,
) -> Option<u8> {
    if !budget.consume() {
        return None;
    }
    match value {
        Type::Named { name, arguments } if name == parameter && arguments.is_empty() => {
            Some(OUTPUT)
        }
        Type::Named { name, arguments } if !arguments.is_empty() => {
            let definition = aliases.get(name)?;
            if definition.kind != TypeDefinitionKind::Alias
                || definition.parameters.len() != arguments.len()
                || !active.insert(name.clone())
            {
                return None;
            }
            let mut result = 0;
            for (slot, argument) in definition.parameters.iter().zip(arguments) {
                let local = mask(
                    &definition.value,
                    &slot.name,
                    false,
                    aliases,
                    active,
                    budget,
                )?;
                let occurrence = mask(argument, parameter, false, aliases, active, budget)?;
                result |= if local == INPUT {
                    flip(occurrence)
                } else {
                    occurrence
                };
            }
            active.remove(name);
            Some(result)
        }
        Type::Function { parameters, result }
        | Type::GenericFunction {
            parameters, result, ..
        } => {
            if matches!(value, Type::GenericFunction {type_parameters, ..}
                if type_parameters.iter().any(|slot| slot.name == parameter))
            {
                return Some(0);
            }
            let mut direction = mask(result, parameter, false, aliases, active, budget)?;
            for slot in parameters {
                let Some(annotation) = &slot.annotation else {
                    continue;
                };
                let occurrence = mask(annotation, parameter, false, aliases, active, budget)?;
                direction |= if method || !budget.checking.strict_function_types {
                    if occurrence == 0 {
                        0
                    } else {
                        BOTH
                    }
                } else {
                    flip(occurrence)
                };
            }
            Some(direction)
        }
        Type::Array(inner) | Type::Readonly(inner) | Type::InferenceBarrier(inner) => {
            mask(inner, parameter, false, aliases, active, budget)
        }
        Type::Tuple(elements) => {
            let mut result = 0;
            for element in elements {
                result |= mask(
                    &element.annotation,
                    parameter,
                    false,
                    aliases,
                    active,
                    budget,
                )?;
            }
            Some(result)
        }
        Type::Record(fields) => {
            let mut result = 0;
            for field in fields {
                result |= mask(
                    &field.value,
                    parameter,
                    field.method,
                    aliases,
                    active,
                    budget,
                )?;
            }
            Some(result)
        }
        Type::Union(parts) | Type::Intersection(parts) => {
            let mut result = 0;
            for part in parts {
                result |= mask(part, parameter, false, aliases, active, budget)?;
            }
            Some(result)
        }
        Type::KeyOf(_)
        | Type::IndexedAccess { .. }
        | Type::Mapped(_)
        | Type::Conditional(_)
        | Type::CallableRecord { .. }
        | Type::IndexedRecord { .. }
        | Type::TemplateLiteral(_)
        | Type::StringMapping(_)
        | Type::Infer(_) => None,
        _ => Some(0),
    }
}
