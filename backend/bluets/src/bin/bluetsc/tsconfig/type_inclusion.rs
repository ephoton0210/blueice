// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Implicit declaration roots are selected before the source graph is checked.

use super::*;

pub(super) fn files(invocation: &mut Invocation) -> Result<Vec<PathBuf>, String> {
    let Some(project) = &invocation.project_config else {
        return Ok(Vec::new());
    };
    let roots = if let Some(values) = project.options.get("typeRoots").and_then(Value::as_array) {
        values
            .iter()
            .filter_map(Value::as_str)
            .map(PathBuf::from)
            .collect::<Vec<_>>()
    } else {
        project
            .directory
            .ancestors()
            .take_while(|directory| directory.starts_with(&invocation.root))
            .map(|directory| directory.join("node_modules/@types"))
            .collect()
    };
    let mut allowed = vec![invocation.root.clone()];
    if let Some(packages) = &invocation.packages {
        allowed.extend(packages.extra_roots.iter().cloned());
    }
    let mut canonical_roots = Vec::new();
    for root in roots {
        if !root.exists() {
            continue;
        }
        let canonical = fs::canonicalize(&root)
            .map_err(|error| format!("cannot resolve typeRoots: {error}"))?;
        if !allowed.iter().any(|root| canonical.starts_with(root)) {
            return Err("typeRoots resolves outside project root".into());
        }
        if !canonical.is_dir() {
            return Err("typeRoots must name directories".into());
        }
        canonical_roots.push(canonical);
    }
    let names = if let Some(values) = project.options.get("types").and_then(Value::as_array) {
        values
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect::<BTreeSet<_>>()
    } else {
        let mut names = BTreeSet::new();
        for root in &canonical_roots {
            for entry in
                fs::read_dir(root).map_err(|error| format!("cannot list typeRoots: {error}"))?
            {
                let entry = entry.map_err(|error| format!("cannot list typeRoots: {error}"))?;
                let name = entry.file_name().to_string_lossy().into_owned();
                if !name.starts_with('.') && entry.path().is_dir() {
                    names.insert(name);
                }
            }
        }
        names
    };
    let resolver = invocation
        .packages
        .as_ref()
        .map(|settings| package_resolver(&invocation.root, settings))
        .unwrap_or_else(|| relative_resolver(&invocation.root));
    let mut files = Vec::new();
    for name in names {
        let encoded = if let Some(scoped) = name.strip_prefix('@') {
            scoped.replace('/', "__")
        } else {
            name.clone()
        };
        if encoded.is_empty()
            || Path::new(&encoded)
                .components()
                .any(|component| !matches!(component, std::path::Component::Normal(_)))
        {
            return Err("type library names must remain within their configured roots".into());
        }
        let mut found = None;
        for root in &canonical_roots {
            match resolver.resolve_relative(root, &format!("./{encoded}")) {
                Ok(file) => {
                    if !file.declaration {
                        return Err(format!(
                            "type library `{name}` requires a declaration entry"
                        ));
                    }
                    found = Some(file.path);
                    break;
                }
                Err(ResolveError::NotFound { .. }) => {}
                Err(error) => return Err(error.to_string()),
            }
        }
        files
            .push(found.ok_or_else(|| format!("cannot resolve configured type library `{name}`"))?);
    }
    files.sort();
    files.dedup();
    invocation
        .options
        .resolver_fingerprint
        .push_str(&format!("+types:{}", resolver.fingerprint()));
    Ok(files)
}
