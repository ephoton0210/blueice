// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Import diagnostics print declaration provenance, not an internal query key.
use super::*;
use crate::Declaration;

pub(crate) fn display(value: &Type, project: &crate::Project) -> Option<Type> {
    let Type::Named { name, arguments } = value else {
        return None;
    };
    if let Some((owner, reference)) = project.import_type_reference(name) {
        if reference.value_query {
            return None;
        }
        let origin = project.import_type_declaration(&owner.id, reference)?;
        let arguments = crate::checker::complete_type_arguments(origin.parameters, arguments)?;
        return Some(Type::Named {
            name: origin.name.into(),
            arguments,
        });
    }
    // Preserve ordinary aliases and their lexical names. Only a direct erased
    // import alias adopts the imported declaration's native diagnostic spelling.
    let (name, owner) = name
        .split_once('@')
        .map_or((name.as_str(), None), |(name, owner)| (name, Some(owner)));
    if name.contains('.') && !name.contains('(') {
        for module in project
            .modules
            .values()
            .filter(|module| owner.is_none_or(|owner| module.id == owner))
        {
            if let Some(origin) = project.imported_type_declaration(&module.id, name) {
                return Some(Type::Named {
                    name: origin.name.into(),
                    arguments: crate::checker::complete_type_arguments(
                        origin.parameters,
                        arguments,
                    )?,
                });
            }
        }
    }
    for module in project
        .modules
        .values()
        .filter(|module| owner.is_none_or(|owner| module.id == owner))
    {
        for declaration in &module.declarations {
            let Declaration::TypeAlias(alias) = declaration else {
                continue;
            };
            if alias.name != name {
                continue;
            }
            let substitutions = alias
                .type_parameters
                .iter()
                .zip(arguments)
                .map(|(parameter, value)| (parameter.name.clone(), value.clone()))
                .collect();
            let Type::Named {
                name: target,
                arguments,
            } = crate::checker::substitute_type(&alias.value, &substitutions)
            else {
                continue;
            };
            if target.starts_with("\0import:") || (target.contains('.') && target != name) {
                return display(
                    &Type::Named {
                        name: target,
                        arguments,
                    },
                    project,
                );
            }
        }
    }
    None
}
