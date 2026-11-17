// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Diagnostic identity comparison within one known owner project.

use std::path::Path;

pub fn relative_file(file: &str, root: &Path) -> String {
    // Native-style --build uses forward slashes even for Windows verbatim
    // paths. Compare both owner identities with the same separators before
    // using Rust's platform-specific Path prefix interpretation.
    let file = file.replace('\\', "/");
    let root = root.to_string_lossy().replace('\\', "/");
    if let Some(relative) = file.strip_prefix(&(root + "/")) {
        return relative.to_owned();
    }
    assert!(
        !Path::new(&file).is_absolute(),
        "diagnostic file is outside the known project: {file}"
    );
    file
}
