// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn extension_storage_aggregate_quota_counts_every_new_key_byte() {
    let storage = ExtensionStorage::default();
    let identity = "sha256:key-byte-quota";
    for index in 0..15 {
        storage
            .set(
                identity,
                format!("k{index}"),
                "x".repeat(blueice_ipc::extension::MAX_STORAGE_VALUE_BYTES),
            )
            .unwrap();
    }
    let current_bytes = {
        let buckets = storage.buckets.lock().unwrap();
        bucket_storage_bytes(buckets.get(identity).unwrap()).unwrap()
    };
    let remaining = MAX_STORAGE_BYTES_PER_EXTENSION - current_bytes;
    let long_key = "k".repeat(blueice_ipc::extension::MAX_STORAGE_KEY_BYTES);
    assert!(remaining <= blueice_ipc::extension::MAX_STORAGE_VALUE_BYTES);
    assert!(storage
        .set(identity, long_key.clone(), "x".repeat(remaining))
        .is_err());
    assert_eq!(storage.get(identity, &long_key).unwrap(), None);

    storage
        .set(
            identity,
            long_key.clone(),
            "x".repeat(remaining - long_key.len()),
        )
        .unwrap();
    let buckets = storage.buckets.lock().unwrap();
    assert_eq!(
        bucket_storage_bytes(buckets.get(identity).unwrap()).unwrap(),
        MAX_STORAGE_BYTES_PER_EXTENSION
    );
    drop(buckets);

    let full_value_len = remaining - long_key.len();
    storage
        .set(identity, long_key.clone(), "y".repeat(full_value_len))
        .unwrap();
    assert!(storage
        .set(identity, long_key.clone(), "z".repeat(full_value_len + 1))
        .is_err());
    assert_eq!(
        storage.get(identity, &long_key).unwrap(),
        Some("y".repeat(full_value_len))
    );
}

