// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! File selection uses TypeScript's component wildcards, not shell expansion.

use super::*;

pub(super) fn select(document: &Document, reader: &Reader) -> Result<Vec<PathBuf>, String> {
    if document.local_empty_files {
        return Err("config `files` must not be an empty array".to_string());
    }
    let mut selected = document
        .files
        .clone()
        .unwrap_or_default()
        .into_iter()
        .collect::<BTreeSet<_>>();
    for path in &selected {
        reader.authorize_future(path, "configured file", false)?;
    }
    let default_include = vec![reader.root.join("**/*")];
    let include = document
        .include
        .as_deref()
        .unwrap_or(if document.files_supplied {
            &[]
        } else {
            &default_include
        });
    let default_exclude = document
        .options
        .get("outDir")
        .and_then(Value::as_str)
        .map(|value| vec![PathBuf::from(value)])
        .unwrap_or_default();
    let exclude = document.exclude.as_ref().unwrap_or(&default_exclude);
    let allow_js = document
        .options
        .get("allowJs")
        .and_then(Value::as_bool)
        .unwrap_or(
            document
                .options
                .get("checkJs")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        );
    let mut active = BTreeSet::new();
    let mut visited = 0;
    if !include.is_empty() {
        walk(
            &reader.root,
            include,
            exclude,
            allow_js,
            reader,
            &mut active,
            &mut visited,
            &mut selected,
        )?;
    }
    if selected.is_empty() && !document.files_supplied {
        return Err("no inputs were found in the tsconfig file".to_string());
    }
    // Glob-discovered siblings follow the .ts, .tsx, .d.ts precedence.
    let explicit = document
        .files
        .clone()
        .unwrap_or_default()
        .into_iter()
        .collect::<BTreeSet<_>>();
    let all = selected.clone();
    selected.retain(|path| {
        if explicit.contains(path) {
            return true;
        }
        let text = path.file_name().unwrap_or_default().to_string_lossy();
        let base = text
            .strip_suffix(".d.ts")
            .or_else(|| text.strip_suffix(".tsx"));
        !base.is_some_and(|base| all.contains(&path.with_file_name(format!("{base}.ts"))))
    });
    Ok(selected.into_iter().collect())
}

#[allow(clippy::too_many_arguments)]
fn walk(
    directory: &Path,
    include: &[PathBuf],
    exclude: &[PathBuf],
    allow_js: bool,
    reader: &Reader,
    active: &mut BTreeSet<PathBuf>,
    visited: &mut usize,
    selected: &mut BTreeSet<PathBuf>,
) -> Result<(), String> {
    reader.authorize_future(directory, "included directory", false)?;
    let canonical = fs::canonicalize(directory)
        .map_err(|error| format!("cannot inspect included directory: {error}"))?;
    if !active.insert(canonical.clone()) {
        return Ok(());
    }
    let result = (|| {
        let mut entries = fs::read_dir(directory)
            .map_err(|error| format!("cannot list input files: {error}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            *visited += 1;
            if *visited > 100_000 {
                return Err("configuration file-discovery resource limit exceeded".to_string());
            }
            let path = entry.path();
            if exclude.iter().any(|pattern| matches(pattern, &path, true)) {
                continue;
            }
            if path.is_dir() {
                let name = entry.file_name().to_string_lossy().to_string();
                let implicit_hidden = name.starts_with('.')
                    || matches!(
                        name.as_str(),
                        "node_modules" | "bower_components" | "jspm_packages"
                    );
                if implicit_hidden
                    && !include
                        .iter()
                        .any(|pattern| output_path(pattern).contains(&format!("/{name}/")))
                {
                    continue;
                }
                // Do not open an unrelated directory merely because it exists.
                if include.iter().any(|pattern| could_contain(pattern, &path)) {
                    walk(
                        &path, include, exclude, allow_js, reader, active, visited, selected,
                    )?;
                }
            } else if source_file(&path, allow_js)
                && include.iter().any(|pattern| matches(pattern, &path, false))
            {
                reader.authorize_future(&path, "included file", false)?;
                selected.insert(path);
            }
        }
        Ok(())
    })();
    active.remove(&canonical);
    result
}

fn source_file(path: &Path, allow_js: bool) -> bool {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("ts" | "tsx" | "mts" | "cts") => true,
        Some("js" | "jsx" | "mjs" | "cjs") => allow_js,
        _ => false,
    }
}

fn could_contain(pattern: &Path, directory: &Path) -> bool {
    let prefix = pattern
        .components()
        .take_while(|component| !wild_component(*component))
        .collect::<PathBuf>();
    prefix.starts_with(directory)
        || directory.starts_with(&prefix)
        || prefix
            .parent()
            .is_some_and(|parent| directory.starts_with(parent))
}

fn matches(pattern: &Path, path: &Path, directory: bool) -> bool {
    let text = |path: &Path| {
        path.components()
            .filter_map(|component| match component {
                std::path::Component::Normal(value) => Some(value.to_string_lossy().into_owned()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("/")
    };
    let pattern = text(pattern);
    let path = text(path);
    let (pattern, path) = if cfg!(windows) {
        (pattern.to_lowercase(), path.to_lowercase())
    } else {
        (pattern, path)
    };
    let mut parts = pattern.split('/').collect::<Vec<_>>();
    if !parts.iter().any(|part| part.contains(['*', '?'])) {
        return path == pattern
            || path
                .strip_prefix(&pattern)
                .is_some_and(|tail| tail.starts_with('/'));
    }
    if directory && parts.last() == Some(&"**") {
        parts.push("*");
    }
    let target = path.split('/').collect::<Vec<_>>();
    glob_parts(&parts, &target)
}

fn glob_parts(pattern: &[&str], path: &[&str]) -> bool {
    let mut previous = vec![false; path.len() + 1];
    previous[0] = true;
    for component in pattern {
        let mut current = vec![false; path.len() + 1];
        if *component == "**" {
            current[0] = previous[0];
        }
        for index in 1..=path.len() {
            current[index] = if *component == "**" {
                previous[index] || current[index - 1]
            } else {
                previous[index - 1]
                    && wildcard(
                        &component.encode_utf16().collect::<Vec<_>>(),
                        &path[index - 1].encode_utf16().collect::<Vec<_>>(),
                    )
            };
        }
        previous = current;
    }
    previous[path.len()]
}

fn wildcard(pattern: &[u16], value: &[u16]) -> bool {
    let mut previous = vec![false; value.len() + 1];
    previous[0] = true;
    for byte in pattern {
        let mut current = vec![false; value.len() + 1];
        if *byte == u16::from(b'*') {
            current[0] = previous[0];
        }
        for index in 1..=value.len() {
            current[index] = match *byte {
                42 => previous[index] || current[index - 1],
                63 => previous[index - 1],
                literal => previous[index - 1] && literal == value[index - 1],
            };
        }
        previous = current;
    }
    previous[value.len()]
}
