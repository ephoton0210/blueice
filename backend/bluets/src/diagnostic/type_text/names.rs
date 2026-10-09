// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Names printed from the authorized declaration graph.
use crate::{Declaration, Project};

pub(super) fn display(name: &str, project: Option<&Project>) -> String {
    if name.starts_with("#typeparam@") {
        return crate::parser::source_type_name(name).into();
    }
    let name = crate::parser::source_type_name(name);
    let Some(project) = project else {
        return name.into();
    };
    if let Some(owner) = name
        .strip_prefix("super ")
        .and_then(|owner| owner.split('@').next())
    {
        if let Some(base) = project
            .modules
            .values()
            .flat_map(|module| &module.declarations)
            .find_map(|item| match item {
                Declaration::Class(class) if class.name == owner => class.extends_name.as_ref(),
                _ => None,
            })
        {
            return display(base, Some(project));
        }
    }
    for module in project.modules.values() {
        for item in &module.declarations {
            let Declaration::Import(import) = item else {
                continue;
            };
            if let Some(binding) = import.bindings.iter().find(|binding| binding.local == name) {
                if let Some(resolved) = project
                    .resolved_import(&module.id, import)
                    .and_then(|id| project.modules.get(id))
                {
                    if resolved.declarations.iter().any(|item|matches!(item,Declaration::Class(class) if class.name==binding.imported)) {return binding.imported.clone();}
                }
            }
        }
    }
    if name.ends_with("JSX.Element") {
        return "Element".into();
    }
    if let Some((_, tail)) = name.rsplit_once('.') {
        fn found(items: &[Declaration], name: &str) -> bool {
            items.iter().any(|item| match item {
                Declaration::Class(class) => class.name == name,
                Declaration::Namespace(ns) => ns.name == name || found(&ns.body, name),
                _ => false,
            })
        }
        if project
            .modules
            .values()
            .any(|module| found(&module.declarations, tail))
        {
            return format!(
                "{}{tail}",
                if name.starts_with("typeof ") {
                    "typeof "
                } else {
                    ""
                }
            );
        }
    }
    name.into()
}
