// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Owner-authorized file loading and canonical source identities.

use super::*;

pub(super) struct FileLoader {
    pub(super) root: PathBuf,
    pub(super) imports: BTreeMap<String, PathBuf>,
    /// Authorized package roots beyond `root`; a file under one has the module
    /// id `@external/<index>/<path inside it>`.
    pub(super) extra_roots: Vec<PathBuf>,
    /// Present only when the owner configured a `moduleResolution`.
    pub(super) packages: Option<PackageResolver<OsPackageFs>>,
    /// Relative file probing stays root-confined even when bare package
    /// resolution was not configured by the owner.
    pub(super) relative: PackageResolver<OsPackageFs>,
    pub(super) resolve_json_module: bool,
    /// The verified cache and the pinned sources whose specifiers resolve to it.
    pub(super) remote: Option<(DeclarationCache, Vec<RemoteDeclarationSource>)>,
    pub(super) import_mode: ImportMode,
}

impl ModuleLoader for FileLoader {
    fn implied_module_kind(&self, module_id: &str) -> Result<ModuleKind, String> {
        node_modules::implied_kind(self, module_id)
    }
    fn load(&self, module_id: &str) -> Result<ModuleSource, String> {
        if let Some(pin) = module_id
            .strip_prefix("@remote/")
            .and_then(|rest| rest.strip_suffix(".d.ts"))
        {
            let (cache, sources) = self.remote.as_ref().ok_or_else(|| {
                format!("module `{module_id}` is not an authorized remote declaration")
            })?;
            if !sources.iter().any(|source| source.sha256 == pin) {
                return Err(format!(
                    "module `{module_id}` is not an authorized remote declaration"
                ));
            }
            // Re-verified against its pin on every read.
            let text = cache
                .read(pin)
                .map_err(|error| error.to_string())?
                .ok_or_else(|| format!("remote declaration {pin} is not cached"))?;
            return Ok(ModuleSource::new(module_id, text));
        }
        let path = self.source_path(module_id)?;
        if !matches!(
            path.extension().and_then(|extension| extension.to_str()),
            Some("ts" | "tsx" | "mts" | "cts" | "json")
        ) {
            return Err(format!(
                "module `{module_id}` is not a supported .ts, .tsx, or .d.ts source file"
            ));
        }
        if path
            .extension()
            .is_some_and(|extension| extension == "json")
            && !self.resolve_json_module
        {
            return Err(format!(
                "JSON source `{module_id}` requires resolveJsonModule"
            ));
        }
        let text = fs::read_to_string(&path).map_err(|error| error.to_string())?;
        Ok(ModuleSource::new(module_id, text))
    }

    fn resolve(&self, from_module: &str, specifier: &str) -> Result<String, String> {
        self.resolve_with_mode(from_module, specifier, None)
    }

    fn resolve_with_mode(
        &self,
        from_module: &str,
        specifier: &str,
        mode: Option<ImportMode>,
    ) -> Result<String, String> {
        let is_relative = matches!(specifier, "." | "..")
            || specifier.starts_with("./")
            || specifier.starts_with("../");
        if from_module.starts_with("@remote/") {
            return Err(format!(
                "remote declaration `{from_module}` cannot import `{specifier}`: a remote \
                 declaration must be self-contained"
            ));
        }
        if let Some((_, sources)) = &self.remote {
            if let Some(source) = sources.iter().find(|source| source.specifier == specifier) {
                return Ok(format!("@remote/{}.d.ts", source.sha256));
            }
        }
        if is_relative {
            if specifier.ends_with(".json") && !self.resolve_json_module {
                return Err(format!("module `{specifier}` is not a supported .ts, .tsx, or .d.ts source file; enable resolveJsonModule for JSON data"));
            }
            let from_directory = self
                .source_path(from_module)?
                .parent()
                .map(Path::to_path_buf)
                .ok_or_else(|| format!("module `{from_module}` has no directory"))?;
            let resolver = self.packages.as_ref().unwrap_or(&self.relative);
            let found = if self.resolve_json_module && specifier.ends_with(".json") {
                resolver.resolve_relative_json(&from_directory, specifier)
            } else {
                resolver.resolve_relative(&from_directory, specifier)
            }
            .map_err(|error| match error {
                error @ (ResolveError::NotFound { .. } | ResolveError::JavaScriptOnly { .. }) => {
                    format!("cannot resolve `{specifier}` from `{from_module}`: {error}")
                }
                error @ ResolveError::OutsideRoots { .. } => format!(
                    "specifier `{specifier}` resolves outside the declared project root: {error}"
                ),
                error => format!("`{specifier}` from `{from_module}`: {error}"),
            })?;
            return if blueice_bluets::is_external_library_module(from_module) {
                self.package_module_id(&found.path)
            } else {
                self.module_id(&found.path)
            };
        }
        if let Some(packages) = &self.packages {
            let from_directory = self
                .source_path(from_module)?
                .parent()
                .map(Path::to_path_buf)
                .ok_or_else(|| format!("module `{from_module}` has no directory"))?;
            if !self.imports.contains_key(specifier) && !self.matches_import_prefix(specifier) {
                return match packages.resolve(
                    &from_directory,
                    specifier,
                    mode.unwrap_or(self.import_mode),
                ) {
                    Ok(found) => self.package_module_id(&found.path),
                    Err(
                        error @ (ResolveError::NotFound { .. }
                        | ResolveError::JavaScriptOnly { .. }),
                    ) => Err(format!(
                        "cannot resolve `{specifier}` from `{from_module}`: {error}"
                    )),
                    Err(error) => Err(format!("`{specifier}` from `{from_module}`: {error}")),
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
            ));
        }
        self.module_id(&resolved)
    }

