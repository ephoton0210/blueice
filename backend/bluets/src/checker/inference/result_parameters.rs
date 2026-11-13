// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Empty-record restrictions keep an otherwise direct result parameter naked.
use super::*;

pub(super) fn naked(
    value: &Type,
    parameters: &[TypeParameter],
    aliases: &BTreeMap<String, TypeDefinition>,
    active: &mut HashSet<String>,
    budget: &mut TypeExpansionBudget,
) -> Option<String> {
    match value {
        Type::Named { .. } if declared_parameter(value, parameters).is_some() => {
            Some(declared_parameter(value, parameters)?.name.clone())
        }
        Type::Intersection(parts)
            if parts
                .iter()
                .any(|part| matches!(part, Type::Record(fields) if fields.is_empty())) =>
        {
            let mut other = parts
                .iter()
                .filter(|part| !matches!(part, Type::Record(fields) if fields.is_empty()));
            let operand = other.next()?;
            if other.next().is_some() || !budget.consume() {
                return None;
            }
            naked(operand, parameters, aliases, active, budget)
        }
        Type::Named { name, .. }
            if aliases.get(name).is_some_and(|d| {
                d.kind == TypeDefinitionKind::Alias
                    && matches!(d.value, Type::Named { .. } | Type::Intersection(_))
            }) =>
        {
            let value =
                instantiate_named(value, aliases, active, budget, "direct result parameter")?;
            naked(&value, parameters, aliases, active, budget)
        }
        _ => None,
    }
}
