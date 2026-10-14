// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn unauthenticated_development_peer_cannot_spend_an_armed_ephemeral_lease() {
    let mut registry = ExtensionRegistry::with_supported_capabilities();
    registry.declare_runtime_ephemeral("sha256:installed", CAPABILITY_DOM_READ);
    let ticket = registry
        .arm_runtime_ephemeral("sha256:installed", CAPABILITY_DOM_READ, 1, 0)
        .unwrap();
    let registry = Arc::new(registry);
    let (mut client, mut server) = UnixStream::pair().unwrap();
    let worker_registry = Arc::clone(&registry);
    let worker = thread::spawn(move || handle_extension_connection(&worker_registry, &mut server));
    write_extension_request(
        &mut client,
        &hello_with_capabilities("sha256:installed", [(CAPABILITY_DOM_READ, 3)]),
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut client).unwrap(),
        empty_hello_ack()
    );
    write_extension_request(&mut client, &ExtensionRequest::DomReadTab { tab_id: 1 }).unwrap();
    assert!(matches!(read_extension_reply(&mut client).unwrap(),
        ExtensionReply::CapabilityDenied { capability, .. } if capability == CAPABILITY_DOM_READ));
    write_extension_request(
        &mut client,
        &ExtensionRequest::DomReadTabEphemeral {
            tab_id: 1,
            ticket: ticket.clone(),
        },
    )
    .unwrap();
    assert!(matches!(read_extension_reply(&mut client).unwrap(),
        ExtensionReply::CapabilityDenied { capability, .. } if capability == CAPABILITY_DOM_READ));
    assert!(registry.has_unspent_runtime_ephemeral_lease("sha256:installed", CAPABILITY_DOM_READ));
    drop(client);
    worker.join().unwrap().unwrap();
}

#[test]
fn optional_grant_and_revocation_take_effect_on_an_existing_negotiated_connection() {
    let mut registry = ExtensionRegistry::with_supported_capabilities();
    registry.declare_optional(MINIMAL_SLICE_EXTENSION_ID, CAPABILITY_DOM_READ);
    assert!(registry
        .grant_optional("other-extension", CAPABILITY_DOM_READ)
        .is_err());
    assert!(registry
        .grant_optional(MINIMAL_SLICE_EXTENSION_ID, CAPABILITY_DOM_WRITE)
        .is_err());
    assert_eq!(
        registry.capability_generation(MINIMAL_SLICE_EXTENSION_ID, CAPABILITY_DOM_READ),
        None
    );
    let registry = Arc::new(registry);
    let (mut client, mut server) = UnixStream::pair().unwrap();
    let handler_registry = Arc::clone(&registry);
    let handle = thread::spawn(move || handle_extension_connection(&handler_registry, &mut server));
    write_extension_request(
        &mut client,
        &hello_with_capabilities(MINIMAL_SLICE_EXTENSION_ID, [(CAPABILITY_DOM_READ, 1)]),
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut client).unwrap(),
        empty_hello_ack()
    );

    write_extension_request(&mut client, &ExtensionRequest::DomRead).unwrap();
    assert!(matches!(read_extension_reply(&mut client).unwrap(),
        ExtensionReply::CapabilityDenied { capability, .. } if capability == CAPABILITY_DOM_READ));
    assert!(registry
        .grant_optional(MINIMAL_SLICE_EXTENSION_ID, CAPABILITY_DOM_READ)
        .unwrap());
    assert_eq!(
        registry.capability_generation(MINIMAL_SLICE_EXTENSION_ID, CAPABILITY_DOM_READ),
        Some(0)
    );
    assert!(!registry
        .grant_optional(MINIMAL_SLICE_EXTENSION_ID, CAPABILITY_DOM_READ)
        .unwrap());
    write_extension_request(&mut client, &ExtensionRequest::DomRead).unwrap();
    assert_eq!(
        read_extension_reply(&mut client).unwrap(),
        ExtensionReply::DomReadResult {
            value: PLACEHOLDER_DOM_READ_VALUE.to_string(),
        }
    );

    assert!(registry
        .revoke_optional(MINIMAL_SLICE_EXTENSION_ID, CAPABILITY_DOM_READ)
        .unwrap());
    assert_eq!(
        registry.capability_generation(MINIMAL_SLICE_EXTENSION_ID, CAPABILITY_DOM_READ),
        None
    );
    assert!(!registry
        .revoke_optional(MINIMAL_SLICE_EXTENSION_ID, CAPABILITY_DOM_READ)
        .unwrap());
    assert!(registry
        .grant_optional(MINIMAL_SLICE_EXTENSION_ID, CAPABILITY_DOM_READ)
        .unwrap());
    assert_eq!(
        registry.capability_generation(MINIMAL_SLICE_EXTENSION_ID, CAPABILITY_DOM_READ),
        Some(1)
    );
    assert!(registry
        .revoke_optional(MINIMAL_SLICE_EXTENSION_ID, CAPABILITY_DOM_READ)
        .unwrap());
    write_extension_request(&mut client, &ExtensionRequest::DomRead).unwrap();
    assert!(matches!(read_extension_reply(&mut client).unwrap(),
        ExtensionReply::CapabilityDenied { capability, .. } if capability == CAPABILITY_DOM_READ));
    drop(client);
    handle.join().unwrap().unwrap();
}

