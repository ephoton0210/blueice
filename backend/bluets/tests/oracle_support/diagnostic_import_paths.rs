// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Preserve virtual-root corpus comparisons against physical project CLI paths.
use std::path::Path;

pub fn relative_message(message: &str, canonical_root: &Path) -> String {
    let root = canonical_root.to_string_lossy().replace('\\', "/");
    let ordinary = if let Some(unc) = root.strip_prefix("//?/UNC/") {
        format!("//{unc}")
    } else {
        root.strip_prefix("//?/").unwrap_or(&root).to_string()
    };
    let mut roots = vec![ordinary.clone()];
    if let Some(unc) = ordinary.strip_prefix("//") {
        roots.push(format!("//?/UNC/{unc}"));
    } else if ordinary.as_bytes().get(1) == Some(&b':') {
        roots.push(format!("//?/{ordinary}"));
    }
    // The separator belongs to the matched prefix. Foreign owners, sibling
    // prefixes, source filenames and every type/member suffix remain exact.
    roots
        .into_iter()
        .fold(message.to_string(), |message, root| {
            let quoted = serde_json::to_string(&format!("{root}/")).unwrap();
            let prefix = format!("import({}", quoted.strip_suffix('"').unwrap());
            message.replace(&prefix, "import(\"")
        })
}
