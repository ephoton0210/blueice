// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Installed-package resolution (J.4.2, J.4.3): TypeScript 5.9.3's `node10`,
//! `node16` and `bundler` module resolution of a bare specifier, against a
//! dependency tree the owner authorized.
//!
//! The resolver never installs anything and never leaves the owner's canonical
//! roots. It looks only at directories inside the authorized roots (the
//! `node_modules` of each ancestor of the importing file, nearest first), in
//! TypeScript's order: the package itself, then its `@types` package. A package
//! directory is canonicalized before anything inside it is read, and a result
//! whose canonical path is outside every root is an error, not a reason to try a
//! farther candidate, so a symlink cannot turn a lookup into a read of
//! something the owner did not authorize.
//!
//! Every file the lookup depended on is recorded: each `package.json` read (with
//! its content hash) and each candidate that was probed and absent. That record
//! is the resolution's fingerprint, and [`PackageResolver::revalidate`] re-checks
//! it, so a closer package appearing, a `package.json` changing, or a symlink
//! being repointed invalidates what was cached.

use std::collections::BTreeMap;
use std::fmt;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;

use ring::digest::{digest, SHA256};
use serde::de::{Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};

mod features;
mod javascript;
mod module_scope;
mod version_ranges;

/// The TypeScript release whose resolution this follows.
pub const RESOLUTION_VERSION: &str = "typescript-5.9.3-resolution-v3";

/// TypeScript's `moduleResolution` for a bare specifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModuleResolution {
    /// File lookup in the importing directory and its authorized ancestors.
    Classic,
    /// `main`/`types`/`typings`, then `index`; no `exports`.
    Node10,
    /// `exports` and `imports` with the conditions `types`, `node` and the
    /// import mode.
    Node16,
    /// `exports` and `imports` with the conditions `types` and the import mode.
    Bundler,
}

impl ModuleResolution {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Classic => "classic",
            Self::Node10 => "node10",
            Self::Node16 => "node16",
            Self::Bundler => "bundler",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "classic" => Some(Self::Classic),
            "node10" | "node" => Some(Self::Node10),
            "node16" | "nodenext" => Some(Self::Node16),
            "bundler" => Some(Self::Bundler),
            _ => None,
        }
    }
}

/// Whether the importing file asks for the `import` or the `require` condition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ImportMode {
    Import,
    Require,
}

impl ImportMode {
    fn condition(self) -> &'static str {
        match self {
            Self::Import => "import",
            Self::Require => "require",
        }
    }
}

/// The file system the resolver reads. It is a trait so the same algorithm runs
/// on the real disk and on an in-memory tree.
pub trait PackageFs {
    fn read_to_string(&self, path: &Path) -> io::Result<String>;
    fn is_file(&self, path: &Path) -> bool;
    fn is_dir(&self, path: &Path) -> bool;
    /// The absolute path with every symlink resolved.
    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf>;
}

/// The real disk.
#[derive(Debug, Default, Clone, Copy)]
pub struct OsPackageFs;

impl PackageFs for OsPackageFs {
    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        std::fs::read_to_string(path)
    }
    fn is_file(&self, path: &Path) -> bool {
        path.is_file()
    }
    fn is_dir(&self, path: &Path) -> bool {
        path.is_dir()
    }
    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        std::fs::canonicalize(path)
    }
}

#[derive(Debug, Clone)]
pub struct PackageResolverConfig {
    /// Canonical directories the owner authorized: the project root and any
    /// other tree dependencies may be read from. Nothing outside them is read.
    pub roots: Vec<PathBuf>,
    pub resolution: ModuleResolution,
    /// TypeScript's `customConditions`.
    pub custom_conditions: Vec<String>,
}

