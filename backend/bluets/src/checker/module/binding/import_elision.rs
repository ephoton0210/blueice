// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Verbatim syntax keeps value imports; it cannot reinterpret a type as a value.
use super::*;

impl ModuleChecker<'_> {
    pub(super) fn validate_verbatim_type_imports(&mut self) {
        if !self.verbatim_module_syntax || !self.enforce_types {
            return;
        }
        for declaration in &self.module.declarations {
            let Declaration::Import(import) = declaration else {
                continue;
            };
            if import.is_type_only() {
                continue;
            }
            let Some(target) = self.project.resolved_import(&self.module.id, import) else {
                continue;
            };
            for binding in import
                .bindings
                .iter()
                .filter(|binding| !binding.type_only && binding.imported != "*")
            {
                if self
                    .exports
                    .types
                    .get(target)
                    .is_some_and(|types| types.contains_key(&binding.imported))
                    && !self
                        .exports
                        .values
                        .get(target)
                        .is_some_and(|values| values.contains_key(&binding.imported))
                {
                    self.diagnostics.push(Diagnostic::error(DiagnosticCode::TypeMismatch,binding.span.clone(),format!("type `{}` requires a type-only import under verbatim module syntax",binding.local)).with_typescript(1484,vec![binding.local.clone()]));
                }
            }
        }
    }
}
