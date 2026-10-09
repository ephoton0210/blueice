// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Per-edge static conditions retain owner-selected canonical identities.

use super::*;
use crate::package_resolution::ImportMode;
use crate::parser::{ImportAttributes, ImportDeclaration, TypeExportDeclaration};

impl Project {
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
            .flatten();
        self.resolution_target(from, &import.specifier, mode)
    }

    pub(crate) fn resolved_type_export(
        &self,
        from: &str,
        export: &TypeExportDeclaration,
    ) -> Option<&String> {
        self.resolution_target(
            from,
            export.specifier.as_ref()?,
            export
                .attributes
                .as_ref()
                .and_then(ImportAttributes::resolution_mode),
        )
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
    }
}
