// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Owner-authorized graph loading and existing work bounds.

use super::*;
mod augmentations;
mod helper_providers;
mod references;
use std::collections::HashMap;

pub(super) struct ProjectBuilder<'a> {
    loader: &'a dyn ModuleLoader,
    previous: Option<&'a Project>,
    pub(super) project: Project,
    pub(super) diagnostics: Vec<Diagnostic>,
    state: HashMap<String, VisitState>,
    limits: CompilerLimits,
    pub(super) resolve_json_module: bool,
    pub(super) module_kind: ModuleKind,
    total_source_bytes: usize,
    pub(super) parsed_modules: BTreeSet<String>,
    pub(super) reused_parsed_modules: BTreeSet<String>,
    unresolved_imports: Vec<(
        usize,
        String,
        String,
        Option<crate::package_resolution::ImportMode>,
    )>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VisitState {
    Visiting,
    Done,
}

impl<'a> ProjectBuilder<'a> {
    pub(super) fn new(
        loader: &'a dyn ModuleLoader,
        previous: Option<&'a Project>,
        limits: CompilerLimits,
    ) -> Self {
        Self {
            loader,
            previous,
            project: Project::empty(""),
            diagnostics: Vec::new(),
            state: HashMap::new(),
            limits,
            resolve_json_module: false,
            module_kind: ModuleKind::Esm,
            total_source_bytes: 0,
            parsed_modules: BTreeSet::new(),
            reused_parsed_modules: BTreeSet::new(),
            unresolved_imports: Vec::new(),
        }
    }

    pub(super) fn visit(&mut self, module_id: &str, depth: usize) {
        if self.project.entry.is_empty() {
            self.project.entry = module_id.to_string();
        }
        match self.state.get(module_id) {
            Some(VisitState::Done) => return,
            // A closed cycle is a valid static module graph. Declared import
            // types seed checking; unannotated circular inference is diagnosed
            // by the checker. Emitted execution retains the module system's TDZ
            // or partial-export behavior without reading any additional source.
            Some(VisitState::Visiting) => return,
            None => {}
        }
        if depth > self.limits.max_module_depth {
            self.diagnostics.push(Diagnostic::error(
                DiagnosticCode::ResourceLimit,
                SourceSpan::new(module_id, 0, 0),
                format!(
                    "module graph exceeds the {} import-depth limit",
                    self.limits.max_module_depth
                ),
            ));
            return;
        }
        if self.state.len() >= self.limits.max_modules {
            self.diagnostics.push(Diagnostic::error(
                DiagnosticCode::ResourceLimit,
                SourceSpan::new(module_id, 0, 0),
                format!(
                    "project exceeds the {} module limit",
                    self.limits.max_modules
                ),
            ));
            return;
        }
        self.state
            .insert(module_id.to_string(), VisitState::Visiting);
        let loaded = {
            let _timer = crate::performance::timer(crate::performance::Stage::Load);
            self.loader.load(module_id)
        };
        let source = match loaded {
            Ok(source) => source,
            Err(message) => {
                self.diagnostics.push(Diagnostic::error(
                    DiagnosticCode::ModuleNotFound,
                    SourceSpan::new(module_id, 0, 0),
                    message,
                ));
                self.state.insert(module_id.to_string(), VisitState::Done);
                return;
            }
        };
        self.visit_loaded_source(module_id, source, depth);
    }

    pub(super) fn visit_ambient_declaration(&mut self, source: &ModuleSource) {
        let module_id = source.id.as_str();
        if !is_declaration_module(module_id) {
            self.diagnostics.push(Diagnostic::error(
                DiagnosticCode::InvalidDeclarationFile,
                SourceSpan::new(module_id, 0, 0),
                "ambient host declaration modules must use a `.d.ts` identity",
            ));
            return;
        }
        if self.state.contains_key(module_id) {
            if self
                .project
                .referenced_declaration_modules
                .contains(module_id)
                && self.project.modules.get(module_id).is_some_and(|module| {
                    module.source == source.text
                        && !crate::parser::has_module_syntax(&module.declarations)
                })
            {
                self.project
                    .ambient_declaration_modules
                    .insert(module_id.to_string());
                return;
            }
            self.diagnostics.push(Diagnostic::error(
                DiagnosticCode::InvalidDeclarationFile,
                SourceSpan::new(module_id, 0, 0),
                "ambient host declaration module duplicates a source-graph module",
            ));
            return;
        }
        if self.state.len() >= self.limits.max_modules {
            self.diagnostics.push(Diagnostic::error(
                DiagnosticCode::ResourceLimit,
                SourceSpan::new(module_id, 0, 0),
                format!(
                    "project exceeds the {} module limit",
                    self.limits.max_modules
                ),
            ));
            return;
        }
        self.state
            .insert(module_id.to_string(), VisitState::Visiting);
        self.project
            .ambient_declaration_modules
            .insert(module_id.to_string());
        self.visit_loaded_source(module_id, source.clone(), 0);
    }

