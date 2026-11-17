// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Native emission replaces individual artifacts and preserves project inputs.

use super::*;
use crate::publication_ownership::{self, OwnedOutputs};

pub(super) struct Published {
    pub(super) paths: Vec<PathBuf>,
    pub(super) declarations: BTreeMap<PathBuf, PathBuf>,
    pub(super) runtimes: BTreeMap<PathBuf, PathBuf>,
}

pub(super) fn publish(
    invocation: &Invocation,
    summary: &CompileSummary,
    loader: &FileLoader,
    source_files: &[PathBuf],
    retirement: publication_ownership::RetirementPolicy,
) -> io::Result<Published> {
    let metadata = build_metadata(invocation, summary, package_manifest(invocation, loader));
    let project = invocation.project_config.as_ref().expect("native project");
    if invocation.options.runtime_policy == RuntimePolicy::StrictRuntime {
        if invocation.options.emit_declaration_only
            || invocation.options.declaration_map
            || invocation.declaration_dir.is_some()
        {
            return Err(io::Error::other(
                "declaration option publishing requires checked runtime policy",
            ));
        }
        let out = invocation
            .out_dir
            .as_ref()
            .ok_or_else(|| io::Error::other("strict-runtime publishing requires owner outDir"))?;
        if project
            .config_inputs
            .iter()
            .any(|path| path.starts_with(out))
        {
            return Err(io::Error::other(
                "output directory contains a project configuration",
            ));
        }
        publish_build(
            &invocation.root,
            out,
            &summary.artifacts,
            &summary.declaration_modules,
            &summary.assets,
            &metadata,
        )?;
        let mut emitted = Vec::new();
        let mut declarations = BTreeMap::new();
        let mut runtimes = BTreeMap::new();
        for (module, artifact) in &summary.artifacts {
            let source = invocation.root.join(module);
            let relative = tsconfig::emitted_path(module, &metadata)?;
            let extension =
                output_extension(&relative, invocation.options.jsx == Some(JsxMode::Preserve));
            let path = out.join(relative.with_extension(extension));
            if artifact.source_map.is_some() && !invocation.inline_source_map {
                emitted.push(path.with_extension(format!("{extension}.map")));
            }
            if artifact.declaration.is_some() {
                let declaration =
                    path.with_extension(node_modules::declaration_extension(extension));
                declarations.insert(source.clone(), declaration.clone());
                emitted.push(declaration);
            }
            runtimes.insert(source, path.clone());
            emitted.push(path);
        }
        emitted.extend([
            out.join(RUNTIME_HELPER_V1_FILE),
            out.join("bluetsc.manifest.json"),
        ]);
        if metadata.has_configured_imports {
            emitted.push(out.join("bluetsc.importmap.json"));
        }
        emitted.sort();
        return Ok(Published {
            paths: emitted,
            declarations,
            runtimes,
        });
    }
    strict_publish::verify(&metadata, &summary.artifacts, &summary.declaration_modules)?;
    let mut writes = BTreeMap::new();
    let mut declarations = BTreeMap::new();
    let mut runtimes = BTreeMap::new();
    for (module, artifact) in &summary.artifacts {
        let source = invocation.root.join(module);
        let relative = tsconfig::emitted_path(module, &metadata)?;
        let extension =
            output_extension(&relative, invocation.options.jsx == Some(JsxMode::Preserve));
        let javascript_path = invocation.out_dir.as_ref().map_or_else(
            || source.with_extension(extension),
            |out| out.join(relative.with_extension(extension)),
        );
        let mut javascript = artifact.javascript.clone();
        if let Some(map) = &artifact.source_map {
            if !invocation.options.emit_declaration_only {
                let (map, external_url) = source_maps::publication(
                    map,
                    invocation,
                    &source,
                    &relative,
                    &javascript_path,
                    extension,
                )?;
                let url = if invocation.inline_source_map {
                    super::super::source_map_data::url(&map)
                } else {
                    external_url
                };
                let newline = match invocation.options.new_line {
                    blueice_bluets::NewLine::Lf => "\n",
                    blueice_bluets::NewLine::CrLf => "\r\n",
                };
                if invocation.options.javascript_layout
                    == blueice_bluets::JavaScriptLayout::TypeScript
                {
                    javascript.push_str(&format!("//# sourceMappingURL={url}"));
                } else {
                    javascript.push_str(&format!("{newline}//# sourceMappingURL={url}{newline}"));
                }
                if !invocation.inline_source_map {
                    insert(
                        &mut writes,
                        javascript_path.with_extension(format!("{extension}.map")),
                        map.to_json().into_bytes(),
                    )?;
                }
            }
        }
        if let Some(declaration) = &artifact.declaration {
            let extension = node_modules::declaration_extension(extension);
            let path = invocation.declaration_dir.as_ref().map_or_else(
                || javascript_path.with_extension(extension),
                |directory| directory.join(relative.with_extension(extension)),
            );
            let mut declaration = declaration.clone();
            if let Some(map) = &artifact.declaration_map {
                let (map, url) = source_maps::publication(
                    map, invocation, &source, &relative, &path, extension,
                )?;
                declaration.push_str(&format!("//# sourceMappingURL={url}"));
                insert(
                    &mut writes,
                    declaration_map_path(&path),
                    map.to_json().into_bytes(),
                )?;
            }
            declarations.insert(source.clone(), path.clone());
            insert(&mut writes, path, declaration.into_bytes())?;
        }
        if !invocation.options.emit_declaration_only {
            runtimes.insert(source.clone(), javascript_path.clone());
            insert(&mut writes, javascript_path, javascript.into_bytes())?;
        }
    }
    for (module, source) in summary
        .assets
        .iter()
        .filter(|_| !invocation.options.emit_declaration_only)
    {
        let relative = tsconfig::emitted_path(module, &metadata)?;
        let path = invocation
            .out_dir
            .as_ref()
            .map_or_else(|| invocation.root.join(module), |out| out.join(relative));
        insert(&mut writes, path, source.as_bytes().to_vec())?;
    }
    // Validate the complete destination set before creating any output.
    for path in writes.keys() {
        authorize(path, &invocation.root)?;
        let canonical = fs::canonicalize(path).ok();
        if source_files
            .iter()
            .chain(project.config_inputs.iter())
            .chain(project.files.iter())
            .any(|input| input == path || canonical.as_ref() == Some(input))
        {
            return Err(io::Error::other(
                "output path would replace a project input",
            ));
        }
    }
    let paths = writes.keys().cloned().collect();
    let receipt = publication_ownership::PrivateState::new(&invocation.root, &project.config_path)?;
    let mut previous = receipt.read()?;
    // Migrate the earlier project-local receipt in the same public file plan.
    // Its bytes must still prove ownership before it can be retired.
    let legacy_receipt = project.config_path.with_extension("bluetsc.outputs.json");
    authorize(&legacy_receipt, &invocation.root)?;
    if fs::symlink_metadata(&legacy_receipt).is_ok() {
        let legacy = OwnedOutputs::read(&invocation.root, &legacy_receipt)?;
        previous.extend_rebased(&legacy, &invocation.root, &invocation.root)?;
        previous.insert_existing(&invocation.root, &legacy_receipt)?;
    }
    if let Some(manifest) = legacy_manifest_to_retire(invocation, source_files, &metadata)? {
        let out = manifest.parent().expect("legacy manifest parent");
        previous.extend_rebased(&OwnedOutputs::legacy(out)?, out, &invocation.root)?;
        previous.insert_existing(&invocation.root, &manifest)?;
    }
    let owned = OwnedOutputs::from_writes(&invocation.root, &writes)?;
    let retired = previous.retired_with_policy(&invocation.root, &owned, retirement)?;
    for path in &retired {
        let canonical = fs::canonicalize(path).ok();
        if source_files
            .iter()
            .chain(project.config_inputs.iter())
            .chain(project.files.iter())
            .any(|input| input == path || canonical.as_ref() == Some(input))
        {
            return Err(io::Error::other(
                "output retirement would replace a project input",
            ));
        }
    }
    if !writes.is_empty() || !retired.is_empty() || receipt.exists() {
        publication_ownership::publish(&invocation.root, writes, retired, &receipt, &owned)?;
    }
    Ok(Published {
        paths,
        declarations,
        runtimes,
    })
}

