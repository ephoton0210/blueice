// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Descriptor-relative file operations for download transaction files.
//!
//! A canonical path check alone is not confinement: another local process
//! can replace a parent directory with a symlink between that check and an
//! ordinary path-based `open`.  Walk every parent with `openat(O_NOFOLLOW)`
//! and perform the final operation relative to the retained directory fd.

use std::ffi::CString;
use std::fs::File;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::ffi::OsStrExt;
use std::path::{Component, Path};

struct Parent {
    dir: File,
    name: CString,
}

fn c_name(path: &Path) -> io::Result<CString> {
    CString::new(path.as_os_str().as_bytes()).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("path contains a NUL: {}", path.display()),
        )
    })
}

fn open_dir_at(parent: &File, name: &CString) -> io::Result<File> {
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe { File::from_raw_fd(fd) })
}

/// Resolve only the platform-owned temporary-directory prefix once, so macOS
/// `/var -> /private/var` does not make tests and temporary destinations
/// fail. Download-manager roots are canonicalized before reaching this
/// module; every untrusted component below either root is opened by fd with
/// `O_NOFOLLOW` below and is never canonicalized again.
fn physical_path(path: &Path) -> io::Result<std::path::PathBuf> {
    let temp = std::env::temp_dir();
    if let Ok(relative) = path.strip_prefix(&temp) {
        let mut resolved = std::fs::canonicalize(&temp)?;
        resolved.push(relative);
        return Ok(resolved);
    }
    Ok(path.to_path_buf())
}

fn parent(path: &Path, create: bool) -> io::Result<Parent> {
    if !path.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("download path must be absolute: {}", path.display()),
        ));
    }
    let path = physical_path(path)?;
    let name = c_name(Path::new(path.file_name().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("download path has no file name: {}", path.display()),
        )
    })?))?;
    let mut dir = File::open("/")?;
    let parent = path.parent().expect("a path with a file name has a parent");
    for component in parent.components() {
        if matches!(component, Component::ParentDir) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("download path must not contain '..': {}", path.display()),
            ));
        }
        let Component::Normal(component) = component else {
            continue;
        };
        let component = c_name(Path::new(component))?;
        match open_dir_at(&dir, &component) {
            Ok(next) => dir = next,
            Err(error) if create && error.kind() == io::ErrorKind::NotFound => {
                if unsafe { libc::mkdirat(dir.as_raw_fd(), component.as_ptr(), 0o700) } != 0 {
                    let error = io::Error::last_os_error();
                    if error.kind() != io::ErrorKind::AlreadyExists {
                        return Err(error);
                    }
                }
                dir = open_dir_at(&dir, &component)?;
            }
            Err(error) => return Err(error),
        }
    }
    Ok(Parent { dir, name })
}

fn open_at(parent: &Parent, flags: libc::c_int) -> io::Result<File> {
    let fd = unsafe {
        libc::openat(
            parent.dir.as_raw_fd(),
            parent.name.as_ptr(),
            flags | libc::O_NOFOLLOW,
            0o600,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe { File::from_raw_fd(fd) })
}

pub(crate) fn open_new(path: &Path) -> io::Result<File> {
    open_at(
        &parent(path, true)?,
        libc::O_RDWR | libc::O_CREAT | libc::O_EXCL,
    )
}

pub(crate) fn open_existing(path: &Path, writable: bool) -> io::Result<File> {
    open_at(
        &parent(path, false)?,
        if writable {
            libc::O_RDWR
        } else {
            libc::O_RDONLY
        },
    )
}

pub(crate) fn open_replace(path: &Path) -> io::Result<File> {
    open_at(
        &parent(path, false)?,
        libc::O_RDWR | libc::O_CREAT | libc::O_TRUNC,
    )
}

pub(crate) fn open_replace_creating_parent(path: &Path) -> io::Result<File> {
    open_at(
        &parent(path, true)?,
        libc::O_RDWR | libc::O_CREAT | libc::O_TRUNC,
    )
}

pub(crate) fn remove(path: &Path) -> io::Result<()> {
    let parent = parent(path, false)?;
    if unsafe { libc::unlinkat(parent.dir.as_raw_fd(), parent.name.as_ptr(), 0) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

pub(crate) fn replace(source: &Path, dest: &Path) -> io::Result<()> {
    let source = parent(source, false)?;
    let dest = parent(dest, false)?;
    if unsafe {
        libc::renameat(
            source.dir.as_raw_fd(),
            source.name.as_ptr(),
            dest.dir.as_raw_fd(),
            dest.name.as_ptr(),
        )
    } != 0
    {
        return Err(io::Error::last_os_error());
    }
    dest.dir.sync_all()
}

pub(crate) fn link_no_replace(source: &Path, dest: &Path) -> io::Result<()> {
    let source = parent(source, false)?;
    let dest = parent(dest, false)?;
    // The source was opened with O_NOFOLLOW when created/resumed. `linkat`
    // is still used rather than rename so an existing destination cannot be
    // replaced between a check and the move.
    if unsafe {
        libc::linkat(
            source.dir.as_raw_fd(),
            source.name.as_ptr(),
            dest.dir.as_raw_fd(),
            dest.name.as_ptr(),
            0,
        )
    } != 0
    {
        return Err(io::Error::last_os_error());
    }
    if unsafe { libc::unlinkat(source.dir.as_raw_fd(), source.name.as_ptr(), 0) } != 0 {
        return Err(io::Error::last_os_error());
    }
    dest.dir.sync_all()
}
