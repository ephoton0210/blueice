// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn runtime_ephemeral_lease_is_exactly_scoped_one_shot_and_never_a_persistent_grant() {
    let mut registry = ExtensionRegistry::with_supported_capabilities();
    registry.declare_runtime_ephemeral("sha256:installed", CAPABILITY_DOM_READ);
    assert!(registry
        .arm_runtime_ephemeral("sha256:other", CAPABILITY_DOM_READ, 1, 2)
        .is_err());
    assert!(registry
        .arm_runtime_ephemeral("sha256:installed", CAPABILITY_DOM_WRITE, 1, 2)
        .is_err());
    assert!(registry
        .arm_runtime_ephemeral("sha256:installed", CAPABILITY_DOM_READ, 0, 2)
        .is_err());
    assert!(registry
        .grant_optional("sha256:installed", CAPABILITY_DOM_READ)
        .is_err());

    let ticket = registry
        .arm_runtime_ephemeral("sha256:installed", CAPABILITY_DOM_READ, 1, 2)
        .unwrap();
    assert_eq!(ticket.len(), 64);
    assert!(ticket.bytes().all(|byte| byte.is_ascii_hexdigit()));
    assert!(!registry.has_capability("sha256:installed", CAPABILITY_DOM_READ));
    assert_eq!(
        registry.capability_generation("sha256:installed", CAPABILITY_DOM_READ),
        None
    );
    assert!(
        !registry.consume_runtime_ephemeral("sha256:installed", CAPABILITY_DOM_READ, &ticket, 2, 2),
        "another tab cannot use or spend the lease"
    );
    assert!(
        !registry.consume_runtime_ephemeral(
            "sha256:installed",
            CAPABILITY_DOM_READ,
            &"0".repeat(64),
            1,
            2
        ),
        "an invalid bearer must not spend a valid lease"
    );
    assert!(registry.consume_runtime_ephemeral(
        "sha256:installed",
        CAPABILITY_DOM_READ,
        &ticket,
        1,
        2
    ));
    assert!(
        !registry.consume_runtime_ephemeral("sha256:installed", CAPABILITY_DOM_READ, &ticket, 1, 2),
        "one gesture never authorizes a second operation"
    );

    let next_ticket = registry
        .arm_runtime_ephemeral("sha256:installed", CAPABILITY_DOM_READ, 1, 2)
        .unwrap();
    assert_ne!(ticket, next_ticket);
    assert!(
        !registry.consume_runtime_ephemeral("sha256:installed", CAPABILITY_DOM_READ, &ticket, 1, 2),
        "a request queued before a later gesture cannot borrow it"
    );
    assert!(
        !registry.consume_runtime_ephemeral(
            "sha256:installed",
            CAPABILITY_DOM_READ,
            &next_ticket,
            1,
            3
        ),
        "a same-tab document replacement invalidates the old lease"
    );
    assert!(
        !registry.consume_runtime_ephemeral(
            "sha256:installed",
            CAPABILITY_DOM_READ,
            &next_ticket,
            1,
            2
        ),
        "a stale identity cannot be revived after replacement"
    );
    let last_ticket = registry
        .arm_runtime_ephemeral("sha256:installed", CAPABILITY_DOM_READ, 1, 3)
        .unwrap();
    assert!(registry
        .revoke_runtime_ephemeral("sha256:installed", CAPABILITY_DOM_READ)
        .unwrap());
    assert!(!registry
        .revoke_runtime_ephemeral("sha256:installed", CAPABILITY_DOM_READ)
        .unwrap());
    assert!(!registry.consume_runtime_ephemeral(
        "sha256:installed",
        CAPABILITY_DOM_READ,
        &last_ticket,
        1,
        3
    ));
    let expired_ticket = registry
        .arm_runtime_ephemeral("sha256:installed", CAPABILITY_DOM_READ, 1, 3)
        .unwrap();
    {
        let state = registry
            .ephemeral_state("sha256:installed", CAPABILITY_DOM_READ)
            .unwrap();
        let mut slot = state.lock().unwrap();
        slot.lease.as_mut().unwrap().expires_at = Instant::now() - Duration::from_nanos(1);
    }
    assert!(!registry.has_unspent_runtime_ephemeral_lease("sha256:installed", CAPABILITY_DOM_READ));
    assert!(
        !registry.consume_runtime_ephemeral(
            "sha256:installed",
            CAPABILITY_DOM_READ,
            &expired_ticket,
            1,
            3
        ),
        "an event delayed past the gesture deadline must not read the document"
    );
}