#[test]
fn extension_storage_is_bounded_and_isolated_by_extension_identity() {
    let storage = ExtensionStorage::default();
    storage
        .set(
            "sha256:first",
            "task-state".to_string(),
            "complete".to_string(),
        )
        .unwrap();
    assert_eq!(
        storage.get("sha256:first", "task-state").unwrap(),
        Some("complete".to_string())
    );
    assert_eq!(storage.get("sha256:second", "task-state").unwrap(), None);
    assert!(storage
        .set(
            "sha256:first",
            "not a valid key".to_string(),
            "x".to_string()
        )
        .is_err());
    assert!(storage
        .set(
            "sha256:first",
            "oversized".to_string(),
            "x".repeat(blueice_ipc::extension::MAX_STORAGE_VALUE_BYTES + 1),
        )
        .is_err());
    assert!(storage.remove("sha256:first", "task-state").unwrap());
    assert!(!storage.remove("sha256:first", "task-state").unwrap());
    assert_eq!(storage.get("sha256:first", "task-state").unwrap(), None);

    for index in 0..MAX_STORAGE_ENTRIES_PER_EXTENSION {
        storage
            .set("sha256:count", format!("key-{index}"), "x".to_string())
            .unwrap();
    }
    assert!(storage
        .set("sha256:count", "one-too-many".to_string(), "x".to_string())
        .is_err());

    let quota_storage = ExtensionStorage::default();
    for index in 0..15 {
        quota_storage
            .set(
                "sha256:quota",
                format!("value-{index}"),
                "x".repeat(blueice_ipc::extension::MAX_STORAGE_VALUE_BYTES),
            )
            .unwrap();
    }
    assert!(quota_storage
        .set(
            "sha256:quota",
            "exceeds-total".to_string(),
            "x".repeat(blueice_ipc::extension::MAX_STORAGE_VALUE_BYTES),
        )
        .is_err());
}

