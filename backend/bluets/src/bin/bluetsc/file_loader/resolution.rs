// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Preserve structured resolution causes without another lookup or source load.
use super::*;

pub(super) struct ResolutionFailure {
    pub(super) message: String,
    pub(super) missing: bool,
    pub(super) javascript_path: Option<String>,
}

impl From<String> for ResolutionFailure {
    fn from(message: String) -> Self {
        Self {
            missing: message.starts_with("cannot resolve "),
            message,
            javascript_path: None,
        }
    }
}

impl ResolutionFailure {
    fn package(error: ResolveError, specifier: &str, from: &str, relative: bool) -> Self {
        let missing = matches!(
            error,
            ResolveError::NotFound { .. } | ResolveError::ExportsNotDefined { .. }
        );
        let javascript_path = match &error {
            ResolveError::JavaScriptOnly { path, .. } => Some(path.display().to_string()),
            _ => None,
        };
        let message = if relative && matches!(error, ResolveError::OutsideRoots { .. }) {
            format!("specifier `{specifier}` resolves outside the declared project root (outside the authorized package roots)")
        } else if matches!(
            error,
            ResolveError::NotFound { .. } | ResolveError::JavaScriptOnly { .. }
        ) || !relative && matches!(error, ResolveError::ExportsNotDefined { .. })
        {
            format!("cannot resolve `{specifier}` from `{from}`: {error}")
        } else {
            format!("`{specifier}` from `{from}`: {error}")
        };
        Self {
            message,
            missing,
            javascript_path,
        }
    }
}

impl FileLoader {
    pub(super) fn resolve_owned(
        &self,
        from_module: &str,
        specifier: &str,
        mode: Option<ImportMode>,
        jsx_query: bool,
    ) -> Result<String, ResolutionFailure> {
        let is_relative = matches!(specifier, "." | "..")
            || specifier.starts_with("./")
            || specifier.starts_with("../");
        if from_module.starts_with("@remote/") {
            return Err(format!(
                "remote declaration `{from_module}` cannot import `{specifier}`: a remote \
                 declaration must be self-contained"
            )
            .into());
        }
        if let Some((_, sources)) = &self.remote {
            if let Some(source) = sources.iter().find(|source| source.specifier == specifier) {
                return Ok(format!("@remote/{}.d.ts", source.sha256));
            }
        }
        if is_relative {
            if specifier.ends_with(".json") && !self.resolve_json_module {
                return Err(format!("JSON source `{specifier}` requires resolveJsonModule").into());
            }
            let from_directory = self
                .source_path(from_module)?
                .parent()
                .map(Path::to_path_buf)
                .ok_or_else(|| format!("module `{from_module}` has no directory"))?;
            if let Some(reference) = self.reference_import(&from_directory, specifier) {
                return Ok(reference);
            }
            let resolver = self.packages.as_ref().unwrap_or(&self.relative);
            let found = if self.resolve_json_module && specifier.ends_with(".json") {
                resolver.resolve_relative_json(&from_directory, specifier)
            } else {
                resolver.resolve_relative_javascript(&from_directory, specifier)
            };
            let found = match found {
                Err(ResolveError::NotFound { .. }) if self.mappings.is_some() => {
                    match self
                        .mappings
                        .as_ref()
                        .expect("configured mappings")
                        .relative(self, &from_directory, specifier)?
                    {
                        Some(path) => {
                            return Ok(
                                self.inherit_javascript_depth(from_module, self.module_id(&path)?)
                            )
                        }
                        None => found,
                    }
                }
                other => other,
            }
            .map_err(|error| ResolutionFailure::package(error, specifier, from_module, true))?;
            if is_javascript(&found.path) && !self.allow_js {
                return Err(ResolutionFailure {
                    message: format!(
                        "JavaScript source `{specifier}` requires allowJs: {}",
                        found.path.display()
                    ),
                    missing: false,
                    javascript_path: Some(found.path.display().to_string()),
                });
            }
            let id = if blueice_bluets::is_external_library_module(from_module) {
                self.package_module_id(&found.path)
            } else {
                self.module_id(&found.path)
            }?;
            let id = if (specifier.ends_with(".cjs") || specifier.ends_with(".mjs"))
                && !blueice_bluets::is_external_library_module(&id)
                && resolver
                    .resolve_runtime_sibling(&found.path)
                    .map_err(|error| error.to_string())?
                    .is_some()
            {
                format!("@node-runtime/{id}")
            } else {
                id
            };
            return Ok(self.inherit_javascript_depth(from_module, id));
        }
        if !self.imports.contains_key(specifier) && !self.matches_import_prefix(specifier) {
            if let Some(mappings) = &self.mappings {
                if let Some(path) = mappings.bare(self, specifier)? {
                    return Ok(self.inherit_javascript_depth(from_module, self.module_id(&path)?));
                }
            }
        }
        if let Some(packages) = &self.packages {
            let from_directory = self
                .source_path(from_module)?
                .parent()
                .map(Path::to_path_buf)
                .ok_or_else(|| format!("module `{from_module}` has no directory"))?;
            if !self.imports.contains_key(specifier) && !self.matches_import_prefix(specifier) {
                let result = if self.allow_js || jsx_query {
                    packages.resolve_javascript(
                        &from_directory,
                        specifier,
                        mode.unwrap_or(self.import_mode),
                    )
                } else {
                    packages.resolve(&from_directory, specifier, mode.unwrap_or(self.import_mode))
                };
                return match result {
                    Ok(found) => {
                        if jsx_query && !self.allow_js && is_javascript(&found.path) {
                            return Err(ResolutionFailure {
                                message: format!(
                                    "JavaScript source `{specifier}` requires allowJs: {}",
                                    found.path.display()
                                ),
                                missing: false,
                                javascript_path: Some(found.path.display().to_string()),
                            });
                        }
                        let internal = found
                            .package
                            .as_ref()
                            .is_none_or(|package| from_directory.starts_with(&package.root));
                        let depth = self
                            .javascript_depths
                            .lock()
                            .expect("JavaScript depths")
                            .get(from_module)
                            .copied()
                            .unwrap_or(0)
                            + usize::from(!internal);
                        if is_javascript(&found.path) && depth > self.max_node_module_js_depth {
                            return Err(format!(
                                "JavaScript source `{specifier}` exceeds maxNodeModuleJsDepth: {}",
                                found.path.display()
                            )
                            .into());
                        }
                        let id = if internal
                            && !blueice_bluets::is_external_library_module(from_module)
                        {
                            self.module_id(&found.path)
                        } else {
                            self.package_module_id(&found.path)
                        }?;
                        self.javascript_depths
                            .lock()
                            .expect("JavaScript depths")
                            .entry(id.clone())
                            .and_modify(|old| *old = (*old).min(depth))
                            .or_insert(depth);
                        Ok(id)
                    }
                    Err(error) => Err(ResolutionFailure::package(
                        error,
                        specifier,
                        from_module,
                        false,
                    )),
                };
            }
        }
        let candidate = self.resolve_import_map(specifier)?;
        let resolved = fs::canonicalize(&candidate).map_err(|error| {
            format!("cannot resolve `{specifier}` from `{from_module}`: {error}")
        })?;
        if !resolved.starts_with(&self.root) {
            return Err(format!(
                "specifier `{specifier}` resolves outside the declared project root"
            )
            .into());
        }
        Ok(self.inherit_javascript_depth(from_module, self.module_id(&resolved)?))
    }
}
