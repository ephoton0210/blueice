// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Preserve virtual-root corpus comparisons against physical project CLI paths.
use std::path::Path;

pub fn relative_message(message: &str, canonical_root: &Path) -> String {
    let root = canonical_root.to_string_lossy().replace('\\', "/");
    let root = root.strip_prefix("//?/").unwrap_or(&root);
    let quoted = serde_json::to_string(&format!("{root}/")).unwrap();
    let prefix = format!("import({}", quoted.strip_suffix('"').unwrap());
    // The separator belongs to the matched prefix. Foreign owners, sibling
    // prefixes, source filenames and every type/member suffix remain exact.
    message.replace(&prefix, "import(\"")
}
