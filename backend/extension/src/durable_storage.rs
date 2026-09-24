// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Version-two extension storage. Version one remains an in-memory bucket;
//! these separately named operations persist only under a core-selected,
//! private directory and a manifest-derived identity. A guest never sends a
//! path or bucket ID. Every operation reloads under a nonblocking OS file lock
//! so two core processes cannot silently overwrite each other's changes.

use crate::{
    bucket_storage_bytes, set_bounded_storage_value, validate_storage_key,
    MAX_STORAGE_BYTES_PER_EXTENSION, MAX_STORAGE_ENTRIES_PER_EXTENSION,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

const SCHEMA_VERSION: u32 = 1;
// Escaped JSON can be larger than the 256 KiB of original UTF-8 key/value
// data, but a valid bounded bucket stays far below this file-size ceiling.
const MAX_STORED_BYTES: u64 = 2 * 1024 * 1024;
static NEXT_TEMPORARY: AtomicU64 = AtomicU64::new(1);

#[derive(Clone)]
pub(crate) struct DurableExtensionStorage {
    root: PathBuf,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredBucket {
    schema_version: u32,
    extension_id: String,
    entries: BTreeMap<String, String>,
}

/// A persistent per-user data location, not the ephemeral socket directory.
/// If no user data directory can be determined, core must not quietly call a
/// temporary directory "durable".
pub fn default_durable_storage_root() -> Result<PathBuf, String> {
    let base = std::env::var_os("XDG_DATA_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| {
            std::env::var_os("HOME")
                .filter(|value| !value.is_empty())
                .map(|home| PathBuf::from(home).join(".local/share"))
                .filter(|path| path.is_absolute())
        })
        .ok_or_else(|| {
            "no user data directory is available for durable extension storage".to_string()
        })?;
    Ok(base.join("blueice/extension-storage-v2"))
}

impl DurableExtensionStorage {
    pub(crate) fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub(crate) fn get(&self, extension_id: &str, key: &str) -> Result<Option<String>, String> {
        validate_storage_key(key)?;
        let (_lock, _path, bucket) = self.lock_and_load(extension_id)?;
        Ok(bucket.get(key).cloned())
    }

    pub(crate) fn set(&self, extension_id: &str, key: String, value: String) -> Result<(), String> {
        let (_lock, path, mut bucket) = self.lock_and_load(extension_id)?;
        set_bounded_storage_value(&mut bucket, key, value)?;
        persist_bucket(&self.root, &path, extension_id, &bucket)
    }

    pub(crate) fn remove(&self, extension_id: &str, key: &str) -> Result<bool, String> {
        validate_storage_key(key)?;
        let (_lock, path, mut bucket) = self.lock_and_load(extension_id)?;
        let removed = bucket.remove(key).is_some();
        if removed {
            persist_bucket(&self.root, &path, extension_id, &bucket)?;
        }
        Ok(removed)
    }

    fn lock_and_load(
        &self,
        extension_id: &str,
    ) -> Result<(File, PathBuf, BTreeMap<String, String>), String> {
        let hash = identity_hash(extension_id)?;
        if !self.root.is_absolute() {
            return Err(
                "durable extension storage requires an absolute core-selected directory"
                    .to_string(),
            );
        }
        blueice_ipc::local_socket::ensure_private_dir(&self.root).map_err(|error| {
            format!(
                "preparing private extension storage directory {}: {error}",
                self.root.display()
            )
        })?;
        let lock_path = self.root.join(format!("{hash}.lock"));
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&lock_path)
            .map_err(|error| {
                format!(
                    "opening extension storage lock {}: {error}",
                    lock_path.display()
                )
            })?;
        validate_private_file(&lock, &lock_path)?;
        // SAFETY: flock only borrows the live file descriptor; the returned
        // File retains the lock until it is dropped after the operation.
        if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            return Err(format!(
                "extension storage is busy: {}",
                std::io::Error::last_os_error()
            ));
        }
        let path = self.root.join(format!("{hash}.json"));
        let bucket = read_bucket(&path, extension_id)?;
        Ok((lock, path, bucket))
    }
}

