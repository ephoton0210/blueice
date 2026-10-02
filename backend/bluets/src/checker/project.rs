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
                        // An installed package's declarations describe code the
                        // package provides at run time, which the host that runs
                        // the output resolves by the package's own specifier. A
                        // project or host-supplied declaration has no run-time
                        // counterpart, so a value import of one stays refused.
                        .is_some_and(|resolved| {
                            is_declaration_module(resolved)
                                && !crate::compiler::is_external_library_module(resolved)
                        })
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
                Declaration::Class(_) => true,
                Declaration::Enum(_) => true,
                Declaration::Namespace(namespace) => !namespace.declared,
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
                Declaration::Class(class) if class.exported => {
                    values.insert(
                        class.name.clone(),
                        TypeDefinition {
                            kind: TypeDefinitionKind::Class,
                            parameters: Vec::new(),
                            value: module::class_instance_type(class),
                        },
                    );
                }
                Declaration::ValueExport(export) => {
                    for binding in &export.bindings {
                        if let Some(definition) = declared
                            .get(&binding.local)
                            .filter(|definition| definition.kind == TypeDefinitionKind::Class)
                        {
                            values.insert(binding.exported.clone(), definition.clone());
                        }
                    }
                }
                Declaration::TypeExport(export) if export.specifier.is_none() => {
                    for binding in &export.bindings {
                        let (local, exported) = binding
                            .split_once(" as ")
                            .map_or((binding.as_str(), binding.as_str()), |(local, exported)| {
                                (local, exported)
                            });
                        if let Some(definition) = declared.get(local) {
                            values.insert(exported.to_string(), definition.clone());
                        }
                    }
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

pub(super) fn exported_classes(
    project: &Project,
    max_type_expansions: usize,
) -> BTreeMap<String, BTreeMap<String, ExportedClass>> {
    let mut modules = BTreeMap::new();
    for (id, module) in &project.modules {
        modules.insert(
            id.clone(),
            local_exported_class_surfaces(module, &BTreeMap::new(), max_type_expansions),
        );
    }
    // A derived class may extend a value imported from another closed module.
    // Advance those exported surfaces one module edge per bounded pass.
    for _ in 0..project.modules.len() {
        let snapshot = modules.clone();
        let mut changed = false;
        for (module_id, module) in &project.modules {
            let mut imported = BTreeMap::new();
            for declaration in &module.declarations {
                let Declaration::Import(import) = declaration else {
                    continue;
                };
                if import.type_only {
                    continue;
                }
                let Some(source_id) = project
                    .resolutions
                    .get(&(module_id.clone(), import.specifier.clone()))
                else {
                    continue;
                };
                let Some(source_classes) = snapshot.get(source_id) else {
                    continue;
                };
                for binding in &import.bindings {
                    if binding.type_only {
                        continue;
                    }
                    if let Some(base) = source_classes
                        .get(&binding.imported)
                        .filter(|base| base.value_exported)
                    {
                        imported.insert(binding.local.clone(), base.clone());
                    }
                }
            }
            let exports = local_exported_class_surfaces(module, &imported, max_type_expansions);
            let target = modules.entry(module_id.clone()).or_default();
            for (name, value) in exports {
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
                let Some(source_classes) = modules.get(source_id) else {
                    continue;
                };
                for binding in &export.bindings {
                    if binding == "*" {
                        additions.extend(source_classes.iter().map(|(name, value)| {
                            let mut value = value.clone();
                            value.value_exported = false;
                            (name.clone(), value)
                        }));
                        continue;
                    }
                    let (local, exported) = binding
                        .split_once(" as ")
                        .map_or((binding.as_str(), binding.as_str()), |(local, exported)| {
                            (local, exported)
                        });
                    if let Some(value) = source_classes.get(local) {
                        let mut value = value.clone();
                        value.value_exported = false;
                        additions.insert(exported.to_string(), value);
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

fn local_exported_class_surfaces(
    module: &Module,
    imported: &BTreeMap<String, ExportedClass>,
    max_type_expansions: usize,
) -> BTreeMap<String, ExportedClass> {
    let mut declared = BTreeMap::new();
    for declaration in &module.declarations {
        let Declaration::Class(class) = declaration else {
            continue;
        };
        let mut value = module::class_export(class);
        if let Some(base_name) = &class.extends_name {
            if let Some(base) = declared.get(base_name).or_else(|| imported.get(base_name)) {
                let depth = base.heritage_depth.saturating_add(1);
                value.heritage_depth = depth;
                if depth <= max_type_expansions {
                    inherit_exported_class_surface(&mut value, base);
                }
            }
        }
        declared.insert(class.name.clone(), value);
    }
    let mut exports = BTreeMap::new();
    for declaration in &module.declarations {
        match declaration {
            Declaration::Class(class) if class.exported => {
                if let Some(value) = declared.get(&class.name) {
                    exports.insert(class.name.clone(), value.clone());
                }
            }
            Declaration::ValueExport(export) => {
                for binding in &export.bindings {
                    if let Some(value) = declared.get(&binding.local) {
                        let mut value = value.clone();
                        value.value_exported = true;
                        exports.insert(binding.exported.clone(), value);
                    }
                }
            }
            Declaration::TypeExport(export) if export.specifier.is_none() => {
                for binding in &export.bindings {
                    let (local, exported) = binding
                        .split_once(" as ")
                        .map_or((binding.as_str(), binding.as_str()), |(local, exported)| {
                            (local, exported)
                        });
                    if let Some(value) = declared.get(local) {
                        let mut value = value.clone();
                        value.value_exported = false;
                        exports.insert(exported.to_string(), value);
                    }
                }
            }
            _ => {}
        }
    }
    exports
}

fn inherit_exported_class_surface(derived: &mut ExportedClass, base: &ExportedClass) {
    fn append_unshadowed(own: &mut Type, inherited: &Type) {
        let (Type::Record(own), Type::Record(inherited)) = (own, inherited) else {
            return;
        };
        let names = own
            .iter()
            .map(|field| field.name.clone())
            .collect::<BTreeSet<_>>();
        own.extend(
            inherited
                .iter()
                .filter(|field| !names.contains(&field.name))
                .cloned(),
        );
    }

    append_unshadowed(&mut derived.instance_type, &base.instance_type);
    append_unshadowed(&mut derived.constructor_type, &base.constructor_type);
    if derived.constructor_binding.inherited && !base.constructor_binding.inherited {
        derived.constructor_binding.signatures = base
            .constructor_binding
            .signatures
            .iter()
            .cloned()
            .map(|mut signature| {
                signature.return_type = Type::Named {
                    name: derived.source_name.clone(),
                    arguments: Vec::new(),
                };
                signature
            })
            .collect();
        derived.constructor_binding.inherited = false;
    }
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
            Declaration::Class(class) => Some((
                class.name.clone(),
                TypeDefinition {
                    kind: TypeDefinitionKind::Class,
                    parameters: Vec::new(),
                    value: module::class_instance_type(class),
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

/// The enums each module exports, by exported name. A declaration counts as
/// exported when it is written `export enum`, or named in an `export { .. }`.
pub(super) fn exported_enums(
    project: &Project,
) -> BTreeMap<String, BTreeMap<String, ExportedEnum>> {
    let mut modules = BTreeMap::new();
    for (id, module) in &project.modules {
        let evaluated = crate::enum_eval::evaluate_enums(module);
        let mut evaluations = evaluated.iter();
        let mut merged: BTreeMap<String, (ExportedEnum, bool)> = BTreeMap::new();
        for declaration in &module.declarations {
            let Declaration::Enum(enum_declaration) = declaration else {
                continue;
            };
            let evaluation = evaluations.next().expect("every enum was evaluated");
            let entry = merged
                .entry(enum_declaration.name.clone())
                .or_insert_with(|| {
                    (
                        ExportedEnum {
                            is_const: enum_declaration.is_const,
                            declared: enum_declaration.declared,
                            members: Vec::new(),
                        },
                        false,
                    )
                });
            entry.0.members.extend(evaluation.members.iter().cloned());
            entry.1 |= enum_declaration.exported;
        }
        let mut exported = BTreeMap::new();
        for (name, (value, is_exported)) in &merged {
            if *is_exported {
                exported.insert(name.clone(), value.clone());
            }
        }
        for declaration in &module.declarations {
            if let Declaration::ValueExport(export) = declaration {
                for binding in &export.bindings {
                    if let Some((value, _)) = merged.get(&binding.local) {
                        exported.insert(binding.exported.clone(), value.clone());
                    }
                }
            }
        }
        modules.insert(id.clone(), exported);
    }
    modules
}

/// The names each module exports, as values or as types, and whether the set is
/// open (a type re-export from another module could add more). Value
/// re-exports from another module are not supported, so only types can.
pub(super) fn exported_names(project: &Project) -> BTreeMap<String, (BTreeSet<String>, bool)> {
    let mut modules = BTreeMap::new();
    for (id, module) in &project.modules {
        let mut names = BTreeSet::new();
        let mut open = false;
        for declaration in &module.declarations {
            match declaration {
                Declaration::Variable(variable) if variable.exported => {
                    names.insert(variable.name.clone());
                }
                Declaration::Function(function) if function.exported => {
                    names.insert(function.name.clone());
                }
                Declaration::Class(class) if class.exported => {
                    names.insert(class.name.clone());
                }
                Declaration::Enum(declaration) if declaration.exported => {
                    names.insert(declaration.name.clone());
                }
                Declaration::Namespace(namespace) if namespace.exported => {
                    names.insert(namespace.name.clone());
                }
                Declaration::TypeAlias(alias) if alias.exported => {
                    names.insert(alias.name.clone());
                }
                Declaration::Interface(interface) if interface.exported => {
                    names.insert(interface.name.clone());
                }
                Declaration::ValueExport(export) => {
                    names.extend(
                        export
                            .bindings
                            .iter()
                            .map(|binding| binding.exported.clone()),
                    );
                }
                Declaration::TypeExport(export) => {
                    if export.specifier.is_some() {
                        open = true;
                    }
                    for binding in &export.bindings {
                        let exported = binding
                            .split_once(" as ")
                            .map_or(binding.as_str(), |(_, exported)| exported);
                        names.insert(exported.to_string());
                    }
                }
                Declaration::DefaultExport(_) => {
                    names.insert("default".to_string());
                }
                Declaration::Function(function) if function.default_export => {
                    names.insert("default".to_string());
                }
                _ => {}
            }
        }
        modules.insert(id.clone(), (names, open));
    }
    modules
}
