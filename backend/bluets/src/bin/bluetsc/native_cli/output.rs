// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Native emission replaces individual artifacts and preserves project inputs.

use super::*;
use std::io::Write;

pub(super) fn publish(
    invocation: &Invocation,
    summary: &CompileSummary,
    loader: &FileLoader,
    source_files: &[PathBuf],
) -> io::Result<Vec<PathBuf>> {
    let metadata = build_metadata(invocation, summary, package_manifest(invocation, loader));
    let project = invocation.project_config.as_ref().expect("native project");
    if invocation.options.runtime_policy == RuntimePolicy::StrictRuntime {
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
        for (module, artifact) in &summary.artifacts {
            let relative = tsconfig::emitted_path(module, &metadata)?;
            let extension =
                output_extension(&relative, invocation.options.jsx == Some(JsxMode::Preserve));
            let path = out.join(relative.with_extension(extension));
            if artifact.source_map.is_some() {
                emitted.push(path.with_extension(format!("{extension}.map")));
            }
            if artifact.declaration.is_some() {
                emitted.push(path.with_extension("d.ts"));
            }
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
        return Ok(emitted);
    }
    strict_publish::verify(&metadata, &summary.artifacts, &summary.declaration_modules)?;
    let mut writes = BTreeMap::new();
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
            let name = javascript_path
                .file_name()
                .and_then(|n| n.to_str())
                .ok_or_else(|| io::Error::other("output filename is not UTF-8"))?;
            javascript.push_str(&format!("\n//# sourceMappingURL={name}.map\n"));
            insert(
                &mut writes,
                javascript_path.with_extension(format!("{extension}.map")),
                map.to_json().into_bytes(),
            )?;
        }
        if let Some(declaration) = &artifact.declaration {
            insert(
                &mut writes,
                javascript_path.with_extension("d.ts"),
                declaration.as_bytes().to_vec(),
            )?;
        }
        insert(&mut writes, javascript_path, javascript.into_bytes())?;
    }
    for (module, source) in &summary.assets {
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
    for (path, contents) in writes {
        atomic_write(&path, &contents, &invocation.root)?;
    }
    Ok(paths)
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
fn atomic_write(path: &Path, contents: &[u8], root: &Path) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("output has no parent"))?;
    fs::create_dir_all(parent)?;
    authorize(path, root)?;
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let stage = parent.join(format!(".bluetsc-write-{}-{nonce}", std::process::id()));
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&stage)?;
        file.write_all(contents)?;
        fs::rename(&stage, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(stage);
    }
    result
}