fn identity_hash(extension_id: &str) -> Result<&str, String> {
    let hash = extension_id
        .strip_prefix("sha256:")
        .ok_or_else(|| "durable storage requires a manifest-derived sha256 identity".to_string())?;
    if hash.len() != 64
        || !hash
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err("durable storage requires a canonical lowercase sha256 identity".to_string());
    }
    Ok(hash)
}

fn validate_private_file(file: &File, path: &Path) -> Result<(), String> {
    let metadata = file.metadata().map_err(|error| {
        format!(
            "inspecting extension storage file {}: {error}",
            path.display()
        )
    })?;
    if !metadata.is_file()
        || metadata.uid() != blueice_ipc::local_socket::current_uid()
        || metadata.permissions().mode() & 0o077 != 0
    {
        return Err(format!(
            "extension storage file {} is not private to this user",
            path.display()
        ));
    }
    Ok(())
}

fn read_bucket(path: &Path, extension_id: &str) -> Result<BTreeMap<String, String>, String> {
    let file = match OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
    {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
        Err(error) => {
            return Err(format!(
                "opening extension storage {}: {error}",
                path.display()
            ))
        }
    };
    validate_private_file(&file, path)?;
    if file.metadata().map_err(|error| error.to_string())?.len() > MAX_STORED_BYTES {
        return Err(format!(
            "extension storage {} exceeds the file-size limit",
            path.display()
        ));
    }
    let mut bytes = Vec::new();
    file.take(MAX_STORED_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("reading extension storage {}: {error}", path.display()))?;
    if bytes.len() as u64 > MAX_STORED_BYTES {
        return Err(format!(
            "extension storage {} exceeds the file-size limit",
            path.display()
        ));
    }
    let stored: StoredBucket = serde_json::from_slice(&bytes)
        .map_err(|error| format!("parsing extension storage {}: {error}", path.display()))?;
    if stored.schema_version != SCHEMA_VERSION || stored.extension_id != extension_id {
        return Err(format!(
            "extension storage {} has a mismatched schema or identity",
            path.display()
        ));
    }
    validate_loaded_bucket(&stored.entries)?;
    Ok(stored.entries)
}

fn validate_loaded_bucket(bucket: &BTreeMap<String, String>) -> Result<(), String> {
    if bucket.len() > MAX_STORAGE_ENTRIES_PER_EXTENSION
        || bucket_storage_bytes(bucket)? > MAX_STORAGE_BYTES_PER_EXTENSION
    {
        return Err("stored extension bucket exceeds its aggregate quota".to_string());
    }
    for (key, value) in bucket {
        validate_storage_key(key)?;
        if value.len() > blueice_ipc::extension::MAX_STORAGE_VALUE_BYTES {
            return Err("stored extension bucket contains an oversized value".to_string());
        }
    }
    Ok(())
}