#[test]
fn granted_storage_v1_sets_reads_and_removes_only_its_handshake_bucket() {
    let registry = registry_with_storage_granted();
    let (mut client, mut server) = UnixStream::pair().unwrap();
    let handle = thread::spawn(move || {
        handle_extension_connection_with_gatekeeper(
            &registry,
            Path::new("/not-reached-for-storage-only-operation.sock"),
            &mut server,
        )
    });

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
            value: "complete".to_string(),
        },
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut client).unwrap(),
        ExtensionReply::StorageSetAck
    );
    write_extension_request(
        &mut client,
        &ExtensionRequest::StorageGet {
            key: "task-state".to_string(),
        },
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut client).unwrap(),
        ExtensionReply::StorageGetResult {
            value: Some("complete".to_string()),
        }
    );
    write_extension_request(
        &mut client,
        &ExtensionRequest::StorageRemove {
            key: "task-state".to_string(),
        },
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut client).unwrap(),
        ExtensionReply::StorageRemoveAck { removed: true }
    );
    write_extension_request(
        &mut client,
        &ExtensionRequest::StorageGet {
            key: "task-state".to_string(),
        },
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut client).unwrap(),
        ExtensionReply::StorageGetResult { value: None }
    );

    drop(client);
    handle.join().unwrap().unwrap();
}

#[test]
fn optional_storage_revoke_denies_every_operation_on_an_existing_connection() {
    let mut registry = ExtensionRegistry::with_supported_capabilities();
    registry.declare_optional(MINIMAL_SLICE_EXTENSION_ID, CAPABILITY_STORAGE);
    let registry = Arc::new(registry);
    registry
        .grant_optional(MINIMAL_SLICE_EXTENSION_ID, CAPABILITY_STORAGE)
        .unwrap();
    let handler_registry = Arc::clone(&registry);
    let (mut client, mut server) = UnixStream::pair().unwrap();
    let worker = thread::spawn(move || {
        handle_extension_connection_with_gatekeeper(
            &handler_registry,
            Path::new("/not-reached-for-storage-only-operation.sock"),
            &mut server,
        )
    });
    let exchange = |client: &mut UnixStream, request: ExtensionRequest| {
        write_extension_request(client, &request).unwrap();
        read_extension_reply(client).unwrap()
    };
    assert_eq!(
        exchange(
            &mut client,
            hello_with_capabilities(MINIMAL_SLICE_EXTENSION_ID, [(CAPABILITY_STORAGE, 3)])
        ),
        empty_hello_ack()
    );
    assert_eq!(
        exchange(
            &mut client,
            ExtensionRequest::StorageSet {
                key: "task".into(),
                value: "safe".into()
            }
        ),
        ExtensionReply::StorageSetAck
    );
    registry
        .revoke_optional(MINIMAL_SLICE_EXTENSION_ID, CAPABILITY_STORAGE)
        .unwrap();
    for request in [
        ExtensionRequest::StorageGet { key: "task".into() },
        ExtensionRequest::StorageSet {
            key: "task".into(),
            value: "changed".into(),
        },
        ExtensionRequest::StorageRemove { key: "task".into() },
        ExtensionRequest::DurableStorageGet { key: "task".into() },
        ExtensionRequest::DurableStorageSet {
            key: "task".into(),
            value: "changed".into(),
        },
        ExtensionRequest::DurableStorageRemove { key: "task".into() },
        ExtensionRequest::DurableStorageListKeys,
    ] {
        assert!(matches!(
            exchange(&mut client, request),
            ExtensionReply::CapabilityDenied { capability, .. } if capability == CAPABILITY_STORAGE
        ));
    }
    registry
        .grant_optional(MINIMAL_SLICE_EXTENSION_ID, CAPABILITY_STORAGE)
        .unwrap();
    assert_eq!(
        exchange(
            &mut client,
            ExtensionRequest::StorageGet { key: "task".into() }
        ),
        ExtensionReply::StorageGetResult {
            value: Some("safe".into())
        }
    );
    drop(client);
    worker.join().unwrap().unwrap();
}

