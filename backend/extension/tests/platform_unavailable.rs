// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![cfg(not(unix))]

use blueice_extension_host::{default_durable_storage_root, ExtensionStorage};

#[test]
fn unavailable_durable_storage_never_acknowledges_a_write_or_uses_the_memory_bucket() {
    let storage = ExtensionStorage::default().with_durable_root(std::env::temp_dir());
    let id = format!("sha256:{}", "a".repeat(64));
    storage
        .set(&id, "key".into(), "memory value".into())
        .unwrap();
    assert!(default_durable_storage_root().is_err());
    assert!(storage.durable_get(&id, "key").is_err());
    assert!(storage.durable_list_keys(&id).is_err());
    assert!(storage
        .durable_set(&id, "key".into(), "disk value".into())
        .is_err());
    assert!(storage.durable_remove(&id, "key").is_err());
    assert_eq!(
        storage.get(&id, "key").unwrap(),
        Some("memory value".into())
    );
}
