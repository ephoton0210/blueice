// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Host-local receipts are separate from source/output filesystem authority.

use super::*;
#[path = "private_state/paths.rs"]
mod paths;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    version: u8,
    owner_root: String,
    configuration: String,
    outputs: OwnedOutputs,
}

pub(crate) struct PrivateState {
    root: PathBuf,
    path: PathBuf,
    owner: PathBuf,
    configuration: String,
}

impl PrivateState {
    pub(crate) fn new(owner: &Path, configuration: &Path) -> io::Result<Self> {
        let root = paths::from_environment()?;
        if root.starts_with(owner) {
            return Err(io::Error::other(
                "private state root must be outside the project owner root",
            ));
        }
        let owner_text = identity(owner)?;
        let configuration = identity(configuration)?.to_string();
        let key =
            serde_json::to_vec(&(1u8, owner_text, &configuration)).map_err(io::Error::other)?;
        let key = blueice_bluets::remote_declarations::sha256_hex(&key);
        Ok(Self {
            path: root.join(format!("{key}.json")),
            root,
            owner: owner.to_path_buf(),
            configuration,
        })
    }

    pub(crate) fn read(&self) -> io::Result<OwnedOutputs> {
        if !paths::prepare(&self.root, false)? {
            return Ok(OwnedOutputs::default());
        }
        let Some(bytes) = read_receipt(&self.root, &self.path)? else {
            return Ok(OwnedOutputs::default());
        };
        let envelope: Envelope = serde_json::from_slice(&bytes)
            .map_err(|_| io::Error::other("private ownership receipt has invalid data"))?;
        if envelope.version != 1
            || envelope.owner_root != identity(&self.owner)?
            || envelope.configuration != self.configuration
        {
            return Err(io::Error::other(
                "private ownership receipt belongs to another project",
            ));
        }
        envelope.outputs.validate(&self.owner)?;
        Ok(envelope.outputs)
    }

    pub(crate) fn exists(&self) -> bool {
        fs::symlink_metadata(&self.path).is_ok()
    }

    pub(super) fn stage(&self, owned: &OwnedOutputs) -> io::Result<StagedReceipt> {
        let bytes = serde_json::to_vec_pretty(&Envelope {
            version: 1,
            owner_root: identity(&self.owner)?.to_string(),
            configuration: self.configuration.clone(),
            outputs: owned.clone(),
        })
        .map_err(io::Error::other)?;
        if bytes.len() as u64 > MAX_RECEIPT_BYTES {
            return Err(io::Error::other(
                "private ownership receipt exceeds its byte budget",
            ));
        }
        paths::prepare(&self.root, true)?;
        let temporary = self.root.join(format!(".pending-{}", paths::nonce()?));
        let staged = StagedReceipt {
            root: self.root.clone(),
            path: self.path.clone(),
            temporary,
        };
        use std::io::Write;
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&staged.temporary)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        Ok(staged)
    }
}

pub(super) struct StagedReceipt {
    root: PathBuf,
    path: PathBuf,
    temporary: PathBuf,
}
impl StagedReceipt {
    pub(super) fn commit(self) -> io::Result<()> {
        paths::prepare(&self.root, false)?;
        checked_path(
            &self.root,
            self.path
                .strip_prefix(&self.root)
                .expect("private receipt child"),
        )?;
        match fs::symlink_metadata(&self.path) {
            Ok(metadata) if !metadata.is_file() => {
                return Err(io::Error::other(
                    "private receipt destination is not a regular file",
                ))
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        // This is the last fallible operation: an unsuccessful commit leaves
        // the previous receipt intact, so the caller can roll back outputs.
        fs::rename(&self.temporary, &self.path)
    }
}
impl Drop for StagedReceipt {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.temporary);
    }
}

fn identity(path: &Path) -> io::Result<&str> {
    path.to_str()
        .filter(|text| text.len() <= 16_384)
        .ok_or_else(|| io::Error::other("private state requires bounded UTF-8 project identities"))
}