/// What a resolved package file is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageIdentity {
    pub name: String,
    pub version: Option<String>,
    /// The canonical package directory.
    pub root: PathBuf,
    /// Whether the file came from the `@types` package of `name`.
    pub from_types_package: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedPackageFile {
    /// The canonical path of the file, inside an authorized root.
    pub path: PathBuf,
    /// Whether it is a `.d.ts` file (type-only: no runtime code).
    pub declaration: bool,
    pub package: Option<PackageIdentity>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveError {
    InvalidSpecifier(String),
    /// Not found; the package directories that were looked in.
    NotFound {
        specifier: String,
        searched: Vec<PathBuf>,
    },
    /// A candidate's canonical path is outside every authorized root.
    OutsideRoots {
        specifier: String,
        path: PathBuf,
    },
    InvalidPackageJson {
        path: PathBuf,
        message: String,
    },
    /// The package's `exports` has no entry (or no matching condition) for the
    /// subpath.
    ExportsNotDefined {
        package: String,
        subpath: String,
    },
    /// Only JavaScript was found, which BlueTS does not run through a package
    /// import without a declaration.
    JavaScriptOnly {
        specifier: String,
        path: PathBuf,
    },
    ImportsNotDefined {
        specifier: String,
    },
}

impl fmt::Display for ResolveError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSpecifier(specifier) => {
                write!(formatter, "`{specifier}` is not a valid package specifier")
            }
            Self::NotFound {
                specifier,
                searched,
            } => {
                write!(
                    formatter,
                    "cannot find an installed package for `{specifier}`; searched {} authorized \
                     package location(s) (nothing is installed implicitly)",
                    searched.len()
                )
            }
            Self::OutsideRoots { specifier, path } => write!(
                formatter,
                "`{specifier}` resolves to {}, outside the authorized package roots",
                path.display()
            ),
            Self::InvalidPackageJson { path, message } => {
                write!(formatter, "invalid {}: {message}", path.display())
            }
            Self::ExportsNotDefined { package, subpath } => write!(
                formatter,
                "package `{package}` does not export `{subpath}` for the active conditions"
            ),
            Self::JavaScriptOnly { specifier, path } => write!(
                formatter,
                "`{specifier}` resolves only to JavaScript ({}); install or configure its \
                 declarations",
                path.display()
            ),
            Self::ImportsNotDefined { specifier } => {
                write!(
                    formatter,
                    "no package `imports` entry matches `{specifier}`"
                )
            }
        }
    }
}

impl std::error::Error for ResolveError {}

/// What a lookup depended on.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Observation {
    /// A candidate that was probed and was not a file.
    Absent,
    /// An authorized asset whose content was read, and its SHA-256.
    Content(String),
    /// A directory resolved through symlinks to this canonical path.
    Canonical(PathBuf),
    /// A module-scope manifest retains both its target and content identity.
    Manifest { canonical: PathBuf, hash: String },
}

pub struct PackageResolver<F: PackageFs> {
    fs: F,
    config: PackageResolverConfig,
    observed: Mutex<BTreeMap<PathBuf, Observation>>,
    packages: Mutex<BTreeMap<PathBuf, PackageIdentity>>,
}

/// JSON that keeps the order of an object's keys, which `exports` conditions
/// depend on: the first matching condition wins.
#[derive(Debug, Clone, PartialEq)]
enum Value {
    Null,
    Other,
    String(String),
    Array(Vec<Value>),
    Object(Vec<(String, Value)>),
}

impl Value {
    fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Self::Object(entries) => entries
                .iter()
                .find(|(name, _)| name == key)
                .map(|(_, value)| value),
            _ => None,
        }
    }

    fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(text) => Some(text),
            _ => None,
        }
    }

    fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    fn is_object(&self) -> bool {
        matches!(self, Self::Object(_))
    }
}

impl<'de> Deserialize<'de> for Value {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct ValueVisitor;
        impl<'de> Visitor<'de> for ValueVisitor {
            type Value = Value;
            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("any JSON value")
            }
            fn visit_unit<E>(self) -> Result<Value, E> {
                Ok(Value::Null)
            }
            fn visit_bool<E>(self, _: bool) -> Result<Value, E> {
                Ok(Value::Other)
            }
            fn visit_i64<E>(self, _: i64) -> Result<Value, E> {
                Ok(Value::Other)
            }
            fn visit_u64<E>(self, _: u64) -> Result<Value, E> {
                Ok(Value::Other)
            }
            fn visit_f64<E>(self, _: f64) -> Result<Value, E> {
                Ok(Value::Other)
            }
            fn visit_str<E>(self, text: &str) -> Result<Value, E> {
                Ok(Value::String(text.to_string()))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Value, A::Error> {
                let mut items = Vec::new();
                while let Some(item) = seq.next_element()? {
                    items.push(item);
                }
                Ok(Value::Array(items))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Value, A::Error> {
                let mut entries = Vec::new();
                while let Some((key, value)) = map.next_entry()? {
                    entries.push((key, value));
                }
                Ok(Value::Object(entries))
            }
        }
        deserializer.deserialize_any(ValueVisitor)
    }
}