#[test]
fn concurrent_runtime_ephemeral_consumers_have_exactly_one_winner() {
    use std::sync::Barrier;

    let mut registry = ExtensionRegistry::with_supported_capabilities();
    registry.declare_runtime_ephemeral("sha256:installed", CAPABILITY_DOM_READ);
    let ticket = registry
        .arm_runtime_ephemeral("sha256:installed", CAPABILITY_DOM_READ, 1, 4)
        .unwrap();
    let registry = Arc::new(registry);
    let barrier = Arc::new(Barrier::new(8));
    let workers: Vec<_> = (0..8)
        .map(|_| {
            let registry = Arc::clone(&registry);
            let barrier = Arc::clone(&barrier);
            let ticket = ticket.clone();
            thread::spawn(move || {
                barrier.wait();
                registry.consume_runtime_ephemeral(
                    "sha256:installed",
                    CAPABILITY_DOM_READ,
                    &ticket,
                    1,
                    4,
                )
            })
        })
        .collect();
    assert_eq!(
        workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .filter(|won| *won)
            .count(),
        1
    );
}

#[test]
fn extension_registry_default_grants_nothing() {
    let registry = ExtensionRegistry::default();
    assert!(!registry.has_capability(MINIMAL_SLICE_EXTENSION_ID, CAPABILITY_DOM_READ));
}

