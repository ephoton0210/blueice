// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Per-edge static conditions retain owner-selected canonical identities.

use super::*;
use crate::package_resolution::ImportMode;
use crate::parser::{ImportAttributes, ImportDeclaration, TypeExportDeclaration};

impl Project {
    /// Already-authorized erased import queries, without filesystem access.
    pub fn resolved_import_queries<'a>(
        &'a self,
        from: &'a str,
    ) -> impl Iterator<Item = (&'a str, &'a str)> + 'a {
        self.modules
            .get(from)
            .into_iter()
            .flat_map(|module| &module.import_types)
            .filter_map(move |reference| {
                self.resolved_module(from, &reference.specifier)
                    .map(|target| (reference.specifier.as_str(), target))
            })
    }
    /// Iterates already-authorized reference edges as (source, kind, name, target).
    /// This is immutable graph metadata: it performs no I/O or new resolution.
    pub fn resolved_source_references(&self) -> impl Iterator<Item = (&str, &str, &str, &str)> {
        self.reference_resolutions
            .iter()
            .map(|((source, kind, name), target)| {
                (
                    source.as_str(),
                    kind.as_str(),
                    name.as_str(),
                    target.as_str(),
                )
            })
    }

    /// Returns a previously authorized target for this exact static condition.
    /// This neither reads a source nor grants a runtime dependency.
    pub fn resolved_module_with_mode(
        &self,
        from_module: &str,
        specifier: &str,
        mode: Option<ImportMode>,
    ) -> Option<&str> {
        self.resolution_target(from_module, specifier, mode)
            .map(String::as_str)
    }

    fn resolution_target(
        &self,
        from: &str,
        specifier: &str,
        mode: Option<ImportMode>,
    ) -> Option<&String> {
        if let Some(mode) = mode {
            self.mode_resolutions
                .get(&(from.to_string(), specifier.to_string(), mode))
        } else {
            self.resolutions
                .get(&(from.to_string(), specifier.to_string()))
        }
    }

    pub(crate) fn resolved_import(
        &self,
        from: &str,
        import: &ImportDeclaration,
    ) -> Option<&String> {
        let mode = import
            .type_only
            .then(|| {
                import
                    .attributes
                    .as_ref()
                    .and_then(ImportAttributes::resolution_mode)
            })
            .flatten()
            .or_else(|| import.equals_require.then_some(ImportMode::Require));
        self.resolution_target(from, &import.specifier, mode)
            .or_else(|| {
                self.ambient_resolutions
                    .get(&(from.to_string(), import.specifier.clone(), mode))
            })
    }

    /// Static type queries reuse the same authorized and ambient edge ledger.
    pub(crate) fn resolved_import_type(&self, from: &str, specifier: &str) -> Option<&String> {
        self.resolution_target(from, specifier, None).or_else(|| {
            self.ambient_resolutions
                .get(&(from.to_string(), specifier.to_string(), None))
        })
    }

    pub(crate) fn resolved_type_export(
        &self,
        from: &str,
        export: &TypeExportDeclaration,
    ) -> Option<&String> {
        let mode = export
            .attributes
            .as_ref()
            .and_then(ImportAttributes::resolution_mode);
        self.resolution_target(from, export.specifier.as_ref()?, mode)
            .or_else(|| {
                self.ambient_resolutions
                    .get(&(from.to_string(), export.specifier.clone()?, mode))
            })
    }

    pub(crate) fn resolution_edges(&self) -> impl Iterator<Item = (&str, &str)> {
        self.resolutions
            .iter()
            .map(|((from, _), target)| (from.as_str(), target.as_str()))
            .chain(
                self.mode_resolutions
                    .iter()
                    .map(|((from, _, _), target)| (from.as_str(), target.as_str())),
            )
            .chain(
                self.ambient_resolutions
                    .iter()
                    .flat_map(|((from, _, _), target)| {
                        self.ambient_module_bodies(target)
                            .into_iter()
                            .map(move |body| (from.as_str(), body.span.module.as_str()))
                    }),
            )
            .chain(
                self.augmentation_resolutions
                    .iter()
                    .flat_map(|((from, _), target)| {
                        [
                            (from.as_str(), target.as_str()),
                            (target.as_str(), from.as_str()),
                        ]
                    }),
            )
            .chain(
                self.reference_resolutions
                    .iter()
                    .map(|((from, _, _), target)| (from.as_str(), target.as_str())),
            )
    }

    pub(crate) fn ambient_modules(
        &self,
    ) -> BTreeMap<String, Vec<&crate::parser::AmbientDeclaration>> {
        let mut result: BTreeMap<String, Vec<&crate::parser::AmbientDeclaration>> = BTreeMap::new();
        for module in self
            .modules
            .values()
            .filter(|module| !crate::parser::has_module_syntax(&module.declarations))
        {
            for declaration in &module.declarations {
                let crate::parser::Declaration::Ambient(item) = declaration else {
                    continue;
                };
                let Some(name) = &item.specifier else {
                    continue;
                };
                if name.starts_with('.') || name.starts_with('/') {
                    continue;
                }
                result
                    .entry(format!("\0ambient-module:{name}"))
                    .or_default()
                    .push(item);
            }
        }
        result
    }

    pub(crate) fn ambient_module_bodies(
        &self,
        target: &str,
    ) -> Vec<&crate::parser::AmbientDeclaration> {
        if !target.starts_with("\0ambient-module:") {
            return Vec::new();
        }
        self.ambient_modules().remove(target).unwrap_or_default()
    }
}
