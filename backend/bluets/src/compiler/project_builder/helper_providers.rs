// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Optional helper declarations use the ordinary owner resolver and edge budget.
use super::*;

impl ProjectBuilder<'_> {
    pub(in crate::compiler) fn resolve_helper_providers(&mut self, options: &CompilerOptions) {
        if !options.import_helpers {
            return;
        }
        let requests = self
            .project
            .modules
            .iter()
            .filter(|(id, module)| {
                !is_declaration_module(id)
                    && !is_external_library_module(id)
                    && self.project.is_external_module(module)
            })
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        for id in requests {
            let mode = self.project.module_kinds.get(&id).map(|kind| {
                if *kind == ModuleKind::CommonJs {
                    crate::package_resolution::ImportMode::Require
                } else {
                    crate::package_resolution::ImportMode::Import
                }
            });
            // Select an available provider even when this output needs no
            // helper. Lowering diagnoses absence only for a selected ABI.
            let Ok(target) = self.loader.resolve_with_mode(&id, "tslib", mode) else {
                continue;
            };
            let key = (id.clone(), "tslib".to_string());
            if !self.project.resolutions.contains_key(&key)
                && self.project.resolutions.len()
                    + self.project.mode_resolutions.len()
                    + self.project.augmentation_resolutions.len()
                    + self.project.reference_resolutions.len()
                    >= self.limits.max_module_edges
            {
                self.diagnostics.push(Diagnostic::error(
                    DiagnosticCode::ResourceLimit,
                    SourceSpan::new(&id, 0, 0),
                    "imported helper exceeds the project edge limit",
                ));
                continue;
            }
            self.project.resolutions.insert(key, target.clone());
            self.visit(&target, 1);
        }
    }
}