#[test]
fn network_observation_requires_its_own_grant_and_negotiated_version() {
    let mut registry = ExtensionRegistry::minimal_slice();
    registry.grant(MINIMAL_SLICE_EXTENSION_ID, CAPABILITY_NETWORK_OBSERVE);
    let (mut client, mut server) = UnixStream::pair().unwrap();
    let handle = thread::spawn(move || {
        handle_extension_connection_with_actions_and_authentication_and_network_rules(
            &registry,
            Path::new("/not-used-for-read-only-observation"),
            &mut server,
            ExtensionConnectionAuthentication::unauthenticated(),
            ExtensionActionDelegates::new(
                |_| Ok(String::new()),
                unused_write_delegate,
                || Ok(()),
                |_, _| Ok(()),
                || Ok(()),
            )
            .with_network_observer(|tab_id| {
                assert_eq!(tab_id, 7);
                Ok(Some(NetworkResponseInfo {
                    method: "GET".to_string(),
                    final_url: "https://example.test/final".to_string(),
                    status: 200,
                    content_type: Some("text/html".to_string()),
                }))
            })
            .with_network_trace_observer(|tab_id| {
                assert_eq!(tab_id, 7);
                Ok(Some(NetworkTraceInfo {
                    request_url: "https://example.test/start".to_string(),
                    redirects: vec![blueice_ipc::extension::NetworkRedirectInfo {
                        request_url: "https://example.test/start".to_string(),
                        status: 302,
                        target_url: "https://example.test/final".to_string(),
                    }],
                    response: NetworkResponseInfo {
                        method: "GET".to_string(),
                        final_url: "https://example.test/final".to_string(),
                        status: 200,
                        content_type: Some("text/html".to_string()),
                    },
                }))
            }),
        )
    });

    write_extension_request(
        &mut client,
        &hello_with_capabilities(
            MINIMAL_SLICE_EXTENSION_ID,
            [(CAPABILITY_NETWORK_OBSERVE, 1)],
        ),
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut client).unwrap(),
        empty_hello_ack()
    );
    write_extension_request(
        &mut client,
        &ExtensionRequest::ReadNetworkResponse { tab_id: 7 },
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut client).unwrap(),
        ExtensionReply::NetworkResponseResult {
            response: Some(NetworkResponseInfo {
                method: "GET".to_string(),
                final_url: "https://example.test/final".to_string(),
                status: 200,
                content_type: Some("text/html".to_string()),
            })
        }
    );
    write_extension_request(
        &mut client,
        &ExtensionRequest::ReadNetworkTrace { tab_id: 7 },
    )
    .unwrap();
    assert!(matches!(read_extension_reply(&mut client).unwrap(),
        ExtensionReply::CapabilityDenied { capability, .. } if capability == CAPABILITY_NETWORK_OBSERVE));

    write_extension_request(
        &mut client,
        &hello_with_capabilities(MINIMAL_SLICE_EXTENSION_ID, [(CAPABILITY_DOM_READ, 1)]),
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut client).unwrap(),
        empty_hello_ack()
    );
    write_extension_request(
        &mut client,
        &ExtensionRequest::ReadNetworkResponse { tab_id: 7 },
    )
    .unwrap();
    assert!(matches!(
        read_extension_reply(&mut client).unwrap(),
        ExtensionReply::CapabilityDenied { capability, .. } if capability == CAPABILITY_NETWORK_OBSERVE
    ));
    write_extension_request(
        &mut client,
        &hello_with_capabilities(
            MINIMAL_SLICE_EXTENSION_ID,
            [(CAPABILITY_NETWORK_OBSERVE, 2)],
        ),
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut client).unwrap(),
        empty_hello_ack()
    );
    write_extension_request(
        &mut client,
        &ExtensionRequest::ReadNetworkTrace { tab_id: 7 },
    )
    .unwrap();
    assert!(matches!(read_extension_reply(&mut client).unwrap(),
        ExtensionReply::NetworkTraceResult { trace: Some(trace) }
            if trace.request_url == "https://example.test/start"
                && trace.redirects.len() == 1
                && trace.redirects[0].status == 302));
    write_extension_request(
        &mut client,
        &hello_with_capabilities(
            MINIMAL_SLICE_EXTENSION_ID,
            [(CAPABILITY_NETWORK_OBSERVE, 3)],
        ),
    )
    .unwrap();
    assert!(matches!(
        read_extension_reply(&mut client).unwrap(),
        ExtensionReply::HelloAck { unsupported_capabilities }
            if matches!(
                unsupported_capabilities.get(CAPABILITY_NETWORK_OBSERVE),
                Some(UnsupportedCapabilityVersion::OutsideSupportedRange {
                    min_inclusive: 1,
                    max_inclusive: 2,
                })
            )
    ));
    write_extension_request(
        &mut client,
        &ExtensionRequest::ReadNetworkTrace { tab_id: 7 },
    )
    .unwrap();
    assert!(matches!(
        read_extension_reply(&mut client).unwrap(),
        ExtensionReply::CapabilityDenied { capability, .. } if capability == CAPABILITY_NETWORK_OBSERVE
    ));
    drop(client);
    handle.join().unwrap().unwrap();
}

