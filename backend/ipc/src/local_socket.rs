// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Shared safe defaults for BlueIce's local Unix-domain sockets.
//!
//! All production protocol endpoints rendezvous below one directory that is
//! private to the real Unix user.  `std::process::id` is deliberately not a
//! fallback identity: processes must independently derive the same path.

use std::ffi::OsString;
use std::fs;
use std::io;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};

/// The actual Unix user id, including on platforms without Linux's
/// `/proc/self/status` (notably macOS).
pub fn current_uid() -> u32 {
    // SAFETY: `getuid` has no preconditions and only reads the calling
    // process's credential.
    unsafe { libc::getuid() }
}

fn socket_dir_from(runtime_dir: Option<OsString>, temp_dir: PathBuf, uid: u32) -> PathBuf {
    match runtime_dir.filter(|dir| !dir.is_empty()) {
        Some(dir) => PathBuf::from(dir).join("blueice"),
        None => temp_dir.join(format!("blueice-{uid}")),
    }
}

/// The default directory containing all local BlueIce sockets.
pub fn default_socket_dir() -> PathBuf {
    socket_dir_from(std::env::var_os("XDG_RUNTIME_DIR"), std::env::temp_dir(), current_uid())
}

/// Creates `dir` if necessary, then verifies that it is an ordinary directory
/// owned by this user and restricts it to that user.  This prevents another
/// local user from pre-creating a predictable fallback directory and hosting
/// a counterfeit gatekeeper or downloads socket there.
pub fn ensure_private_dir(dir: &Path) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    let metadata = fs::symlink_metadata(dir)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, format!("the socket directory {} is not a real directory", dir.display())));
    }
    if metadata.uid() != current_uid() {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, format!("the socket directory {} is not owned by this user", dir.display())));
    }
    fs::set_permissions(dir, fs::Permissions::from_mode(0o700))
}

/// The socket-specific spelling retained for call sites that establish a
/// local IPC listener.
pub fn ensure_private_socket_dir(dir: &Path) -> io::Result<()> {
    ensure_private_dir(dir)
}

/// Binds a Unix socket while a restrictive umask is in effect, then pins its
/// mode to `0600`.  The umask closes the interval between `bind` and
/// `set_permissions` in which another local user could otherwise connect.
pub fn bind_private_listener(path: &Path) -> io::Result<std::os::unix::net::UnixListener> {
    let old_umask = unsafe { libc::umask(0o077) };
    let listener = std::os::unix::net::UnixListener::bind(path);
    unsafe { libc::umask(old_umask) };
    let listener = listener?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(listener)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn scratch(label: &str) -> PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!("bi-ipc-{label}-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn fallback_socket_paths_are_keyed_by_the_real_uid_not_a_process_id() {
        let temp = PathBuf::from("/tmp/blueice-test");
        assert_eq!(socket_dir_from(None, temp.clone(), 42), temp.join("blueice-42"));
        assert_eq!(socket_dir_from(Some(OsString::from("/run/user/42")), temp, 999), PathBuf::from("/run/user/42/blueice"));
        assert_eq!(current_uid(), unsafe { libc::getuid() });
    }

    #[test]
    fn a_socket_directory_and_socket_are_private_from_creation() {
        let root = scratch("private");
        let dir = root.join("blueice");
        ensure_private_socket_dir(&dir).unwrap();
        assert_eq!(fs::metadata(&dir).unwrap().permissions().mode() & 0o777, 0o700);

        let socket = dir.join("test.sock");
        let listener = bind_private_listener(&socket).unwrap();
        assert_eq!(fs::metadata(&socket).unwrap().permissions().mode() & 0o777, 0o600);
        drop(listener);
        let _ = fs::remove_dir_all(root);
    }
}
