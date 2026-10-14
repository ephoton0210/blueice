// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn a_granted_capability_must_still_be_declared_and_version_negotiated() {
    let registry = ExtensionRegistry::minimal_slice();
    let (mut client, mut server) = UnixStream::pair().unwrap();
    let handle = thread::spawn(move || handle_extension_connection(&registry, &mut server));

    write_extension_request(
        &mut client,
        &hello_with_capabilities(MINIMAL_SLICE_EXTENSION_ID, [(CAPABILITY_DOM_WRITE, 1)]),
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut client).unwrap(),
        empty_hello_ack()
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
fn an_ungranted_dom_write_is_denied_with_the_capability_and_a_reason() {
    let registry = ExtensionRegistry::minimal_slice();
    let (mut client, mut server) = UnixStream::pair().unwrap();
    let handle = thread::spawn(move || handle_extension_connection(&registry, &mut server));

    write_extension_request(&mut client, &hello(MINIMAL_SLICE_EXTENSION_ID)).unwrap();
    assert_eq!(
        read_extension_reply(&mut client).unwrap(),
        empty_hello_ack()
    );

    write_extension_request(
        &mut client,
        &ExtensionRequest::DomWrite {
            value: "hijacked".to_string(),
            target: DomWriteTarget::Document,
        },
    )
    .unwrap();
    match read_extension_reply(&mut client).unwrap() {
        ExtensionReply::CapabilityDenied { capability, reason } => {
            assert_eq!(capability, CAPABILITY_DOM_WRITE);
            assert!(!reason.is_empty());
        }
        other => panic!("expected CapabilityDenied, got {other:?}"),
    }

    drop(client);
    handle.join().unwrap().unwrap();
}

#[test]
fn late_core_permission_failure_is_a_structured_capability_denial() {
    let mut wire = Vec::new();
    write_network_registration_reply(
        &mut wire,
        Err(grant_changed_reason(CAPABILITY_NETWORK_INTERCEPT)),
    )
    .unwrap();
    assert!(
        matches!(read_extension_reply(&mut std::io::Cursor::new(wire)).unwrap(),
        ExtensionReply::CapabilityDenied { capability, .. }
            if capability == CAPABILITY_NETWORK_INTERCEPT)
    );
}