#[test]
fn a_hello_reports_only_unsupported_capabilities_and_keeps_compatible_ones_usable() {
    let registry = ExtensionRegistry::minimal_slice();
    let (mut client, mut server) = UnixStream::pair().unwrap();
    let handle = thread::spawn(move || handle_extension_connection(&registry, &mut server));

    write_extension_request(
        &mut client,
        &hello_with_capabilities(
            MINIMAL_SLICE_EXTENSION_ID,
            [(CAPABILITY_DOM_READ, 1), ("future:capability", 1)],
        ),
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut client).unwrap(),
        ExtensionReply::HelloAck {
            unsupported_capabilities: BTreeMap::from([(
                "future:capability".to_string(),
                UnsupportedCapabilityVersion::UnknownCapability,
            )]),
        }
    );

    // An independently unsupported future capability must not end a
    // connection that successfully negotiated dom:read.
    write_extension_request(&mut client, &ExtensionRequest::DomRead).unwrap();
    assert_eq!(
        read_extension_reply(&mut client).unwrap(),
        ExtensionReply::DomReadResult {
            value: PLACEHOLDER_DOM_READ_VALUE.to_string()
        }
    );

    drop(client);
    handle.join().unwrap().unwrap();
}

#[test]
fn an_incompatible_version_is_reported_and_cannot_be_used_after_handshake() {
    let registry = ExtensionRegistry::minimal_slice();
    let (mut client, mut server) = UnixStream::pair().unwrap();
    let handle = thread::spawn(move || handle_extension_connection(&registry, &mut server));

    write_extension_request(
        &mut client,
        &hello_with_capabilities(MINIMAL_SLICE_EXTENSION_ID, [(CAPABILITY_DOM_READ, 4)]),
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut client).unwrap(),
        ExtensionReply::HelloAck {
            unsupported_capabilities: BTreeMap::from([(
                CAPABILITY_DOM_READ.to_string(),
                UnsupportedCapabilityVersion::OutsideSupportedRange {
                    min_inclusive: 1,
                    max_inclusive: 3
                },
            )]),
        }
    );

    write_extension_request(&mut client, &ExtensionRequest::DomRead).unwrap();
    match read_extension_reply(&mut client).unwrap() {
        ExtensionReply::CapabilityDenied { capability, reason } => {
            assert_eq!(capability, CAPABILITY_DOM_READ);
            assert!(reason.contains("did not negotiate a supported version"));
        }
        other => panic!("expected CapabilityDenied, got {other:?}"),
    }

    drop(client);
    handle.join().unwrap().unwrap();
}

#[test]
fn v2_tab_reads_are_denied_after_a_v1_handshake_but_v1_reads_keep_working() {
    let registry = ExtensionRegistry::minimal_slice();
    let (mut client, mut server) = UnixStream::pair().unwrap();
    let handle = thread::spawn(move || handle_extension_connection(&registry, &mut server));

    write_extension_request(
        &mut client,
        &hello_with_capabilities(MINIMAL_SLICE_EXTENSION_ID, [(CAPABILITY_DOM_READ, 1)]),
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut client).unwrap(),
        empty_hello_ack()
    );
    write_extension_request(&mut client, &ExtensionRequest::DomReadTab { tab_id: 2 }).unwrap();
    match read_extension_reply(&mut client).unwrap() {
        ExtensionReply::CapabilityDenied { capability, reason } => {
            assert_eq!(capability, CAPABILITY_DOM_READ);
            assert!(reason.contains("requires version 2"));
        }
        other => panic!("expected a v2 version denial, got {other:?}"),
    }
    write_extension_request(&mut client, &ExtensionRequest::DomRead).unwrap();
    assert_eq!(
        read_extension_reply(&mut client).unwrap(),
        ExtensionReply::DomReadResult {
            value: PLACEHOLDER_DOM_READ_VALUE.to_string()
        }
    );

    drop(client);
    handle.join().unwrap().unwrap();
}

