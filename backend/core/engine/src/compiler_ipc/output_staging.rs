// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Owner-only atomic directory staging for registered compiler generations.

use super::{OwnerOutputWriteGrant, RegisteredProjectGeneration};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_STAGE: AtomicU64 = AtomicU64::new(0);

struct StageCleanup {
    path: PathBuf,
    active: bool,
}

impl Drop for StageCleanup {
    fn drop(&mut self) {
        if self.active {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

pub(super) fn stage_and_publish<F>(
    grant: &OwnerOutputWriteGrant,
    generation: RegisteredProjectGeneration,
    populate: F,
) -> io::Result<PathBuf>
where
    F: FnOnce(&Path) -> io::Result<()>,
{
    grant.validate_current_root()?;
    let root = grant.canonical_root();
    let published = root.join(format!(
        "blueice-project-{}-generation-{}",
        generation.project_id().as_u64(),
        generation.sequence()
    ));
    reject_existing_destination(&published)?;
    let stage = create_stage_directory(root)?;
    let mut cleanup = StageCleanup {
        path: stage.clone(),
        active: true,
    };
    populate(&stage)?;
    reject_existing_destination(&published)?;
    fs::rename(&stage, &published)?;
    cleanup.active = false;
    Ok(published)
}

fn create_stage_directory(root: &Path) -> io::Result<PathBuf> {
    for _ in 0..16 {
        let nonce = NEXT_STAGE.fetch_add(1, Ordering::Relaxed);
        let path = root.join(format!(".blueice-stage-{}-{nonce}", std::process::id()));
        match fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "output staging name is exhausted",
    ))
}

fn reject_existing_destination(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(_) => Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "compiler generation output already exists",
        )),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}