#[test]
fn reviewed_network_rules_cannot_borrow_a_regranted_optional_permission() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let cases = [
        (
            ExtensionRequest::RegisterNetworkBlockUrl {
                url: "https://example.test/blocked".into(),
            },
            2,
        ),
        (
            ExtensionRequest::RegisterNetworkBlockHost {
                host: "example.test".into(),
            },
            4,
        ),
        (
            ExtensionRequest::RegisterNetworkBlockPathPrefix {
                host: "example.test".into(),
                path_prefix: "/blocked".into(),
            },
            5,
        ),
        (
            ExtensionRequest::RegisterNetworkRedirectUrl {
                source_url: "https://example.test/old".into(),
                target_url: "https://example.test/new".into(),
            },
            6,
        ),
    ];
    for (request, version) in cases {
        let mut registry = ExtensionRegistry::with_supported_capabilities();
        registry.declare_optional(MINIMAL_SLICE_EXTENSION_ID, CAPABILITY_NETWORK_INTERCEPT);
        assert!(registry
            .grant_optional(MINIMAL_SLICE_EXTENSION_ID, CAPABILITY_NETWORK_INTERCEPT)
            .unwrap());
        let registry = Arc::new(registry);
        let gatekeeper_socket = unique_gatekeeper_socket("optional-network-race");
        let _ = std::fs::remove_file(&gatekeeper_socket);
        let listener = UnixListener::bind(&gatekeeper_socket).unwrap();
        let (reviewed_tx, reviewed_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let gatekeeper = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let reviewed = read_gatekeeper_request(&mut stream).unwrap();
            reviewed_tx.send(reviewed).unwrap();
            release_rx.recv_timeout(Duration::from_secs(2)).unwrap();
            write_gatekeeper_reply(&mut stream, &GatekeeperReply::Cleared).unwrap();
        });
        let invoked = Arc::new(AtomicUsize::new(0));
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let handler_registry = Arc::clone(&registry);
        let handler_socket = gatekeeper_socket.clone();
        let called_url = Arc::clone(&invoked);
        let called_host = Arc::clone(&invoked);
        let called_path = Arc::clone(&invoked);
        let called_redirect = Arc::clone(&invoked);
        let handler = thread::spawn(move || {
            handle_extension_connection_with_actions_and_authentication_and_network_rules(
                &handler_registry,
                &handler_socket,
                &mut server,
                ExtensionConnectionAuthentication::unauthenticated(),
                ExtensionActionDelegates::new(
                    |_| Ok(String::new()),
                    unused_write_delegate,
                    || Ok(()),
                    move |_, _| {
                        called_url.fetch_add(1, Ordering::SeqCst);
                        Ok(())
                    },
                    || Ok(()),
                )
                .with_network_block_host(move |_, _| {
                    called_host.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                })
                .with_network_block_path_prefix(move |_, _, _| {
                    called_path.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                })
                .with_network_redirect_url(move |_, _, _| {
                    called_redirect.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                }),
            )
        });
        write_extension_request(
            &mut client,
            &hello_with_capabilities(
                MINIMAL_SLICE_EXTENSION_ID,
                [(CAPABILITY_NETWORK_INTERCEPT, version)],
            ),
        )
        .unwrap();
        assert_eq!(
            read_extension_reply(&mut client).unwrap(),
            empty_hello_ack()
        );
        write_extension_request(&mut client, &request).unwrap();
        assert!(matches!(
            reviewed_rx.recv_timeout(Duration::from_secs(2)).unwrap(),
            GatekeeperRequest::CheckExtensionAction { .. }
        ));
        assert!(registry
            .revoke_optional(MINIMAL_SLICE_EXTENSION_ID, CAPABILITY_NETWORK_INTERCEPT)
            .unwrap());
        assert!(registry
            .grant_optional(MINIMAL_SLICE_EXTENSION_ID, CAPABILITY_NETWORK_INTERCEPT)
            .unwrap());
        release_tx.send(()).unwrap();
        assert!(matches!(read_extension_reply(&mut client).unwrap(),
            ExtensionReply::CapabilityDenied { capability, .. } if capability == CAPABILITY_NETWORK_INTERCEPT));
        assert_eq!(invoked.load(Ordering::SeqCst), 0);
        drop(client);
        handler.join().unwrap().unwrap();
        gatekeeper.join().unwrap();
        let _ = std::fs::remove_file(gatekeeper_socket);
    }
}

