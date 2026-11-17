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
    super::super::publication_maps::publication(
        map,
        source,
        &project.emit_root,
        relative,
        javascript,
        extension,
        invocation.options.map_root.as_deref(),
    )
}