#[test]
fn native_toolbar_requires_ui_grant_and_validates_before_delegation() {
    let mut registry = ExtensionRegistry::minimal_slice();
    registry.grant(MINIMAL_SLICE_EXTENSION_ID, CAPABILITY_UI_INJECT);
    let (gatekeeper, reviewed) =
        start_gatekeeper_replies("toolbar-clear", vec![GatekeeperReply::Cleared; 2]);
    let (seen_tx, seen_rx) = mpsc::channel();
    let (clear_tx, clear_rx) = mpsc::channel();
    let (mut client, mut server) = UnixStream::pair().unwrap();
    let gatekeeper_for_host = gatekeeper.clone();
    let handle = thread::spawn(move || {
        handle_extension_connection_with_actions_and_authentication_and_network_rules(
            &registry,
            &gatekeeper_for_host,
            &mut server,
            ExtensionConnectionAuthentication::unauthenticated(),
            ExtensionActionDelegates::new(
                |_| Ok(String::new()),
                unused_write_delegate,
                || Ok(()),
                |_, _| Ok(()),
                || Ok(()),
            )
            .with_toolbar_button(move |label, _| {
                seen_tx.send(label).unwrap();
                Ok(())
            })
            .with_toolbar_clearer(move || {
                clear_tx.send(()).unwrap();
                Ok(())
            }),
        )
    });
    write_extension_request(
        &mut client,
        &hello_with_capabilities(MINIMAL_SLICE_EXTENSION_ID, [(CAPABILITY_UI_INJECT, 1)]),
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut client).unwrap(),
        empty_hello_ack()
    );
    write_extension_request(
        &mut client,
        &ExtensionRequest::SetToolbarButton {
            label: "Bad\nLabel".to_string(),
        },
    )
    .unwrap();
    assert!(matches!(
        read_extension_reply(&mut client).unwrap(),
        ExtensionReply::OperationUnavailable { capability, .. } if capability == CAPABILITY_UI_INJECT
    ));
    assert!(seen_rx.try_recv().is_err());
    write_extension_request(
        &mut client,
        &ExtensionRequest::SetToolbarButton {
            label: "Notes".to_string(),
        },
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut client).unwrap(),
        ExtensionReply::UiInjectAck
    );
    assert_eq!(
        seen_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
        "Notes"
    );
    write_extension_request(&mut client, &ExtensionRequest::ClearToolbarButton).unwrap();
    assert_eq!(
        read_extension_reply(&mut client).unwrap(),
        ExtensionReply::UiInjectAck
    );
    clear_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    write_extension_request(
        &mut client,
        &ExtensionRequest::SetToolbarButton {
            label: "Notes".to_string(),
        },
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut client).unwrap(),
        ExtensionReply::UiInjectAck
    );
    assert_eq!(
        seen_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
        "Notes"
    );
    write_extension_request(
        &mut client,
        &hello_with_capabilities(MINIMAL_SLICE_EXTENSION_ID, [(CAPABILITY_DOM_READ, 1)]),
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut client).unwrap(),
        empty_hello_ack()
    );
    clear_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    write_extension_request(
        &mut client,
        &ExtensionRequest::SetToolbarButton {
            label: "Denied".to_string(),
        },
    )
    .unwrap();
    assert!(matches!(
        read_extension_reply(&mut client).unwrap(),
        ExtensionReply::CapabilityDenied { capability, .. } if capability == CAPABILITY_UI_INJECT
    ));
    assert!(seen_rx.try_recv().is_err());
    write_extension_request(
        &mut client,
        &hello_with_capabilities(MINIMAL_SLICE_EXTENSION_ID, [(CAPABILITY_UI_INJECT, 4)]),
    )
    .unwrap();
    assert!(matches!(
        read_extension_reply(&mut client).unwrap(),
        ExtensionReply::HelloAck { unsupported_capabilities }
            if matches!(
                unsupported_capabilities.get(CAPABILITY_UI_INJECT),
                Some(UnsupportedCapabilityVersion::OutsideSupportedRange {
                    min_inclusive: 1,
                    max_inclusive: 3,
                })
            )
    ));
    assert!(clear_rx.try_recv().is_err());
    write_extension_request(
        &mut client,
        &ExtensionRequest::SetToolbarButton {
            label: "Denied".to_string(),
        },
    )
    .unwrap();
    assert!(matches!(
        read_extension_reply(&mut client).unwrap(),
        ExtensionReply::CapabilityDenied { capability, .. } if capability == CAPABILITY_UI_INJECT
    ));
    drop(client);
    handle.join().unwrap().unwrap();
    let requests = reviewed.join().unwrap();
    assert_eq!(requests.len(), 2);
    assert!(requests.iter().all(|request| matches!(request,
        GatekeeperRequest::CheckExtensionAction { detail, .. }
            if detail.contains("action=set-native-toolbar-button") && detail.contains("Notes")
    )));
    let _ = std::fs::remove_file(gatekeeper);
}