#[test]
fn reviewed_dom_writes_cannot_borrow_a_regranted_optional_permission() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    for (request, version) in [
        (
            ExtensionRequest::SetTextInputValue {
                tab_id: 1,
                node_id: 2,
                value: "new value".into(),
            },
            2,
        ),
        (
            ExtensionRequest::SetVisibleTextContent {
                tab_id: 1,
                node_id: 2,
                value: "New heading".into(),
            },
            9,
        ),
    ] {
        let mut registry = ExtensionRegistry::with_supported_capabilities();
        registry.declare_optional(MINIMAL_SLICE_EXTENSION_ID, CAPABILITY_DOM_WRITE);
        registry
            .grant_optional(MINIMAL_SLICE_EXTENSION_ID, CAPABILITY_DOM_WRITE)
            .unwrap();
        let registry = Arc::new(registry);
        let socket = unique_gatekeeper_socket("optional-dom-write-race");
        let _ = std::fs::remove_file(&socket);
        let listener = UnixListener::bind(&socket).unwrap();
        let (reviewed_tx, reviewed_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let gatekeeper = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            reviewed_tx
                .send(read_gatekeeper_request(&mut stream).unwrap())
                .unwrap();
            release_rx.recv_timeout(Duration::from_secs(2)).unwrap();
            write_gatekeeper_reply(&mut stream, &GatekeeperReply::Cleared).unwrap();
        });
        let invoked = Arc::new(AtomicUsize::new(0));
        let invoked_by_handler = Arc::clone(&invoked);
        let handler_registry = Arc::clone(&registry);
        let handler_socket = socket.clone();
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let handler = thread::spawn(move || {
            handle_extension_connection_with_actions(
                &handler_registry,
                &handler_socket,
                &mut server,
                |_| Ok(String::new()),
                move |_, _, _, _| {
                    invoked_by_handler.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                },
                || Ok(()),
            )
        });
        write_extension_request(
            &mut client,
            &hello_with_capabilities(
                MINIMAL_SLICE_EXTENSION_ID,
                [(CAPABILITY_DOM_WRITE, version)],
            ),
        )
        .unwrap();
        assert_eq!(
            read_extension_reply(&mut client).unwrap(),
            empty_hello_ack()
        );
        write_extension_request(&mut client, &request).unwrap();
        assert!(
            matches!(reviewed_rx.recv_timeout(Duration::from_secs(2)).unwrap(),
            GatekeeperRequest::CheckExtensionAction { capability, .. }
                if capability == CAPABILITY_DOM_WRITE)
        );
        registry
            .revoke_optional(MINIMAL_SLICE_EXTENSION_ID, CAPABILITY_DOM_WRITE)
            .unwrap();
        registry
            .grant_optional(MINIMAL_SLICE_EXTENSION_ID, CAPABILITY_DOM_WRITE)
            .unwrap();
        release_tx.send(()).unwrap();
        assert!(matches!(read_extension_reply(&mut client).unwrap(),
            ExtensionReply::CapabilityDenied { capability, reason }
                if capability == CAPABILITY_DOM_WRITE && reason.contains("grant changed")));
        assert_eq!(invoked.load(Ordering::SeqCst), 0);
        drop(client);
        handler.join().unwrap().unwrap();
        gatekeeper.join().unwrap();
        let _ = std::fs::remove_file(socket);
    }
}

#[test]
fn an_ungranted_network_intercept_never_reaches_an_unavailable_gatekeeper() {
    let registry = ExtensionRegistry::minimal_slice();
    let unavailable_socket = unique_gatekeeper_socket("ungranted-network-intercept");
    let _ = std::fs::remove_file(&unavailable_socket);
    let (mut client, mut server) = UnixStream::pair().unwrap();
    let socket_for_handler = unavailable_socket.clone();
    let handle = thread::spawn(move || {
        handle_extension_connection_with_gatekeeper(&registry, &socket_for_handler, &mut server)
    });

    write_extension_request(
        &mut client,
        &hello_with_capabilities(
            MINIMAL_SLICE_EXTENSION_ID,
            [(CAPABILITY_NETWORK_INTERCEPT, 1)],
        ),
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut client).unwrap(),
        empty_hello_ack()
    );
    write_extension_request(&mut client, &ExtensionRequest::NetworkIntercept).unwrap();
    match read_extension_reply(&mut client).unwrap() {
        ExtensionReply::CapabilityDenied { capability, reason } => {
            assert_eq!(capability, CAPABILITY_NETWORK_INTERCEPT);
            assert!(reason.contains("not granted"));
        }
        other => panic!("expected CapabilityDenied before gatekeeper review, got {other:?}"),
    }

    drop(client);
    handle.join().unwrap().unwrap();
}

#[test]
fn an_unrecognized_extension_id_gets_capability_denied_not_a_crash_or_a_free_pass() {
    let registry = ExtensionRegistry::minimal_slice();
    let (mut client, mut server) = UnixStream::pair().unwrap();
    let handle = thread::spawn(move || handle_extension_connection(&registry, &mut server));

    write_extension_request(&mut client, &hello("never-registered-extension")).unwrap();
    assert_eq!(
        read_extension_reply(&mut client).unwrap(),
        empty_hello_ack()
    );

    write_extension_request(&mut client, &ExtensionRequest::DomRead).unwrap();
    match read_extension_reply(&mut client).unwrap() {
        ExtensionReply::CapabilityDenied { capability, .. } => {
            assert_eq!(capability, CAPABILITY_DOM_READ)
        }
        other => panic!("expected CapabilityDenied, got {other:?}"),
    }

    drop(client);
    handle.join().unwrap().unwrap();
}

