// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Layout changes retain original source identities and audited helper sites.

use super::*;

pub(super) fn prepare(
    invocation: &mut Invocation,
    summary: &mut CompileSummary,
) -> Result<(), String> {
    let Some(project) = &mut invocation.project_config else {
        return Ok(());
    };
    let mut sources = summary
        .artifacts
        .keys()
        .chain(summary.assets.keys())
        .map(|module| invocation.root.join(module))
        .collect::<Vec<_>>();
    if project
        .options
        .get("rootDir")
        .and_then(Value::as_str)
        .is_none()
    {
        // Owner import-map directories also need stable published URLs.
        sources.extend(
            invocation
                .imports
                .values()
                .filter(|target| !is_declaration_path(target))
                .map(|target| {
                    if target.is_dir() {
                        target.join("__import_map_prefix")
                    } else {
                        target.clone()
                    }
                }),
        );
        project.emit_root = common_source_root(&sources, &invocation.root);
    }
    for target in invocation
        .imports
        .values()
        .filter(|target| !is_declaration_path(target))
    {
        if !target.starts_with(&project.emit_root) {
            return Err("owner import-map target is outside configured rootDir".to_string());
        }
    }
    for (module, artifact) in &mut summary.artifacts {
        let relative = invocation
            .root
            .join(module)
            .strip_prefix(&project.emit_root)
            .map_err(|_| format!("source `{module}` is outside configured rootDir"))?
            .to_path_buf();
        let Some(record) = &mut artifact.strict_runtime else {
            continue;
        };
        let original = &record.helper_import.expected_text;
        if record.helper_import.generated_start != 0 || !artifact.javascript.starts_with(original) {
            return Err("strict artifact has an invalid helper import before layout".to_string());
        }
        let depth = relative.components().count().saturating_sub(1);
        let helper = if depth == 0 {
            format!("./{RUNTIME_HELPER_V1_FILE}")
        } else {
            format!("{}{RUNTIME_HELPER_V1_FILE}", "../".repeat(depth))
        };
        let replacement =
            format!("import {{ validateStringV1 as __bluetsValidateStringV1 }} from '{helper}';\n");
        let old_length = original.len();
        let new_length = replacement.len();
        artifact
            .javascript
            .replace_range(..old_length, &replacement);
        record.helper_import.expected_text = replacement;
        for boundary in &mut record.boundaries {
            for site in boundary
                .ingress
                .iter_mut()
                .chain(std::iter::once(&mut boundary.egress))
            {
                site.generated_start = site
                    .generated_start
                    .checked_sub(old_length)
                    .and_then(|offset| offset.checked_add(new_length))
                    .ok_or_else(|| "strict helper site cannot be relocated".to_string())?;
            }
        }
    }
    Ok(())
}

pub(super) fn emitted_path(module: &str, metadata: &BuildMetadata) -> io::Result<PathBuf> {
    let relative = artifact_relative_path(Path::new(""), Path::new(module), module)?;
    let Some(root) = &metadata.root_dir else {
        return Ok(relative);
    };
    if root == "." {
        return Ok(relative);
    }
    relative
        .strip_prefix(root)
        .map(Path::to_path_buf)
        .map_err(|_| io::Error::other(format!("artifact `{module}` is outside configured rootDir")))
}
