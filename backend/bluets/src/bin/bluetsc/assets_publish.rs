// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Data assets share source preservation and atomic owner publication.

use super::*;

pub(super) fn validate_sources(
    output: &Path,
    root: &Path,
    assets: &BTreeMap<String, String>,
) -> io::Result<()> {
    for id in assets.keys() {
        if root.join(id).starts_with(output) {
            return Err(io::Error::other(format!(
                "output directory would replace source asset {id}"
            )));
        }
    }
    Ok(())
}

pub(super) fn stage(
    root: &Path,
    stage: &Path,
    assets: &BTreeMap<String, String>,
    metadata: &BuildMetadata,
) -> io::Result<()> {
    if !assets.is_empty() && metadata.runtime_policy == RuntimePolicy::StrictRuntime.as_str() {
        return Err(io::Error::other(
            "strict-runtime data assets require a supported runtime profile",
        ));
    }
    for (id, source) in assets {
        let relative = if metadata.root_dir.is_some() {
            tsconfig::emitted_path(id, metadata)?
        } else {
            artifact_relative_path(root, Path::new(id), id)?
        };
        if relative
            .extension()
            .is_none_or(|extension| extension != "json")
            || matches!(
                relative.to_str(),
                Some("bluetsc.manifest.json" | "bluetsc.importmap.json")
            )
        {
            return Err(io::Error::other("invalid or reserved JSON asset path"));
        }
        let destination = stage.join(relative);
        if destination.exists() {
            return Err(io::Error::other("JSON asset collides with another output"));
        }
        fs::create_dir_all(
            destination
                .parent()
                .ok_or_else(|| io::Error::other("asset path has no parent"))?,
        )?;
        fs::write(destination, source)?;
    }
    Ok(())
}