/// The candidate files an `exports`/`imports` entry names, in order, the way
/// TypeScript tries them: the first that exists wins. `None` ends the list,
/// which is what a `null` entry means.
type Targets = Vec<Option<String>>;

const TYPESCRIPT_EXTENSIONS: [&str; 3] = [".ts", ".tsx", ".d.ts"];

impl<F: PackageFs> PackageResolver<F> {
    pub fn new(fs: F, config: PackageResolverConfig) -> Self {
        Self {
            fs,
            config,
            observed: Mutex::new(BTreeMap::new()),
            packages: Mutex::new(BTreeMap::new()),
        }
    }

    /// Every package a lookup has resolved into, by canonical directory.
    pub fn resolved_packages(&self) -> Vec<PackageIdentity> {
        self.packages
            .lock()
            .expect("package lock")
            .values()
            .cloned()
            .collect()
    }

    pub fn config(&self) -> &PackageResolverConfig {
        &self.config
    }

    /// Whether `path` is inside one of the authorized roots.
    pub fn within_roots(&self, path: &Path) -> bool {
        self.config.roots.iter().any(|root| path.starts_with(root))
    }

    fn conditions(&self, mode: ImportMode) -> Vec<String> {
        let mut conditions = vec!["types".to_string()];
        if self.config.resolution == ModuleResolution::Node16 {
            conditions.push("node".to_string());
        }
        conditions.push(mode.condition().to_string());
        conditions.extend(self.config.custom_conditions.iter().cloned());
        conditions
    }

    fn observe(&self, path: &Path, observation: Observation) {
        self.observed
            .lock()
            .expect("observation lock")
            .insert(path.to_path_buf(), observation);
    }

    fn probe_file(&self, path: &Path) -> bool {
        if self.fs.is_file(path) {
            true
        } else {
            self.observe(path, Observation::Absent);
            false
        }
    }

    fn read_package_json(&self, directory: &Path) -> Result<Option<Value>, ResolveError> {
        let path = directory.join("package.json");
        if !self.fs.is_file(&path) {
            self.observe(&path, Observation::Absent);
            return Ok(None);
        }
        let canonical = self.confine("package.json", &path)?;
        let text = self.fs.read_to_string(&canonical).map_err(|error| {
            ResolveError::InvalidPackageJson {
                path: path.clone(),
                message: error.to_string(),
            }
        })?;
        self.observe(
            &path,
            Observation::Manifest {
                canonical,
                hash: sha256_hex(text.as_bytes()),
            },
        );
        serde_json::from_str::<Value>(&text)
            .map(Some)
            .map_err(|error| ResolveError::InvalidPackageJson {
                path,
                message: error.to_string(),
            })
    }