#[test]
fn optional_read_revocation_denies_dom_and_network_reads_on_existing_connection() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let mut registry = ExtensionRegistry::with_supported_capabilities();
    registry.declare_optional(MINIMAL_SLICE_EXTENSION_ID, CAPABILITY_DOM_READ);
    registry.declare_optional(MINIMAL_SLICE_EXTENSION_ID, CAPABILITY_NETWORK_OBSERVE);
    let registry = Arc::new(registry);
    registry
        .grant_optional(MINIMAL_SLICE_EXTENSION_ID, CAPABILITY_DOM_READ)
        .unwrap();
    registry
        .grant_optional(MINIMAL_SLICE_EXTENSION_ID, CAPABILITY_NETWORK_OBSERVE)
        .unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let dom_calls = Arc::clone(&calls);
    let response_calls = Arc::clone(&calls);
    let trace_calls = Arc::clone(&calls);
    let handler_registry = Arc::clone(&registry);
    let (mut client, mut server) = UnixStream::pair().unwrap();
    let worker = thread::spawn(move || {
        handle_extension_connection_with_actions_and_authentication_and_network_rules(
            &handler_registry,
            Path::new("/not-reached-for-read-only-operation.sock"),
            &mut server,
            ExtensionConnectionAuthentication::unauthenticated(),
            ExtensionActionDelegates::new(
                move |_| {
                    dom_calls.fetch_add(1, Ordering::SeqCst);
                    Ok("safe".into())
                },
                unused_write_delegate,
                || Ok(()),
                |_, _| Ok(()),
                || Ok(()),
            )
            .with_network_observer(move |_| {
                response_calls.fetch_add(1, Ordering::SeqCst);
                Ok(None)
            })
            .with_network_trace_observer(move |_| {
                trace_calls.fetch_add(1, Ordering::SeqCst);
                Ok(None)
            }),
        )
    });
    let exchange = |client: &mut UnixStream, request: ExtensionRequest| {
        write_extension_request(client, &request).unwrap();
        read_extension_reply(client).unwrap()
    };
    assert_eq!(
        exchange(
            &mut client,
            hello_with_capabilities(
                MINIMAL_SLICE_EXTENSION_ID,
                [(CAPABILITY_DOM_READ, 2), (CAPABILITY_NETWORK_OBSERVE, 2)]
            )
        ),
        empty_hello_ack()
    );
    registry
        .revoke_optional(MINIMAL_SLICE_EXTENSION_ID, CAPABILITY_DOM_READ)
        .unwrap();
    registry
        .revoke_optional(MINIMAL_SLICE_EXTENSION_ID, CAPABILITY_NETWORK_OBSERVE)
        .unwrap();
    for (request, capability) in [
        (ExtensionRequest::DomRead, CAPABILITY_DOM_READ),
        (
            ExtensionRequest::DomReadTab { tab_id: 1 },
            CAPABILITY_DOM_READ,
        ),
        (
            ExtensionRequest::ReadNetworkResponse { tab_id: 1 },
            CAPABILITY_NETWORK_OBSERVE,
        ),
        (
            ExtensionRequest::ReadNetworkTrace { tab_id: 1 },
            CAPABILITY_NETWORK_OBSERVE,
        ),
    ] {
        assert!(matches!(
            exchange(&mut client, request),
            ExtensionReply::CapabilityDenied { capability: denied, .. } if denied == capability
        ));
    }
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    registry
        .grant_optional(MINIMAL_SLICE_EXTENSION_ID, CAPABILITY_DOM_READ)
        .unwrap();
    registry
        .grant_optional(MINIMAL_SLICE_EXTENSION_ID, CAPABILITY_NETWORK_OBSERVE)
        .unwrap();
    assert_eq!(
        exchange(&mut client, ExtensionRequest::DomRead),
        ExtensionReply::DomReadResult {
            value: "safe".into()
        }
    );
    assert_eq!(
        exchange(&mut client, ExtensionRequest::DomReadTab { tab_id: 1 }),
        ExtensionReply::DomReadResult {
            value: "safe".into()
        }
    );
    assert_eq!(
        exchange(
            &mut client,
            ExtensionRequest::ReadNetworkResponse { tab_id: 1 }
        ),
        ExtensionReply::NetworkResponseResult { response: None }
    );
    assert_eq!(
        exchange(
            &mut client,
            ExtensionRequest::ReadNetworkTrace { tab_id: 1 }
        ),
        ExtensionReply::NetworkTraceResult { trace: None }
    );
    assert_eq!(calls.load(Ordering::SeqCst), 4);
    drop(client);
    worker.join().unwrap().unwrap();
}

