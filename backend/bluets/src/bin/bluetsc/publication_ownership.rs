// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Publication receipts authorize retirement only while prior bytes still match.

use super::*;
#[path = "publication_ownership/preserved.rs"]
mod preserved;
#[path = "publication_ownership/private_state.rs"]
mod private_state;
#[path = "publication_ownership/transaction.rs"]
mod transaction;
pub(super) use private_state::PrivateState;

/// Ordinary build-graph cleanup may release changed regular files from its
/// ownership. Explicit incremental and owner/project publications stay strict.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum RetirementPolicy {
    Strict,
    PreserveModified,
}

const MAX_RECEIPT_BYTES: u64 = 8 * 1024 * 1024;
const MAX_FILES: usize = 65_536;
const MAX_FILE_BYTES: u64 = 128 * 1024 * 1024;
const MAX_TOTAL_BYTES: u64 = 1024 * 1024 * 1024;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct OwnedFile {
    bytes: u64,
    sha256: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct OwnedOutputs {
    version: u8,
    files: BTreeMap<String, OwnedFile>,
}

impl Default for OwnedOutputs {
    fn default() -> Self {
        Self {
            version: 1,
            files: BTreeMap::new(),
        }
    }
}

impl OwnedOutputs {
    pub(super) fn from_writes(
        root: &Path,
        writes: &BTreeMap<PathBuf, Vec<u8>>,
    ) -> io::Result<Self> {
        let mut owned = Self::default();
        for (path, bytes) in writes {
            owned.insert(root, path, bytes)?;
        }
        owned.validate(root)?;
        Ok(owned)
    }

    pub(super) fn from_directory(root: &Path) -> io::Result<Self> {
        fn visit(root: &Path, directory: &Path, owned: &mut OwnedOutputs) -> io::Result<()> {
            for entry in fs::read_dir(directory)? {
                let entry = entry?;
                let path = entry.path();
                if entry.file_type()?.is_dir() {
                    visit(root, &path, owned)?;
                } else if entry.file_type()?.is_file() {
                    let metadata = entry.metadata()?;
                    if metadata.len() > MAX_FILE_BYTES || owned.files.len() >= MAX_FILES {
                        return Err(io::Error::other("publication ownership exceeds its budget"));
                    }
                    owned.insert(root, &path, &fs::read(&path)?)?;
                } else {
                    return Err(io::Error::other("staged output is not a regular file"));
                }
            }
            Ok(())
        }
        let mut owned = Self::default();
        visit(root, root, &mut owned)?;
        owned.validate(root)?;
        Ok(owned)
    }

    pub(super) fn read(root: &Path, receipt: &Path) -> io::Result<Self> {
        match read_receipt(root, receipt)? {
            Some(bytes) => Self::decode(
                root,
                serde_json::from_slice(&bytes).map_err(io::Error::other)?,
            ),
            None => Ok(Self::default()),
        }
    }

    pub(super) fn legacy(root: &Path) -> io::Result<Self> {
        let Some(bytes) = read_receipt(root, &root.join("bluetsc.manifest.json"))? else {
            return Ok(Self::default());
        };
        let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
        match value.get("outputOwnership") {
            Some(value) => Self::decode(root, value.clone()),
            // Older generations cannot prove ownership of unspecified files.
            None => Ok(Self::default()),
        }
    }

    fn decode(root: &Path, value: serde_json::Value) -> io::Result<Self> {
        let owned: Self = serde_json::from_value(value).map_err(io::Error::other)?;
        owned.validate(root)?;
        Ok(owned)
    }

    fn validate(&self, root: &Path) -> io::Result<()> {
        if self.version != 1 || self.files.len() > MAX_FILES {
            return Err(io::Error::other(
                "unsupported or oversized output ownership receipt",
            ));
        }
        let mut total = 0u64;
        for (name, file) in &self.files {
            checked_path(root, Path::new(name))?;
            total = total
                .checked_add(file.bytes)
                .ok_or_else(|| io::Error::other("output size overflow"))?;
            if file.bytes > MAX_FILE_BYTES
                || total > MAX_TOTAL_BYTES
                || file.sha256.len() != 64
                || !file.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
            {
                return Err(io::Error::other(
                    "invalid or oversized output ownership entry",
                ));
            }
        }
        Ok(())
    }

    pub(super) fn insert_existing(&mut self, root: &Path, path: &Path) -> io::Result<()> {
        let Some(bytes) = read_receipt(root, path)? else {
            return Err(io::Error::other("prior publication metadata disappeared"));
        };
        self.insert(root, path, &bytes)
    }

    fn insert(&mut self, root: &Path, path: &Path, bytes: &[u8]) -> io::Result<()> {
        let name = relative(root, path)?;
        self.files.insert(
            name,
            OwnedFile {
                bytes: bytes.len() as u64,
                sha256: blueice_bluets::remote_declarations::sha256_hex(bytes),
            },
        );
        Ok(())
    }

    pub(super) fn extend_rebased(
        &mut self,
        other: &Self,
        from: &Path,
        to: &Path,
    ) -> io::Result<()> {
        for (name, file) in &other.files {
            self.files
                .insert(relative(to, &from.join(name))?, file.clone());
        }
        self.validate(to)
    }

    pub(super) fn retired(&self, root: &Path, current: &Self) -> io::Result<Vec<PathBuf>> {
        self.retired_with_policy(root, current, RetirementPolicy::Strict)
    }

    pub(super) fn retired_with_policy(
        &self,
        root: &Path,
        current: &Self,
        policy: RetirementPolicy,
    ) -> io::Result<Vec<PathBuf>> {
        let mut retired = Vec::new();
        for (name, previous) in &self.files {
            if current.files.contains_key(name) {
                continue;
            }
            let path = checked_path(root, Path::new(name))?;
            let metadata = match fs::symlink_metadata(&path) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error),
            };
            if !metadata.is_file()
                || metadata.len() != previous.bytes
                || blueice_bluets::remote_declarations::sha256_hex(&fs::read(&path)?)
                    != previous.sha256
            {
                if metadata.is_file() && policy == RetirementPolicy::PreserveModified {
                    // Keep user bytes without carrying their ownership into the
                    // next receipt. Symlinks, directories and unsafe parents
                    // still refuse before any public output is changed.
                    continue;
                }
                return Err(io::Error::other(format!(
                    "retired output no longer matches prior ownership: {}",
                    path.display()
                )));
            }
            retired.push(path);
        }
        Ok(retired)
    }

    pub(super) fn validate_destinations(&self, root: &Path) -> io::Result<()> {
        for name in self.files.keys() {
            let path = checked_path(root, Path::new(name))?;
            match fs::symlink_metadata(path) {
                Ok(metadata) if !metadata.is_file() => {
                    return Err(io::Error::other("output destination is not a regular file"));
                }
                Ok(_) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }

    pub(super) fn preserve(&self, root: &Path, stage: &Path, current: &Self) -> io::Result<()> {
        preserved::copy(root, stage, &self.files, &current.files)
    }
}

fn relative(root: &Path, path: &Path) -> io::Result<String> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| io::Error::other("owned output leaves its owner root"))?;
    checked_path(root, relative)?;
    Ok(output_path(relative))
}