    /// Resolves a bare package specifier (or a `#imports` specifier) imported
    /// from a file in `from_dir`.
    pub fn resolve(
        &self,
        from_dir: &Path,
        specifier: &str,
        mode: ImportMode,
    ) -> Result<ResolvedPackageFile, ResolveError> {
        if !self.within_roots(from_dir) {
            return Err(ResolveError::OutsideRoots {
                specifier: specifier.to_string(),
                path: from_dir.to_path_buf(),
            });
        }
        if self.config.resolution == ModuleResolution::Classic {
            split_package_specifier(specifier)
                .ok_or_else(|| ResolveError::InvalidSpecifier(specifier.into()))?;
            return self.resolve_classic(from_dir, specifier);
        }
        if specifier.starts_with('#') {
            return self.resolve_package_imports(from_dir, specifier, mode);
        }
        let (name, subpath) = split_package_specifier(specifier)
            .ok_or_else(|| ResolveError::InvalidSpecifier(specifier.to_string()))?;
        if let Some(found) = self.resolve_self_name(from_dir, specifier, name, &subpath, mode)? {
            return Ok(found);
        }
        let mut searched = Vec::new();
        let mut javascript_only = None;
        for directory in from_dir.ancestors() {
            if !self.within_roots(directory) {
                break;
            }
            let modules = directory.join("node_modules");
            if !self.fs.is_dir(&modules) {
                self.observe(&modules, Observation::Absent);
                continue;
            }
            for (package_name, from_types) in
                [(name.to_string(), false), (types_package_name(name), true)]
            {
                let candidate = modules.join(&package_name);
                if !self.fs.is_dir(&candidate) {
                    self.observe(&candidate, Observation::Absent);
                    continue;
                }
                searched.push(candidate.clone());
                let package_dir = self.canonical_package_dir(specifier, &candidate)?;
                match self.load_from_package(
                    specifier,
                    name,
                    &subpath,
                    &package_dir,
                    mode,
                    from_types,
                ) {
                    Ok(Some(file)) => return Ok(file),
                    Ok(None) => {}
                    Err(ResolveError::JavaScriptOnly { specifier, path }) => {
                        javascript_only = Some((specifier, path));
                    }
                    Err(other) => return Err(other),
                }
            }
        }
        if let Some((specifier, path)) = javascript_only {
            return Err(ResolveError::JavaScriptOnly { specifier, path });
        }
        Err(ResolveError::NotFound {
            specifier: specifier.to_string(),
            searched,
        })
    }

    fn canonical_package_dir(
        &self,
        specifier: &str,
        candidate: &Path,
    ) -> Result<PathBuf, ResolveError> {
        let canonical = self
            .fs
            .canonicalize(candidate)
            .map_err(|_| ResolveError::NotFound {
                specifier: specifier.to_string(),
                searched: vec![candidate.to_path_buf()],
            })?;
        self.observe(candidate, Observation::Canonical(canonical.clone()));
        if !self.within_roots(&canonical) {
            return Err(ResolveError::OutsideRoots {
                specifier: specifier.to_string(),
                path: canonical,
            });
        }
        Ok(canonical)
    }

    fn load_from_package(
        &self,
        specifier: &str,
        name: &str,
        subpath: &str,
        package_dir: &Path,
        mode: ImportMode,
        from_types_package: bool,
    ) -> Result<Option<ResolvedPackageFile>, ResolveError> {
        let manifest = self.read_package_json(package_dir)?;
        let version = manifest
            .as_ref()
            .and_then(|json| json.get("version"))
            .and_then(Value::as_str)
            .map(str::to_string);
        let identity = PackageIdentity {
            name: name.to_string(),
            version,
            root: package_dir.to_path_buf(),
            from_types_package,
        };
        let uses_exports = self.config.resolution != ModuleResolution::Node10
            && manifest
                .as_ref()
                .and_then(|json| json.get("exports"))
                .is_some_and(|exports| !exports.is_null());
        let path = if uses_exports {
            let exports = manifest
                .as_ref()
                .and_then(|json| json.get("exports"))
                .expect("exports came from a manifest");
            let key = if subpath.is_empty() {
                ".".to_string()
            } else {
                format!(".{subpath}")
            };
            let targets = resolve_package_target_map(exports, &key, &self.conditions(mode), false)
                .map_err(|message| ResolveError::InvalidPackageJson {
                    path: package_dir.join("package.json"),
                    message,
                })?;
            if targets.is_empty() {
                return Err(ResolveError::ExportsNotDefined {
                    package: name.to_string(),
                    subpath: key,
                });
            }
            match self.load_export_targets(package_dir, &targets) {
                Some(path) => Some(path),
                // Every target was `null`: the entry is blocked.
                None if matches!(targets.first(), Some(None)) => {
                    return Err(ResolveError::ExportsNotDefined {
                        package: name.to_string(),
                        subpath: key,
                    })
                }
                None => None,
            }
        } else {
            self.load_legacy_package_entry(package_dir, manifest.as_ref(), subpath, mode)?
        };
        let Some(path) = path else {
            return self.javascript_only_or_none(specifier, package_dir, subpath);
        };
        let path = self.confine(specifier, &path)?;
        self.packages
            .lock()
            .expect("package lock")
            .insert(identity.root.clone(), identity.clone());
        Ok(Some(ResolvedPackageFile {
            declaration: features::declaration_file(&path),
            path,
            package: Some(identity),
        }))
    }

