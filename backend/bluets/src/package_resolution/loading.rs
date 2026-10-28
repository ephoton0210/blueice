// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! File probes shared by typed and explicitly enabled JavaScript passes.

use super::*;

impl<F: PackageFs> PackageResolver<F> {
    /// The first of an entry's targets that is a file, with TypeScript's
    /// substitution of a JavaScript extension for its source or declaration.
    pub(super) fn load_export_targets(
        &self,
        package_dir: &Path,
        targets: &Targets,
    ) -> Option<PathBuf> {
        for target in targets {
            let target = target.as_ref()?;
            let path = package_dir.join(target.strip_prefix("./").unwrap_or(target));
            for candidate in self.candidates(&path, true) {
                if self.probe_file(&candidate) {
                    return Some(candidate);
                }
            }
        }
        None
    }

    /// `types`/`typings`/`main`, then `index`, of a package or directory.
    pub(super) fn load_package_main(
        &self,
        directory: &Path,
        manifest: Option<&Value>,
    ) -> Result<Option<PathBuf>, ResolveError> {
        if let Some(manifest) = manifest {
            for field in ["typings", "types", "main"] {
                let Some(entry) = manifest.get(field).and_then(Value::as_str) else {
                    continue;
                };
                if entry.is_empty() {
                    continue;
                }
                // A manifest field never leads out of its own package, and the
                // probe does not even look at what is outside it.
                if Path::new(entry).components().any(|component| {
                    matches!(
                        component,
                        Component::ParentDir | Component::RootDir | Component::Prefix(_)
                    )
                }) {
                    continue;
                }
                let path = directory.join(entry.trim_start_matches("./"));
                if let Some(found) = self.load_as_file_or_directory(&path)? {
                    return Ok(Some(found));
                }
            }
        }
        self.load_index(directory)
    }

    pub(super) fn load_index(&self, directory: &Path) -> Result<Option<PathBuf>, ResolveError> {
        for extension in TYPESCRIPT_EXTENSIONS
            .iter()
            .copied()
            .chain(self.javascript.then_some(".js"))
        {
            let candidate = directory.join(format!("index{extension}"));
            if self.probe_file(&candidate) {
                return Ok(Some(candidate));
            }
        }
        Ok(None)
    }

    pub(super) fn load_as_file_or_directory(
        &self,
        path: &Path,
    ) -> Result<Option<PathBuf>, ResolveError> {
        if let Some(found) = self.load_as_file(path) {
            return Ok(Some(found));
        }
        if self.fs.is_dir(path) {
            let manifest = self.read_package_json(path)?;
            return self.load_package_main(path, manifest.as_ref());
        }
        self.observe(path, Observation::Absent);
        Ok(None)
    }

    /// A file by its name, with the TypeScript extensions added, or a
    /// JavaScript extension substituted.
    pub(super) fn load_as_file(&self, path: &Path) -> Option<PathBuf> {
        self.candidates(path, false)
            .into_iter()
            .find(|candidate| self.probe_file(candidate))
    }
}
