// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Resolve declaration origins and fresh literals through owned module edges.
use super::*;

pub(super) fn public_type_name(module: &Module, name: &str) -> Option<String> {
    for declaration in &module.declarations {
        let direct = match declaration {
            Declaration::Interface(item) => item.exported && item.name == name,
            Declaration::TypeAlias(item) => item.exported && item.name == name,
            Declaration::Class(item) => item.exported && item.name == name,
            Declaration::Enum(item) => item.exported && item.name == name,
            Declaration::Namespace(item) => item.exported && item.name == name,
            _ => false,
        };
        if direct {
            return Some(name.to_string());
        }
        if let Declaration::TypeExport(export) = declaration {
            if export.specifier.is_none() {
                for binding in &export.bindings {
                    if binding.local == name {
                        return Some(binding.exported.clone());
                    }
                }
            }
        }
        if let Declaration::ValueExport(export) = declaration {
            if let Some(binding) = export.bindings.iter().find(|binding| binding.local == name) {
                return Some(binding.exported.clone());
            }
        }
    }
    None
}

pub(super) fn exported_local(module: &Module, exported: &str) -> String {
    for declaration in &module.declarations {
        match declaration {
            Declaration::DefaultExport(item) if exported == "default" => return item.name.clone(),
            Declaration::Function(item) if item.default_export && exported == "default" => {
                return item.name.clone()
            }
            Declaration::ValueExport(item) => {
                if let Some(binding) = item
                    .bindings
                    .iter()
                    .find(|binding| binding.exported == exported)
                {
                    return binding.local.clone();
                }
            }
            _ => {}
        }
    }
    exported.to_string()
}

pub(super) fn declared_function(
    module: &Module,
    name: &str,
    project: &Project,
    seen: &mut BTreeSet<(String, String)>,
    depth: usize,
) -> bool {
    if depth >= 128 || !seen.insert((module.id.clone(), name.to_string())) {
        return false;
    }
    for declaration in &module.declarations {
        match declaration {
            Declaration::Function(function) if function.name == name => return true,
            Declaration::Variable(variable)
                if variable.name == name && variable.annotation.is_none() =>
            {
                if let [source] = variable.initializer.as_slice() {
                    return declared_function(module, &source.text, project, seen, depth + 1);
                }
            }
            Declaration::Import(import) => {
                if let Some(binding) = import.bindings.iter().find(|binding| binding.local == name)
                {
                    if let Some(source) = project
                        .resolved_import(&module.id, import)
                        .and_then(|id| project.modules.get(id))
                    {
                        return declared_function(
                            source,
                            &exported_local(source, &binding.imported),
                            project,
                            seen,
                            depth + 1,
                        );
                    }
                }
            }
            _ => {}
        }
    }
    false
}

pub(super) fn fresh_variable(
    module: &Module,
    variable: &VariableDeclaration,
    project: &Project,
    seen: &mut BTreeSet<(String, String)>,
    depth: usize,
) -> bool {
    if depth >= 128
        || variable.annotation.is_some()
        || variable.kind != VariableKind::Const
        || !seen.insert((module.id.clone(), variable.name.clone()))
    {
        return false;
    }
    let [value] = variable.initializer.as_slice() else {
        return false;
    };
    if matches!(
        value.kind,
        crate::syntax::TokenKind::Number
            | crate::syntax::TokenKind::String
            | crate::syntax::TokenKind::Template
    ) || value.is("true")
        || value.is("false")
    {
        return true;
    }
    for declaration in &module.declarations {
        if let Declaration::Variable(local) = declaration {
            if local.name == value.text {
                return fresh_variable(module, local, project, seen, depth + 1);
            }
        }
        if let Declaration::Import(import) = declaration {
            if let Some(binding) = import
                .bindings
                .iter()
                .find(|binding| binding.local == value.text)
            {
                let source = project
                    .resolved_import(&module.id, import)
                    .and_then(|id| project.modules.get(id));
                if let Some(source) = source {
                    let local = exported_local(source, &binding.imported);
                    if let Some(variable) =
                        source
                            .declarations
                            .iter()
                            .find_map(|declaration| match declaration {
                                Declaration::Variable(variable) if variable.name == local => {
                                    Some(variable)
                                }
                                _ => None,
                            })
                    {
                        return fresh_variable(source, variable, project, seen, depth + 1);
                    }
                }
            }
        }
    }
    false
}

pub(super) fn declares_symbol_value(module: &Module) -> bool {
    module
        .declarations
        .iter()
        .any(|declaration| match declaration {
            Declaration::Variable(value) => value.name == "Symbol",
            Declaration::Function(value) => value.name == "Symbol",
            Declaration::Class(value) => value.name == "Symbol",
            Declaration::Enum(value) => value.name == "Symbol",
            Declaration::Namespace(value) => value.name == "Symbol",
            Declaration::Import(value) => value
                .bindings
                .iter()
                .any(|binding| binding.local == "Symbol"),
            _ => false,
        })
}

pub(super) fn record_annotation(value: &Type) -> bool {
    match value {
        Type::Record(_) | Type::Mapped(_) => true,
        Type::Named { arguments, .. } => arguments.iter().any(record_annotation),
        Type::Predicate(predicate) => predicate.target.as_deref().is_some_and(record_annotation),
        Type::Union(parts) | Type::Intersection(parts) => parts.iter().any(record_annotation),
        _ => false,
    }
}