    fn visit_loaded_source(&mut self, module_id: &str, source: ModuleSource, depth: usize) {
        if source.id != module_id {
            self.diagnostics.push(Diagnostic::error(
                DiagnosticCode::ModuleNotFound,
                SourceSpan::new(module_id, 0, 0),
                format!(
                    "loader returned `{}` while `{module_id}` was requested; module identities must be stable",
                    source.id
                ),
            ));
            self.state.insert(module_id.to_string(), VisitState::Done);
            return;
        }
        let Some(total_source_bytes) = self.total_source_bytes.checked_add(source.text.len())
        else {
            self.diagnostics.push(Diagnostic::error(
                DiagnosticCode::ResourceLimit,
                SourceSpan::new(module_id, 0, 0),
                "project source-byte accounting overflowed",
            ));
            self.state.insert(module_id.to_string(), VisitState::Done);
            return;
        };
        if total_source_bytes > self.limits.max_total_source_bytes {
            self.diagnostics.push(Diagnostic::error(
                DiagnosticCode::ResourceLimit,
                SourceSpan::new(module_id, 0, 0),
                format!(
                    "project exceeds the {} total source-byte limit",
                    self.limits.max_total_source_bytes
                ),
            ));
            self.state.insert(module_id.to_string(), VisitState::Done);
            return;
        }
        self.total_source_bytes = total_source_bytes;
        if matches!(self.module_kind, ModuleKind::Node16 | ModuleKind::NodeNext) {
            match self.loader.implied_module_kind(module_id) {
                Ok(kind @ (ModuleKind::Esm | ModuleKind::CommonJs)) => {
                    self.project
                        .module_kinds
                        .insert(module_id.to_string(), kind);
                }
                Ok(_) => {
                    self.diagnostics.push(Diagnostic::error(
                        DiagnosticCode::UnsupportedSyntax,
                        SourceSpan::new(module_id, 0, 0),
                        "owner must select ESM or CommonJS for a Node file",
                    ));
                }
                Err(message) => self.diagnostics.push(Diagnostic::error(
                    DiagnosticCode::ModuleNotFound,
                    SourceSpan::new(module_id, 0, 0),
                    message,
                )),
            }
        }
        let module = if module_id.ends_with(".json") && self.resolve_json_module {
            match json::parse(&source, &self.limits) {
                Ok((module, schema)) => {
                    self.project
                        .json_modules
                        .insert(module_id.to_string(), schema);
                    self.parsed_modules.insert(module_id.to_string());
                    module
                }
                Err(diagnostic) => {
                    self.project
                        .failed_sources
                        .insert(module_id.to_string(), source.text);
                    self.diagnostics.push(diagnostic);
                    self.state.insert(module_id.to_string(), VisitState::Done);
                    return;
                }
            }
        } else if let Some(module) = self
            .previous
            .and_then(|previous| previous.modules.get(module_id))
            .filter(|module| module.source == source.text)
        {
            self.reused_parsed_modules.insert(module_id.to_string());
            module.clone()
        } else {
            self.parsed_modules.insert(module_id.to_string());
            let failed_source = source.text.clone();
            let parsed = if self.limits.parser == ParserLimits::default() {
                parse_module(source.id.clone(), source.text)
            } else {
                parse_module_with_limits(source.id.clone(), source.text, self.limits.parser.clone())
            };
            match parsed {
                Ok(module) => module,
                Err(mut parse_diagnostics) => {
                    self.project
                        .failed_sources
                        .insert(module_id.to_string(), failed_source);
                    self.diagnostics.append(&mut parse_diagnostics);
                    self.state.insert(module_id.to_string(), VisitState::Done);
                    return;
                }
            }
        };
        self.visit_augmentations(&module, depth);
        self.visit_references(&module, depth);
        for declaration in &module.declarations {
            let (specifier, span, mode) = match declaration {
                crate::parser::Declaration::Import(import) => (
                    &import.specifier,
                    &import.span,
                    import
                        .type_only
                        .then(|| {
                            import
                                .attributes
                                .as_ref()
                                .and_then(crate::ImportAttributes::resolution_mode)
                        })
                        .flatten(),
                ),
                crate::parser::Declaration::TypeExport(export) => {
                    let Some(specifier) = &export.specifier else {
                        continue;
                    };
                    (
                        specifier,
                        &export.span,
                        export
                            .attributes
                            .as_ref()
                            .and_then(crate::ImportAttributes::resolution_mode),
                    )
                }
                crate::parser::Declaration::ValueExport(export) => {
                    let Some(specifier) = &export.specifier else {
                        continue;
                    };
                    (
                        specifier,
                        export.specifier_span.as_ref().unwrap_or(&export.span),
                        None,
                    )
                }
                _ => continue,
            };
            if self.project.ambient_declaration_modules.contains(module_id) {
                self.diagnostics.push(Diagnostic::error(
                    DiagnosticCode::InvalidDeclarationFile,
                    span.clone(),
                    "ambient host declaration modules cannot import or re-export another module",
                ));
                continue;
            }
            let owner_mode = mode.or_else(|| {
                self.project.module_kinds.get(module_id).map(|kind| {
                    if *kind == ModuleKind::CommonJs {
                        crate::package_resolution::ImportMode::Require
                    } else {
                        crate::package_resolution::ImportMode::Import
                    }
                })
            });
            match self
                .loader
                .resolve_with_mode(module_id, specifier, owner_mode)
            {
                Ok(resolved) => {
                    if self
                        .project
                        .resolved_module_with_mode(module_id, specifier, mode)
                        .is_none()
                        && self.project.resolutions.len()
                            + self.project.mode_resolutions.len()
                            + self.project.augmentation_resolutions.len()
                            + self.project.reference_resolutions.len()
                            >= self.limits.max_module_edges
                    {
                        self.diagnostics.push(Diagnostic::error(
                            DiagnosticCode::ResourceLimit,
                            span.clone(),
                            format!(
                                "module graph exceeds the {} import-edge limit",
                                self.limits.max_module_edges
                            ),
                        ));
                        continue;
                    }
                    if let Some(mode) = mode {
                        self.project.mode_resolutions.insert(
                            (module_id.to_string(), specifier.clone(), mode),
                            resolved.clone(),
                        );
                    } else {
                        self.project
                            .resolutions
                            .insert((module_id.to_string(), specifier.clone()), resolved.clone());
                    }
                    self.visit(&resolved, depth.saturating_add(1));
                }
                Err(message) => {
                    self.unresolved_imports.push((
                        self.diagnostics.len(),
                        module_id.to_string(),
                        specifier.clone(),
                        mode,
                    ));
                    self.diagnostics.push(Diagnostic::error(
                        DiagnosticCode::ModuleNotFound,
                        span.clone(),
                        message,
                    ));
                }
            }
        }
        // A module that imports a namespace is parsed again with what the
        // namespace exports, so a reference to its members is one token.
        let mut imported_namespaces: Vec<(String, crate::parser::NamespaceTree)> = Vec::new();
        for declaration in &module.declarations {
            let crate::parser::Declaration::Import(import) = declaration else {
                continue;
            };
            let Some(resolved) = self.project.resolved_import(module_id, import) else {
                continue;
            };
            let Some(dependency) = self.project.modules.get(resolved) else {
                continue;
            };
            let trees = crate::parser::exported_namespace_trees(dependency);
            for binding in &import.bindings {
                if let Some(tree) = trees.get(&binding.imported) {
                    imported_namespaces.push((binding.local.clone(), tree.clone()));
                }
            }
        }
        let module = if imported_namespaces.is_empty() {
            module
        } else {
            match crate::parser::parse_module_with_namespaces(
                module.id.clone(),
                module.source.clone(),
                self.limits.parser.clone(),
                &imported_namespaces,
            ) {
                Ok(merged) => merged,
                Err(mut parse_diagnostics) => {
                    self.diagnostics.append(&mut parse_diagnostics);
                    module
                }
            }
        };
        self.project.modules.insert(module_id.to_string(), module);
        self.state.insert(module_id.to_string(), VisitState::Done);
    }