#[test]
fn optional_storage_revoke_waits_for_inflight_effect_and_old_grant_cannot_be_reused() {
    use std::time::Duration;

    let mut registry = ExtensionRegistry::with_supported_capabilities();
    registry.declare_optional(MINIMAL_SLICE_EXTENSION_ID, CAPABILITY_STORAGE);
    let registry = Arc::new(registry);
    registry
        .grant_optional(MINIMAL_SLICE_EXTENSION_ID, CAPABILITY_STORAGE)
        .unwrap();
    let old_generation = registry
        .capability_generation(MINIMAL_SLICE_EXTENSION_ID, CAPABILITY_STORAGE)
        .unwrap();
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let worker_registry = Arc::clone(&registry);
    #[allow(clippy::result_large_err)]
    let worker = thread::spawn(move || {
        let identity = ConnectionIdentity {
            extension_id: MINIMAL_SLICE_EXTENSION_ID.to_string(),
            negotiated_capabilities: BTreeMap::from([(CAPABILITY_STORAGE.to_string(), 3)]),
        };
        with_stable_storage_grant(&worker_registry, &identity, Some(old_generation), || {
            entered_tx.send(()).unwrap();
            release_rx.recv_timeout(Duration::from_secs(2)).unwrap();
            Ok(())
        })
    });
    entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    let (attempt_tx, attempt_rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel();
    let revoker_registry = Arc::clone(&registry);
    let revoker = thread::spawn(move || {
        attempt_tx.send(()).unwrap();
        revoker_registry
            .revoke_optional(MINIMAL_SLICE_EXTENSION_ID, CAPABILITY_STORAGE)
            .unwrap();
        done_tx.send(()).unwrap();
    });
    attempt_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(done_rx.recv_timeout(Duration::from_millis(50)).is_err());
    release_tx.send(()).unwrap();
    assert!(worker.join().unwrap().is_ok());
    done_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    revoker.join().unwrap();

    registry
        .grant_optional(MINIMAL_SLICE_EXTENSION_ID, CAPABILITY_STORAGE)
        .unwrap();
    let identity = ConnectionIdentity {
        extension_id: MINIMAL_SLICE_EXTENSION_ID.to_string(),
        negotiated_capabilities: BTreeMap::from([(CAPABILITY_STORAGE.to_string(), 3)]),
    };
    assert!(matches!(
        with_stable_storage_grant(&registry, &identity, Some(old_generation), || -> Result<(), String> {
            panic!("a revoked generation must not run a storage effect")
        }),
        Err(ExtensionReply::CapabilityDenied { capability, .. }) if capability == CAPABILITY_STORAGE
    ));
}

#[test]
fn durable_storage_v2_and_v3_require_grants_and_survive_a_new_service() {
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_ROOT: AtomicU64 = AtomicU64::new(1);
    const ID: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const OTHER_ID: &str =
        "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    let root = std::env::temp_dir().join(format!(
        "blueice-storage-v2-test-{}-{}",
        std::process::id(),
        NEXT_ROOT.fetch_add(1, Ordering::Relaxed)
    ));
    let spawn = |storage: ExtensionStorage| {
        let mut registry = ExtensionRegistry::with_supported_capabilities();
        registry.grant(ID, CAPABILITY_STORAGE);
        let (client, mut server) = UnixStream::pair().unwrap();
        let worker = thread::spawn(move || {
            handle_extension_connection_with_actions_and_authentication_and_network_rules(
                &registry,
                Path::new("/not-reached-for-storage-only-operation.sock"),
                &mut server,
                ExtensionConnectionAuthentication::unauthenticated(),
                ExtensionActionDelegates::new(
                    |_| Ok(String::new()),
                    unused_write_delegate,
                    || Ok(()),
                    |_, _| Ok(()),
                    || Ok(()),
                )
                .with_storage(storage),
            )
        });
        (client, worker)
    };
    let exchange = |client: &mut UnixStream, request: ExtensionRequest| {
        write_extension_request(client, &request).unwrap();
        read_extension_reply(client).unwrap()
    };

    let (mut client, worker) = spawn(ExtensionStorage::default().with_durable_root(root.clone()));
    assert_eq!(
        exchange(
            &mut client,
            hello_with_capabilities(ID, [(CAPABILITY_STORAGE, 1)])
        ),
        empty_hello_ack()
    );
    assert!(matches!(
        exchange(&mut client, ExtensionRequest::DurableStorageSet {
            key: "task".into(), value: "persistent".into(),
        }),
        ExtensionReply::CapabilityDenied { capability, .. } if capability == CAPABILITY_STORAGE
    ));
    assert_eq!(
        exchange(
            &mut client,
            ExtensionRequest::StorageSet {
                key: "task".into(),
                value: "ephemeral".into(),
            }
        ),
        ExtensionReply::StorageSetAck
    );
    assert_eq!(
        exchange(
            &mut client,
            hello_with_capabilities(ID, [(CAPABILITY_STORAGE, 2)])
        ),
        empty_hello_ack()
    );
    assert!(
        matches!(exchange(&mut client, ExtensionRequest::DurableStorageListKeys),
        ExtensionReply::CapabilityDenied { capability, .. } if capability == CAPABILITY_STORAGE)
    );
    assert_eq!(
        exchange(
            &mut client,
            ExtensionRequest::DurableStorageSet {
                key: "task".into(),
                value: "persistent".into(),
            }
        ),
        ExtensionReply::StorageSetAck
    );
    assert_eq!(
        exchange(
            &mut client,
            ExtensionRequest::DurableStorageSet {
                key: "alpha".into(),
                value: "other".into(),
            }
        ),
        ExtensionReply::StorageSetAck
    );
    assert_eq!(
        exchange(
            &mut client,
            ExtensionRequest::DurableStorageGet { key: "task".into() }
        ),
        ExtensionReply::StorageGetResult {
            value: Some("persistent".into())
        }
    );
    assert_eq!(
        exchange(
            &mut client,
            ExtensionRequest::StorageGet { key: "task".into() }
        ),
        ExtensionReply::StorageGetResult {
            value: Some("ephemeral".into())
        }
    );
    assert_eq!(
        exchange(
            &mut client,
            hello_with_capabilities(ID, [(CAPABILITY_STORAGE, 3)])
        ),
        empty_hello_ack()
    );
    assert_eq!(
        exchange(&mut client, ExtensionRequest::DurableStorageListKeys),
        ExtensionReply::StorageKeysResult {
            keys: vec!["alpha".into(), "task".into()]
        }
    );
    assert_eq!(
        exchange(
            &mut client,
            hello_with_capabilities(OTHER_ID, [(CAPABILITY_STORAGE, 3)])
        ),
        empty_hello_ack()
    );
    assert!(matches!(
        exchange(&mut client, ExtensionRequest::DurableStorageListKeys),
        ExtensionReply::CapabilityDenied { capability, .. } if capability == CAPABILITY_STORAGE
    ));
    drop(client);
    worker.join().unwrap().unwrap();

    let (mut restarted, worker) =
        spawn(ExtensionStorage::default().with_durable_root(root.clone()));
    assert_eq!(
        exchange(
            &mut restarted,
            hello_with_capabilities(ID, [(CAPABILITY_STORAGE, 3)])
        ),
        empty_hello_ack()
    );
    assert_eq!(
        exchange(&mut restarted, ExtensionRequest::DurableStorageListKeys),
        ExtensionReply::StorageKeysResult {
            keys: vec!["alpha".into(), "task".into()]
        }
    );
    assert_eq!(
        exchange(
            &mut restarted,
            ExtensionRequest::DurableStorageGet { key: "task".into() }
        ),
        ExtensionReply::StorageGetResult {
            value: Some("persistent".into())
        }
    );
    assert_eq!(
        exchange(
            &mut restarted,
            ExtensionRequest::StorageGet { key: "task".into() }
        ),
        ExtensionReply::StorageGetResult { value: None }
    );
    assert_eq!(
        exchange(
            &mut restarted,
            ExtensionRequest::DurableStorageRemove { key: "task".into() }
        ),
        ExtensionReply::StorageRemoveAck { removed: true }
    );
    assert_eq!(
        exchange(&mut restarted, ExtensionRequest::DurableStorageListKeys),
        ExtensionReply::StorageKeysResult {
            keys: vec!["alpha".into()]
        }
    );
    drop(restarted);
    worker.join().unwrap().unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn ungranted_storage_never_reaches_the_core_owned_bucket() {
    let registry = ExtensionRegistry::minimal_slice();
    let (mut client, mut server) = UnixStream::pair().unwrap();
    let handle = thread::spawn(move || handle_extension_connection(&registry, &mut server));

    write_extension_request(
        &mut client,
        &hello_with_capabilities(MINIMAL_SLICE_EXTENSION_ID, [(CAPABILITY_STORAGE, 1)]),
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut client).unwrap(),
        empty_hello_ack()
    );
    write_extension_request(
        &mut client,
        &ExtensionRequest::StorageSet {
            key: "task-state".to_string(),
            value: "attacker-controlled".to_string(),
        },
    )
    .unwrap();
    match read_extension_reply(&mut client).unwrap() {
        ExtensionReply::CapabilityDenied { capability, .. } => {
            assert_eq!(capability, CAPABILITY_STORAGE)
        }
        other => panic!("expected storage capability denial, got {other:?}"),
    }

    drop(client);
    handle.join().unwrap().unwrap();
}
