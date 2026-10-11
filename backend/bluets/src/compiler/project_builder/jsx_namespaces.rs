// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Automatic JSX type providers remain owner-resolved, bounded graph inputs.
use super::*;
use crate::package_resolution::ImportMode;

impl ProjectBuilder<'_> {
    pub(in crate::compiler) fn resolve_jsx_runtime_namespaces(
        &mut self,
        options: &CompilerOptions,
    ) {
        let Some(mode) = options.jsx.filter(|mode| mode.lowers()) else {
            return;
        };
        let mut seen = BTreeSet::new();
        loop {
            let requests = self
                .project
                .modules
                .iter()
                .filter_map(|(id, module)| {
                    if !id.ends_with(".tsx")
                        || is_declaration_module(id)
                        || is_external_library_module(id)
                        || seen.contains(id)
                    {
                        return None;
                    }
                    let tokens = crate::lex(id, &module.source).ok()?;
                    let token = tokens
                        .iter()
                        .find(|token| token.kind == crate::TokenKind::JsxElement)?;
                    let pragmas = crate::jsx::Pragmas::of(&module.source);
                    let automatic = match pragmas.runtime.as_deref() {
                        Some("classic") => false,
                        Some("automatic") => true,
                        _ => {
                            matches!(mode, JsxMode::ReactJsx | JsxMode::ReactJsxDev)
                                || pragmas.import_source.is_some()
                        }
                    };
                    if !automatic {
                        return None;
                    }
                    let source = pragmas
                        .import_source
                        .as_deref()
                        .or(options.jsx_import_source.as_deref())
                        .unwrap_or("react");
                    let runtime = if mode == JsxMode::ReactJsxDev {
                        "jsx-dev-runtime"
                    } else {
                        "jsx-runtime"
                    };
                    Some((
                        id.clone(),
                        format!("{source}/{runtime}"),
                        SourceSpan::new(id, token.start, token.end),
                    ))
                })
                .collect::<Vec<_>>();
            if requests.is_empty() {
                break;
            }
            for (id, specifier, span) in requests {
                seen.insert(id.clone());
                let condition = match self.project.module_kind(&id, options.module_kind) {
                    ModuleKind::CommonJs => ImportMode::Require,
                    _ => ImportMode::Import,
                };
                let target = match self.loader.resolve_jsx_runtime_namespace(
                    &id,
                    &specifier,
                    Some(condition),
                ) {
                    Ok(Some(target))
                        if target.ends_with(".ts")
                            || target.ends_with(".tsx")
                            || target.ends_with(".mts")
                            || target.ends_with(".cts") =>
                    {
                        target
                    }
                    Ok(_) => continue,
                    Err(message) => {
                        self.diagnostics.push(Diagnostic::error(
                            DiagnosticCode::ModuleNotFound,
                            span,
                            format!("JSX namespace provider cannot be resolved: {message}"),
                        ));
                        continue;
                    }
                };
                let key = (id.clone(), specifier);
                if !self.project.resolutions.contains_key(&key)
                    && self.project.resolutions.len()
                        + self.project.mode_resolutions.len()
                        + self.project.augmentation_resolutions.len()
                        + self.project.reference_resolutions.len()
                        >= self.limits.max_module_edges
                {
                    self.diagnostics.push(Diagnostic::error(
                        DiagnosticCode::ResourceLimit,
                        span,
                        "JSX namespace provider exceeds the project edge limit",
                    ));
                    continue;
                }
                self.project.resolutions.insert(key, target.clone());
                self.project
                    .jsx_runtime_modules
                    .insert(id.clone(), target.clone());
                let depth = self.module_depths.get(&id).copied().unwrap_or(0);
                self.visit(&target, depth.saturating_add(1));
            }
        }
    }
}
