// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Project-wide static type-surface collection.

use super::*;

pub(super) fn declaration_module_diagnostics(
    project: &Project,
    module_kind: crate::ModuleKind,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for (module_id, module) in &project.modules {
        for declaration in &module.declarations {
            if let Declaration::ValueExport(export) = declaration {
                if let Some(resolved) = export.specifier.as_ref().and_then(|specifier| {
                    project
                        .resolutions
                        .get(&(module_id.clone(), specifier.clone()))
                }) {
                    if is_declaration_module(resolved)
                        && !crate::compiler::is_external_library_module(resolved)
                    {
                        diagnostics.push(Diagnostic::error(
                            DiagnosticCode::InvalidDeclarationFile,
                            export.span.clone(),
                            "value re-export resolves to a declaration module; declaration modules are type-only",
                        ));
                    }
                }
            }
            if let Declaration::Import(import) = declaration {
                if !import.type_only && import.specifier.ends_with(".d.ts") {
                    let stem = import.specifier.strip_suffix(".d.ts").unwrap();
                    let implementation = match module_kind {
                        crate::ModuleKind::CommonJs => stem.to_string(),
                        crate::ModuleKind::Esm => format!("{stem}.ts"),
                    };
                    diagnostics.push(
                        Diagnostic::error(
                            DiagnosticCode::InvalidDeclarationFile,
                            import.specifier_span.clone(),
                            "explicit declaration imports require a whole type-only import",
                        )
                        .with_typescript(2846, vec![implementation]),
                    );
                    continue;
                }
                if !import.is_type_only()
                    && project
                        .resolved_import(module_id, import)
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
                | Declaration::UmdExport(_)
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
                        interface.export_name().to_string(),
                        TypeDefinition {
                            kind: TypeDefinitionKind::Interface,
                            parameters: interface.type_parameters.clone(),
                            value: exported_interface_value(interface, &declared),
                        },
                    );
                }
                Declaration::Class(class) if class.exported => {
                    values.insert(
                        class.export_name().to_string(),
                        TypeDefinition {
                            kind: TypeDefinitionKind::Class,
                            parameters: module::class_definition_parameters(class),
                            value: module::class_instance_type(class),
                        },
                    );
                }
                Declaration::ValueExport(export) if export.specifier.is_none() => {
                    for binding in &export.bindings {
                        if let Some(definition) =
                            declared.get(&binding.local).filter(|definition| {
                                binding.type_only
                                    || export.export_assignment && is_declaration_module(id)
                                    || definition.kind == TypeDefinitionKind::Class
                            })
                        {
                            values.insert(binding.exported.clone(), definition.clone());
                        }
                    }
                }
                Declaration::TypeExport(export) if export.specifier.is_none() => {
                    for binding in &export.bindings {
                        if let Some(definition) = declared.get(&binding.local) {
                            values.insert(binding.exported.clone(), definition.clone());
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
                let Some(source_id) = project.resolved_type_export(module_id, export) else {
                    continue;
                };
                let Some(source_types) = modules.get(source_id) else {
                    continue;
                };
                if export.star && export.namespace.is_none() {
                    additions.extend(
                        source_types
                            .iter()
                            .filter(|(name, _)| name.as_str() != "default")
                            .map(|(name, value)| (name.clone(), value.clone())),
                    );
                }
                if let Some(namespace) = &export.namespace {
                    let substitutions = source_types
                        .keys()
                        .map(|name| {
                            (
                                name.clone(),
                                Type::Named {
                                    name: format!("{namespace}.{name}"),
                                    arguments: Vec::new(),
                                },
                            )
                        })
                        .collect();
                    for (name, definition) in source_types {
                        let mut definition = definition.clone();
                        definition.value = substitute_type(&definition.value, &substitutions);
                        additions.insert(format!("{namespace}.{name}"), definition);
                    }
                }
                for binding in &export.bindings {
                    if let Some(value) = source_types.get(&binding.local) {
                        additions.insert(binding.exported.clone(), value.clone());
                    }
                    let prefix = format!("{}.", binding.local);
                    additions.extend(source_types.iter().filter_map(|(name, value)| {
                        name.strip_prefix(&prefix)
                            .map(|member| (format!("{}.{member}", binding.exported), value.clone()))
                    }));
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
                let Some(source_id) = project.resolved_import(module_id, import) else {
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
                let Some(source_id) = project.resolved_type_export(module_id, export) else {
                    continue;
                };
                let Some(source_classes) = modules.get(source_id) else {
                    continue;
                };
                if export.star && export.namespace.is_none() {
                    additions.extend(
                        source_classes
                            .iter()
                            .filter(|(name, _)| name.as_str() != "default")
                            .map(|(name, value)| {
                                let mut value = value.clone();
                                value.value_exported = false;
                                (name.clone(), value)
                            }),
                    );
                }
                for binding in &export.bindings {
                    if let Some(value) = source_classes.get(&binding.local) {
                        let mut value = value.clone();
                        value.value_exported = false;
                        additions.insert(binding.exported.clone(), value);
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
        let merged = module::class_with_interface_heritage(
            class,
            module,
            &local_type_definitions(module),
            max_type_expansions,
        )
        .unwrap_or_else(|| class.clone());
        let mut value = module::class_export(&merged);
        if let Some(base_name) = &class.extends_name {
            if let Some(base) = declared.get(base_name).or_else(|| imported.get(base_name)) {
                let depth = base.heritage_depth.saturating_add(1);
                value.heritage_depth = depth;
                if depth <= max_type_expansions {
                    let arguments = class
                        .extends_arguments
                        .iter()
                        .map(|argument| module::formal_class_type(class, argument))
                        .collect::<Vec<_>>();
                    inherit_exported_class_surface(&mut value, base, &arguments);
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
                    exports.insert(class.export_name().to_string(), value.clone());
                }
            }
            Declaration::ValueExport(export) if export.specifier.is_none() => {
                for binding in &export.bindings {
                    if let Some(value) = declared.get(&binding.local) {
                        let mut value = value.clone();
                        value.value_exported = !binding.type_only;
                        exports.insert(binding.exported.clone(), value);
                    }
                }
            }
            Declaration::TypeExport(export) if export.specifier.is_none() => {
                for binding in &export.bindings {
                    if let Some(value) = declared.get(&binding.local) {
                        let mut value = value.clone();
                        value.value_exported = false;
                        exports.insert(binding.exported.clone(), value);
                    }
                }
            }
            _ => {}
        }
    }
    exports
}

fn inherit_exported_class_surface(
    derived: &mut ExportedClass,
    base: &ExportedClass,
    arguments: &[Type],
) {
    derived
        .constructor_binding
        .modifiers
        .inherit(&base.constructor_binding.modifiers);
    fn append_unshadowed(own: &mut Type, inherited: &Type) {
        let (
            Type::Record(own_fields)
            | Type::CallableRecord {
                fields: own_fields, ..
            },
            Type::Record(inherited_fields)
            | Type::CallableRecord {
                fields: inherited_fields,
                ..
            },
        ) = (own.object_type_mut(), inherited.object_type())
        else {
            return;
        };
        let names = own_fields
            .iter()
            .map(|field| field.name.clone())
            .collect::<BTreeSet<_>>();
        own_fields.extend(
            inherited_fields
                .iter()
                .filter(|field| !names.contains(&field.name))
                .cloned(),
        );
        if let Type::IndexedRecord {
            indices: inherited, ..
        } = inherited
        {
            if let Type::IndexedRecord { indices, .. } = own {
                let keys = indices
                    .iter()
                    .map(|index| index.key.clone())
                    .collect::<Vec<_>>();
                indices.extend(
                    inherited
                        .iter()
                        .filter(|index| !keys.contains(&index.key))
                        .cloned(),
                );
            } else if !inherited.is_empty() {
                *own = Type::IndexedRecord {
                    object: Box::new(own.clone()),
                    indices: inherited.clone(),
                };
            }
        }
    }

    let substitutions = module::heritage_substitutions(&base.type_parameters, arguments);
    append_unshadowed(
        &mut derived.instance_type,
        &substitute_type(&base.instance_type, &substitutions),
    );
    append_unshadowed(&mut derived.constructor_type, &base.constructor_type);
    if derived.constructor_binding.inherited && !base.constructor_binding.inherited {
        derived.constructor_binding.signatures = base
            .constructor_binding
            .signatures
            .iter()
            .map(|signature| {
                module::specialize_constructor(
                    signature,
                    &substitutions,
                    &derived.source_name,
                    &derived.type_parameters,
                )
            })
            .collect();
        derived.constructor_binding.inherited = false;
        derived.constructor_binding.visibility = base.constructor_binding.visibility;
        if let Type::CallableRecord { signatures, .. } = derived.constructor_type.object_type_mut()
        {
            if let Some(span) = signatures.first().map(|signature| signature.span.clone()) {
                *signatures = derived.constructor_binding.value_signatures(&span);
            }
        }
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
                    parameters: module::class_definition_parameters(class),
                    value: module::class_instance_type(class),
                },
            )),
            _ => None,
        })
        .fold(
            BTreeMap::<String, TypeDefinition>::new(),
            |mut definitions, (name, definition)| {
                if let Some(existing) = definitions.get_mut(&name).filter(|existing| {
                    existing.kind == TypeDefinitionKind::Interface
                        && definition.kind == TypeDefinitionKind::Interface
                }) {
                    existing.value = merge_interface_values(&existing.value, &definition.value);
                } else {
                    definitions.insert(name, definition);
                }
                definitions
            },
        )
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
    interface_value_with_heritage(&heritage, interface.body_type())
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
                if export.specifier.is_some() {
                    continue;
                }
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

/// Explicit names and the pre-existing closed static re-export surface.
/// Runtime forwarding is resolved separately without adding value authority to
/// declarations reached through type-only edges.
pub(super) fn local_exported_names(
    project: &Project,
) -> BTreeMap<String, (BTreeSet<String>, bool)> {
    let static_types = exported_types(project);
    let mut modules = BTreeMap::new();
    for (id, module) in &project.modules {
        let mut names = static_types[id].keys().cloned().collect::<BTreeSet<_>>();
        if let Some(schema) = project.json_modules.get(id) {
            names.insert("default".to_string());
            if let Type::Record(fields) = schema {
                names.extend(fields.iter().map(|field| field.name.clone()));
            }
        }
        let mut open = false;
        for declaration in &module.declarations {
            match declaration {
                Declaration::Variable(variable) if variable.exported => {
                    names.insert(variable.name.clone());
                }
                Declaration::Function(function)
                    if function.exported && !function.default_export =>
                {
                    names.insert(function.name.clone());
                }
                Declaration::Class(class) if class.exported => {
                    names.insert(class.export_name().to_string());
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
                    names.insert(interface.export_name().to_string());
                }
                Declaration::ValueExport(export) => {
                    if let Some(name) = &export.namespace {
                        names.insert(name.clone());
                    }
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
                    if let Some(name) = &export.namespace {
                        names.insert(name.clone());
                    }
                    for binding in &export.bindings {
                        names.insert(binding.exported.clone());
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