#[test]
fn v7_range_write_is_denied_after_a_v6_handshake_before_review_or_delegate() {
    let registry = registry_with_dom_write_granted();
    let (mut client, mut server) = UnixStream::pair().unwrap();
    let handle = thread::spawn(move || {
        handle_extension_connection_with_actions(
            &registry,
            Path::new("/not-reached-for-v6-range-version-denial.sock"),
            &mut server,
            |_| Ok("unused in this test".to_string()),
            |_, _, _, _| panic!("a v6 connection must not delegate a v7 range request"),
            || Ok(()),
        )
    });

    write_extension_request(
        &mut client,
        &hello_with_capabilities(MINIMAL_SLICE_EXTENSION_ID, [(CAPABILITY_DOM_WRITE, 6)]),
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut client).unwrap(),
        empty_hello_ack()
    );
    write_extension_request(
        &mut client,
        &ExtensionRequest::SetRangeInputValue {
            tab_id: 1,
            node_id: 2,
            value: 50,
        },
    )
    .unwrap();
    match read_extension_reply(&mut client).unwrap() {
        ExtensionReply::CapabilityDenied { capability, reason } => {
            assert_eq!(capability, CAPABILITY_DOM_WRITE);
            assert!(reason.contains("requires version 7"));
        }
        other => panic!("expected a v7 version denial, got {other:?}"),
    }

    drop(client);
    handle.join().unwrap().unwrap();
}

#[test]
fn v6_select_option_is_denied_after_a_v5_handshake_before_review_or_delegate() {
    let registry = registry_with_dom_write_granted();
    let (mut client, mut server) = UnixStream::pair().unwrap();
    let handle = thread::spawn(move || {
        handle_extension_connection_with_actions(
            &registry,
            Path::new("/not-reached-for-v5-select-version-denial.sock"),
            &mut server,
            |_| Ok("unused in this test".to_string()),
            |_, _, _, _| panic!("a v5 connection must not delegate a v6 select request"),
            || Ok(()),
        )
    });

    write_extension_request(
        &mut client,
        &hello_with_capabilities(MINIMAL_SLICE_EXTENSION_ID, [(CAPABILITY_DOM_WRITE, 5)]),
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut client).unwrap(),
        empty_hello_ack()
    );
    write_extension_request(
        &mut client,
        &ExtensionRequest::SelectOption {
            tab_id: 1,
            node_id: 2,
        },
    )
    .unwrap();
    match read_extension_reply(&mut client).unwrap() {
        ExtensionReply::CapabilityDenied { capability, reason } => {
            assert_eq!(capability, CAPABILITY_DOM_WRITE);
            assert!(reason.contains("requires version 6"));
        }
        other => panic!("expected a v6 version denial, got {other:?}"),
    }

    drop(client);
    handle.join().unwrap().unwrap();
}

#[test]
fn v5_radio_selection_is_denied_after_a_v4_handshake_before_review_or_delegate() {
    let registry = registry_with_dom_write_granted();
    let (mut client, mut server) = UnixStream::pair().unwrap();
    let handle = thread::spawn(move || {
        handle_extension_connection_with_actions(
            &registry,
            Path::new("/not-reached-for-v4-radio-version-denial.sock"),
            &mut server,
            |_| Ok("unused in this test".to_string()),
            |_, _, _, _| panic!("a v4 connection must not delegate a v5 radio request"),
            || Ok(()),
        )
    });

    write_extension_request(
        &mut client,
        &hello_with_capabilities(MINIMAL_SLICE_EXTENSION_ID, [(CAPABILITY_DOM_WRITE, 4)]),
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut client).unwrap(),
        empty_hello_ack()
    );
    write_extension_request(
        &mut client,
        &ExtensionRequest::SetRadioChecked {
            tab_id: 1,
            node_id: 2,
        },
    )
    .unwrap();
    match read_extension_reply(&mut client).unwrap() {
        ExtensionReply::CapabilityDenied { capability, reason } => {
            assert_eq!(capability, CAPABILITY_DOM_WRITE);
            assert!(reason.contains("requires version 5"));
        }
        other => panic!("expected a v5 version denial, got {other:?}"),
    }

    drop(client);
    handle.join().unwrap().unwrap();
}

