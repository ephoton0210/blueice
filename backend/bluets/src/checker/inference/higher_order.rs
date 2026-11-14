// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Scoped callable parameters survive only non-overlapping inference proposals.

use super::*;

struct Callable {
    signature: FunctionSignature,
    shape: Type,
    construct: bool,
    single: bool,
    generic: bool,
}

fn callable(
    value: &Type,
    aliases: &BTreeMap<String, TypeDefinition>,
    budget: &mut TypeExpansionBudget,
    formal: bool,
) -> Option<Callable> {
    let mut value = value.clone();
    let mut active = HashSet::new();
    loop {
        if !budget.consume() {
            return None;
        }
        match value {
            Type::Function { parameters, result } => {
                return Some(Callable {
                    signature: FunctionSignature {
                        type_parameters: Vec::new(),
                        parameters: parameters.clone(),
                        return_type: (*result).clone(),
                    },
                    shape: Type::Function { parameters, result },
                    construct: false,
                    single: true,
                    generic: false,
                });
            }
            Type::GenericFunction {
                type_parameters,
                parameters,
                result,
                ..
            } if !formal => {
                return Some(Callable {
                    generic: !type_parameters.is_empty(),
                    signature: FunctionSignature {
                        type_parameters,
                        parameters: parameters.clone(),
                        return_type: (*result).clone(),
                    },
                    shape: Type::Function { parameters, result },
                    construct: false,
                    single: true,
                });
            }
            Type::CallableRecord { fields, signatures } => {
                if formal && (!fields.is_empty() || signatures.len() != 1) {
                    return None;
                }
                let mut selected = signatures.last()?.clone();
                if formal && !selected.type_parameters.is_empty() {
                    return None;
                }
                let signature = FunctionSignature {
                    type_parameters: selected.type_parameters.clone(),
                    parameters: selected.parameters.clone(),
                    return_type: selected.result.clone(),
                };
                let construct = selected.construct;
                selected.type_parameters.clear();
                let shape = if construct {
                    Type::CallableRecord {
                        fields: Vec::new(),
                        signatures: vec![selected],
                    }
                } else {
                    Type::Function {
                        parameters: selected.parameters,
                        result: Box::new(selected.result),
                    }
                };
                return Some(Callable {
                    signature,
                    shape,
                    construct,
                    single: signatures.len() == 1,
                    generic: signatures
                        .iter()
                        .any(|signature| !signature.type_parameters.is_empty()),
                });
            }
            Type::Intersection(parts) => {
                let mut callables = Vec::new();
                for part in parts {
                    if let Some(found) = callable(&part, aliases, budget, formal) {
                        callables.push(found);
                    } else if formal {
                        return None;
                    }
                }
                if formal && callables.len() != 1 {
                    return None;
                }
                let generic = callables.iter().any(|value| value.generic);
                let single = callables.len() == 1 && callables[0].single;
                let mut selected = callables.pop()?;
                selected.generic = generic;
                selected.single = single;
                return Some(selected);
            }
            Type::Named { ref name, .. }
                if aliases.get(name).is_some_and(|definition| {
                    matches!(
                        definition.kind,
                        TypeDefinitionKind::Alias
                            | TypeDefinitionKind::Interface
                            | TypeDefinitionKind::LibraryInterface
                    )
                }) =>
            {
                value = instantiate_named(
                    &value,
                    aliases,
                    &mut active,
                    budget,
                    "higher-order callable",
                )?;
            }
            _ => return None,
        }
    }
}

fn requires_signature_context(value: &Type, aliases: &BTreeMap<String, TypeDefinition>) -> bool {
    match value {
        Type::GenericFunction {
            type_parameters, ..
        } => !type_parameters.is_empty(),
        Type::CallableRecord { signatures, .. } => {
            signatures.len() > 1
                || signatures
                    .iter()
                    .any(|signature| !signature.type_parameters.is_empty())
        }
        Type::Intersection(_) => true,
        Type::Named { name, .. } => aliases.get(name).is_some_and(|definition| {
            matches!(
                definition.kind,
                TypeDefinitionKind::Alias | TypeDefinitionKind::Interface
            )
        }),
        _ => false,
    }
}

