// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Skip candidate collection from types without an inferable dependency.
//! Argument validation still compares every instantiated parameter afterward.

use super::*;

pub(super) fn contains(
    value: &Type,
    parameters: &BTreeSet<String>,
    aliases: &BTreeMap<String, TypeDefinition>,
    active: &mut HashSet<String>,
    budget: &mut TypeExpansionBudget,
) -> bool {
    if let Type::Named { name, arguments } = value {
        if parameters.contains(name) {
            return true;
        }
        if !budget.consume() {
            return false;
        }
        if arguments
            .iter()
            .any(|value| contains(value, parameters, aliases, active, budget))
        {
            return true;
        }
        let Some(definition) = aliases.get(name) else {
            return false;
        };
        if definition.kind == TypeDefinitionKind::Parameter {
            return parameters.contains(crate::parser::source_type_name(name));
        }
        // The immutable library cannot capture a source binder. All its
        // source-dependent evidence is carried by the arguments above.
        if definition.kind == TypeDefinitionKind::LibraryInterface {
            return false;
        }
        let identity = type_identity(value);
        if !active.insert(identity.clone()) {
            return false;
        }
        let mut path = HashSet::new();
        let result = instantiate_named(value, aliases, &mut path, budget, "inference dependency")
            .is_some_and(|value| contains(&value, parameters, aliases, active, budget));
        active.remove(&identity);
        return result;
    }
    if !budget.consume() {
        return false;
    }
    let mut visit = |value: &Type| contains(value, parameters, aliases, active, budget);
    match value {
        Type::Array(value) | Type::KeyOf(value) => visit(value),
        Type::IndexedAccess { object, index, .. } => visit(object) || visit(index),
        Type::Tuple(values) => values.iter().any(|value| visit(&value.annotation)),
        Type::Record(fields) => fields.iter().any(|field| visit(&field.value)),
        Type::CallableRecord { fields, signatures } => {
            fields.iter().any(|field| visit(&field.value))
                || signatures
                    .iter()
                    .any(|signature| visit(&signature.function_type()))
        }
        Type::Function { parameters, result } => {
            parameters
                .iter()
                .filter_map(|parameter| parameter.annotation.as_ref())
                .any(&mut visit)
                || visit(result)
        }
        Type::GenericFunction {
            type_parameters,
            parameters: arguments,
            result,
            ..
        } => {
            if type_parameters.iter().any(|parameter| {
                parameter
                    .constraint
                    .iter()
                    .chain(&parameter.default)
                    .any(&mut visit)
            }) {
                return true;
            }
            let mut parameters = parameters.clone();
            for parameter in type_parameters {
                parameters.remove(&parameter.name);
                parameters.remove(&crate::parser::type_parameter_identity(parameter));
            }
            arguments
                .iter()
                .filter_map(|parameter| parameter.annotation.as_ref())
                .chain(std::iter::once(result.as_ref()))
                .any(|value| contains(value, &parameters, aliases, active, budget))
        }
        Type::Predicate(predicate) => predicate.target.as_deref().is_some_and(&mut visit),
        Type::Union(values) | Type::Intersection(values) => values.iter().any(&mut visit),
        _ => value
            .operator_children()
            .is_some_and(|children| children.into_iter().any(visit)),
    }
}