fn legacy_manifest_to_retire(
    invocation: &Invocation,
    source_files: &[PathBuf],
    metadata: &BuildMetadata,
) -> io::Result<Option<PathBuf>> {
    let Some(out) = &invocation.out_dir else {
        return Ok(None);
    };
    let path = out.join("bluetsc.manifest.json");
    let information = match fs::symlink_metadata(&path) {
        Ok(information) => information,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    if information.file_type().is_symlink() {
        return Err(io::Error::other(
            "cannot retire symlinked publication metadata",
        ));
    }
    if !information.is_file() || information.len() > 8 * 1024 * 1024 {
        return Ok(None);
    }
    authorize(&path, &invocation.root)?;
    let contents = match fs::read(&path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(&contents) else {
        return Ok(None);
    };
    let owned = value["languageVersion"] == metadata.language_version
        && value["classHelperVersion"] == metadata.class_helper_version
        && value["entries"].is_array()
        && value["fingerprint"].as_str().is_some_and(|fingerprint| {
            fingerprint
                .strip_prefix("bts-project-")
                .is_some_and(|hash| {
                    hash.len() == 16 && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
                })
        });
    if !owned {
        return Ok(None);
    }
    let project = invocation.project_config.as_ref().expect("native project");
    let canonical = fs::canonicalize(&path)?;
    if source_files
        .iter()
        .chain(project.config_inputs.iter())
        .chain(project.files.iter())
        .any(|input| input == &path || input == &canonical)
    {
        return Err(io::Error::other("publication metadata is a project input"));
    }
    Ok(Some(path))
}
fn insert(
    writes: &mut BTreeMap<PathBuf, Vec<u8>>,
    path: PathBuf,
    contents: Vec<u8>,
) -> io::Result<()> {
    if writes.insert(path, contents).is_some() {
        return Err(io::Error::other(
            "multiple inputs would write the same output path",
        ));
    }
    Ok(())
}
fn authorize(path: &Path, root: &Path) -> io::Result<()> {
    if !path.starts_with(root)
        || path
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(io::Error::other(
            "output path leaves the canonical project root",
        ));
    }
    let mut ancestor = path;
    loop {
        match fs::symlink_metadata(ancestor) {
            Ok(_) => {
                let canonical = fs::canonicalize(ancestor)?;
                ensure_within(&canonical, root, "output path").map_err(io::Error::other)?;
                return Ok(());
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                ancestor = ancestor.parent().ok_or(error)?;
            }
            Err(error) => return Err(error),
        }
    }
}
fn declaration_map_path(declaration: &Path) -> PathBuf {
    let mut path = declaration.as_os_str().to_os_string();
    path.push(".map");
    PathBuf::from(path)
}
