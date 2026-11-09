// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Create the original filesystem fixtures with the platform's actual link API.

use std::io;
use std::path::Path;

pub(crate) fn symlink(target: impl AsRef<Path>, link: impl AsRef<Path>) -> io::Result<()> {
    let target = target.as_ref();
    let link = link.as_ref();
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link)
    }
    #[cfg(windows)]
    {
        // Windows reparse targets use native separators even when the caller
        // supplies slash-separated fixture paths; verbatim canonicalization
        // does not normalize a relative reparse target's forward slashes.
        let target = target.components().collect::<std::path::PathBuf>();
        // Relative link targets are resolved from the link's parent, not cwd.
        let resolved = if target.is_absolute() {
            target.to_path_buf()
        } else {
            link.parent().unwrap_or(Path::new(".")).join(&target)
        };
        if resolved.is_dir() {
            std::os::windows::fs::symlink_dir(&target, link)
        } else {
            std::os::windows::fs::symlink_file(&target, link)
        }
    }
}
