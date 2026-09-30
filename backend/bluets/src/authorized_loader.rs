// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Closed, host-authorized TypeScript module loading.
//!
//! A page host supplies canonical source identities and every permitted
//! resolution edge before compilation. This loader never reads a path, URL,
//! import map, package registry, or ambient current directory; a missing edge
//! is rejected rather than falling back to BlueTS's relative resolver.

use crate::{ModuleLoader, ModuleSource};
use std::collections::BTreeMap;
use std::fmt;

/// One host-authorized source module. The identity is opaque to BlueTS: the
/// host is responsible for canonicalization, origin checks, and source policy
/// before constructing this record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorizedModule {
    canonical_id: String,
    text: String,
}

impl AuthorizedModule {
    /// Creates one exact source record. Validation happens while constructing
    /// the complete loader so duplicate records can be rejected atomically.
    pub fn new(canonical_id: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            canonical_id: canonical_id.into(),
            text: text.into(),
        }
    }

    /// The caller-authorized identity that BlueTS preserves in diagnostics,
    /// source hashes, and direct-bridge artifacts.
    pub fn canonical_id(&self) -> &str {
        &self.canonical_id
    }
}

/// One exact import edge selected by the host's resolver. The source specifier
/// remains only a lookup key; BlueTS receives the selected canonical target
/// and never resolves the specifier again under another policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorizedModuleResolution {
    from_module: String,
    specifier: String,
    target_module: String,
}

impl AuthorizedModuleResolution {
    /// Creates one resolution record. Loader construction validates that both
    /// module identities exist and that the `(from, specifier)` pair is unique.
    pub fn new(
        from_module: impl Into<String>,
        specifier: impl Into<String>,
        target_module: impl Into<String>,
    ) -> Self {
        Self {
            from_module: from_module.into(),
            specifier: specifier.into(),
            target_module: target_module.into(),
        }
    }
}

/// Failure to form a closed, host-authorized module graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthorizedModuleLoaderError {
    EmptyModuleId,
    DuplicateModuleId(String),
    EmptySpecifier {
        from_module: String,
    },
    MissingResolutionSource {
        from_module: String,
    },
    MissingResolutionTarget {
        target_module: String,
    },
    DuplicateResolution {
        from_module: String,
        specifier: String,
    },
}

impl fmt::Display for AuthorizedModuleLoaderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyModuleId => formatter.write_str("authorized module ID is empty"),
            Self::DuplicateModuleId(id) => write!(formatter, "duplicate authorized module `{id}`"),
            Self::EmptySpecifier { from_module } => {
                write!(
                    formatter,
                    "authorized resolution from `{from_module}` has an empty specifier"
                )
            }
            Self::MissingResolutionSource { from_module } => write!(
                formatter,
                "authorized resolution source `{from_module}` has no source record"
            ),
            Self::MissingResolutionTarget { target_module } => write!(
                formatter,
                "authorized resolution target `{target_module}` has no source record"
            ),
            Self::DuplicateResolution {
                from_module,
                specifier,
            } => write!(
                formatter,
                "duplicate authorized resolution `{specifier}` from `{from_module}`"
            ),
        }
    }
}

impl std::error::Error for AuthorizedModuleLoaderError {}

/// A closed module graph backed exclusively by records supplied to
/// [`Self::new`]. Its [`ModuleLoader`] implementation deliberately has no
/// fallback resolver, so an omitted edge cannot accidentally gain filesystem
/// or network authority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorizedModuleLoader {
    modules: BTreeMap<String, String>,
    resolutions: BTreeMap<(String, String), String>,
}

impl AuthorizedModuleLoader {
    /// Validates and stores a complete host-authorized source graph.
    pub fn new(
        modules: impl IntoIterator<Item = AuthorizedModule>,
        resolutions: impl IntoIterator<Item = AuthorizedModuleResolution>,
    ) -> Result<Self, AuthorizedModuleLoaderError> {
        let mut authorized_modules = BTreeMap::new();
        for module in modules {
            if module.canonical_id.is_empty() || module.canonical_id.contains('\0') {
                return Err(AuthorizedModuleLoaderError::EmptyModuleId);
            }
            if authorized_modules
                .insert(module.canonical_id.clone(), module.text)
                .is_some()
            {
                return Err(AuthorizedModuleLoaderError::DuplicateModuleId(
                    module.canonical_id,
                ));
            }
        }

        let mut authorized_resolutions = BTreeMap::new();
        for resolution in resolutions {
            if resolution.specifier.is_empty() {
                return Err(AuthorizedModuleLoaderError::EmptySpecifier {
                    from_module: resolution.from_module,
                });
            }
            if !authorized_modules.contains_key(&resolution.from_module) {
                return Err(AuthorizedModuleLoaderError::MissingResolutionSource {
                    from_module: resolution.from_module,
                });
            }
            if !authorized_modules.contains_key(&resolution.target_module) {
                return Err(AuthorizedModuleLoaderError::MissingResolutionTarget {
                    target_module: resolution.target_module,
                });
            }
            let key = (resolution.from_module.clone(), resolution.specifier.clone());
            if authorized_resolutions
                .insert(key, resolution.target_module)
                .is_some()
            {
                return Err(AuthorizedModuleLoaderError::DuplicateResolution {
                    from_module: resolution.from_module,
                    specifier: resolution.specifier,
                });
            }
        }

        Ok(Self {
            modules: authorized_modules,
            resolutions: authorized_resolutions,
        })
    }

