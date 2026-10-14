// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Declaration-local types retain module identity without publishing globals.

use super::*;

pub(super) fn private_name(module: &str, name: &str) -> String {
    format!("\0ambient:{module}\0{name}")
}

fn named(module: &str, name: &str) -> Type {
    Type::Named {
        name: private_name(module, name),
        arguments: Vec::new(),
    }
}

pub(in crate::checker) fn collect_contexts(
    project: &Project,
    exports: &ProjectExports,
    ambient: &mut AmbientDeclarations,
    diagnostics: &mut Vec<Diagnostic>,
    max_type_expansions: usize,
) {
    fn contains_global(declarations: &[Declaration]) -> bool {
        declarations.iter().any(|item| match item {
            Declaration::Ambient(_) => true,
            Declaration::Namespace(item) => contains_global(&item.body),
            _ => false,
        })
    }
    if !project
        .modules
        .values()
        .any(|module| contains_global(&module.declarations))
    {
        return;
    }
    let mut budget = TypeExpansionBudget::new(max_type_expansions);
    for (id, module) in &project.modules {
        let local = project::local_type_definitions(module);
        let mut context = local
            .keys()
            .map(|name| (name.clone(), named(id, name)))
            .collect::<BTreeMap<_, _>>();
        for declaration in &module.declarations {
            let Declaration::Import(import) = declaration else {
                continue;
            };
            let Some(target) = project.resolved_import(id, import) else {
                continue;
            };
            let Some(types) = exports.types.get(target) else {
                continue;
            };
            for binding in &import.bindings {
                if binding.imported == "*" {
                    for name in types.keys() {
                        context.insert(format!("{}.{name}", binding.local), named(target, name));
                    }
                } else if types.contains_key(&binding.imported) {
                    context.insert(binding.local.clone(), named(target, &binding.imported));
                }
            }
        }
        ambient.contexts.insert(id.clone(), context);
    }
    for (id, module) in &project.modules {
        let mut definitions = project::local_type_definitions(module);
        definitions.extend(
            exports
                .types
                .get(id)
                .into_iter()
                .flat_map(|types| types.clone()),
        );
        for (name, mut definition) in definitions {
            if !budget.consume() {
                diagnostics.push(Diagnostic::error(
                    DiagnosticCode::ResourceLimit,
                    SourceSpan::new(id, 0, 0),
                    "ambient declaration contexts exceed the type-expansion limit",
                ));
                return;
            }
            let substitutions = ambient.contexts[id]
                .iter()
                .filter(|(name, _)| {
                    !definition
                        .parameters
                        .iter()
                        .any(|parameter| &parameter.name == *name)
                })
                .map(|(name, value)| (name.clone(), value.clone()))
                .collect();
            definition.value = substitute_type(&definition.value, &substitutions);
            for parameter in &mut definition.parameters {
                parameter.constraint = parameter
                    .constraint
                    .as_ref()
                    .map(|value| substitute_type(value, &substitutions));
                parameter.default = parameter
                    .default
                    .as_ref()
                    .map(|value| substitute_type(value, &substitutions));
            }
            ambient
                .private_types
                .insert(private_name(id, &name), definition);
        }
    }
    for (target, definitions) in &exports.types {
        if project.ambient_module_bodies(target).is_empty() {
            continue;
        }
        let context = definitions
            .keys()
            .map(|name| (name.clone(), named(target, name)))
            .collect::<BTreeMap<_, _>>();
        for (name, definition) in definitions {
            if !budget.consume() {
                diagnostics.push(Diagnostic::error(
                    DiagnosticCode::ResourceLimit,
                    project.ambient_module_bodies(target)[0].span.clone(),
                    "ambient declaration contexts exceed the type-expansion limit",
                ));
                return;
            }
            let mut definition = definition.clone();
            let substitutions = context
                .iter()
                .filter(|(name, _)| {
                    !definition
                        .parameters
                        .iter()
                        .any(|parameter| &parameter.name == *name)
                })
                .map(|(name, value)| (name.clone(), value.clone()))
                .collect();
            definition.value = substitute_type(&definition.value, &substitutions);
            for parameter in &mut definition.parameters {
                parameter.constraint = parameter
                    .constraint
                    .as_ref()
                    .map(|value| substitute_type(value, &substitutions));
                parameter.default = parameter
                    .default
                    .as_ref()
                    .map(|value| substitute_type(value, &substitutions));
            }
            ambient
                .private_types
                .insert(private_name(target, name), definition);
        }
    }
}
