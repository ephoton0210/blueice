// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Inspect type operations without confusing lexical parameters with aliases.

use super::*;

pub(super) fn contains(
    value: &Type,
    aliases: &BTreeMap<String, TypeDefinition>,
    budget: &mut TypeExpansionBudget,
) -> bool {
    visit(value, aliases, budget, &mut HashSet::new(), &HashSet::new())
}

fn visit(
    value: &Type,
    aliases: &BTreeMap<String, TypeDefinition>,
    budget: &mut TypeExpansionBudget,
    visited: &mut HashSet<String>,
    bound: &HashSet<String>,
) -> bool {
    if !budget.consume() {
        return false;
    }
    if matches!(value, Type::InferenceBarrier(_)) {
        return true;
    }
    let mut found = false;
    value.map_children(|child, bindings| {
        if !found {
            let mut bound = bound.clone();
            bound.extend(bindings.iter().cloned());
            found = visit(child, aliases, budget, visited, &bound);
        }
        child.clone()
    });
    if found {
        return true;
    }
    let Type::Named { name, .. } = value else {
        return false;
    };
    if bound.contains(name) || !visited.insert(name.clone()) {
        return false;
    }
    let Some(definition) = aliases
        .get(name)
        .filter(|definition| definition.kind == TypeDefinitionKind::Alias)
    else {
        return false;
    };
    let mut bound = bound.clone();
    for parameter in &definition.parameters {
        bound.insert(parameter.name.clone());
        bound.insert(crate::parser::type_parameter_identity(parameter));
    }
    visit(&definition.value, aliases, budget, visited, &bound)
}
