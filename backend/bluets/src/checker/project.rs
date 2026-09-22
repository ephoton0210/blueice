// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Project-wide static type-surface collection.

use super::*;

pub(super) fn declaration_module_diagnostics(project: &Project) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for (module_id, module) in &project.modules {
        for declaration in &module.declarations {
            if let Declaration::Import(import) = declaration {
                if !import.type_only
                    && project
                        .resolutions
                        .get(&(module_id.clone(), import.specifier.clone()))
                        .is_some_and(|resolved| is_declaration_module(resolved))
                {
                    diagnostics.push(Diagnostic::error(
                        DiagnosticCode::InvalidDeclarationFile,
                        import.span.clone(),
                        format!(
                            "value import `{}` resolves to declaration module; declaration modules are type-only",
                            import.specifier
                        ),
                    ));
                }
            }
        }
        if !is_declaration_module(module_id) {
            continue;
        }
        for declaration in &module.declarations {
            let runtime_declaration = match declaration {
                Declaration::Import(import) => !import.type_only,
                Declaration::Variable(variable) => !variable.declared,
                Declaration::Function(function) => !function.declared && !function.overload,
                Declaration::Raw(_) => true,
                Declaration::DefaultExport(_)
                | Declaration::ValueExport(_)
                | Declaration::TypeExport(_)
                | Declaration::TypeAlias(_)
                | Declaration::Interface(_) => false,
            };
            if runtime_declaration {
                diagnostics.push(Diagnostic::error(
                    DiagnosticCode::InvalidDeclarationFile,
                    declaration.span().clone(),
                    "declaration module contains a runtime declaration",
                ));
            }
        }
    }
    diagnostics
}

pub(super) fn exported_types(
    project: &Project,
) -> BTreeMap<String, BTreeMap<String, TypeDefinition>> {
    let mut modules = BTreeMap::new();
    for (id, module) in &project.modules {
        // An exported interface can inherit a private, local parent. Its
        // exported definition must therefore carry the inherited shape rather
        // than make consumers resolve an unimportable implementation detail.
        let declared = local_type_definitions(module);
        let mut values = BTreeMap::new();
        for declaration in &module.declarations {
            match declaration {
                Declaration::TypeAlias(alias) if alias.exported => {
                    values.insert(
                        alias.name.clone(),
                        TypeDefinition {
                            kind: TypeDefinitionKind::Alias,
                            parameters: alias.type_parameters.clone(),
                            value: alias.value.clone(),
                        },
                    );
                }
                Declaration::Interface(interface) if interface.exported => {
                    values.insert(
                        interface.name.clone(),
                        TypeDefinition {
                            kind: TypeDefinitionKind::Interface,
                            parameters: interface.type_parameters.clone(),
                            value: exported_interface_value(interface, &declared),
                        },
                    );
                }
                _ => {}
            }
        }
        modules.insert(id.clone(), values);
    }
    // Type-only re-exports are static edges, but they still contribute to the
    // public type surface consumed by another module.  Resolve this small
    // fixed point without executing module code; the graph is already closed
    // and bounded by the project resolver.
    for _ in 0..project.modules.len() {
        let mut changed = false;
        for (module_id, module) in &project.modules {
            let mut additions = BTreeMap::new();
            for declaration in &module.declarations {
                let Declaration::TypeExport(export) = declaration else {
                    continue;
                };
                let Some(specifier) = &export.specifier else {
                    continue;
                };
                let Some(source_id) = project
                    .resolutions
                    .get(&(module_id.clone(), specifier.clone()))
                else {
                    continue;
                };
                let Some(source_types) = modules.get(source_id) else {
                    continue;
                };
                for binding in &export.bindings {
                    if binding == "*" {
                        additions.extend(source_types.clone());
                        continue;
                    }
                    let (local, exported) = binding
                        .split_once(" as ")
                        .map_or((binding.as_str(), binding.as_str()), |(local, exported)| {
                            (local, exported)
                        });
                    if let Some(value) = source_types.get(local) {
                        additions.insert(exported.to_string(), value.clone());
                    }
                }
            }
            let target = modules.entry(module_id.clone()).or_default();
            for (name, value) in additions {
                if target.get(&name) != Some(&value) {
                    target.insert(name, value);
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }
    modules
}

pub(super) fn local_type_definitions(module: &Module) -> BTreeMap<String, TypeDefinition> {
    module
        .declarations
        .iter()
        .filter_map(|declaration| match declaration {
            Declaration::TypeAlias(alias) => Some((
                alias.name.clone(),
                TypeDefinition {
                    kind: TypeDefinitionKind::Alias,
                    parameters: alias.type_parameters.clone(),
                    value: alias.value.clone(),
                },
            )),
            Declaration::Interface(interface) => Some((
                interface.name.clone(),
                TypeDefinition {
                    kind: TypeDefinitionKind::Interface,
                    parameters: interface.type_parameters.clone(),
                    value: interface_value(interface),
                },
            )),
            _ => None,
        })
        .collect()
}

pub(super) fn exported_interface_value(
    interface: &InterfaceDeclaration,
    declared: &BTreeMap<String, TypeDefinition>,
) -> Type {
    let mut active = HashSet::new();
    let heritage = interface
        .heritage
        .iter()
        .map(|parent| expand_exported_heritage(parent, declared, &mut active))
        .collect::<Vec<_>>();
    interface_value_with_heritage(&heritage, &interface.fields)
}

pub(super) fn expand_exported_heritage(
    value: &Type,
    declared: &BTreeMap<String, TypeDefinition>,
    active: &mut HashSet<String>,
) -> Type {
    match value {
        Type::Named { name, arguments } => {
            let Some(definition) = declared.get(name) else {
                return value.clone();
            };
            let Some(arguments) = complete_type_arguments(&definition.parameters, arguments) else {
                return value.clone();
            };
            let key = format!("heritage:{}", type_identity(value));
            if !active.insert(key.clone()) {
                return value.clone();
            }
            let substitutions = type_parameter_substitutions(&definition.parameters, arguments);
            let expanded = substitute_type(&definition.value, &substitutions);
            let result = expand_exported_heritage(&expanded, declared, active);
            active.remove(&key);
            result
        }
        Type::Intersection(parts) => Type::Intersection(
            parts
                .iter()
                .map(|part| expand_exported_heritage(part, declared, active))
                .collect(),
        ),
        _ => value.clone(),
    }
}
