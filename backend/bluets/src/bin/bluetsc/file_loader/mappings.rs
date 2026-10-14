// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Compile-time mappings reuse the resolver's confined, observed file probes.

use super::*;
use serde_json::{Map, Value};

#[derive(Debug, Clone)]
pub(crate) struct PathMappings {
    base_url: Option<PathBuf>,
    patterns: BTreeMap<String, Vec<PathBuf>>,
    root_dirs: Vec<PathBuf>,
}

impl PathMappings {
    pub(crate) fn from_options(
        options: &Map<String, Value>,
        origin: &Path,
        authorize: impl Fn(&Path) -> Result<(), String>,
    ) -> Result<Option<Self>, String> {
        let base_url = options
            .get("baseUrl")
            .and_then(Value::as_str)
            .map(PathBuf::from);
        let base = base_url.as_deref().unwrap_or(origin);
        let mut patterns = BTreeMap::new();
        if let Some(values) = options.get("paths").and_then(Value::as_object) {
            for (pattern, targets) in values {
                let mut paths = Vec::new();
                for target in targets
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                {
                    let path = tsconfig::clean_path(&base.join(target));
                    let checked =
                        PathBuf::from(path.to_string_lossy().replace('*', "__bluets_wildcard__"));
                    authorize(&checked)?;
                    paths.push(path);
                }
                patterns.insert(pattern.clone(), paths);
            }
        }
        let root_dirs = options
            .get("rootDirs")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(PathBuf::from)
            .collect::<Vec<_>>();
        if base_url.is_none() && patterns.is_empty() && root_dirs.is_empty() {
            return Ok(None);
        }
        Ok(Some(Self {
            base_url,
            patterns,
            root_dirs,
        }))
    }

    pub(super) fn bare(
        &self,
        loader: &FileLoader,
        specifier: &str,
    ) -> Result<Option<PathBuf>, String> {
        let selected = self
            .patterns
            .get(specifier)
            .map(|paths| (paths, ""))
            .or_else(|| {
                self.patterns
                    .iter()
                    .filter_map(|(pattern, paths)| {
                        let (prefix, suffix) = pattern.split_once('*')?;
                        if !specifier.starts_with(prefix)
                            || !specifier.ends_with(suffix)
                            || specifier.len() < prefix.len() + suffix.len()
                        {
                            return None;
                        }
                        Some((
                            prefix.len(),
                            paths,
                            &specifier[prefix.len()..specifier.len() - suffix.len()],
                        ))
                    })
                    .max_by_key(|(length, _, _)| *length)
                    .map(|(_, paths, wildcard)| (paths, wildcard))
            });
        if let Some((paths, wildcard)) = selected {
            for path in paths {
                let path = PathBuf::from(path.to_string_lossy().replace('*', wildcard));
                if let Some(found) = self.probe(loader, &path)? {
                    return Ok(Some(found));
                }
            }
        }
        if let Some(base) = &self.base_url {
            return self.probe(loader, &tsconfig::clean_path(&base.join(specifier)));
        }
        Ok(None)
    }

    pub(super) fn relative(
        &self,
        loader: &FileLoader,
        from: &Path,
        specifier: &str,
    ) -> Result<Option<PathBuf>, String> {
        let joined = tsconfig::clean_path(&from.join(specifier));
        let Some(current) = self
            .root_dirs
            .iter()
            .filter(|root| joined.starts_with(root))
            .max_by_key(|root| root.components().count())
        else {
            return Ok(None);
        };
        let suffix = joined.strip_prefix(current).expect("matching virtual root");
        for root in &self.root_dirs {
            if root != current {
                if let Some(found) = self.probe(loader, &root.join(suffix))? {
                    return Ok(Some(found));
                }
            }
        }
        Ok(None)
    }

    fn probe(&self, loader: &FileLoader, path: &Path) -> Result<Option<PathBuf>, String> {
        let parent = path.parent().ok_or("mapped path has no parent")?;
        let name = path
            .file_name()
            .ok_or("mapped path has no filename")?
            .to_string_lossy();
        let resolver = loader.packages.as_ref().unwrap_or(&loader.relative);
        match resolver.resolve_relative(parent, &format!("./{name}")) {
            Ok(found) => Ok(Some(found.path)),
            Err(ResolveError::NotFound { .. }) => Ok(None),
            Err(error) => Err(error.to_string()),
        }
    }
}