    pub(super) fn resolve_ambient_imports(&mut self) {
        let modules = self
            .project
            .ambient_modules()
            .into_keys()
            .collect::<BTreeSet<_>>();
        let mut removed = BTreeSet::new();
        for (index, from, specifier, mode) in &self.unresolved_imports {
            let target = format!("\0ambient-module:{specifier}");
            if !modules.contains(&target) {
                continue;
            }
            let key = (from.clone(), specifier.clone(), *mode);
            if !self.project.ambient_resolutions.contains_key(&key)
                && self.project.resolutions.len()
                    + self.project.mode_resolutions.len()
                    + self.project.ambient_resolutions.len()
                    + self.project.augmentation_resolutions.len()
                    + self.project.reference_resolutions.len()
                    >= self.limits.max_module_edges
            {
                self.diagnostics[*index] = Diagnostic::error(
                    DiagnosticCode::ResourceLimit,
                    self.diagnostics[*index].span.clone(),
                    format!(
                        "module graph exceeds the {} import-edge limit",
                        self.limits.max_module_edges
                    ),
                );
                continue;
            }
            self.project.ambient_resolutions.insert(key, target);
            removed.insert(*index);
        }
        let mut index = 0;
        self.diagnostics.retain(|_| {
            let keep = !removed.contains(&index);
            index += 1;
            keep
        });
    }
}
