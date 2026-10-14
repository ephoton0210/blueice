// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Re-export diagnostics use retained edges and source binding identities.

use super::*;

impl ModuleChecker<'_> {
    pub(super) fn missing_type_import(
        &self,
        import: &crate::parser::ImportDeclaration,
        binding: &crate::parser::ImportBinding,
        target: &str,
        diagnostic: Diagnostic,
    ) -> Diagnostic {
        if binding.imported == "default" {
            return self.refine_missing_default(
                import,
                binding,
                target,
                diagnostic.with_typescript(1192, vec![import.specifier.clone()]),
            );
        }
        let declared = self.project.modules.get(target).is_some_and(|module| {
            module
                .declarations
                .iter()
                .any(|declaration| match declaration {
                    Declaration::TypeAlias(alias) => alias.name == binding.imported,
                    Declaration::Interface(interface) => interface.name == binding.imported,
                    Declaration::Class(class) => class.name == binding.imported,
                    Declaration::Function(function) => function.name == binding.imported,
                    Declaration::Variable(variable) => variable.name == binding.imported,
                    Declaration::Enum(item) => item.name == binding.imported,
                    Declaration::Namespace(namespace) => namespace.name == binding.imported,
                    Declaration::Import(import) => import
                        .bindings
                        .iter()
                        .any(|local| local.local == binding.imported),
                    _ => false,
                })
        });
        diagnostic.with_typescript(
            if declared { 2459 } else { 2305 },
            vec![import.specifier.clone(), binding.imported.clone()],
        )
    }

    pub(super) fn refine_missing_default(
        &self,
        import: &crate::parser::ImportDeclaration,
        binding: &crate::parser::ImportBinding,
        target: &str,
        diagnostic: Diagnostic,
    ) -> Diagnostic {
        let stars = self
            .project
            .modules
            .get(target)
            .into_iter()
            .flat_map(|module| module.declarations.iter())
            .filter_map(|declaration| match declaration {
                Declaration::ValueExport(export) if export.star && export.namespace.is_none() => {
                    Some(export)
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        if stars.is_empty() && !binding.type_only {
            return diagnostic;
        }
        let module_name = target
            .strip_suffix(".d.ts")
            .or_else(|| target.strip_suffix(".tsx"))
            .or_else(|| target.strip_suffix(".ts"))
            .unwrap_or(target);
        let mut diagnostic = diagnostic.with_typescript(1192, vec![format!("{module_name:?}")]);
        let counterpart = diagnostic.typescript.as_mut().unwrap();
        if let Some(token) = crate::syntax::lex(&self.module.id, &self.module.source)
            .unwrap_or_default()
            .iter()
            .find(|token| {
                token.start >= import.span.start
                    && token.end <= import.span.end
                    && token.text == binding.local
            })
        {
            counterpart.span = token.span(&self.module.id);
        }
        counterpart
            .related_information
            .extend(
                stars
                    .iter()
                    .map(|export| crate::TypeScriptRelatedInformation {
                        code: 1195,
                        message: "'export *' does not re-export a default.".to_string(),
                        span: export.span.clone(),
                        position: None,
                    }),
            );
        diagnostic
    }

    pub(super) fn validate_reexport(&mut self, export: &crate::parser::ValueExportDeclaration) {
        let Some(specifier) = &export.specifier else {
            return;
        };
        let Some(target) = self.project.resolved_module(&self.module.id, specifier) else {
            return;
        };
        if export.star && self.project.json_modules.contains_key(target) {
            let name = target
                .rsplit('/')
                .next()
                .unwrap_or(target)
                .trim_end_matches(".json");
            self.diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::TypeMismatch,
                    export
                        .specifier_span
                        .as_ref()
                        .unwrap_or(&export.span)
                        .clone(),
                    "JSON export assignment cannot be used with a star re-export",
                )
                .with_typescript(2498, vec![format!("{name:?}")]),
            );
            return;
        }
        let origins = crate::checker::reexports::origins(self.project);
        let source = &origins[target];
        for binding in &export.bindings {
            if !source.contains_key(&binding.local) {
                self.diagnostics.push(
                    Diagnostic::error(
                        DiagnosticCode::UnknownName,
                        binding.span.clone(),
                        format!(
                            "module `{specifier}` has no exported member `{}`",
                            binding.local
                        ),
                    )
                    .with_typescript(2305, vec![specifier.clone(), binding.local.clone()]),
                );
            }
        }
        if !export.star || export.namespace.is_some() {
            return;
        }
        for (name, candidates) in source {
            if name == "default" || name == "export=" {
                continue;
            }
            let Some(all) = origins[&self.module.id]
                .get(name)
                .filter(|origins| origins.len() > 1)
            else {
                continue;
            };
            let previous = self
                .module
                .declarations
                .iter()
                .take_while(|item| item.span().start < export.span.start)
                .find_map(|item| {
                    let Declaration::ValueExport(item) = item else {
                        return None;
                    };
                    if !item.star || item.namespace.is_some() {
                        return None;
                    }
                    let specifier = item.specifier.as_ref()?;
                    let source = self.project.resolved_module(&self.module.id, specifier)?;
                    let other = origins[source].get(name)?;
                    (other != candidates && other.iter().any(|origin| all.contains(origin)))
                        .then_some(specifier)
                });
            if let Some(previous) = previous {
                self.diagnostics.push(
                    Diagnostic::error(
                        DiagnosticCode::DuplicateDeclaration,
                        export.span.clone(),
                        format!("conflicting star export `{name}`"),
                    )
                    .with_typescript(2308, vec![format!("{previous:?}"), name.clone()]),
                );
            }
        }
    }
}
