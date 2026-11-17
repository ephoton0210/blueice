// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Only the local process environment selects the private state root.

use super::*;

pub(super) fn from_environment() -> io::Result<PathBuf> {
    let root = if let Some(path) = env::var_os("BLUETSC_OWNER_STATE_DIR") {
        PathBuf::from(path)
    } else {
        default_root()?
    };
    if !root.is_absolute()
        || root.file_name().is_none()
        || root.as_os_str().len() > 16_384
        || root.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir | std::path::Component::CurDir
            )
        })
    {
        return Err(io::Error::other(
            "private state root must be a bounded absolute directory",
        ));
    }
    Ok(root)
}

fn required(name: &str) -> io::Result<PathBuf> {
    env::var_os(name)
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| {
            io::Error::other(format!(
                "private state requires {name} or BLUETSC_OWNER_STATE_DIR"
            ))
        })
}

fn default_root() -> io::Result<PathBuf> {
    #[cfg(target_os = "windows")]
    let cache = required("LOCALAPPDATA")?;
    #[cfg(target_os = "macos")]
    let cache = required("HOME")?.join("Library/Caches");
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let cache = if let Some(path) = env::var_os("XDG_CACHE_HOME").filter(|s| !s.is_empty()) {
        PathBuf::from(path)
    } else {
        required("HOME")?.join(".cache")
    };
    Ok(cache.join("blueice/bluets/owner-state-v1"))
}

pub(super) fn prepare(root: &Path, create: bool) -> io::Result<bool> {
    let mut current = PathBuf::new();
    for component in root.components() {
        current.push(component.as_os_str());
        if matches!(
            component,
            std::path::Component::Prefix(_) | std::path::Component::RootDir
        ) {
            continue;
        }
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
            Ok(_) => {
                return Err(io::Error::other(
                    "private state path contains a symlink or non-directory",
                ))
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound && !create => return Ok(false),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let mut builder = fs::DirBuilder::new();
                builder.recursive(false);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::DirBuilderExt;
                    builder.mode(0o700);
                }
                match builder.create(&current) {
                    Ok(()) => {}
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                    Err(error) => return Err(error),
                }
                let metadata = fs::symlink_metadata(&current)?;
                if !metadata.is_dir() || metadata.file_type().is_symlink() {
                    return Err(io::Error::other(
                        "private state directory changed during creation",
                    ));
                }
            }
            Err(error) => return Err(error),
        }
    }
    #[cfg(unix)]
    verify_owner(root)?;
    Ok(true)
}

#[cfg(unix)]
fn verify_owner(root: &Path) -> io::Result<()> {
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
    if fs::symlink_metadata(root)?.permissions().mode() & 0o077 != 0 {
        return Err(io::Error::other(
            "private state directory permits access by another user",
        ));
    }
    // A newly created file identifies the effective filesystem user without
    // unsafe platform calls or trusting an environment-provided numeric UID.
    let probe = root.join(format!(".owner-probe-{}", nonce()?));
    let file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&probe)?;
    let result = (|| {
        let directory = fs::symlink_metadata(root)?;
        if directory.file_type().is_symlink() || directory.uid() != file.metadata()?.uid() {
            return Err(io::Error::other(
                "private state directory belongs to another filesystem user",
            ));
        }
        Ok(())
    })();
    drop(file);
    let cleanup = fs::remove_file(probe);
    result?;
    cleanup
}

pub(super) fn nonce() -> io::Result<String> {
    use ring::rand::SecureRandom;
    let mut bytes = [0; 16];
    ring::rand::SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| io::Error::other("private state entropy is unavailable"))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}
