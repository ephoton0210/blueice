// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `transfers.json`: every transfer -- including finished ones -- so that
//! history, and anything resumable, survives the downloads process
//! restarting (`phase-10-download-manager/PLAN.md`'s "Persistence"). That
//! is also what lets the process be torn down while idle without losing
//! the downloads list.

use blueice_ipc::downloads::TransferInfo;
use blueice_ipc::local_socket::ensure_private_dir;
use serde::{Deserialize, Serialize};
use std::fs::OpenOptions;
use std::io::{self, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

const STORE_VERSION: u32 = 1;
const FILE_NAME: &str = "transfers.json";

/// One persisted transfer: the record clients see, plus what only the
/// process itself needs in order to run it again.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoredTransfer {
    pub info: TransferInfo,
    #[serde(default)]
    pub overwrite: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Loaded {
    pub next_id: u64,
    pub transfers: Vec<StoredTransfer>,
}

#[derive(Serialize, Deserialize)]
struct StoreFile {
    version: u32,
    next_id: u64,
    transfers: Vec<StoredTransfer>,
}

pub struct Store {
    dir: PathBuf,
}

impl Store {
    pub fn new(dir: impl AsRef<Path>) -> Self {
        Store { dir: dir.as_ref().to_path_buf() }
    }

    fn path(&self) -> PathBuf {
        self.dir.join(FILE_NAME)
    }

    fn corrupt_path(&self) -> PathBuf {
        let path = self.path();
        let first = path.with_file_name(format!("{FILE_NAME}.corrupt"));
        if std::fs::symlink_metadata(&first).is_err() {
            return first;
        }
        (1u64..)
            .map(|n| path.with_file_name(format!("{FILE_NAME}.corrupt.{n}")))
            .find(|candidate| std::fs::symlink_metadata(candidate).is_err())
            .expect("an unbounded counter yields an unused corrupt-store name")
    }

    /// The stored transfers, or an empty store if there is none. A file that
    /// can't be read as this version's format is moved aside to
    /// `transfers.json.corrupt` -- the user's history is never thrown away
    /// -- and the process starts empty. `next_id` is never below the highest
    /// stored id, so an id is never reused.
    pub fn load(&self) -> Loaded {
        let empty = Loaded { next_id: 1, transfers: Vec::new() };
        let path = self.path();
        let Ok(bytes) = std::fs::read(&path) else { return empty };
        match serde_json::from_slice::<StoreFile>(&bytes) {
            Ok(file) if file.version == STORE_VERSION => {
                // A hand-edited store containing u64::MAX cannot wrap the
                // next id around to zero and silently reuse an old record.
                let floor = file.transfers.iter().map(|t| t.info.id.saturating_add(1)).max().unwrap_or(1);
                Loaded { next_id: file.next_id.max(floor), transfers: file.transfers }
            }
            _ => {
                let _ = std::fs::rename(&path, self.corrupt_path());
                empty
            }
        }
    }

    /// Writes atomically (a temporary file, `fsync`, rename), creating the
    /// directory if needed.
    pub fn save(&self, next_id: u64, transfers: &[StoredTransfer]) -> io::Result<()> {
        ensure_private_dir(&self.dir)?;
        let bytes = serde_json::to_vec(&StoreFile { version: STORE_VERSION, next_id, transfers: transfers.to_vec() }).map_err(io::Error::other)?;
        let path = self.path();
        let tmp = self.dir.join(format!("{FILE_NAME}.tmp"));
        let result = (|| {
            // The temporary file contains the same token-bearing transfer
            // history as the final file, so it must be private from creation,
            // not merely chmodded after it has been written.
            let mut file = OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&tmp)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
            std::fs::rename(&tmp, &path)
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use blueice_ipc::downloads::{TransferInfo, TransferState};

    struct Scratch(PathBuf);

    impl Scratch {
        fn new(label: &str) -> Self {
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!("bd-store-{label}-{}-{}", std::process::id(), NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
            std::fs::create_dir_all(&path).unwrap();
            Scratch(path)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn stored(id: u64, state: TransferState) -> StoredTransfer {
        StoredTransfer {
            info: TransferInfo { id, url: format!("https://example.com/{id}"), dest_path: format!("/d/{id}.bin"), state, total_bytes: Some(100), completed_bytes: 40, ..TransferInfo::default() },
            overwrite: id.is_multiple_of(2),
        }
    }

    #[test]
    fn transfers_and_the_next_id_survive_a_save_and_load() {
        let dir = Scratch::new("roundtrip");
        let store = Store::new(&dir.0);
        let transfers = vec![stored(1, TransferState::Completed), stored(2, TransferState::Paused)];
        store.save(3, &transfers).unwrap();
        assert_eq!(store.load(), Loaded { next_id: 3, transfers });
    }

    #[test]
    fn a_missing_store_loads_as_empty_starting_at_id_one() {
        let dir = Scratch::new("missing");
        assert_eq!(Store::new(&dir.0).load(), Loaded { next_id: 1, transfers: Vec::new() });
    }

    #[test]
    fn saving_creates_the_data_directory_and_leaves_no_temporary_file() {
        let dir = Scratch::new("create");
        let nested = dir.0.join("a").join("b");
        Store::new(&nested).save(1, &[]).unwrap();
        let names: Vec<String> = std::fs::read_dir(&nested).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        assert_eq!(names, vec!["transfers.json".to_string()]);
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&nested).unwrap().permissions().mode() & 0o777, 0o700, "transfer history's directory can contain tokenized URLs");
        assert_eq!(std::fs::metadata(nested.join(FILE_NAME)).unwrap().permissions().mode() & 0o777, 0o600, "transfer history itself is private");
    }

    #[test]
    fn a_second_save_replaces_the_first() {
        let dir = Scratch::new("replace");
        let store = Store::new(&dir.0);
        store.save(2, &[stored(1, TransferState::Active)]).unwrap();
        store.save(3, &[stored(1, TransferState::Completed), stored(2, TransferState::Failed)]).unwrap();
        let loaded = store.load();
        assert_eq!(loaded.next_id, 3);
        assert_eq!(loaded.transfers.len(), 2);
        assert_eq!(loaded.transfers[0].info.state, TransferState::Completed);
    }

    #[test]
    fn a_corrupt_store_loads_as_empty_and_is_kept_aside_instead_of_deleted() {
        let dir = Scratch::new("corrupt");
        std::fs::write(dir.0.join("transfers.json"), b"{ not json").unwrap();
        let loaded = Store::new(&dir.0).load();
        assert_eq!(loaded, Loaded { next_id: 1, transfers: Vec::new() });
        assert!(!dir.0.join("transfers.json").exists());
        assert_eq!(std::fs::read(dir.0.join("transfers.json.corrupt")).unwrap(), b"{ not json", "the user's history is not thrown away");
    }

    #[test]
    fn a_store_from_a_newer_version_is_kept_aside_rather_than_misread() {
        let dir = Scratch::new("version");
        std::fs::write(dir.0.join("transfers.json"), br#"{"version":99,"next_id":5,"transfers":[]}"#).unwrap();
        assert_eq!(Store::new(&dir.0).load().next_id, 1);
        assert!(dir.0.join("transfers.json.corrupt").exists());
    }

    #[test]
    fn each_bad_store_is_kept_aside_without_replacing_an_earlier_one() {
        let dir = Scratch::new("multiple-corrupt");
        let path = dir.0.join(FILE_NAME);
        std::fs::write(&path, b"first").unwrap();
        Store::new(&dir.0).load();
        std::fs::write(&path, b"second").unwrap();
        Store::new(&dir.0).load();
        assert_eq!(std::fs::read(dir.0.join("transfers.json.corrupt")).unwrap(), b"first");
        assert_eq!(std::fs::read(dir.0.join("transfers.json.corrupt.1")).unwrap(), b"second");
    }

    #[test]
    fn a_stored_record_missing_newer_fields_still_loads() {
        // TransferInfo fields are all `#[serde(default)]`; a record written
        // before a field existed must keep loading.
        let dir = Scratch::new("olderrecord");
        std::fs::write(dir.0.join("transfers.json"), br#"{"version":1,"next_id":8,"transfers":[{"info":{"id":7,"url":"u"},"overwrite":false}]}"#).unwrap();
        let loaded = Store::new(&dir.0).load();
        assert_eq!(loaded.next_id, 8);
        assert_eq!(loaded.transfers[0].info.id, 7);
    }

    #[test]
    fn next_id_never_falls_below_the_highest_stored_id() {
        // Guards against reusing an id if the file was edited or truncated.
        let dir = Scratch::new("nextid");
        std::fs::write(dir.0.join("transfers.json"), br#"{"version":1,"next_id":1,"transfers":[{"info":{"id":9,"url":"u"},"overwrite":false}]}"#).unwrap();
        assert_eq!(Store::new(&dir.0).load().next_id, 10);
    }

    #[test]
    fn a_maximum_stored_id_exhausts_ids_instead_of_wrapping() {
        let dir = Scratch::new("max-id");
        std::fs::write(
            dir.0.join(FILE_NAME),
            format!(r#"{{"version":1,"next_id":1,"transfers":[{{"info":{{"id":{},"url":"u"}},"overwrite":false}}]}}"#, u64::MAX),
        )
        .unwrap();
        assert_eq!(Store::new(&dir.0).load().next_id, u64::MAX);
    }
}