#[test]
fn reviewed_toolbar_cannot_publish_after_optional_revoke_and_regrant() {
    let mut registry = ExtensionRegistry::with_supported_capabilities();
    registry.declare_optional(MINIMAL_SLICE_EXTENSION_ID, CAPABILITY_UI_INJECT);
    let registry = Arc::new(registry);
    registry
        .grant_optional(MINIMAL_SLICE_EXTENSION_ID, CAPABILITY_UI_INJECT)
        .unwrap();
    let socket = unique_gatekeeper_socket("toolbar-revoke");
    let _ = std::fs::remove_file(&socket);
    let listener = UnixListener::bind(&socket).unwrap();
    let (review_started_tx, review_started_rx) = mpsc::channel();
    let (release_review_tx, release_review_rx) = mpsc::channel();
    let reviewer = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let request = read_gatekeeper_request(&mut stream).unwrap();
        review_started_tx.send(request).unwrap();
        release_review_rx
            .recv_timeout(Duration::from_secs(1))
            .unwrap();
        write_gatekeeper_reply(&mut stream, &GatekeeperReply::Cleared).unwrap();
    });
    let (published_tx, published_rx) = mpsc::channel();
    let (mut client, mut server) = UnixStream::pair().unwrap();
    let registry_for_host = Arc::clone(&registry);
    let socket_for_host = socket.clone();
    let host = thread::spawn(move || {
        handle_extension_connection_with_actions_and_authentication_and_network_rules(
            &registry_for_host,
            &socket_for_host,
            &mut server,
            ExtensionConnectionAuthentication::unauthenticated(),
            ExtensionActionDelegates::new(
                |_| Ok(String::new()),
                unused_write_delegate,
                || Ok(()),
                |_, _| Ok(()),
                || Ok(()),
            )
            .with_toolbar_button(move |label, _| {
                published_tx.send(label).unwrap();
                Ok(())
            }),
        )
    });
    write_extension_request(
        &mut client,
        &hello_with_capabilities(MINIMAL_SLICE_EXTENSION_ID, [(CAPABILITY_UI_INJECT, 1)]),
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut client).unwrap(),
        empty_hello_ack()
    );
    write_extension_request(
        &mut client,
        &ExtensionRequest::SetToolbarButton {
            label: "Notes".to_string(),
        },
    )
    .unwrap();
    assert!(
        matches!(review_started_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
        GatekeeperRequest::CheckExtensionAction { detail, .. }
            if detail.contains("action=set-native-toolbar-button"))
    );
    registry
        .revoke_optional(MINIMAL_SLICE_EXTENSION_ID, CAPABILITY_UI_INJECT)
        .unwrap();
    registry
        .grant_optional(MINIMAL_SLICE_EXTENSION_ID, CAPABILITY_UI_INJECT)
        .unwrap();
    release_review_tx.send(()).unwrap();
    assert!(matches!(read_extension_reply(&mut client).unwrap(),
        ExtensionReply::CapabilityDenied { capability, .. }
            if capability == CAPABILITY_UI_INJECT));
    assert!(published_rx.try_recv().is_err());
    drop(client);
    host.join().unwrap().unwrap();
    reviewer.join().unwrap();
    let _ = std::fs::remove_file(socket);
}

#[test]
fn extension_registry_minimal_slice_grants_dom_read_but_not_dom_write() {
    let registry = ExtensionRegistry::minimal_slice();
    assert!(registry.has_capability(MINIMAL_SLICE_EXTENSION_ID, CAPABILITY_DOM_READ));
    assert!(!registry.has_capability(MINIMAL_SLICE_EXTENSION_ID, CAPABILITY_DOM_WRITE));
    assert!(!registry.has_capability(MINIMAL_SLICE_EXTENSION_ID, CAPABILITY_NETWORK_INTERCEPT));
}

#[test]
fn extension_registry_reports_no_capabilities_for_an_unknown_extension_id() {
    let registry = ExtensionRegistry::minimal_slice();
    assert!(!registry.has_capability("some-other-extension", CAPABILITY_DOM_READ));
    assert!(!registry.has_capability("some-other-extension", CAPABILITY_DOM_WRITE));
}

