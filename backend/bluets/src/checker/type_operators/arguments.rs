// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Reduce union-valued naked-check arguments before substituting either branch.
use super::*;

pub(in crate::checker) fn prepare(
    definition: &TypeDefinition,
    mut arguments: Vec<Type>,
    aliases: &BTreeMap<String, TypeDefinition>,
    visited: &HashSet<String>,
    budget: &mut TypeExpansionBudget,
) -> Option<Vec<Type>> {
    if arguments.is_empty() {
        return Some(arguments);
    }
    let candidates = definition
        .parameters
        .iter()
        .map(|p| p.name.clone())
        .collect();
    let mut checked = HashSet::new();
    // Library syntax is immutable, bounded embedded metadata. Inspecting its
    // dependency slots is not another generic expansion for every API member.
    // Owner/source definitions retain their work charge; argument expansion
    // and every selected conditional below still use the same owner's budget.
    let charge = definition.kind != TypeDefinitionKind::LibraryInterface;
    collect(&definition.value, &candidates, &mut checked, budget, charge);
    for (parameter, argument) in definition.parameters.iter().zip(&mut arguments) {
        if !checked.contains(&parameter.name) {
            continue;
        }
        let mut pending = vec![(argument.clone(), visited.clone())];
        let mut parts = Vec::new();
        while let Some((original, mut path)) = pending.pop() {
            if !budget.consume() {
                return None;
            }
            match expanded(&original, aliases, &mut path, budget) {
                Type::Union(values) => {
                    pending.extend(values.into_iter().rev().map(|value| (value, path.clone())))
                }
                Type::Never => {}
                // Preserve ordinary/nominal aliases rather than replacing all
                // arguments with their structural expansion.
                _ => parts.push(original),
            }
            if budget.exhausted {
                return None;
            }
        }
        *argument = union(parts);
    }
    (!budget.exhausted).then_some(arguments)
}

fn collect(
    value: &Type,
    candidates: &HashSet<String>,
    checked: &mut HashSet<String>,
    budget: &mut TypeExpansionBudget,
    charge: bool,
) {
    if candidates.is_empty() || (charge && !budget.consume()) {
        return;
    }
    if let Type::Conditional(conditional) = value {
        if let Type::Named { name, arguments } = &conditional.check {
            if arguments.is_empty() && candidates.contains(name) {
                checked.insert(name.clone());
            }
        }
    }
    if value
        .map_operator(|child, bound| {
            let mut candidates = candidates.clone();
            for name in bound {
                candidates.remove(name);
            }
            collect(child, &candidates, checked, budget, charge);
            child.clone()
        })
        .is_some()
    {
        return;
    }
    match value {
        Type::Array(inner) | Type::KeyOf(inner) => {
            collect(inner, candidates, checked, budget, charge)
        }
        Type::IndexedAccess { object, index, .. } => {
            collect(object, candidates, checked, budget, charge);
            collect(index, candidates, checked, budget, charge);
        }
        Type::Union(values)
        | Type::Intersection(values)
        | Type::Named {
            arguments: values, ..
        } => {
            for value in values {
                collect(value, candidates, checked, budget, charge);
            }
        }
        Type::Tuple(elements) => {
            for element in elements {
                collect(&element.annotation, candidates, checked, budget, charge);
            }
        }
        Type::Record(fields) | Type::CallableRecord { fields, .. } => {
            for field in fields {
                collect(&field.value, candidates, checked, budget, charge);
                if let Some(write) = &field.accessor_write_type {
                    collect(write, candidates, checked, budget, charge);
                }
            }
            if let Type::CallableRecord { signatures, .. } = value {
                for signature in signatures {
                    collect(
                        &signature.function_type(),
                        candidates,
                        checked,
                        budget,
                        charge,
                    );
                }
            }
        }
        Type::Function { parameters, result }
        | Type::GenericFunction {
            parameters, result, ..
        } => {
            let mut candidates = candidates.clone();
            if let Type::GenericFunction {
                type_parameters, ..
            } = value
            {
                for parameter in type_parameters {
                    candidates.remove(&parameter.name);
                }
                for parameter in type_parameters {
                    for value in parameter.constraint.iter().chain(parameter.default.iter()) {
                        collect(value, &candidates, checked, budget, charge);
                    }
                }
            }
            for parameter in parameters {
                if let Some(annotation) = &parameter.annotation {
                    collect(annotation, &candidates, checked, budget, charge);
                }
            }
            collect(result, &candidates, checked, budget, charge);
        }
        Type::Predicate(predicate) => {
            if let Some(target) = &predicate.target {
                collect(target, candidates, checked, budget, charge);
            }
        }
        _ => {}
    }
}