    fn resolve_reference_types(&self, from_module: &str, name: &str) -> Result<String, String> {
        if from_module.starts_with("@remote/") {
            return Err("remote declarations cannot resolve type references".to_string());
        }
        let directory = self
            .source_path(from_module)?
            .parent()
            .map(Path::to_path_buf)
            .ok_or_else(|| format!("module `{from_module}` has no directory"))?;
        let resolver = self.packages.as_ref().unwrap_or(&self.relative);
        let package = blueice_bluets::package_resolution::types_package_name(name);
        let found = resolver
            .resolve(&directory, &package, self.import_mode)
            .map_err(|error| error.to_string())?;
        self.package_module_id(&found.path)
    }

    fn resolution_fingerprint(&self) -> String {
        self.packages
            .as_ref()
            .unwrap_or(&self.relative)
            .fingerprint()
    }
}

impl FileLoader {
    /// The module id of a file found by package resolution: ordinary, except
    /// that a package reached through a symlink to a directory with no
    /// `node_modules` in its path is still marked as an installed package.
    /// A relative import from inside a package stays in its marking.
    fn package_module_id(&self, path: &Path) -> Result<String, String> {
        let id = self.module_id(path)?;
        Ok(if blueice_bluets::is_external_library_module(&id) {
            id
        } else {
            format!("@package/{id}")
        })
    }

    fn matches_import_prefix(&self, specifier: &str) -> bool {
        self.imports
            .keys()
            .any(|prefix| prefix.ends_with('/') && specifier.starts_with(prefix.as_str()))
    }

    pub(super) fn source_path(&self, module_id: &str) -> Result<PathBuf, String> {
        let module_path = Path::new(module_id);
        if module_path.is_absolute()
            || module_path
                .components()
                .any(|component| matches!(component, std::path::Component::ParentDir))
        {
            return Err(format!(
                "module `{module_id}` escapes the declared project root"
            ));
        }
        let module_id = module_id.strip_prefix("@package/").unwrap_or(module_id);
        let module_path = Path::new(module_id);
        let (base, relative) = match module_id
            .strip_prefix("@external/")
            .and_then(|rest| rest.split_once('/'))
        {
            Some((index, relative)) => {
                let base = index
                    .parse::<usize>()
                    .ok()
                    .and_then(|index| self.extra_roots.get(index))
                    .ok_or_else(|| {
                        format!("module `{module_id}` names no authorized package root")
                    })?;
                (base, Path::new(relative))
            }
            None => (&self.root, module_path),
        };
        let path = fs::canonicalize(base.join(relative))
            .map_err(|error| format!("cannot read module `{module_id}`: {error}"))?;
        if !path.starts_with(base) {
            return Err(format!(
                "module `{module_id}` escapes the declared project root"
            ));
        }
        Ok(path)
    }

    fn module_id(&self, path: &Path) -> Result<String, String> {
        if let Ok(relative) = path.strip_prefix(&self.root) {
            return Ok(output_path(relative));
        }
        for (index, root) in self.extra_roots.iter().enumerate() {
            if let Ok(relative) = path.strip_prefix(root) {
                return Ok(format!("@external/{index}/{}", output_path(relative)));
            }
        }
        Err(format!(
            "module {} escapes the declared project root",
            path.display()
        ))
    }

    fn resolve_import_map(&self, specifier: &str) -> Result<PathBuf, String> {
        if let Some(target) = self.imports.get(specifier) {
            return Ok(target.clone());
        }
        let Some((prefix, target)) = self
            .imports
            .iter()
            .filter(|(prefix, _)| prefix.ends_with('/') && specifier.starts_with(prefix.as_str()))
            .max_by_key(|(prefix, _)| prefix.len())
        else {
            return Err(format!(
                "bare specifier `{specifier}` is unsupported; add an exact or trailing-slash `imports` mapping"
            ));
        };
        Ok(target.join(&specifier[prefix.len()..]))
    }
}