pub(super) fn checked_path(root: &Path, relative: &Path) -> io::Result<PathBuf> {
    match fs::symlink_metadata(root) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
        Ok(_) => {
            return Err(io::Error::other(
                "output owner root is not a regular directory",
            ))
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let mut path = root.to_path_buf();
    let mut count = 0;
    for component in relative.components() {
        let std::path::Component::Normal(name) = component else {
            return Err(io::Error::other("invalid owned output path"));
        };
        count += 1;
        path.push(name);
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(io::Error::other("owned output path contains a symlink"));
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    if count == 0 {
        return Err(io::Error::other("empty owned output path"));
    }
    Ok(path)
}

fn read_receipt(root: &Path, path: &Path) -> io::Result<Option<Vec<u8>>> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| io::Error::other("receipt leaves its owner root"))?;
    checked_path(root, relative)?;
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() && metadata.len() <= MAX_RECEIPT_BYTES => {
            let bytes = fs::read(path)?;
            if bytes.len() as u64 > MAX_RECEIPT_BYTES {
                return Err(io::Error::other("output receipt exceeds its byte budget"));
            }
            Ok(Some(bytes))
        }
        Ok(_) => Err(io::Error::other(
            "output receipt is not a bounded regular file",
        )),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

pub(super) fn publish(
    root: &Path,
    writes: BTreeMap<PathBuf, Vec<u8>>,
    retired: Vec<PathBuf>,
    receipt: &PrivateState,
    owned: &OwnedOutputs,
) -> io::Result<()> {
    let staged = receipt.stage(owned)?;
    transaction::publish(root, writes, retired, || staged.commit())
}
