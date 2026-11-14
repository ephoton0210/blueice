// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Preserve lexical parameter identities before another binder can shadow them.

use super::*;

pub(super) fn generic_constraints_compatible(
    actual: &[TypeParameter],
    expected: &[TypeParameter],
    aliases: &BTreeMap<String, TypeDefinition>,
    visited: &HashSet<String>,
    budget: &mut TypeExpansionBudget,
) -> bool {
    if actual.len() != expected.len() {
        return true;
    }
    let binders = actual
        .iter()
        .zip(expected)
        .map(|(source, target)| {
            (
                source.name.clone(),
                Type::Named {
                    name: target.name.clone(),
                    arguments: Vec::new(),
                },
            )
        })
        .collect();
    actual.iter().zip(expected).all(|(source, target)| {
        let Some(constraint) = &source.constraint else {
            return true;
        };
        let source =
            canonical_parameter_references(&substitute_type(constraint, &binders), aliases);
        let target = canonical_parameter_references(
            target.constraint.as_ref().unwrap_or(&Type::StrictUnknown),
            aliases,
        );
        is_assignable(&target, &source, aliases, &mut visited.clone(), budget)
    })
}

/// Alpha-renamed signatures retain the target binder's constraints. The
/// temporary identities belong to that declaration, so nested generic
/// signatures cannot capture a shared textual placeholder.
pub(super) fn generic_functions_assignable(
    actual: &Type,
    expected: &Type,
    aliases: &BTreeMap<String, TypeDefinition>,
    visited: &mut HashSet<String>,
    budget: &mut TypeExpansionBudget,
) -> Option<bool> {
    let function =
        |value: &Type| matches!(value, Type::Function { .. } | Type::GenericFunction { .. });
    if !function(actual)
        || !function(expected)
        || !matches!(actual, Type::GenericFunction { .. })
            && !matches!(expected, Type::GenericFunction { .. })
    {
        return None;
    }
    if let Type::GenericFunction {
        type_parameters,
        parameters,
        result,
        ..
    } = actual
    {
        if type_parameters.is_empty() {
            let source = Type::Function {
                parameters: parameters.clone(),
                result: result.clone(),
            };
            return Some(is_assignable(&source, expected, aliases, visited, budget));
        }
    }
    if let (
        Type::GenericFunction {
            type_parameters,
            parameters,
            result,
            ..
        },
        Type::Function {
            parameters: target,
            result: target_result,
        },
    ) = (actual, expected)
    {
        if let Some(actuals) = target
            .iter()
            .map(|parameter| parameter.annotation.clone())
            .collect::<Option<Vec<_>>>()
        {
            let signature = FunctionSignature {
                type_parameters: type_parameters.clone(),
                parameters: parameters.clone(),
                return_type: (**result).clone(),
            };
            let inferred = crate::checker::inference::infer_contextual_result(
                &signature,
                &actuals,
                aliases,
                budget,
                (**target_result != Type::Void).then_some(&**target_result),
            );
            if inferred.rejected_constraint.is_some() || budget.exhausted {
                return Some(false);
            }
            let source = substitute_type(
                &Type::Function {
                    parameters: parameters.clone(),
                    result: result.clone(),
                },
                &inferred.substitutions,
            );
            return Some(is_assignable(&source, expected, aliases, visited, budget));
        }
    }
    let paired = if let (
        Type::GenericFunction {
            type_parameters: source,
            span: source_span,
            ..
        },
        Type::GenericFunction {
            type_parameters: target,
            span: target_span,
            ..
        },
    ) = (actual, expected)
    {
        let same_declaration = source_span == target_span
            && !source_span.module.is_empty()
            && source_span.start < source_span.end;
        if !same_declaration
            && !generic_constraints_compatible(source, target, aliases, visited, budget)
        {
            return Some(false);
        }
        Some(target.as_slice())
    } else {
        None
    };
    let substitutions = |parameters: &[TypeParameter]| {
        parameters
            .iter()
            .enumerate()
            .flat_map(|(index, parameter)| {
                let name = paired
                    .and_then(|target| target.get(index))
                    .map(crate::parser::type_parameter_identity)
                    .unwrap_or_else(|| format!("<generic:{index}>"));
                let value = Type::Named {
                    name,
                    arguments: Vec::new(),
                };
                [
                    (parameter.name.clone(), value.clone()),
                    (crate::parser::type_parameter_identity(parameter), value),
                ]
            })
            .collect::<BTreeMap<_, _>>()
    };
    let normalize = |value: &Type| match value {
        Type::GenericFunction {
            type_parameters,
            parameters,
            result,
            ..
        } => substitute_type(
            &Type::Function {
                parameters: parameters.clone(),
                result: result.clone(),
            },
            &substitutions(type_parameters),
        ),
        value => value.clone(),
    };
    let mut scoped = None;
    if let Some(parameters) = paired {
        let mut definitions = aliases.clone();
        let substitutions = substitutions(parameters);
        for parameter in parameters {
            if !budget.consume() {
                return Some(false);
            }
            let value = parameter
                .constraint
                .as_ref()
                .map(|value| {
                    canonical_parameter_references(&substitute_type(value, &substitutions), aliases)
                })
                .unwrap_or(Type::StrictUnknown);
            definitions.insert(
                crate::parser::type_parameter_identity(parameter),
                TypeDefinition {
                    kind: TypeDefinitionKind::Parameter,
                    parameters: Vec::new(),
                    value,
                },
            );
        }
        scoped = Some(definitions);
    }
    Some(is_assignable(
        &normalize(actual),
        &normalize(expected),
        scoped.as_ref().unwrap_or(aliases),
        visited,
        budget,
    ))
}