#[test]
fn queued_ui_activation_from_revoked_grant_never_reaches_the_guest() {
    let mut registry = ExtensionRegistry::with_supported_capabilities();
    registry.declare_optional(MINIMAL_SLICE_EXTENSION_ID, CAPABILITY_UI_INJECT);
    let registry = Arc::new(registry);
    registry
        .grant_optional(MINIMAL_SLICE_EXTENSION_ID, CAPABILITY_UI_INJECT)
        .unwrap();
    let old_generation = registry
        .capability_generation(MINIMAL_SLICE_EXTENSION_ID, CAPABILITY_UI_INJECT)
        .unwrap();
    let (mut client, mut server) = UnixStream::pair().unwrap();
    let (start_tx, start_rx) = mpsc::channel();
    let (event_tx, event_rx) = mpsc::channel();
    let handler_registry = Arc::clone(&registry);
    let handle = thread::spawn(move || {
        handle_extension_connection_with_actions_and_authentication(
            &handler_registry,
            Path::new("/not-used-before-an-action.sock"),
            &mut server,
            ExtensionConnectionAuthentication::required("core-secret")
                .with_runtime_start_receiver(Arc::new(Mutex::new(start_rx)))
                .with_runtime_event_receiver(Arc::new(Mutex::new(event_rx))),
            |_| Ok(String::new()),
            unused_write_delegate,
            || Ok(()),
        )
    });
    write_extension_request(
        &mut client,
        &ExtensionRequest::HelloAuthenticated {
            extension_id: MINIMAL_SLICE_EXTENSION_ID.into(),
            capability_versions: BTreeMap::from([(CAPABILITY_UI_INJECT.into(), 3)]),
            authentication: "core-secret".into(),
        },
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut client).unwrap(),
        empty_hello_ack()
    );
    write_extension_request(&mut client, &ExtensionRequest::RuntimeReady).unwrap();
    start_tx.send(()).unwrap();
    assert_eq!(
        read_extension_reply(&mut client).unwrap(),
        ExtensionReply::RuntimeStart
    );
    event_tx
        .send(ExtensionRuntimeEvent::ToolbarActivated {
            tab_id: 1,
            grant_generation: old_generation,
        })
        .unwrap();
    registry
        .revoke_optional(MINIMAL_SLICE_EXTENSION_ID, CAPABILITY_UI_INJECT)
        .unwrap();
    registry
        .grant_optional(MINIMAL_SLICE_EXTENSION_ID, CAPABILITY_UI_INJECT)
        .unwrap();
    let fresh_generation = registry
        .capability_generation(MINIMAL_SLICE_EXTENSION_ID, CAPABILITY_UI_INJECT)
        .unwrap();
    assert_ne!(old_generation, fresh_generation);
    event_tx
        .send(ExtensionRuntimeEvent::PopupActionActivated {
            tab_id: 1,
            grant_generation: fresh_generation,
        })
        .unwrap();
    write_extension_request(&mut client, &ExtensionRequest::NextRuntimeEvent).unwrap();
    assert_eq!(
        read_extension_reply(&mut client).unwrap(),
        ExtensionReply::RuntimeEvent(ExtensionRuntimeEvent::PopupActionActivated {
            tab_id: 1,
            grant_generation: fresh_generation
        })
    );
    drop(event_tx);
    write_extension_request(&mut client, &ExtensionRequest::NextRuntimeEvent).unwrap();
    assert_eq!(
        read_extension_reply(&mut client).unwrap(),
        ExtensionReply::RuntimeEventStreamClosed
    );
    drop(client);
    handle.join().unwrap().unwrap();
}
