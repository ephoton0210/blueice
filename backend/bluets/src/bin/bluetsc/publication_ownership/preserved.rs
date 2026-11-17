// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Preserve unrelated directory entries without reading or following symlinks.

use super::*;

pub(super) fn copy(
    root: &Path,
    stage: &Path,
    previous: &BTreeMap<String, OwnedFile>,
    current: &BTreeMap<String, OwnedFile>,
) -> io::Result<()> {
    fn visit(
        root: &Path,
        directory: &Path,
        stage: &Path,
        previous: &BTreeMap<String, OwnedFile>,
        current: &BTreeMap<String, OwnedFile>,
    ) -> io::Result<()> {
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            let path = entry.path();
            let relative = path.strip_prefix(root).expect("visited output descendant");
            let name = output_path(relative);
            if name == "bluetsc.manifest.json"
                || previous.contains_key(&name)
                || current.contains_key(&name)
            {
                continue;
            }
            let target = stage.join(relative);
            let kind = entry.file_type()?;
            if kind.is_dir() {
                fs::create_dir_all(&target)?;
                visit(root, &path, stage, previous, current)?;
            } else if kind.is_file() {
                fs::create_dir_all(target.parent().expect("preserved output parent"))?;
                // Stage and output share a filesystem; linking preserves bytes,
                // permissions and storage without copying arbitrary user data.
                fs::hard_link(path, target)?;
            } else if kind.is_symlink() {
                fs::create_dir_all(target.parent().expect("preserved link parent"))?;
                copy_link(&path, &target)?;
            } else {
                return Err(io::Error::other("cannot preserve a special output entry"));
            }
        }
        Ok(())
    }
    if root.exists() {
        visit(root, root, stage, previous, current)?;
    }
    Ok(())
}

#[cfg(unix)]
fn copy_link(source: &Path, target: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(fs::read_link(source)?, target)
}

#[cfg(windows)]
fn copy_link(source: &Path, target: &Path) -> io::Result<()> {
    use std::os::windows::fs::FileTypeExt;
    let link = fs::read_link(source)?;
    if fs::symlink_metadata(source)?.file_type().is_symlink_dir() {
        std::os::windows::fs::symlink_dir(link, target)
    } else {
        std::os::windows::fs::symlink_file(link, target)
    }
}

#[cfg(not(any(unix, windows)))]
fn copy_link(_: &Path, _: &Path) -> io::Result<()> {
    Err(io::Error::other(
        "preserving symlinks is unsupported on this host",
    ))
}
