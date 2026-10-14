// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Package self-name, ordered typesVersions and classic file lookups.

use super::*;

impl<F: PackageFs> PackageResolver<F> {
    pub(super) fn resolve_self_name(
        &self,
        from: &Path,
        specifier: &str,
        name: &str,
        subpath: &str,
        mode: ImportMode,
    ) -> Result<Option<ResolvedPackageFile>, ResolveError> {
        if self.config.resolution == ModuleResolution::Node10 {
            return Ok(None);
        }
        for directory in from.ancestors().take_while(|path| self.within_roots(path)) {
            if directory
                .file_name()
                .is_some_and(|name| name == "node_modules")
            {
                break;
            }
            let Some(manifest) = self.read_package_json(directory)? else {
                continue;
            };
            if manifest.get("name").and_then(Value::as_str) != Some(name)
                || manifest.get("exports").is_none_or(Value::is_null)
            {
                return Ok(None);
            }
            let canonical = self.canonical_package_dir(specifier, directory)?;
            return self.load_from_package(specifier, name, subpath, &canonical, mode, false);
        }
        Ok(None)
    }

    pub(super) fn resolve_classic(
        &self,
        from: &Path,
        specifier: &str,
    ) -> Result<ResolvedPackageFile, ResolveError> {
        let mut searched = Vec::new();
        for directory in from.ancestors().take_while(|path| self.within_roots(path)) {
            let candidate = directory.join(specifier);
            searched.push(candidate.clone());
            if let Some(found) = self.load_as_file(&candidate) {
                let path = self.confine(specifier, &found)?;
                return Ok(ResolvedPackageFile {
                    declaration: declaration_file(&path),
                    path,
                    package: None,
                });
            }
        }
        Err(ResolveError::NotFound {
            specifier: specifier.into(),
            searched,
        })
    }

    pub(super) fn load_legacy_package_entry(
        &self,
        directory: &Path,
        manifest: Option<&Value>,
        subpath: &str,
        mode: ImportMode,
    ) -> Result<Option<PathBuf>, ResolveError> {
        let exact = !subpath.is_empty()
            && self.config.resolution == ModuleResolution::Node16
            && mode == ImportMode::Import;
        let request = if subpath.is_empty() {
            manifest
                .and_then(|m| m.get("typings").or_else(|| m.get("types")))
                .and_then(Value::as_str)
                .unwrap_or("index.d.ts")
        } else {
            &subpath[1..]
        };
        if let Some(targets) = manifest.and_then(|m| versioned_targets(m, request)) {
            for target in targets {
                if Path::new(&target).components().any(|part| {
                    matches!(
                        part,
                        Component::ParentDir | Component::RootDir | Component::Prefix(_)
                    )
                }) {
                    return Err(ResolveError::InvalidPackageJson {
                        path: directory.join("package.json"),
                        message: "typesVersions target escapes its package".into(),
                    });
                }
                if let Some(path) = self.legacy_candidate(&directory.join(target), exact)? {
                    return Ok(Some(path));
                }
            }
            return Ok(None);
        }
        if subpath.is_empty() {
            self.load_package_main(directory, manifest)
        } else {
            self.legacy_candidate(&directory.join(request), exact)
        }
    }

    fn legacy_candidate(&self, path: &Path, exact: bool) -> Result<Option<PathBuf>, ResolveError> {
        if exact {
            Ok(exact_candidates(path)
                .into_iter()
                .find(|path| self.probe_file(path)))
        } else {
            self.load_as_file_or_directory(path)
        }
    }
}

fn versioned_targets(manifest: &Value, request: &str) -> Option<Vec<String>> {
    let Value::Object(ranges) = manifest.get("typesVersions")? else {
        return None;
    };
    let mapping = ranges
        .iter()
        .find_map(|(range, mapping)| version_ranges::matches(range).then_some(mapping))?;
    let Value::Object(patterns) = mapping else {
        return None;
    };
    let request = request.trim_start_matches("./");
    let selected = patterns
        .iter()
        .find(|(pattern, _)| pattern == request)
        .map(|(_, value)| (value, ""))
        .or_else(|| {
            patterns
                .iter()
                .filter_map(|(pattern, value)| {
                    let (prefix, suffix) = pattern.split_once('*')?;
                    if suffix.contains('*')
                        || !request.starts_with(prefix)
                        || !request.ends_with(suffix)
                        || request.len() < prefix.len() + suffix.len()
                    {
                        return None;
                    }
                    Some((
                        (prefix.len(), suffix.len()),
                        value,
                        &request[prefix.len()..request.len() - suffix.len()],
                    ))
                })
                .max_by_key(|(score, _, _)| *score)
                .map(|(_, value, wildcard)| (value, wildcard))
        })?;
    let Value::Array(targets) = selected.0 else {
        return Some(Vec::new());
    };
    Some(
        targets
            .iter()
            .filter_map(Value::as_str)
            .map(|target| target.replace('*', selected.1))
            .collect(),
    )
}

pub(super) fn declaration_file(path: &Path) -> bool {
    let text = path.to_string_lossy();
    [".d.ts", ".d.mts", ".d.cts"]
        .iter()
        .any(|suffix| text.ends_with(suffix))
}
