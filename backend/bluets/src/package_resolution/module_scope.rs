// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Nearest package format with canonical and absent-manifest observations.

use super::*;

impl<F: PackageFs> PackageResolver<F> {
    /// Inspect the nearest package manifest within the existing owner roots.
    /// Absent candidates, canonical targets and bytes affect revalidation.
    pub fn is_es_module_scope(&self, directory: &Path) -> Result<bool, ResolveError> {
        if !self.within_roots(directory) {
            return Err(ResolveError::OutsideRoots {
                specifier: "package.json".to_string(),
                path: directory.to_path_buf(),
            });
        }
        let mut current = Some(directory);
        while let Some(directory) = current.filter(|directory| self.within_roots(directory)) {
            if directory
                .file_name()
                .is_some_and(|name| name == "node_modules")
            {
                break;
            }
            let candidate = directory.join("package.json");
            if self.probe_file(&candidate) {
                let canonical = self.confine("package.json", &candidate)?;
                let text = self.fs.read_to_string(&canonical).map_err(|error| {
                    ResolveError::InvalidPackageJson {
                        path: canonical.clone(),
                        message: error.to_string(),
                    }
                })?;
                let manifest = serde_json::from_str::<Value>(&text).map_err(|error| {
                    ResolveError::InvalidPackageJson {
                        path: canonical.clone(),
                        message: error.to_string(),
                    }
                })?;
                self.observe(
                    &candidate,
                    Observation::Manifest {
                        canonical,
                        hash: sha256_hex(text.as_bytes()),
                    },
                );
                return Ok(manifest.get("type").and_then(Value::as_str) == Some("module"));
            }
            current = directory.parent();
        }
        Ok(false)
    }
}
