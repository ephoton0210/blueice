// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Explicit JavaScript source probes retain TypeScript sibling precedence.

use super::*;

impl<F: PackageFs> PackageResolver<F> {
    /// Probes a JavaScript source after the ordinary TypeScript candidates.
    /// The filesystem owner decides whether to load the resulting source.
    pub fn resolve_relative_javascript(
        &self,
        from_dir: &Path,
        specifier: &str,
    ) -> Result<ResolvedPackageFile, ResolveError> {
        match self.resolve_relative(from_dir, specifier) {
            Err(error @ (ResolveError::NotFound { .. } | ResolveError::JavaScriptOnly { .. })) => {
                let joined = normalize(&from_dir.join(specifier));
                let candidate = if joined
                    .extension()
                    .is_some_and(|extension| extension == "js")
                {
                    joined
                } else {
                    joined.with_extension("js")
                };
                if !self.probe_file(&candidate) {
                    return Err(error);
                }
                let path = self.confine(specifier, &candidate)?;
                Ok(ResolvedPackageFile {
                    path,
                    declaration: false,
                    package: None,
                })
            }
            result => result,
        }
    }
}
