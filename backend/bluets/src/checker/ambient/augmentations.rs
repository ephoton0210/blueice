// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Physical module augmentation shares canonical type surfaces with consumers.

use super::*;

pub(in crate::checker) fn merge_module_types(
    project: &Project,
    exports: &mut ProjectExports,
    diagnostics: &mut Vec<Diagnostic>,
    enforce_types: bool,
    max_type_expansions: usize,
) {
    if project.augmentation_resolutions.is_empty() {
        return;
    }
    let mut contexts = AmbientDeclarations::default();
    collect_contexts(
        project,
        exports,
        &mut contexts,
        diagnostics,
        max_type_expansions,
    );
    let mut budget = TypeExpansionBudget::new(max_type_expansions);
    for module in project.modules.values() {
        for declaration in &module.declarations {
            let Declaration::Ambient(body) = declaration else {
                continue;
            };
            let Some(target) = body.specifier.as_ref().and_then(|specifier| {
                project
                    .augmentation_resolutions
                    .get(&(module.id.clone(), specifier.clone()))
            }) else {
                continue;
            };
            if !budget.consume() {
                diagnostics.push(Diagnostic::error(
                    DiagnosticCode::ResourceLimit,
                    body.span.clone(),
                    "module augmentation surfaces exceed the type-expansion limit",
                ));
                return;
            }
            let mut surface = AmbientDeclarations {
                enforce_types,
                contexts: contexts.contexts.clone(),
                types: exports.types.get(target).cloned().unwrap_or_default(),
                ..AmbientDeclarations::default()
            };
            insert_body(project, &body.body, &mut surface, diagnostics);
            exports
                .exported_names
                .entry(target.clone())
                .or_default()
                .0
                .extend(surface.types.keys().cloned());
            exports.types.insert(target.clone(), surface.types);
        }
    }
}