#[test]
fn extension_registry_new_grants_nothing_until_granted() {
    let mut registry = ExtensionRegistry::new();
    assert!(!registry.has_capability(MINIMAL_SLICE_EXTENSION_ID, CAPABILITY_DOM_READ));
    registry.grant(MINIMAL_SLICE_EXTENSION_ID, CAPABILITY_DOM_READ);
    assert!(registry.has_capability(MINIMAL_SLICE_EXTENSION_ID, CAPABILITY_DOM_READ));
}

#[test]
fn capability_version_window_has_inclusive_validated_bounds() {
    let window = CapabilityVersionWindow::new(1, 2).unwrap();
    assert!(window.contains(1));
    assert!(window.contains(2));
    assert!(!window.contains(0));
    assert!(!window.contains(3));
    assert!(CapabilityVersionWindow::new(2, 1).is_none());
}

#[test]
fn optional_read_revocation_waits_for_reply_and_regrant_rejects_old_generation() {
    use std::time::Duration;

    let mut registry = ExtensionRegistry::with_supported_capabilities();
    registry.declare_optional(MINIMAL_SLICE_EXTENSION_ID, CAPABILITY_DOM_READ);
    let registry = Arc::new(registry);
    registry
        .grant_optional(MINIMAL_SLICE_EXTENSION_ID, CAPABILITY_DOM_READ)
        .unwrap();
    let old_generation = registry
        .capability_generation(MINIMAL_SLICE_EXTENSION_ID, CAPABILITY_DOM_READ)
        .unwrap();
    let (mut client, mut server) = UnixStream::pair().unwrap();
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let worker_registry = Arc::clone(&registry);
    let worker = thread::spawn(move || {
        let identity = ConnectionIdentity {
            extension_id: MINIMAL_SLICE_EXTENSION_ID.to_string(),
            negotiated_capabilities: BTreeMap::from([(CAPABILITY_DOM_READ.to_string(), 2)]),
        };
        write_stable_read_reply(
            &mut server,
            &worker_registry,
            &identity,
            CAPABILITY_DOM_READ,
            Some(old_generation),
            || {
                entered_tx.send(()).unwrap();
                release_rx.recv_timeout(Duration::from_secs(2)).unwrap();
                ExtensionReply::DomReadResult {
                    value: "prior-authorized-result".into(),
                }
            },
        )
        .unwrap();
    });
    entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    let (attempt_tx, attempt_rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel();
    let revoker_registry = Arc::clone(&registry);
    let revoker = thread::spawn(move || {
        attempt_tx.send(()).unwrap();
        revoker_registry
            .revoke_optional(MINIMAL_SLICE_EXTENSION_ID, CAPABILITY_DOM_READ)
            .unwrap();
        done_tx.send(()).unwrap();
    });
    attempt_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(done_rx.recv_timeout(Duration::from_millis(50)).is_err());
    release_tx.send(()).unwrap();
    assert_eq!(
        read_extension_reply(&mut client).unwrap(),
        ExtensionReply::DomReadResult {
            value: "prior-authorized-result".into()
        }
    );
    worker.join().unwrap();
    done_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    revoker.join().unwrap();
    registry
        .grant_optional(MINIMAL_SLICE_EXTENSION_ID, CAPABILITY_DOM_READ)
        .unwrap();
    let identity = ConnectionIdentity {
        extension_id: MINIMAL_SLICE_EXTENSION_ID.to_string(),
        negotiated_capabilities: BTreeMap::from([(CAPABILITY_DOM_READ.to_string(), 2)]),
    };
    let mut denial = Vec::new();
    write_stable_read_reply(
        &mut denial,
        &registry,
        &identity,
        CAPABILITY_DOM_READ,
        Some(old_generation),
        || panic!("the old generation must not fetch data"),
    )
    .unwrap();
    assert!(matches!(
        read_extension_reply(&mut denial.as_slice()).unwrap(),
        ExtensionReply::CapabilityDenied { capability, .. } if capability == CAPABILITY_DOM_READ
    ));
}

#[test]
fn v8_visible_leaf_text_requires_its_version_and_reviews_the_exact_payload() {
    let registry = registry_with_dom_write_granted();
    let (gatekeeper_socket, gatekeeper) =
        start_gatekeeper("clear-v8-visible-leaf", GatekeeperReply::Cleared);
    let (seen_tx, seen_rx) = std::sync::mpsc::channel();
    let (mut client, mut server) = UnixStream::pair().unwrap();
    let socket_for_handler = gatekeeper_socket.clone();
    let handle = thread::spawn(move || {
        handle_extension_connection_with_actions(
            &registry,
            &socket_for_handler,
            &mut server,
            |_| Ok("unused".to_string()),
            move |target, value, kind, _| {
                seen_tx.send((target, value, kind.clone())).unwrap();
                Ok(())
            },
            || Ok(()),
        )
    });
    write_extension_request(
        &mut client,
        &hello_with_capabilities(MINIMAL_SLICE_EXTENSION_ID, [(CAPABILITY_DOM_WRITE, 8)]),
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut client).unwrap(),
        empty_hello_ack()
    );
    write_extension_request(
        &mut client,
        &ExtensionRequest::SetVisibleLeafText {
            tab_id: 7,
            node_id: 19,
            value: "Updated heading".to_string(),
        },
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut client).unwrap(),
        ExtensionReply::DomWriteAck
    );
    assert_eq!(
        seen_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
        (
            Some((7, 19)),
            "Updated heading".to_string(),
            DomWriteTarget::VisibleTextLeaf
        )
    );
    write_extension_request(
        &mut client,
        &ExtensionRequest::SetVisibleLeafText {
            tab_id: 7,
            node_id: 19,
            value: " \n ".to_string(),
        },
    )
    .unwrap();
    assert!(
        matches!(read_extension_reply(&mut client).unwrap(), ExtensionReply::OperationUnavailable { capability, .. } if capability == CAPABILITY_DOM_WRITE)
    );
    assert!(seen_rx.try_recv().is_err());
    drop(client);
    handle.join().unwrap().unwrap();
    assert_eq!(
        gatekeeper.join().unwrap(),
        GatekeeperRequest::CheckExtensionAction {
            extension_id: MINIMAL_SLICE_EXTENSION_ID.to_string(),
            capability: CAPABILITY_DOM_WRITE.to_string(),
            detail: "action=set-visible-leaf-text; text=Updated heading".to_string(),
        }
    );
    let _ = std::fs::remove_file(gatekeeper_socket);

    let registry = registry_with_dom_write_granted();
    let (mut client, mut server) = UnixStream::pair().unwrap();
    let handle = thread::spawn(move || {
        handle_extension_connection_with_actions(
            &registry,
            Path::new("/not-reached-for-v7-visible-leaf.sock"),
            &mut server,
            |_| Ok("unused".to_string()),
            |_, _, _, _| panic!("v7 cannot delegate a v8 request"),
            || Ok(()),
        )
    });
    write_extension_request(
        &mut client,
        &hello_with_capabilities(MINIMAL_SLICE_EXTENSION_ID, [(CAPABILITY_DOM_WRITE, 7)]),
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut client).unwrap(),
        empty_hello_ack()
    );
    write_extension_request(
        &mut client,
        &ExtensionRequest::SetVisibleLeafText {
            tab_id: 7,
            node_id: 19,
            value: "denied".to_string(),
        },
    )
    .unwrap();
    assert!(
        matches!(read_extension_reply(&mut client).unwrap(), ExtensionReply::CapabilityDenied { capability, reason } if capability == CAPABILITY_DOM_WRITE && reason.contains("requires version 8"))
    );
    drop(client);
    handle.join().unwrap().unwrap();
}

