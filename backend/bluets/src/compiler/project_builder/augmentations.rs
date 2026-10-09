// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Augmentation targets use the owner's resolver without runtime imports.

use super::*;

impl ProjectBuilder<'_> {
    pub(super) fn visit_augmentations(&mut self, module: &Module, depth: usize) {
        if !crate::parser::has_module_syntax(&module.declarations) {
            return;
        }
        for declaration in &module.declarations {
            let crate::parser::Declaration::Ambient(body) = declaration else {
                continue;
            };
            let Some(specifier) = &body.specifier else {
                continue;
            };
            let Ok(target) = self.loader.resolve(&module.id, specifier) else {
                // A missing target in a declaration file is accepted by the
                // pinned compiler. No source or runtime binding is created.
                continue;
            };
            let key = (module.id.clone(), specifier.clone());
            if !self.project.augmentation_resolutions.contains_key(&key)
                && self.project.resolutions.len()
                    + self.project.mode_resolutions.len()
                    + self.project.augmentation_resolutions.len()
                    + self.project.reference_resolutions.len()
                    >= self.limits.max_module_edges
            {
                self.diagnostics.push(Diagnostic::error(
                    DiagnosticCode::ResourceLimit,
                    body.specifier_span.clone(),
                    format!(
                        "module graph exceeds the {} import-edge limit",
                        self.limits.max_module_edges
                    ),
                ));
                continue;
            }
            self.project
                .augmentation_resolutions
                .insert(key, target.clone());
            self.visit(&target, depth.saturating_add(1));
        }
    }
}
