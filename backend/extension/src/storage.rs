// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

/// Core-owned storage isolated by manifest-derived extension identity. V1 is
/// a shared in-memory map with mutex-protected quota checks; v2 is a separate
/// disk bucket with a per-identity OS lock. Neither accepts a guest path.
#[derive(Clone, Default)]
pub struct ExtensionStorage {
    pub(super) buckets: Arc<Mutex<HashMap<String, BTreeMap<String, String>>>>,
    pub(super) durable: Option<durable_storage::DurableExtensionStorage>,
}

impl ExtensionStorage {
    /// Adds a separate version-two durable namespace. Version-one operations
    /// keep using only the process-lifetime map above.
    pub fn with_durable_root(mut self, root: std::path::PathBuf) -> Self {
        self.durable = Some(durable_storage::DurableExtensionStorage::new(root));
        self
    }

    pub fn durable_get(&self, extension_id: &str, key: &str) -> Result<Option<String>, String> {
        self.durable
            .as_ref()
            .ok_or_else(|| "durable extension storage is not configured".to_string())?
            .get(extension_id, key)
    }

    pub fn durable_list_keys(&self, extension_id: &str) -> Result<Vec<String>, String> {
        self.durable
            .as_ref()
            .ok_or_else(|| "durable extension storage is not configured".to_string())?
            .list_keys(extension_id)
    }

    pub fn durable_set(
        &self,
        extension_id: &str,
        key: String,
        value: String,
    ) -> Result<(), String> {
        self.durable
            .as_ref()
            .ok_or_else(|| "durable extension storage is not configured".to_string())?
            .set(extension_id, key, value)
    }

    pub fn durable_remove(&self, extension_id: &str, key: &str) -> Result<bool, String> {
        self.durable
            .as_ref()
            .ok_or_else(|| "durable extension storage is not configured".to_string())?
            .remove(extension_id, key)
    }

    /// Reads a value from exactly `extension_id`'s bucket after validating the
    /// bounded identifier syntax shared by all storage operations.
    pub fn get(&self, extension_id: &str, key: &str) -> Result<Option<String>, String> {
        validate_storage_key(key)?;
        let buckets = self
            .buckets
            .lock()
            .map_err(|_| "extension storage state was poisoned".to_string())?;
        Ok(buckets
            .get(extension_id)
            .and_then(|bucket| bucket.get(key))
            .cloned())
    }

    /// Stores one bounded UTF-8 value in the caller's bucket. Replacement is
    /// atomic under the same lock as the aggregate byte/entry quotas.
    pub fn set(&self, extension_id: &str, key: String, value: String) -> Result<(), String> {
        // Preserve v1's no-allocation behavior for malformed inputs: an
        // invalid write must not create an empty identity bucket.
        validate_storage_key(&key)?;
        if value.len() > blueice_ipc::extension::MAX_STORAGE_VALUE_BYTES {
            return Err(format!(
                "storage values cannot exceed {} bytes",
                blueice_ipc::extension::MAX_STORAGE_VALUE_BYTES
            ));
        }
        let mut buckets = self
            .buckets
            .lock()
            .map_err(|_| "extension storage state was poisoned".to_string())?;
        let bucket = buckets.entry(extension_id.to_string()).or_default();
        set_bounded_storage_value(bucket, key, value)
    }

    /// Removes only the identified extension's own key, returning whether it
    /// existed. Empty buckets are discarded so a sequence of absent reads or
    /// removals cannot grow the outer map.
    pub fn remove(&self, extension_id: &str, key: &str) -> Result<bool, String> {
        validate_storage_key(key)?;
        let mut buckets = self
            .buckets
            .lock()
            .map_err(|_| "extension storage state was poisoned".to_string())?;
        let Some(bucket) = buckets.get_mut(extension_id) else {
            return Ok(false);
        };
        let removed = bucket.remove(key).is_some();
        if bucket.is_empty() {
            buckets.remove(extension_id);
        }
        Ok(removed)
    }
}

pub(crate) fn validate_storage_key(key: &str) -> Result<(), String> {
    if key.is_empty() {
        return Err("storage keys must not be empty".to_string());
    }
    if key.len() > blueice_ipc::extension::MAX_STORAGE_KEY_BYTES {
        return Err(format!(
            "storage keys cannot exceed {} bytes",
            blueice_ipc::extension::MAX_STORAGE_KEY_BYTES
        ));
    }
    if !key
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err(
            "storage keys may contain only ASCII letters, digits, '.', '_' or '-'".to_string(),
        );
    }
    Ok(())
}

pub(crate) fn bucket_storage_bytes(bucket: &BTreeMap<String, String>) -> Result<usize, String> {
    bucket.iter().try_fold(0_usize, |total, (key, value)| {
        total
            .checked_add(key.len())
            .and_then(|bytes| bytes.checked_add(value.len()))
            .ok_or_else(|| "extension storage size overflowed".to_string())
    })
}

pub(crate) fn set_bounded_storage_value(
    bucket: &mut BTreeMap<String, String>,
    key: String,
    value: String,
) -> Result<(), String> {
    validate_storage_key(&key)?;
    if value.len() > blueice_ipc::extension::MAX_STORAGE_VALUE_BYTES {
        return Err(format!(
            "storage values cannot exceed {} bytes",
            blueice_ipc::extension::MAX_STORAGE_VALUE_BYTES
        ));
    }
    let new_key = !bucket.contains_key(&key);
    if new_key && bucket.len() >= MAX_STORAGE_ENTRIES_PER_EXTENSION {
        return Err(format!(
            "an extension may store at most {MAX_STORAGE_ENTRIES_PER_EXTENSION} keys"
        ));
    }
    let new_key_bytes = if new_key { key.len() } else { 0 };
    let existing_value_bytes = bucket.get(&key).map_or(0, String::len);
    let current_bytes = bucket_storage_bytes(bucket)?;
    let prospective_bytes = current_bytes
        .checked_sub(existing_value_bytes)
        .and_then(|bytes| bytes.checked_add(new_key_bytes))
        .and_then(|bytes| bytes.checked_add(value.len()))
        .ok_or_else(|| "extension storage size overflowed".to_string())?;
    if prospective_bytes > MAX_STORAGE_BYTES_PER_EXTENSION {
        return Err(format!(
            "an extension storage bucket cannot exceed {MAX_STORAGE_BYTES_PER_EXTENSION} bytes"
        ));
    }
    bucket.insert(key, value);
    Ok(())
}