#[test]
fn v9_text_content_requires_its_version_and_reviews_the_exact_payload() {
    let registry = registry_with_dom_write_granted();
    let (gatekeeper_socket, gatekeeper) =
        start_gatekeeper("clear-v9-text-content", GatekeeperReply::Cleared);
    let (seen_tx, seen_rx) = std::sync::mpsc::channel();
    let (mut client, mut server) = UnixStream::pair().unwrap();
    let socket_for_handler = gatekeeper_socket.clone();
    let handle = thread::spawn(move || {
        handle_extension_connection_with_actions(
            &registry,
            &socket_for_handler,
            &mut server,
            |_| Ok("unused".to_string()),
            move |target, value, kind, _| {
                seen_tx.send((target, value, kind.clone())).unwrap();
                Ok(())
            },
            || Ok(()),
        )
    });
    write_extension_request(
        &mut client,
        &hello_with_capabilities(MINIMAL_SLICE_EXTENSION_ID, [(CAPABILITY_DOM_WRITE, 9)]),
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut client).unwrap(),
        empty_hello_ack()
    );
    write_extension_request(
        &mut client,
        &ExtensionRequest::SetVisibleTextContent {
            tab_id: 7,
            node_id: 19,
            value: "Updated formatted text".to_string(),
        },
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut client).unwrap(),
        ExtensionReply::DomWriteAck
    );
    assert_eq!(
        seen_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
        (
            Some((7, 19)),
            "Updated formatted text".to_string(),
            DomWriteTarget::VisibleTextContent,
        )
    );
    drop(client);
    handle.join().unwrap().unwrap();
    assert_eq!(
        gatekeeper.join().unwrap(),
        GatekeeperRequest::CheckExtensionAction {
            extension_id: MINIMAL_SLICE_EXTENSION_ID.to_string(),
            capability: CAPABILITY_DOM_WRITE.to_string(),
            detail: "action=set-visible-text-content; text=Updated formatted text".to_string(),
        }
    );
    let _ = std::fs::remove_file(gatekeeper_socket);

    let registry = registry_with_dom_write_granted();
    let (mut client, mut server) = UnixStream::pair().unwrap();
    let handle = thread::spawn(move || {
        handle_extension_connection_with_actions(
            &registry,
            Path::new("/not-reached-for-v8-text-content.sock"),
            &mut server,
            |_| Ok("unused".to_string()),
            |_, _, _, _| panic!("v8 cannot delegate a v9 request"),
            || Ok(()),
        )
    });
    write_extension_request(
        &mut client,
        &hello_with_capabilities(MINIMAL_SLICE_EXTENSION_ID, [(CAPABILITY_DOM_WRITE, 8)]),
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut client).unwrap(),
        empty_hello_ack()
    );
    write_extension_request(
        &mut client,
        &ExtensionRequest::SetVisibleTextContent {
            tab_id: 7,
            node_id: 19,
            value: "denied".to_string(),
        },
    )
    .unwrap();
    assert!(matches!(
        read_extension_reply(&mut client).unwrap(),
        ExtensionReply::CapabilityDenied { capability, reason }
            if capability == CAPABILITY_DOM_WRITE && reason.contains("requires version 9")
    ));
    drop(client);
    handle.join().unwrap().unwrap();
}

