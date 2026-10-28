// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Opt-in JavaScript probing preserves the typed pass and its authority checks.

use super::*;

struct BorrowedFs<'a, F>(&'a F);
impl<F: PackageFs> PackageFs for BorrowedFs<'_, F> {
    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        self.0.read_to_string(path)
    }
    fn is_file(&self, path: &Path) -> bool {
        self.0.is_file(path)
    }
    fn is_dir(&self, path: &Path) -> bool {
        self.0.is_dir(path)
    }
    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        self.0.canonicalize(path)
    }
}

impl<F: PackageFs> PackageResolver<F> {
    fn javascript_pass<T>(&self, run: impl FnOnce(&PackageResolver<BorrowedFs<'_, F>>) -> T) -> T {
        let mut resolver = PackageResolver::new(BorrowedFs(&self.fs), self.config.clone());
        resolver.javascript = true;
        let result = run(&resolver);
        self.observed
            .lock()
            .expect("observation lock")
            .extend(resolver.observed.into_inner().expect("observation lock"));
        self.packages
            .lock()
            .expect("package lock")
            .extend(resolver.packages.into_inner().expect("package lock"));
        result
    }

    /// Search ordinary source/declaration candidates, including @types, first.
    /// A refused authority or blocked exports entry never retries.
    pub fn resolve_javascript(
        &self,
        from: &Path,
        specifier: &str,
        mode: ImportMode,
    ) -> Result<ResolvedPackageFile, ResolveError> {
        match self.resolve(from, specifier, mode) {
            Err(ResolveError::NotFound { .. } | ResolveError::JavaScriptOnly { .. }) => {
                self.javascript_pass(|resolver| resolver.resolve(from, specifier, mode))
            }
            result => result,
        }
    }

    /// Relative imports retain TypeScript extension substitution first.
    pub fn resolve_relative_javascript(
        &self,
        from: &Path,
        specifier: &str,
    ) -> Result<ResolvedPackageFile, ResolveError> {
        match self.resolve_relative(from, specifier) {
            Err(ResolveError::NotFound { .. } | ResolveError::JavaScriptOnly { .. }) => {
                self.javascript_pass(|resolver| resolver.resolve_relative(from, specifier))
            }
            result => result,
        }
    }

    /// An explicit existing paths target is exact before extension substitution.
    pub fn resolve_mapped_javascript(
        &self,
        from: &Path,
        specifier: &str,
    ) -> Result<ResolvedPackageFile, ResolveError> {
        if !self.within_roots(from) {
            return Err(ResolveError::OutsideRoots {
                specifier: specifier.into(),
                path: from.into(),
            });
        }
        let path = normalize(&from.join(specifier));
        if javascript_file(&path) && self.probe_file(&path) {
            return Ok(ResolvedPackageFile {
                path: self.confine(specifier, &path)?,
                declaration: false,
                package: None,
            });
        }
        self.resolve_relative_javascript(from, specifier)
    }

    pub(super) fn candidates(&self, path: &Path, exact: bool) -> Vec<PathBuf> {
        let mut candidates = if exact {
            exact_candidates(path)
        } else {
            file_candidates(path)
        };
        if self.javascript {
            if javascript_file(path) {
                candidates.push(path.into());
            } else if path.extension().is_none() && !exact {
                candidates.push(path.with_extension("js"));
            }
        }
        candidates
    }
}

fn javascript_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|extension| extension.to_str()),
        Some("js" | "mjs" | "cjs")
    )
}
