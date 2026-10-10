// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Logical map locations affect URLs without changing authorized write paths.

use super::*;
use blueice_bluets::SourceMap;

pub(super) fn publication(
    map: &SourceMap,
    invocation: &Invocation,
    source: &Path,
    relative: &Path,
    javascript: &Path,
    extension: &str,
) -> io::Result<(SourceMap, String)> {
    let project = invocation.project_config.as_ref().expect("native project");
    let parent = javascript
        .parent()
        .ok_or_else(|| io::Error::other("output has no parent"))?;
    let name = javascript
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| io::Error::other("output filename is not UTF-8"))?;
    let map_relative = relative.with_extension(format!("{extension}.map"));
    let mut map = map.clone();
    map.file = name.to_string();
    let mut base = parent.to_path_buf();
    let mut source_url = false;
    let url = match invocation
        .options
        .map_root
        .as_deref()
        .filter(|root| !root.is_empty())
    {
        None => format!("{name}.map"),
        Some(root) if root.contains("://") => {
            source_url = true;
            format!(
                "{}/{}",
                root.trim_end_matches('/'),
                output_path(&map_relative)
            )
        }
        Some(root) => {
            let logical = tsconfig::clean_path(&project.emit_root.join(root).join(&map_relative));
            base = logical.parent().expect("logical map parent").to_path_buf();
            if Path::new(root).is_absolute() {
                output_path(&logical)
            } else {
                tsconfig::relative_text(parent, &logical)
            }
        }
    };
    map.sources = vec![if !map.source_root.is_empty() {
        tsconfig::relative_text(&project.emit_root, source)
    } else if source_url {
        let source = output_path(source);
        format!(
            "file://{}{source}",
            if source.starts_with('/') { "" } else { "/" }
        )
    } else {
        tsconfig::relative_text(&base, source)
    }];
    Ok((map, url))
}