fn persist_bucket(
    root: &Path,
    path: &Path,
    extension_id: &str,
    bucket: &BTreeMap<String, String>,
) -> Result<(), String> {
    let stored = StoredBucket {
        schema_version: SCHEMA_VERSION,
        extension_id: extension_id.to_string(),
        entries: bucket.clone(),
    };
    let bytes = serde_json::to_vec(&stored)
        .map_err(|error| format!("encoding extension storage: {error}"))?;
    if bytes.len() as u64 > MAX_STORED_BYTES {
        return Err("encoded extension storage exceeds the file-size limit".to_string());
    }
    let sequence = NEXT_TEMPORARY.fetch_add(1, Ordering::Relaxed);
    let temporary = path.with_extension(format!("tmp-{}-{sequence}", std::process::id()));
    let write = (|| -> Result<(), String> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&temporary)
            .map_err(|error| {
                format!(
                    "creating extension storage temporary {}: {error}",
                    temporary.display()
                )
            })?;
        file.write_all(&bytes)
            .and_then(|()| file.sync_all())
            .map_err(|error| {
                format!(
                    "writing extension storage temporary {}: {error}",
                    temporary.display()
                )
            })?;
        fs::rename(&temporary, path)
            .map_err(|error| format!("replacing extension storage {}: {error}", path.display()))?;
        File::open(root)
            .and_then(|directory| directory.sync_all())
            .map_err(|error| {
                format!(
                    "syncing extension storage directory {}: {error}",
                    root.display()
                )
            })
    })();
    if write.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    write
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    static NEXT_TEST: AtomicU64 = AtomicU64::new(1);
    const FIRST_ID: &str =
        "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const SECOND_ID: &str =
        "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    fn scratch(label: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "blueice-durable-{label}-{}-{}",
            std::process::id(),
            NEXT_TEST.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        path
    }

    #[test]
    fn durable_buckets_survive_a_new_owner_and_remain_identity_isolated() {
        let root = scratch("restart");
        let first = DurableExtensionStorage::new(root.clone());
        first
            .set(FIRST_ID, "key".to_string(), "value".to_string())
            .unwrap();
        let hash = identity_hash(FIRST_ID).unwrap();
        assert_eq!(
            fs::metadata(&root).unwrap().permissions().mode() & 0o777,
            0o700
        );
        for path in [
            root.join(format!("{hash}.json")),
            root.join(format!("{hash}.lock")),
        ] {
            assert_eq!(
                fs::metadata(path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        assert_eq!(first.get(SECOND_ID, "key").unwrap(), None);
        let restarted = DurableExtensionStorage::new(root.clone());
        assert_eq!(
            restarted.get(FIRST_ID, "key").unwrap().as_deref(),
            Some("value")
        );
        assert!(restarted.remove(FIRST_ID, "key").unwrap());
        assert!(!first.remove(FIRST_ID, "key").unwrap());
        assert_eq!(first.get(FIRST_ID, "key").unwrap(), None);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn durable_storage_rejects_noncanonical_identity_and_symlinked_data() {
        let root = scratch("symlink");
        let storage = DurableExtensionStorage::new(root.clone());
        assert!(storage
            .set("../escape", "k".to_string(), "v".to_string())
            .is_err());
        assert!(storage
            .set("sha256:ABC", "k".to_string(), "v".to_string())
            .is_err());
        let path = root.join(format!("{}.json", identity_hash(FIRST_ID).unwrap()));
        let target = root.join("other.txt");
        fs::write(&target, "not a bucket").unwrap();
        symlink(&target, &path).unwrap();
        assert!(storage.get(FIRST_ID, "k").is_err());
        assert!(storage
            .set(FIRST_ID, "k".to_string(), "v".to_string())
            .is_err());
        assert_eq!(fs::read_to_string(target).unwrap(), "not a bucket");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn durable_storage_rejects_corrupt_files_without_replacing_them() {
        let root = scratch("corrupt");
        let storage = DurableExtensionStorage::new(root.clone());
        storage
            .set(FIRST_ID, "k".to_string(), "v".to_string())
            .unwrap();
        let path = root.join(format!("{}.json", identity_hash(FIRST_ID).unwrap()));
        fs::write(&path, b"invalid json").unwrap();
        let before = fs::read(&path).unwrap();
        assert!(storage
            .set(FIRST_ID, "new".to_string(), "value".to_string())
            .is_err());
        assert_eq!(fs::read(&path).unwrap(), before);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn durable_storage_rejects_an_aggregate_overquota_write_without_losing_prior_values() {
        let root = scratch("quota");
        let storage = DurableExtensionStorage::new(root.clone());
        let value = "x".repeat(blueice_ipc::extension::MAX_STORAGE_VALUE_BYTES);
        for index in 0..15 {
            storage
                .set(FIRST_ID, format!("key-{index}"), value.clone())
                .unwrap();
        }
        assert!(storage.set(FIRST_ID, "key-15".into(), value).is_err());
        let restarted = DurableExtensionStorage::new(root.clone());
        assert!(restarted.get(FIRST_ID, "key-14").unwrap().is_some());
        assert_eq!(restarted.get(FIRST_ID, "key-15").unwrap(), None);
        fs::remove_dir_all(root).unwrap();
    }
}