#[test]
fn legacy_dom_write_cannot_alias_the_versioned_visible_text_operations() {
    let registry = registry_with_dom_write_granted();
    let (mut client, mut server) = UnixStream::pair().unwrap();
    let handle = thread::spawn(move || {
        handle_extension_connection_with_actions(
            &registry,
            Path::new("/not-reached-for-legacy-visible-text.sock"),
            &mut server,
            |_| Ok("unused".to_string()),
            |_, _, _, _| panic!("legacy mutation must not reach a visible text delegate"),
            || Ok(()),
        )
    });
    write_extension_request(
        &mut client,
        &hello_with_capabilities(MINIMAL_SLICE_EXTENSION_ID, [(CAPABILITY_DOM_WRITE, 9)]),
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut client).unwrap(),
        empty_hello_ack()
    );
    for target in [
        DomWriteTarget::VisibleTextLeaf,
        DomWriteTarget::VisibleTextContent,
    ] {
        write_extension_request(
            &mut client,
            &ExtensionRequest::DomWrite {
                value: "would bypass the explicit node ID".to_string(),
                target,
            },
        )
        .unwrap();
        assert!(matches!(
            read_extension_reply(&mut client).unwrap(),
            ExtensionReply::OperationUnavailable { capability, reason }
                if capability == CAPABILITY_DOM_WRITE && reason.contains("explicit versioned")
        ));
    }
    drop(client);
    handle.join().unwrap().unwrap();
}
