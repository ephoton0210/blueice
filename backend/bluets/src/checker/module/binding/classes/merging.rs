// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Interface heritage contributes erased instance members to a merged class.

use super::*;

pub(in crate::checker) fn class_with_interface_heritage(
    class: &ClassDeclaration,
    module: &Module,
    definitions: &BTreeMap<String, TypeDefinition>,
    max_expansions: usize,
) -> Option<ClassDeclaration> {
    let mut result = class.clone();
    let mut budget = TypeExpansionBudget::new(max_expansions);
    for declaration in &module.declarations {
        let Declaration::Interface(interface) = declaration else {
            continue;
        };
        if interface.name != class.name || !interface.type_parameters.is_empty() {
            continue;
        }
        for parent in &interface.heritage {
            let mut inherited = Vec::new();
            fields(
                parent,
                definitions,
                &mut HashSet::new(),
                &mut budget,
                &mut inherited,
            )?;
            for field in inherited {
                if !result
                    .merged_interface_fields
                    .iter()
                    .any(|own| own.name == field.name)
                {
                    result.merged_interface_fields.push(field);
                }
            }
        }
    }
    Some(result)
}

fn fields(
    value: &Type,
    definitions: &BTreeMap<String, TypeDefinition>,
    visited: &mut HashSet<String>,
    budget: &mut TypeExpansionBudget,
    result: &mut Vec<TypeField>,
) -> Option<()> {
    if !budget.consume() {
        return None;
    }
    match value {
        Type::Record(members) => result.extend(members.iter().cloned()),
        Type::Intersection(parts) => {
            for part in parts {
                fields(part, definitions, &mut visited.clone(), budget, result)?;
            }
        }
        Type::Named { .. } => {
            let expanded =
                instantiate_named(value, definitions, visited, budget, "merged interface")?;
            fields(&expanded, definitions, visited, budget, result)?;
        }
        _ => return None,
    }
    Some(())
}
