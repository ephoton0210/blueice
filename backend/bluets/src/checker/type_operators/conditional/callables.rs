// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Conditional inference observes the final compatible callable signature.

use super::*;

pub(super) fn checking_source(
    actual: &Type,
    pattern: &Type,
    aliases: &BTreeMap<String, TypeDefinition>,
    budget: &mut TypeExpansionBudget,
) -> Option<Type> {
    if !budget.consume() {
        return None;
    }
    if let Type::Intersection(parts) = pattern {
        let mut source = actual.clone();
        for part in parts {
            source = checking_source(&source, part, aliases, budget)?;
        }
        return Some(source);
    }
    if matches!(pattern, Type::Named {name, ..} if aliases.get(name).is_some_and(|definition| definition.kind == TypeDefinitionKind::Alias))
    {
        let target = expanded(pattern, aliases, &mut HashSet::new(), budget);
        if target != *pattern {
            return checking_source(actual, &target, aliases, budget);
        }
    }
    if let (Type::Record(fields), Type::Record(expected)) = (actual, pattern) {
        let mut fields = fields.clone();
        for target in expected {
            if let PropertyType::Found { value, .. } =
                property_type(actual, &target.name, aliases, &mut HashSet::new(), budget)
            {
                let value = expanded(&value, aliases, &mut HashSet::new(), budget);
                let source = fields.iter_mut().find(|field| field.name == target.name)?;
                source.value = checking_source(&value, &target.value, aliases, budget)?;
            }
        }
        return Some(Type::Record(fields));
    }
    if !matches!(
        pattern,
        Type::Function { .. } | Type::GenericFunction { .. } | Type::CallableRecord { .. }
    ) {
        return Some(actual.clone());
    }
    match actual {
        Type::Intersection(parts) => {
            let parts = parts
                .iter()
                .map(|part| checking_source(part, pattern, aliases, budget))
                .collect::<Option<Vec<_>>>()?;
            Some(Type::Intersection(parts))
        }
        Type::GenericFunction {
            type_parameters,
            parameters,
            result,
            ..
        } => erase(type_parameters, parameters, result, budget),
        Type::CallableRecord { fields, signatures } => {
            let mut signatures = signatures.clone();
            for signature in &mut signatures {
                if signature.type_parameters.is_empty() {
                    continue;
                }
                let Type::Function { parameters, result } = erase(
                    &signature.type_parameters,
                    &signature.parameters,
                    &signature.result,
                    budget,
                )?
                else {
                    return None;
                };
                signature.parameters = parameters;
                signature.result = *result;
                signature.type_parameters.clear();
            }
            Some(Type::CallableRecord {
                fields: fields.clone(),
                signatures,
            })
        }
        _ => Some(actual.clone()),
    }
}

pub(super) fn erase(
    type_parameters: &[TypeParameter],
    parameters: &[Parameter],
    result: &Type,
    budget: &mut TypeExpansionBudget,
) -> Option<Type> {
    let mut substitutions = BTreeMap::new();
    for parameter in type_parameters {
        if !budget.consume() {
            return None;
        }
        let bound = parameter
            .constraint
            .as_ref()
            .map(|bound| substitute_type(bound, &substitutions))
            .unwrap_or(Type::StrictUnknown);
        substitutions.insert(parameter.name.clone(), bound.clone());
        substitutions.insert(crate::parser::type_parameter_identity(parameter), bound);
    }
    Some(substitute_type(
        &Type::Function {
            parameters: parameters.to_vec(),
            result: Box::new(result.clone()),
        },
        &substitutions,
    ))
}

pub(super) fn constructor(
    actual: &Type,
    pattern: &Type,
    bindings: &mut MatchBindings,
    aliases: &BTreeMap<String, TypeDefinition>,
    budget: &mut TypeExpansionBudget,
) -> Option<bool> {
    let Type::CallableRecord { fields, signatures } = pattern else {
        return None;
    };
    let [expected] = signatures.as_slice() else {
        return None;
    };
    if !fields.is_empty() || !expected.construct || !expected.type_parameters.is_empty() {
        return None;
    }
    let pattern = Type::Function {
        parameters: expected.parameters.clone(),
        result: Box::new(expected.result.clone()),
    };
    if *actual == Type::Any {
        return Some(matches(actual, &pattern, bindings, aliases, budget));
    }
    let Type::CallableRecord { signatures, .. } = actual else {
        return Some(false);
    };
    let Some(actual) = signatures
        .iter()
        .rev()
        .find(|signature| signature.construct)
    else {
        return Some(false);
    };
    if (actual.abstract_constructor && !expected.abstract_constructor)
        || actual.constructor_visibility != crate::parser::Visibility::Public
    {
        return Some(false);
    }
    let Some(actual) = erase(
        &actual.type_parameters,
        &actual.parameters,
        &actual.result,
        budget,
    ) else {
        return Some(false);
    };
    Some(matches(&actual, &pattern, bindings, aliases, budget))
}

pub(super) fn any_signature(
    pattern: &Type,
    bindings: &mut MatchBindings,
    aliases: &BTreeMap<String, TypeDefinition>,
    budget: &mut TypeExpansionBudget,
) -> Option<bool> {
    let Type::Function { parameters, result } = pattern else {
        return None;
    };
    let positional = parameters
        .iter()
        .filter(|parameter| parameter.name != "this")
        .collect::<Vec<_>>();
    let [rest] = positional.as_slice() else {
        return None;
    };
    if !rest.rest {
        return None;
    }
    if !super::functions::receiver(&[], parameters, bindings, aliases, budget) {
        return Some(false);
    }
    // Inference of a complete parameter list from any yields unknown[], while
    // the return position retains any. This is signature inference, not a
    // fallback for unresolved names or unknown source values.
    if let Some(Type::Infer(parameter)) = &rest.annotation {
        if !matches(
            &Type::Array(Box::new(Type::StrictUnknown)),
            &Type::Infer(parameter.clone()),
            bindings,
            aliases,
            budget,
        ) {
            return Some(false);
        }
    }
    Some(matches(&Type::Any, result, bindings, aliases, budget))
}
