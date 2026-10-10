// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Helper declarations are ordinary owner-authorized, fingerprinted inputs.

use super::*;
use crate::parser::Declaration;

impl ProjectBuilder<'_> {
    pub(in crate::compiler) fn resolve_helper_providers(&mut self, options: &CompilerOptions) {
        if !options.import_helpers || options.target >= EcmaTarget::Es2015 {
            return;
        }
        let mut requests = Vec::new();
        for (id, module) in &self.project.modules {
            if is_declaration_module(id)
                || is_external_library_module(id)
                || !self.project.is_external_module(module)
            {
                continue;
            }
            let classes = module
                .declarations
                .iter()
                .filter_map(|declaration| {
                    if let Declaration::Class(class) = declaration {
                        Some(class)
                    } else {
                        None
                    }
                })
                .chain(
                    module
                        .class_expressions
                        .values()
                        .map(|expression| &expression.class),
                );
            let tokens = crate::lex(id, &module.source).unwrap_or_default();
            for class in classes.filter(|class| class.extends_name.is_some()) {
                let Some(base) = &class.extends_span else {
                    continue;
                };
                let start = tokens
                    .iter()
                    .rev()
                    .find(|token| {
                        token.is("extends")
                            && token.start >= class.name_span.end
                            && token.end <= base.start
                    })
                    .map_or(base.start, |token| token.start);
                requests.push((id.clone(), SourceSpan::new(id, start, base.end)));
            }
        }
        for (id, span) in requests {
            let mode = self.project.module_kinds.get(&id).map(|kind| {
                if *kind == ModuleKind::CommonJs {
                    crate::package_resolution::ImportMode::Require
                } else {
                    crate::package_resolution::ImportMode::Import
                }
            });
            let target = self.loader.resolve_with_mode(&id, "tslib", mode);
            let target = match target {
                Ok(target) => target,
                Err(error) => {
                    self.diagnostics.push(
                        Diagnostic::error(
                            DiagnosticCode::ModuleNotFound,
                            span,
                            format!("imported helper provider cannot be resolved: {error}"),
                        )
                        .with_typescript(2354, vec!["tslib".to_string()]),
                    );
                    continue;
                }
            };
            let key = (id.clone(), "tslib".to_string());
            if !self.project.resolutions.contains_key(&key)
                && self.project.resolutions.len() >= self.limits.max_module_edges
            {
                self.diagnostics.push(Diagnostic::error(
                    DiagnosticCode::ResourceLimit,
                    span,
                    "imported helper exceeds the project edge limit",
                ));
                continue;
            }
            self.project.resolutions.insert(key, target.clone());
            self.visit(&target, 1);
            if let Some(provider) = self.project.modules.get(&target) {
                if !provider.declarations.iter().any(|declaration| matches!(declaration,
                    Declaration::Function(function) if function.name == "__extends" && function.exported))
                {
                    self.diagnostics.push(Diagnostic::error(
                        DiagnosticCode::UnsupportedSyntax, span,
                        "owner-resolved helper provider does not declare the supported __extends ABI",
                    ));
                }
            }
        }
    }
}