fn references(parameters: &[TypeParameter]) -> BTreeMap<String, Type> {
    parameters
        .iter()
        .flat_map(|parameter| {
            let identity = crate::parser::type_parameter_identity(parameter);
            let value = Type::Named {
                name: identity.clone(),
                arguments: Vec::new(),
            };
            [(parameter.name.clone(), value.clone()), (identity, value)]
        })
        .collect()
}

fn insert_parameters(
    aliases: &mut BTreeMap<String, TypeDefinition>,
    parameters: &[TypeParameter],
    budget: &mut TypeExpansionBudget,
) -> Option<()> {
    let bindings = references(parameters);
    for parameter in parameters {
        if !budget.consume() {
            return None;
        }
        aliases.insert(
            crate::parser::type_parameter_identity(parameter),
            TypeDefinition {
                kind: TypeDefinitionKind::Parameter,
                parameters: Vec::new(),
                value: parameter
                    .constraint
                    .as_ref()
                    .map(|value| substitute_type(value, &bindings))
                    .unwrap_or(Type::StrictUnknown),
            },
        );
    }
    Some(())
}

pub(in crate::checker) fn parameter_scope(
    aliases: &BTreeMap<String, TypeDefinition>,
    parameters: &[TypeParameter],
    budget: &mut TypeExpansionBudget,
) -> Option<BTreeMap<String, TypeDefinition>> {
    let mut scoped = aliases.clone();
    insert_parameters(&mut scoped, parameters, budget)?;
    Some(scoped)
}

fn proposal_slots(
    formal: &FunctionSignature,
    source: &FunctionSignature,
    parameters: &BTreeSet<String>,
    identities: &BTreeMap<String, String>,
    aliases: &BTreeMap<String, TypeDefinition>,
    budget: &mut TypeExpansionBudget,
) -> BTreeSet<String> {
    let mut collector = collect::Collector {
        parameters,
        identities,
        aliases,
        budget,
        candidates: BTreeMap::new(),
        active: HashSet::new(),
        dependencies: BTreeMap::new(),
        evidence: 0,
    };
    collector.collect_parameters(&formal.parameters, &source.parameters, false);
    if collector.evidence == 0 {
        return BTreeSet::new();
    }
    collector.collect(&formal.return_type, &source.return_type, false);
    collector.candidates.into_keys().collect()
}

