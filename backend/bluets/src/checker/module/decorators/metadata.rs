// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Isolated legacy metadata distinguishes erased imports from runtime values.

use super::*;

impl ModuleChecker<'_> {
    pub(super) fn check_metadata_import_annotations(&mut self, class: &ClassDeclaration) {
        if !self.experimental_decorators
            || !self.emit_decorator_metadata
            || !self.isolated_modules
            || !matches!(
                self.module_kind,
                crate::ModuleKind::Esm | crate::ModuleKind::Node16 | crate::ModuleKind::NodeNext
            )
        {
            return;
        }
        let mut ranges = Vec::new();
        for shell in &class.members {
            let decorated = !shell.decorators.is_empty()
                || shell.method.as_ref().is_some_and(|method| {
                    method
                        .parameters
                        .iter()
                        .any(|parameter| !parameter.decorators.is_empty())
                })
                || shell.accessor.as_ref().is_some_and(|accessor| {
                    accessor
                        .parameters
                        .iter()
                        .any(|parameter| !parameter.decorators.is_empty())
                });
            if let Some(constructor) = &shell.constructor {
                if !class.decorators.is_empty()
                    || constructor
                        .parameters
                        .iter()
                        .any(|parameter| !parameter.decorators.is_empty())
                {
                    ranges.extend(
                        constructor
                            .parameters
                            .iter()
                            .filter(|parameter| parameter.annotation.is_some())
                            .map(|parameter| parameter.span.clone()),
                    );
                }
            }
            if !decorated {
                continue;
            }
            if let Some(field) = &shell.field {
                ranges.extend(field.annotation_span.iter().cloned());
            }
            if let Some(method) = &shell.method {
                ranges.extend(
                    method
                        .parameters
                        .iter()
                        .filter(|parameter| parameter.annotation.is_some())
                        .map(|parameter| parameter.span.clone()),
                );
                ranges.extend(method.return_type_span.iter().cloned());
            }
            if let Some(accessor) = &shell.accessor {
                ranges.extend(
                    accessor
                        .parameters
                        .iter()
                        .filter(|parameter| parameter.annotation.is_some())
                        .map(|parameter| parameter.span.clone()),
                );
                ranges.extend(accessor.return_type_span.iter().cloned());
            }
        }
        for reference in &self.module.type_references {
            if reference.value_query
                || reference.name.contains('.')
                || !ranges.iter().any(|range| {
                    range.start <= reference.span.start && reference.span.end <= range.end
                })
            {
                continue;
            }
            for declaration in &self.module.declarations {
                let Declaration::Import(import) = declaration else {
                    continue;
                };
                if import.type_only {
                    continue;
                }
                let Some(binding) = import.bindings.iter().find(|binding| {
                    binding.local == reference.name && !binding.type_only && binding.imported != "*"
                }) else {
                    continue;
                };
                if self.values.contains_key(&binding.local)
                    || !self.types.contains_key(&binding.local)
                {
                    continue;
                }
                let message = "A type referenced in a decorated signature must be imported with 'import type' or a namespace import when 'isolatedModules' and 'emitDecoratorMetadata' are enabled.";
                let mut diagnostic = Diagnostic::error(
                    DiagnosticCode::InvalidDeclarationFile,
                    reference.span.clone(),
                    message,
                )
                .with_typescript(1272, Vec::new());
                if let Some(counterpart) = &mut diagnostic.typescript {
                    counterpart
                        .related_information
                        .push(crate::TypeScriptRelatedInformation {
                            code: 1376,
                            message: format!("'{}' was imported here.", binding.local),
                            span: binding.span.clone(),
                            position: None,
                        });
                }
                self.diagnostics.push(diagnostic);
            }
        }
    }
}