#[test]
fn v3_checkbox_write_is_denied_after_a_v2_handshake_before_review_or_delegate() {
    let registry = registry_with_dom_write_granted();
    let (mut client, mut server) = UnixStream::pair().unwrap();
    let handle = thread::spawn(move || {
        handle_extension_connection_with_actions(
            &registry,
            Path::new("/not-reached-for-v2-checkbox-version-denial.sock"),
            &mut server,
            |_| Ok("unused in this test".to_string()),
            |_, _, _, _| panic!("a v2 connection must not delegate a v3 request"),
            || Ok(()),
        )
    });

    write_extension_request(
        &mut client,
        &hello_with_capabilities(MINIMAL_SLICE_EXTENSION_ID, [(CAPABILITY_DOM_WRITE, 2)]),
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut client).unwrap(),
        empty_hello_ack()
    );
    write_extension_request(
        &mut client,
        &ExtensionRequest::SetCheckboxChecked {
            tab_id: 1,
            node_id: 2,
            checked: false,
        },
    )
    .unwrap();
    match read_extension_reply(&mut client).unwrap() {
        ExtensionReply::CapabilityDenied { capability, reason } => {
            assert_eq!(capability, CAPABILITY_DOM_WRITE);
            assert!(reason.contains("requires version 3"));
        }
        other => panic!("expected a v3 version denial, got {other:?}"),
    }

    drop(client);
    handle.join().unwrap().unwrap();
}

#[test]
fn a_granted_dom_read_succeeds_after_handshake() {
    let registry = ExtensionRegistry::minimal_slice();
    let (mut client, mut server) = UnixStream::pair().unwrap();
    let handle = thread::spawn(move || handle_extension_connection(&registry, &mut server));

    write_extension_request(&mut client, &hello(MINIMAL_SLICE_EXTENSION_ID)).unwrap();
    assert_eq!(
        read_extension_reply(&mut client).unwrap(),
        empty_hello_ack()
    );

    write_extension_request(&mut client, &ExtensionRequest::DomRead).unwrap();
    assert_eq!(
        read_extension_reply(&mut client).unwrap(),
        ExtensionReply::DomReadResult {
            value: PLACEHOLDER_DOM_READ_VALUE.to_string()
        }
    );

    drop(client);
    handle.join().unwrap().unwrap();
}

#[test]
fn v4_host_registration_is_denied_after_a_v3_handshake() {
    let registry = registry_with_network_intercept_granted();
    let (mut client, mut server) = UnixStream::pair().unwrap();
    let handle = thread::spawn(move || {
        handle_extension_connection_with_actions_and_authentication_and_network_rules(
            &registry,
            Path::new("/not-reached-for-v3-host-rule-version-denial.sock"),
            &mut server,
            ExtensionConnectionAuthentication::unauthenticated(),
            ExtensionActionDelegates::new(
                |_| Ok("unused in this test".to_string()),
                unused_write_delegate,
                || Ok(()),
                |_, _| Ok(()),
                || Ok(()),
            )
            .with_network_block_host(|_, _| panic!("a v3 connection must not reach core")),
        )
    });
    write_extension_request(
        &mut client,
        &hello_with_capabilities(
            MINIMAL_SLICE_EXTENSION_ID,
            [(CAPABILITY_NETWORK_INTERCEPT, 3)],
        ),
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut client).unwrap(),
        empty_hello_ack()
    );
    write_extension_request(
        &mut client,
        &ExtensionRequest::RegisterNetworkBlockHost {
            host: "example.test".to_string(),
        },
    )
    .unwrap();
    match read_extension_reply(&mut client).unwrap() {
        ExtensionReply::CapabilityDenied { capability, reason } => {
            assert_eq!(capability, CAPABILITY_NETWORK_INTERCEPT);
            assert!(reason.contains("requires version 4"));
        }
        other => panic!("expected a v4 version denial, got {other:?}"),
    }
    drop(client);
    handle.join().unwrap().unwrap();
}

