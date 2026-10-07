// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Bound parameters remain rigid while declared variance directs instantiations.
use super::*;
use crate::parser::Variance;

pub(super) fn assignable(
    actual: &Type,
    expected: &Type,
    aliases: &BTreeMap<String, TypeDefinition>,
    visited: &mut HashSet<String>,
    budget: &mut TypeExpansionBudget,
) -> Option<bool> {
    if actual == expected {
        return Some(true);
    }
    if let Type::Named { name, .. } = actual {
        if let Some(definition) = aliases
            .get(name)
            .filter(|definition| definition.kind == TypeDefinitionKind::Parameter)
        {
            if !visited.insert(format!("parameter:{name}:{}", type_identity(expected)))
                || !budget.consume()
            {
                return Some(false);
            }
            return Some(is_assignable(
                &definition.value,
                expected,
                aliases,
                visited,
                budget,
            ));
        }
    }
    if let Type::Named { name, .. } = expected {
        if aliases
            .get(name)
            .is_some_and(|definition| definition.kind == TypeDefinitionKind::Parameter)
            && !matches!(actual, Type::Any | Type::Never)
        {
            return Some(false);
        }
    }
    let (
        Type::Named {
            name: a,
            arguments: aa,
        },
        Type::Named {
            name: e,
            arguments: ea,
        },
    ) = (actual, expected)
    else {
        return None;
    };
    if a != e {
        return None;
    }
    let definition = aliases.get(a)?;
    if !definition
        .parameters
        .iter()
        .any(|parameter| parameter.variance.is_some())
    {
        return None;
    }
    let actual = complete_type_arguments(&definition.parameters, aa)?;
    let expected = complete_type_arguments(&definition.parameters, ea)?;
    Some(
        definition
            .parameters
            .iter()
            .zip(actual.iter().zip(&expected))
            .all(|(parameter, (a, e))| match parameter.variance {
                Some(Variance::Out) => is_assignable(a, e, aliases, &mut visited.clone(), budget),
                Some(Variance::In) => is_assignable(e, a, aliases, &mut visited.clone(), budget),
                Some(Variance::InOut) => {
                    is_assignable(a, e, aliases, &mut visited.clone(), budget)
                        && is_assignable(e, a, aliases, &mut visited.clone(), budget)
                }
                None => is_assignable(a, e, aliases, &mut visited.clone(), budget),
            }),
    )
}