    /// `Ok(None)` when nothing was found; an error when only JavaScript was.
    fn javascript_only_or_none(
        &self,
        specifier: &str,
        package_dir: &Path,
        subpath: &str,
    ) -> Result<Option<ResolvedPackageFile>, ResolveError> {
        let base = if subpath.is_empty() {
            package_dir.join("index")
        } else {
            package_dir.join(&subpath[1..])
        };
        for extension in [".js", ".mjs", ".cjs"] {
            let candidate = PathBuf::from(format!("{}{extension}", base.display()));
            if self.fs.is_file(&candidate) {
                return Err(ResolveError::JavaScriptOnly {
                    specifier: specifier.to_string(),
                    path: candidate,
                });
            }
        }
        Ok(None)
    }

    /// Canonicalizes a found file and requires it to be inside the roots.
    fn confine(&self, specifier: &str, path: &Path) -> Result<PathBuf, ResolveError> {
        let canonical = self
            .fs
            .canonicalize(path)
            .map_err(|_| ResolveError::NotFound {
                specifier: specifier.to_string(),
                searched: vec![path.to_path_buf()],
            })?;
        if !self.within_roots(&canonical) {
            return Err(ResolveError::OutsideRoots {
                specifier: specifier.to_string(),
                path: canonical,
            });
        }
        self.observe(path, Observation::Canonical(canonical.clone()));
        Ok(canonical)
    }

    /// The first of an entry's targets that is a file, with TypeScript's
    /// substitution of a JavaScript extension for its source or declaration.
    fn load_export_targets(&self, package_dir: &Path, targets: &Targets) -> Option<PathBuf> {
        for target in targets {
            let target = target.as_ref()?;
            let path = package_dir.join(target.strip_prefix("./").unwrap_or(target));
            for candidate in exact_candidates(&path) {
                if self.probe_file(&candidate) {
                    return Some(candidate);
                }
            }
        }
        None
    }