pub(super) fn compatible(
    actual: &[Parameter],
    expected: &[Parameter],
    aliases: &BTreeMap<String, TypeDefinition>,
    visited: &HashSet<String>,
    budget: &mut TypeExpansionBudget,
) -> bool {
    let actual = expanded(actual, aliases, budget);
    let expected = expanded(expected, aliases, budget);
    let receiver = |parameters: &[Parameter]| {
        parameters
            .iter()
            .find(|parameter| parameter.name == "this")
            .and_then(|parameter| parameter.annotation.clone())
    };
    if let (Some(actual), Some(expected)) = (receiver(&actual), receiver(&expected)) {
        if !is_assignable(&expected, &actual, aliases, &mut visited.clone(), budget)
            && (budget.checking.strict_function_types
                || !is_assignable(&actual, &expected, aliases, &mut visited.clone(), budget))
        {
            return false;
        }
    }
    let actual = actual
        .iter()
        .filter(|p| p.name != "this")
        .collect::<Vec<_>>();
    let expected = expected
        .iter()
        .filter(|p| p.name != "this")
        .collect::<Vec<_>>();
    let required = actual
        .iter()
        .filter(|p| !p.optional && p.default.is_none() && !p.rest)
        .count();
    if required > expected.len() && !expected.last().is_some_and(|p| p.rest) {
        return false;
    }
    let checking = budget.checking;
    let value = |parameter: &Parameter| {
        let mut value = parameter.annotation.clone().unwrap_or(Type::Unknown);
        if parameter.rest {
            if let Type::Array(element) = value {
                value = *element;
            }
        }
        if parameter.optional && checking.strict_null_checks {
            union([value, Type::Undefined])
        } else {
            value
        }
    };
    let mut actual_index = 0;
    let mut expected_index = 0;
    for _ in 0..actual.len() + expected.len() + 1 {
        let (Some(actual), Some(expected)) =
            (actual.get(actual_index), expected.get(expected_index))
        else {
            break;
        };
        let actual_type = value(actual);
        let expected_type = value(expected);
        if !is_assignable(
            &expected_type,
            &actual_type,
            aliases,
            &mut visited.clone(),
            budget,
        ) && (budget.checking.strict_function_types
            || !is_assignable(
                &actual_type,
                &expected_type,
                aliases,
                &mut visited.clone(),
                budget,
            ))
        {
            return false;
        }
        if actual.rest && expected.rest {
            break;
        }
        actual_index += usize::from(!actual.rest);
        expected_index += usize::from(!expected.rest);
    }
    true
}

fn expanded(
    parameters: &[Parameter],
    aliases: &BTreeMap<String, TypeDefinition>,
    budget: &mut TypeExpansionBudget,
) -> Vec<Parameter> {
    let mut result = Vec::new();
    for parameter in parameters {
        let mut annotation = parameter.annotation.clone().unwrap_or(Type::Unknown);
        if parameter.rest {
            let mut visited = HashSet::new();
            loop {
                // A quantified tuple tail denotes the caller's exact tuple,
                // not its array constraint. Preserve it on both sides while
                // expanding surrounding aliases and concrete tuple prefixes.
                if matches!(&annotation, Type::Named { name, .. } if aliases.get(name)
                    .is_some_and(|definition| definition.kind == TypeDefinitionKind::Parameter))
                {
                    break;
                }
                let Some(value) =
                    instantiate_named(&annotation, aliases, &mut visited, budget, "rest relation")
                else {
                    break;
                };
                annotation = value;
            }
            if let Type::Tuple(values) = annotation {
                for value in values {
                    let mut parameter = parameter.clone();
                    parameter.annotation = Some(value.annotation);
                    parameter.optional = value.optional;
                    parameter.rest = value.rest;
                    result.push(parameter);
                }
                continue;
            }
        }
        let mut parameter = parameter.clone();
        parameter.annotation = Some(annotation);
        result.push(parameter);
    }
    result
}

pub(in crate::checker) fn canonical_parameter_references(
    value: &Type,
    definitions: &BTreeMap<String, TypeDefinition>,
) -> Type {
    let substitutions = definitions
        .iter()
        .filter_map(|(name, definition)| {
            if definition.kind != TypeDefinitionKind::Alias || !definition.parameters.is_empty() {
                return None;
            }
            let Type::Named {
                name: target,
                arguments,
            } = &definition.value
            else {
                return None;
            };
            (arguments.is_empty()
                && definitions
                    .get(target)
                    .is_some_and(|definition| definition.kind == TypeDefinitionKind::Parameter))
            .then(|| (name.clone(), definition.value.clone()))
        })
        .collect();
    substitute_type(value, &substitutions)
}