    /// Number of source records retained by this closed graph.
    pub fn module_count(&self) -> usize {
        self.modules.len()
    }

    /// Iterates the exact source records selected when this closed graph was
    /// constructed. The iterator has no loading, resolution, filesystem, or
    /// network capability; it is useful to a trusted host that must copy the
    /// graph into another already-authorized execution transport.
    pub fn authorized_modules(&self) -> impl Iterator<Item = (&str, &str)> {
        self.modules
            .iter()
            .map(|(module_id, source)| (module_id.as_str(), source.as_str()))
    }

    /// Iterates the exact static edges selected when this closed graph was
    /// constructed. It never resolves a new specifier or falls back to a
    /// relative, URL, package, or filesystem resolver.
    pub fn authorized_resolutions(&self) -> impl Iterator<Item = (&str, &str, &str)> {
        self.resolutions
            .iter()
            .map(|((from, specifier), target)| (from.as_str(), specifier.as_str(), target.as_str()))
    }
}

impl ModuleLoader for AuthorizedModuleLoader {
    fn load(&self, module_id: &str) -> Result<ModuleSource, String> {
        self.modules
            .get(module_id)
            .map(|text| ModuleSource::new(module_id, text))
            .ok_or_else(|| format!("module `{module_id}` is not authorized by this host"))
    }

    fn resolve(&self, from_module: &str, specifier: &str) -> Result<String, String> {
        self.resolutions
            .get(&(from_module.to_string(), specifier.to_string()))
            .cloned()
            .ok_or_else(|| {
                format!(
                    "specifier `{specifier}` from `{from_module}` is not authorized by this host"
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{compile, CompilerOptions};

    const ENTRY: &str = "page:///app/main.ts";
    const DEPENDENCY: &str = "page:///app/math.ts";

    fn graph() -> AuthorizedModuleLoader {
        AuthorizedModuleLoader::new(
            [
                AuthorizedModule::new(
                    ENTRY,
                    "import { answer } from './math'; export const value: number = answer;",
                ),
                AuthorizedModule::new(DEPENDENCY, "export const answer: number = 42;"),
            ],
            [AuthorizedModuleResolution::new(ENTRY, "./math", DEPENDENCY)],
        )
        .unwrap()
    }

    #[test]
    fn compiles_only_the_explicitly_authorized_canonical_graph() {
        let loader = graph();
        let compilation = compile(
            ENTRY,
            &loader,
            CompilerOptions {
                resolver_fingerprint: "page-authorized-resolver-v1".to_string(),
                ..CompilerOptions::default()
            },
        );
        assert!(!compilation.has_errors(), "{:#?}", compilation.diagnostics);
        assert_eq!(loader.module_count(), 2);
        assert_eq!(
            compilation.project.resolved_module(ENTRY, "./math"),
            Some(DEPENDENCY)
        );
        assert!(compilation.project.modules.contains_key(ENTRY));
        assert!(compilation.project.modules.contains_key(DEPENDENCY));
    }

    #[test]
    fn refuses_duplicate_or_dangling_records_before_compilation() {
        assert_eq!(
            AuthorizedModuleLoader::new(
                [
                    AuthorizedModule::new(ENTRY, "export const value = 1;"),
                    AuthorizedModule::new(ENTRY, "export const value = 2;"),
                ],
                [],
            ),
            Err(AuthorizedModuleLoaderError::DuplicateModuleId(
                ENTRY.to_string()
            ))
        );
        assert_eq!(
            AuthorizedModuleLoader::new(
                [AuthorizedModule::new(ENTRY, "export const value = 1;")],
                [AuthorizedModuleResolution::new(
                    ENTRY,
                    "./missing",
                    DEPENDENCY
                )],
            ),
            Err(AuthorizedModuleLoaderError::MissingResolutionTarget {
                target_module: DEPENDENCY.to_string()
            })
        );
    }

    #[test]
    fn missing_edges_do_not_fall_back_to_relative_resolution() {
        let loader = AuthorizedModuleLoader::new(
            [AuthorizedModule::new(
                ENTRY,
                "import { answer } from './math';",
            )],
            [],
        )
        .unwrap();
        assert!(loader.resolve(ENTRY, "./math").is_err());
        assert!(loader.load("page:///app/other.ts").is_err());
    }
}