#[test]
fn v5_path_prefix_is_denied_after_a_v4_handshake_before_gatekeeper_or_core() {
    let registry = registry_with_network_intercept_granted();
    let (mut client, mut server) = UnixStream::pair().unwrap();
    let handle = thread::spawn(move || {
        handle_extension_connection_with_actions_and_authentication_and_network_rules(
            &registry,
            Path::new("/not-reached-for-v4-path-rule-denial.sock"),
            &mut server,
            ExtensionConnectionAuthentication::unauthenticated(),
            ExtensionActionDelegates::new(
                |_| Ok("unused".into()),
                unused_write_delegate,
                || Ok(()),
                |_, _| Ok(()),
                || Ok(()),
            )
            .with_network_block_path_prefix(|_, _, _| panic!("v4 must not reach core")),
        )
    });
    write_extension_request(
        &mut client,
        &hello_with_capabilities(
            MINIMAL_SLICE_EXTENSION_ID,
            [(CAPABILITY_NETWORK_INTERCEPT, 4)],
        ),
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut client).unwrap(),
        empty_hello_ack()
    );
    write_extension_request(
        &mut client,
        &ExtensionRequest::RegisterNetworkBlockPathPrefix {
            host: "example.test".into(),
            path_prefix: "/private".into(),
        },
    )
    .unwrap();
    assert!(matches!(read_extension_reply(&mut client).unwrap(),
        ExtensionReply::CapabilityDenied { capability, reason }
            if capability == CAPABILITY_NETWORK_INTERCEPT && reason.contains("requires version 5")));
    drop(client);
    handle.join().unwrap().unwrap();
}

#[test]
fn v6_redirect_is_denied_after_a_v5_handshake() {
    let registry = registry_with_network_intercept_granted();
    let (mut client, mut server) = UnixStream::pair().unwrap();
    let handle = thread::spawn(move || {
        handle_extension_connection_with_actions_and_authentication_and_network_rules(
            &registry,
            Path::new("/not-reached-for-v5-redirect-denial.sock"),
            &mut server,
            ExtensionConnectionAuthentication::unauthenticated(),
            ExtensionActionDelegates::new(
                |_| Ok("unused".into()),
                unused_write_delegate,
                || Ok(()),
                |_, _| Ok(()),
                || Ok(()),
            )
            .with_network_redirect_url(|_, _, _| panic!("v5 must not reach core")),
        )
    });
    write_extension_request(
        &mut client,
        &hello_with_capabilities(
            MINIMAL_SLICE_EXTENSION_ID,
            [(CAPABILITY_NETWORK_INTERCEPT, 5)],
        ),
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut client).unwrap(),
        empty_hello_ack()
    );
    write_extension_request(
        &mut client,
        &ExtensionRequest::RegisterNetworkRedirectUrl {
            source_url: "https://example.test/old".into(),
            target_url: "https://example.test/new".into(),
        },
    )
    .unwrap();
    assert!(matches!(read_extension_reply(&mut client).unwrap(),
        ExtensionReply::CapabilityDenied { capability, reason }
            if capability == CAPABILITY_NETWORK_INTERCEPT && reason.contains("requires version 6")));
    drop(client);
    handle.join().unwrap().unwrap();
}
