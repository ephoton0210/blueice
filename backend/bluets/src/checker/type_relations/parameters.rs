// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Preserve lexical parameter identities before another binder can shadow them.

use super::*;

pub(super) fn compatible(
    actual: &[Parameter],
    expected: &[Parameter],
    aliases: &BTreeMap<String, TypeDefinition>,
    visited: &HashSet<String>,
    budget: &mut TypeExpansionBudget,
) -> bool {
    let receiver = |parameters: &[Parameter]| {
        parameters
            .iter()
            .find(|parameter| parameter.name == "this")
            .and_then(|parameter| parameter.annotation.clone())
    };
    if let (Some(actual), Some(expected)) = (receiver(actual), receiver(expected)) {
        if !is_assignable(&expected, &actual, aliases, &mut visited.clone(), budget) {
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