    /// `types`/`typings`/`main`, then `index`, of a package or directory.
    fn load_package_main(
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

    fn load_index(&self, directory: &Path) -> Result<Option<PathBuf>, ResolveError> {
        for extension in TYPESCRIPT_EXTENSIONS {
            let candidate = directory.join(format!("index{extension}"));
            if self.probe_file(&candidate) {
                return Ok(Some(candidate));
            }
        }
        Ok(None)
    }

    fn load_as_file_or_directory(&self, path: &Path) -> Result<Option<PathBuf>, ResolveError> {
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
    fn load_as_file(&self, path: &Path) -> Option<PathBuf> {
        file_candidates(path)
            .into_iter()
            .find(|candidate| self.probe_file(candidate))
    }

    fn resolve_package_imports(
        &self,
        from_dir: &Path,
        specifier: &str,
        mode: ImportMode,
    ) -> Result<ResolvedPackageFile, ResolveError> {
        if self.config.resolution == ModuleResolution::Node10 {
            return Err(ResolveError::ImportsNotDefined {
                specifier: specifier.to_string(),
            });
        }
        for directory in from_dir.ancestors() {
            if !self.within_roots(directory) {
                break;
            }
            let Some(manifest) = self.read_package_json(directory)? else {
                continue;
            };
            let Some(imports) = manifest.get("imports").filter(|value| value.is_object()) else {
                return Err(ResolveError::ImportsNotDefined {
                    specifier: specifier.to_string(),
                });
            };
            let targets =
                resolve_package_target_map(imports, specifier, &self.conditions(mode), true)
                    .map_err(|message| ResolveError::InvalidPackageJson {
                        path: directory.join("package.json"),
                        message,
                    })?;
            let not_defined = || ResolveError::ImportsNotDefined {
                specifier: specifier.to_string(),
            };
            for target in &targets {
                let Some(target) = target else {
                    return Err(not_defined());
                };
                if target.starts_with("./") {
                    if let Some(found) =
                        self.load_export_targets(directory, &vec![Some(target.clone())])
                    {
                        let path = self.confine(specifier, &found)?;
                        return Ok(ResolvedPackageFile {
                            declaration: features::declaration_file(&path),
                            path,
                            package: None,
                        });
                    }
                } else if let Ok(found) = self.resolve(directory, target, mode) {
                    // A bare target names another package.
                    return Ok(found);
                }
            }
            return Err(not_defined());
        }
        Err(ResolveError::ImportsNotDefined {
            specifier: specifier.to_string(),
        })
    }

    /// Resolves a relative specifier (`./x`, `../x/y`) the way a package's own
    /// files import each other, including TypeScript extension substitution.
    pub fn resolve_relative(
        &self,
        from_dir: &Path,
        specifier: &str,
    ) -> Result<ResolvedPackageFile, ResolveError> {
        if !self.within_roots(from_dir) {
            return Err(ResolveError::OutsideRoots {
                specifier: specifier.to_string(),
                path: from_dir.to_path_buf(),
            });
        }
        let joined = normalize(&from_dir.join(specifier));
        let candidate = if self.config.resolution == ModuleResolution::Classic {
            self.load_as_file(&joined)
        } else {
            self.load_as_file_or_directory(&joined)?
        };
        let found = candidate.ok_or_else(|| ResolveError::NotFound {
            specifier: specifier.to_string(),
            searched: vec![joined.clone()],
        })?;
        let path = self.confine(specifier, &found)?;
        Ok(ResolvedPackageFile {
            declaration: features::declaration_file(&path),
            path,
            package: None,
        })
    }

    /// Resolves an explicitly enabled JSON asset under the canonical owner roots.
    pub fn resolve_relative_json(
        &self,
        from_dir: &Path,
        specifier: &str,
    ) -> Result<ResolvedPackageFile, ResolveError> {
        if !self.within_roots(from_dir) {
            return Err(ResolveError::OutsideRoots {
                specifier: specifier.to_string(),
                path: from_dir.to_path_buf(),
            });
        }
        if !specifier.ends_with(".json")
            || !(specifier.starts_with("./") || specifier.starts_with("../"))
        {
            return Err(ResolveError::InvalidSpecifier(specifier.to_string()));
        }
        let joined = normalize(&from_dir.join(specifier));
        if !self.probe_file(&joined) {
            return Err(ResolveError::NotFound {
                specifier: specifier.to_string(),
                searched: vec![joined],
            });
        }
        let path = self.confine(specifier, &joined)?;
        let contents =
            self.fs
                .read_to_string(&path)
                .map_err(|error| ResolveError::InvalidPackageJson {
                    path: path.clone(),
                    message: error.to_string(),
                })?;
        self.observe(&path, Observation::Content(sha256_hex(contents.as_bytes())));
        Ok(ResolvedPackageFile {
            path,
            declaration: false,
            package: None,
        })
    }

    /// The exact identity of this resolver's configuration and of everything it
    /// has looked at since it was created: TypeScript's resolution version, the
    /// strategy, the conditions, the roots and each recorded observation.
    pub fn fingerprint(&self) -> String {
        let mut text = String::new();
        text.push_str(RESOLUTION_VERSION);
        text.push('\n');
        text.push_str(self.config.resolution.as_str());
        text.push('\n');
        for condition in &self.config.custom_conditions {
            text.push_str(&format!("condition:{condition}\n"));
        }
        for root in &self.config.roots {
            text.push_str(&format!("root:{}\n", root.display()));
        }
        for (path, observation) in self.observed.lock().expect("observation lock").iter() {
            match observation {
                Observation::Absent => text.push_str(&format!("absent:{}\n", path.display())),
                Observation::Content(hash) => {
                    text.push_str(&format!("file:{}:{hash}\n", path.display()))
                }
                Observation::Canonical(target) => {
                    text.push_str(&format!("real:{}:{}\n", path.display(), target.display()))
                }
                Observation::Manifest { canonical, hash } => text.push_str(&format!(
                    "scope:{}:{}:{hash}\n",
                    path.display(),
                    canonical.display()
                )),
            }
        }
        format!("bts-packages-{}", sha256_hex(text.as_bytes()))
    }

    /// Re-checks every recorded observation against the file system now: an
    /// absent candidate is still absent, a read file has the same hash and a
    /// symlink still points where it did. `false` means cached results that
    /// depended on this resolver must be discarded.
    pub fn revalidate(&self) -> bool {
        let observed = self.observed.lock().expect("observation lock").clone();
        observed
            .iter()
            .all(|(path, observation)| match observation {
                Observation::Absent => !self.fs.is_file(path) && !self.fs.is_dir(path),
                Observation::Content(hash) => self
                    .fs
                    .read_to_string(path)
                    .is_ok_and(|text| &sha256_hex(text.as_bytes()) == hash),
                Observation::Canonical(target) => self
                    .fs
                    .canonicalize(path)
                    .is_ok_and(|current| &current == target),
                Observation::Manifest { canonical, hash } => {
                    self.fs.canonicalize(path).is_ok_and(|current| {
                        &current == canonical
                            && self.within_roots(&current)
                            && self
                                .fs
                                .read_to_string(&current)
                                .is_ok_and(|text| &sha256_hex(text.as_bytes()) == hash)
                    })
                }
            })
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    digest(&SHA256, bytes)
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// `(@scope/name | name, "" | "/subpath")`, or `None` for something that is not
/// a package specifier: empty, absolute, containing `\`, `.`/`..`/empty segments.
pub fn split_package_specifier(specifier: &str) -> Option<(&str, String)> {
    if specifier.is_empty()
        || specifier.starts_with('/')
        || specifier.starts_with('.')
        || specifier.contains('\\')
        || specifier.contains("://")
    {
        return None;
    }
    let segments: Vec<&str> = specifier.split('/').collect();
    if segments
        .iter()
        .any(|segment| segment.is_empty() || *segment == "." || *segment == "..")
    {
        return None;
    }
    let name_length = if specifier.starts_with('@') {
        if segments.len() < 2 || segments[0].len() < 2 {
            return None;
        }
        segments[0].len() + 1 + segments[1].len()
    } else {
        segments[0].len()
    };
    if segments[0].contains("node_modules") {
        return None;
    }
    let (name, rest) = specifier.split_at(name_length);
    Some((name, rest.to_string()))
}

/// `@scope/name` is `@types/scope__name`; an unscoped name is `@types/name`.
pub fn types_package_name(name: &str) -> String {
    match name.strip_prefix('@').and_then(|rest| rest.split_once('/')) {
        Some((scope, package)) => format!("@types/{scope}__{package}"),
        None => format!("@types/{name}"),
    }
}

/// The files a package file name may stand for, in TypeScript's order.
fn file_candidates(path: &Path) -> Vec<PathBuf> {
    let text = path.to_string_lossy();
    for (javascript, source, declaration) in
        [(".mjs", ".mts", ".d.mts"), (".cjs", ".cts", ".d.cts")]
    {
        if let Some(stem) = text.strip_suffix(javascript) {
            return vec![
                PathBuf::from(format!("{stem}{source}")),
                PathBuf::from(format!("{stem}{declaration}")),
            ];
        }
        if text.ends_with(source) {
            return vec![path.to_path_buf()];
        }
    }
    let text = path.to_string_lossy().into_owned();
    if text.ends_with(".d.ts") {
        return vec![path.to_path_buf()];
    }
    if let Some(stem) = text.strip_suffix(".ts") {
        return TYPESCRIPT_EXTENSIONS
            .iter()
            .map(|extension| PathBuf::from(format!("{stem}{extension}")))
            .collect();
    }
    let mut candidates = Vec::new();
    for extension in TYPESCRIPT_EXTENSIONS {
        if text.ends_with(extension) && !(extension == ".ts" && text.ends_with(".d.ts")) {
            candidates.push(path.to_path_buf());
            return candidates;
        }
    }
    if let Some(stem) = text.strip_suffix(".js") {
        for extension in TYPESCRIPT_EXTENSIONS {
            candidates.push(PathBuf::from(format!("{stem}{extension}")));
        }
        return candidates;
    }
    if let Some(stem) = text.strip_suffix(".jsx") {
        candidates.push(PathBuf::from(format!("{stem}.tsx")));
        candidates.push(PathBuf::from(format!("{stem}.d.ts")));
        return candidates;
    }
    for extension in TYPESCRIPT_EXTENSIONS {
        candidates.push(PathBuf::from(format!("{text}{extension}")));
    }
    candidates
}

/// An `exports` target is an exact file: a TypeScript file as written, or a
/// `.js`/`.jsx` file stood for by its source or declaration.
fn exact_candidates(path: &Path) -> Vec<PathBuf> {
    let text = path.to_string_lossy();
    if [".mts", ".cts", ".mjs", ".cjs"]
        .iter()
        .any(|extension| text.ends_with(extension))
    {
        return file_candidates(path);
    }
    if text.ends_with(".d.ts") || text.ends_with(".ts") || text.ends_with(".tsx") {
        return vec![path.to_path_buf()];
    }
    if text.ends_with(".js") || text.ends_with(".jsx") {
        return file_candidates(path);
    }
    Vec::new()
}

/// Lexically removes `.` and `..`.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Resolves `key` (`.`, `./x`, or `#name`) against an `exports`/`imports` value
/// into the targets to try, empty when nothing matches.
fn resolve_package_target_map(
    map: &Value,
    key: &str,
    conditions: &[String],
    imports: bool,
) -> Result<Targets, String> {
    // A string, an array, or an object of conditions stands for the `.` entry.
    let is_subpath_map = matches!(map, Value::Object(entries)
        if entries.iter().any(|(name, _)| name.starts_with('.') || name.starts_with('#')));
    if !imports && !is_subpath_map {
        return if key == "." {
            resolve_target(map, None, conditions, imports)
        } else {
            Ok(Vec::new())
        };
    }
    let Value::Object(entries) = map else {
        return Ok(Vec::new());
    };
    if entries
        .iter()
        .any(|(name, _)| !(name.starts_with('.') || name.starts_with('#')))
    {
        return Err("`exports` mixes subpath keys and condition keys".to_string());
    }
    if !key.contains('*') {
        if let Some((_, value)) = entries.iter().find(|(name, _)| name == key) {
            return resolve_target(value, None, conditions, imports);
        }
    }
    // The pattern with the longest prefix before `*` wins.
    let mut best: Option<(usize, &Value, String)> = None;
    for (pattern, value) in entries {
        let Some((prefix, suffix)) = pattern.split_once('*') else {
            continue;
        };
        if suffix.contains('*') || key.len() < prefix.len() + suffix.len() {
            continue;
        }
        if key.starts_with(prefix) && key.ends_with(suffix) && key != prefix {
            let matched = key[prefix.len()..key.len() - suffix.len()].to_string();
            if best
                .as_ref()
                .is_none_or(|(current, _, _)| prefix.len() > *current)
            {
                best = Some((prefix.len(), value, matched));
            }
        }
    }
    match best {
        Some((_, value, matched)) => resolve_target(value, Some(&matched), conditions, imports),
        None => Ok(Vec::new()),
    }
}

fn resolve_target(
    target: &Value,
    pattern_match: Option<&str>,
    conditions: &[String],
    imports: bool,
) -> Result<Targets, String> {
    match target {
        Value::String(text) => {
            let text = match pattern_match {
                Some(matched) => text.replace('*', matched),
                None => text.clone(),
            };
            if let Some(relative) = text.strip_prefix("./") {
                let invalid = relative
                    .split('/')
                    .any(|segment| segment == ".." || segment == "." || segment == "node_modules");
                if invalid {
                    return Err(format!("invalid package target `{text}`"));
                }
                Ok(vec![Some(text)])
            } else if imports && split_package_specifier(&text).is_some() {
                Ok(vec![Some(text)])
            } else {
                Err(format!("invalid package target `{text}`"))
            }
        }
        Value::Array(items) => {
            let mut targets = Vec::new();
            for item in items {
                // An invalid element is skipped; the next may be valid.
                if let Ok(more) = resolve_target(item, pattern_match, conditions, imports) {
                    let stop = more.last() == Some(&None);
                    targets.extend(more);
                    if stop {
                        break;
                    }
                }
            }
            Ok(targets)
        }
        Value::Object(entries) => {
            let mut targets = Vec::new();
            for (condition, value) in entries {
                if condition == "default" || conditions.iter().any(|active| active == condition) {
                    let more = resolve_target(value, pattern_match, conditions, imports)?;
                    let stop = more.last() == Some(&None);
                    targets.extend(more);
                    if stop {
                        break;
                    }
                }
            }
            Ok(targets)
        }
        Value::Null => Ok(vec![None]),
        Value::Other => Err("a package target must be a string, array, object or null".to_string()),
    }
}

#[cfg(test)]
mod tests;