pub(super) fn infer(
    signature: &FunctionSignature,
    actuals: &[Type],
    aliases: &BTreeMap<String, TypeDefinition>,
    budget: &mut TypeExpansionBudget,
    context: Option<&Type>,
) -> Option<InferenceResult> {
    let mut pairs = Vec::new();
    for (index, actual) in actuals.iter().enumerate() {
        if !requires_signature_context(actual, aliases) {
            continue;
        }
        let Some(annotation) = function_parameter_for_argument(signature, index)
            .and_then(|parameter| parameter.annotation.as_ref())
        else {
            continue;
        };
        if !matches!(
            annotation,
            Type::Function { .. } | Type::CallableRecord { .. } | Type::Named { .. }
        ) {
            continue;
        }
        let Some(formal) = callable(annotation, aliases, budget, true) else {
            continue;
        };
        let Some(source) = callable(actual, aliases, budget, false) else {
            continue;
        };
        if (source.generic || !source.single) && source.construct == formal.construct {
            pairs.push((index, formal, source));
        }
    }
    if pairs.is_empty() {
        return None;
    }
    let eligible = callable(&signature.return_type, aliases, budget, true).is_some();
    let (parameters, identities) = candidate_parameters(signature);
    let independent_mismatch = actuals.iter().enumerate().any(|(index, actual)| {
        if pairs.iter().any(|(slot, _, _)| *slot == index) || *actual == Type::Unknown {
            return false;
        }
        let Some(annotation) = function_parameter_for_argument(signature, index)
            .and_then(|parameter| parameter.annotation.as_ref())
        else {
            return false;
        };
        let checked = checking_type(annotation, aliases, budget);
        !dependencies::contains(&checked, &parameters, aliases, &mut HashSet::new(), budget)
            && !is_assignable(actual, &checked, aliases, &mut HashSet::new(), budget)
    });
    let mut normalized = actuals.to_vec();
    for (index, _, _) in &pairs {
        normalized[*index] = Type::Unknown;
    }
    let mut result = infer_contextual_result(signature, &normalized, aliases, budget, context);
    let mut scoped = aliases.clone();
    let mut propagated = Vec::new();
    for (index, formal, source) in pairs {
        if budget.exhausted {
            break;
        }
        let fresh = references(&source.signature.type_parameters);
        let fresh_shape = substitute_type(&source.shape, &fresh);
        let fresh_signature = callable(&fresh_shape, &scoped, budget, false)?;
        let proposed = proposal_slots(
            &formal.signature,
            &fresh_signature.signature,
            &parameters,
            &identities,
            &scoped,
            budget,
        );
        let propagate = eligible
            && !independent_mismatch
            && source.single
            && !proposed.is_empty()
            && proposed.is_disjoint(&result.inferred_parameters);
        let mut known = result.substitutions.clone();
        for parameter in &signature.type_parameters {
            if !result.inferred_parameters.contains(&parameter.name) {
                known.insert(parameter.name.clone(), Type::Unknown);
                known.insert(
                    crate::parser::type_parameter_identity(parameter),
                    Type::Unknown,
                );
            }
        }
        let target_shape = substitute_type(&formal.shape, &known);
        let target = callable(&target_shape, &scoped, budget, true)?;
        let (bindings, mut retained) = if propagate {
            (fresh, source.signature.type_parameters.clone())
        } else {
            let inner =
                infer_signature_context(&source.signature, &target.signature, &scoped, budget);
            (inner.substitutions, Vec::new())
        };
        for parameter in &mut retained {
            parameter.constraint = parameter
                .constraint
                .as_ref()
                .map(|value| substitute_type(value, &bindings));
            parameter.default = parameter
                .default
                .as_ref()
                .map(|value| substitute_type(value, &bindings));
        }
        insert_parameters(&mut scoped, &retained, budget)?;
        normalized[index] = substitute_type(&source.shape, &bindings);
        propagated.extend(retained);
        result = infer_contextual_result(signature, &normalized, &scoped, budget, context);
    }
    result.propagated_parameters = propagated;
    Some(result)
}

fn optional_parameters(parameters: &mut [Parameter], budget: &mut TypeExpansionBudget) {
    for parameter in parameters {
        if !budget.consume() {
            return;
        }
        if parameter.optional && budget.checking.strict_null_checks {
            if let Some(annotation) = &parameter.annotation {
                parameter.annotation = Some(super::super::type_operators::union([
                    annotation.clone(),
                    Type::Undefined,
                ]));
            }
        }
    }
}

pub(in crate::checker) fn bind_result(
    value: Type,
    inferred: &InferenceResult,
    budget: &mut TypeExpansionBudget,
    origin: Option<&SourceSpan>,
) -> Type {
    if inferred.propagated_parameters.is_empty() {
        return value;
    }
    match value {
        Type::Function {
            mut parameters,
            result,
        } => {
            optional_parameters(&mut parameters, budget);
            Type::GenericFunction {
                type_parameters: inferred.propagated_parameters.clone(),
                parameters,
                result,
                span: origin.cloned().unwrap_or_else(|| SourceSpan::new("", 0, 0)),
            }
        }
        Type::CallableRecord {
            fields,
            mut signatures,
        } if fields.is_empty() && signatures.len() == 1 => {
            signatures[0].type_parameters = inferred.propagated_parameters.clone();
            optional_parameters(&mut signatures[0].parameters, budget);
            Type::CallableRecord { fields, signatures }
        }
        value => value,
    }
}
