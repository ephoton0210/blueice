// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Fail closed until native private storage and locking are implemented.
use std::path::PathBuf;

const UNAVAILABLE: &str = "durable extension storage is unavailable on this platform";

pub fn default_durable_storage_root() -> Result<PathBuf, String> {
    Err(UNAVAILABLE.into())
}

#[derive(Clone)]
pub(crate) struct DurableExtensionStorage;

impl DurableExtensionStorage {
    pub(crate) fn new(_root: PathBuf) -> Self {
        Self
    }
    pub(crate) fn get(&self, _id: &str, _key: &str) -> Result<Option<String>, String> {
        Err(UNAVAILABLE.into())
    }
    pub(crate) fn list_keys(&self, _id: &str) -> Result<Vec<String>, String> {
        Err(UNAVAILABLE.into())
    }
    pub(crate) fn set(&self, _id: &str, _key: String, _value: String) -> Result<(), String> {
        Err(UNAVAILABLE.into())
    }
    pub(crate) fn remove(&self, _id: &str, _key: &str) -> Result<bool, String> {
        Err(UNAVAILABLE.into())
    }
}
