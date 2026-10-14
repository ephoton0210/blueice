// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Runtime attributes follow the selected module grammar and string index.

use super::*;
use crate::syntax::TokenKind;

impl ModuleChecker<'_> {
    pub(super) fn validate_import_attributes(&mut self) {
        for declaration in &self.module.declarations {
            let attributes = match declaration {
                Declaration::Import(import) if !import.type_only => import.attributes.as_ref(),
                Declaration::ValueExport(export) => export.attributes.as_ref(),
                _ => None,
            };
            let Some(attributes) = attributes else {
                continue;
            };
            if !self.import_attributes || self.module_kind != crate::ModuleKind::Esm {
                self.diagnostics.push(
                    Diagnostic::error(
                        DiagnosticCode::TypeMismatch,
                        attributes.span.clone(),
                        "selected module grammar does not permit runtime import attributes",
                    )
                    .with_typescript(if attributes.assertion { 2821 } else { 2823 }, Vec::new()),
                );
                continue;
            }
            let Some(invalid) = attributes
                .entries
                .iter()
                .find(|entry| entry.value.kind != TokenKind::String)
            else {
                continue;
            };
            let primitive = match invalid.value.kind {
                TokenKind::Number => "number",
                TokenKind::Keyword if matches!(invalid.value.text.as_str(), "true" | "false") => {
                    "boolean"
                }
                _ => {
                    self.diagnostics.push(
                        Diagnostic::error(
                            DiagnosticCode::TypeMismatch,
                            invalid.value.span(&self.module.id),
                            "import attribute values must be string literals",
                        )
                        .with_typescript(2858, Vec::new()),
                    );
                    continue;
                }
            };
            let fields = attributes
                .entries
                .iter()
                .map(|entry| format!("{}: {};", entry.key, entry.value.text))
                .collect::<Vec<_>>()
                .join(" ");
            let mut diagnostic = Diagnostic::error(
                DiagnosticCode::TypeMismatch,
                attributes.span.clone(),
                "import attributes must satisfy the string index signature",
            )
            .with_typescript(
                2322,
                vec![format!("{{ {fields} }}"), "ImportAttributes".into()],
            );
            diagnostic
                .typescript
                .as_mut()
                .unwrap()
                .message
                .push_str(&format!(
                    "\n  Property '{}' is incompatible with index signature.\n    Type '{primitive}' is not assignable to type 'string'.",
                    invalid.key,
                ));
            self.diagnostics.push(diagnostic);
        }
    }
}
