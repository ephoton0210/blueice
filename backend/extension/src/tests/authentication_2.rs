// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn v3_network_rule_clear_requires_a_v3_handshake_before_its_delegate() {
    let registry = registry_with_network_intercept_granted();
    let (mut client, mut server) = UnixStream::pair().unwrap();
    let handle = thread::spawn(move || {
        handle_extension_connection_with_actions_and_authentication_and_network_rules(
            &registry,
            Path::new("/not-reached-for-v2-network-rule-clear-version-denial.sock"),
            &mut server,
            ExtensionConnectionAuthentication::unauthenticated(),
            ExtensionActionDelegates::new(
                |_| Ok("unused in this test".to_string()),
                unused_write_delegate,
                || Ok(()),
                |_, _| panic!("a v2 connection must not register a v2 rule in this test"),
                || panic!("a v2 connection must not clear v3 rules"),
            ),
        )
    });

    write_extension_request(
        &mut client,
        &hello_with_capabilities(
            MINIMAL_SLICE_EXTENSION_ID,
            [(CAPABILITY_NETWORK_INTERCEPT, 2)],
        ),
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut client).unwrap(),
        empty_hello_ack()
    );
    write_extension_request(&mut client, &ExtensionRequest::ClearNetworkBlockUrls).unwrap();
    match read_extension_reply(&mut client).unwrap() {
        ExtensionReply::CapabilityDenied { capability, reason } => {
            assert_eq!(capability, CAPABILITY_NETWORK_INTERCEPT);
            assert!(reason.contains("requires version 3"));
        }
        other => panic!("expected a v3 version denial, got {other:?}"),
    }

    drop(client);
    handle.join().unwrap().unwrap();
}

#[test]
fn v2_network_block_url_requires_a_v2_handshake_before_review_or_delegate() {
    let registry = registry_with_network_intercept_granted();
    let (mut client, mut server) = UnixStream::pair().unwrap();
    let handle = thread::spawn(move || {
        handle_extension_connection_with_actions_and_authentication_and_network_rules(
            &registry,
            Path::new("/not-reached-for-v1-network-block-version-denial.sock"),
            &mut server,
            ExtensionConnectionAuthentication::unauthenticated(),
            ExtensionActionDelegates::new(
                |_| Ok("unused in this test".to_string()),
                unused_write_delegate,
                || Ok(()),
                |_, _| panic!("a v1 connection must not install a v2 network rule"),
                || panic!("a v1 connection must not clear v3 rules"),
            ),
        )
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
    write_extension_request(
        &mut client,
        &ExtensionRequest::RegisterNetworkBlockUrl {
            url: "https://example.test/private".to_string(),
        },
    )
    .unwrap();
    match read_extension_reply(&mut client).unwrap() {
        ExtensionReply::CapabilityDenied { capability, reason } => {
            assert_eq!(capability, CAPABILITY_NETWORK_INTERCEPT);
            assert!(reason.contains("requires version 2"));
        }
        other => panic!("expected a v2 version denial, got {other:?}"),
    }

    drop(client);
    handle.join().unwrap().unwrap();
}

#[test]
fn a_denied_request_does_not_end_the_connection_a_later_granted_request_still_works() {
    // Proves denial is a per-request check, not a connection-ending
    // fault -- and, since this minimal slice has no real `Page`
    // state for `DomWrite` to touch, this is the closest available
    // proof that a rejected attempt has no side effect on later
    // requests either.
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
            value: "x".to_string(),
            target: DomWriteTarget::Document,
        },
    )
    .unwrap();
    assert!(matches!(
        read_extension_reply(&mut client).unwrap(),
        ExtensionReply::CapabilityDenied { .. }
    ));

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
fn a_non_hello_first_message_ends_the_connection_without_a_reply() {
    let registry = ExtensionRegistry::minimal_slice();
    let (mut client, mut server) = UnixStream::pair().unwrap();
    let handle = thread::spawn(move || handle_extension_connection(&registry, &mut server));

    write_extension_request(&mut client, &ExtensionRequest::DomRead).unwrap();

    handle.join().unwrap().unwrap();
    // No reply was ever written -- reading now must fail (EOF, since
    // the connection-handling thread already exited and dropped its
    // end of the socket), not hang or return a stray `HelloAck`.
    assert!(read_extension_reply(&mut client).is_err());
}

#[test]
fn core_spawned_mode_accepts_only_the_authenticated_hello_and_signals_readiness() {
    let registry = ExtensionRegistry::minimal_slice();
    let (mut client, mut server) = UnixStream::pair().unwrap();
    let expected = "a-core-generated-credential".to_string();
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let handle = thread::spawn(move || {
        handle_extension_connection_with_actions_and_authentication(
            &registry,
            Path::new("/not-used-before-a-dom-action.sock"),
            &mut server,
            ExtensionConnectionAuthentication::required(&expected)
                .with_ready_notification(ready_tx),
            |_| Ok(PLACEHOLDER_DOM_READ_VALUE.to_string()),
            |_, _, _, _| Ok(()),
            || Ok(()),
        )
    });

    write_extension_request(
        &mut client,
        &ExtensionRequest::HelloAuthenticated {
            extension_id: MINIMAL_SLICE_EXTENSION_ID.to_string(),
            capability_versions: BTreeMap::from([(CAPABILITY_DOM_READ.to_string(), 1)]),
            authentication: "a-core-generated-credential".to_string(),
        },
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut client).unwrap(),
        empty_hello_ack()
    );
    ready_rx.recv_timeout(Duration::from_secs(1)).unwrap();

    // A later identity change must also prove the connection credential;
    // accepting a plain repeat Hello would let a previously authenticated
    // stream silently fall back to the bearer-claim protocol.
    write_extension_request(&mut client, &hello(MINIMAL_SLICE_EXTENSION_ID)).unwrap();
    handle.join().unwrap().unwrap();
    assert!(read_extension_reply(&mut client).is_err());
}

#[test]
fn authenticated_runtime_waits_for_core_session_start_before_it_can_run() {
    let registry = ExtensionRegistry::minimal_slice();
    let (mut client, mut server) = UnixStream::pair().unwrap();
    let expected = "a-core-generated-runtime-credential".to_string();
    let (runtime_start_tx, runtime_start_rx) = std::sync::mpsc::channel();
    let runtime_start_rx = Arc::new(Mutex::new(runtime_start_rx));
    let (runtime_event_tx, runtime_event_rx) = std::sync::mpsc::channel();
    let runtime_event_rx = Arc::new(Mutex::new(runtime_event_rx));
    let handle = thread::spawn(move || {
        handle_extension_connection_with_actions_and_authentication(
            &registry,
            Path::new("/not-used-before-a-dom-action.sock"),
            &mut server,
            ExtensionConnectionAuthentication::required(&expected)
                .with_runtime_start_receiver(runtime_start_rx)
                .with_runtime_event_receiver(runtime_event_rx),
            |_| Ok(PLACEHOLDER_DOM_READ_VALUE.to_string()),
            |_, _, _, _| Ok(()),
            || Ok(()),
        )
    });

    write_extension_request(
        &mut client,
        &ExtensionRequest::HelloAuthenticated {
            extension_id: MINIMAL_SLICE_EXTENSION_ID.to_string(),
            capability_versions: BTreeMap::from([(CAPABILITY_DOM_READ.to_string(), 1)]),
            authentication: "a-core-generated-runtime-credential".to_string(),
        },
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut client).unwrap(),
        empty_hello_ack()
    );
    write_extension_request(&mut client, &ExtensionRequest::RuntimeReady).unwrap();

    // The handler is waiting at this point; only the core session's
    // explicit sender can release the host to execute guest code.
    runtime_start_tx.send(()).unwrap();
    assert_eq!(
        read_extension_reply(&mut client).unwrap(),
        ExtensionReply::RuntimeStart
    );

    write_extension_request(&mut client, &ExtensionRequest::NextRuntimeEvent).unwrap();
    runtime_event_tx
        .send(ExtensionRuntimeEvent::NavigationCommitted { tab_id: 17 })
        .unwrap();
    assert_eq!(
        read_extension_reply(&mut client).unwrap(),
        ExtensionReply::RuntimeEvent(ExtensionRuntimeEvent::NavigationCommitted { tab_id: 17 })
    );

    drop(client);
    handle.join().unwrap().unwrap();
}

#[test]
fn disconnecting_before_sending_anything_ends_the_connection_cleanly() {
    let registry = ExtensionRegistry::minimal_slice();
    let (client, mut server) = UnixStream::pair().unwrap();
    drop(client);
    assert!(handle_extension_connection(&registry, &mut server).is_ok());
}

#[test]
fn a_second_hello_is_answered_again_and_updates_which_identity_is_checked() {
    // Mirrors `run_session`'s own "a repeat Hello is just answered
    // again, not re-gated" handling on the external client protocol
    // -- applied here to a long-lived extension connection that
    // re-identifies mid-connection.
    let registry = ExtensionRegistry::minimal_slice();
    let (mut client, mut server) = UnixStream::pair().unwrap();
    let handle = thread::spawn(move || handle_extension_connection(&registry, &mut server));

    write_extension_request(&mut client, &hello("some-other-extension")).unwrap();
    assert_eq!(
        read_extension_reply(&mut client).unwrap(),
        empty_hello_ack()
    );

    // Not yet granted anything under this first identity.
    write_extension_request(&mut client, &ExtensionRequest::DomRead).unwrap();
    assert!(matches!(
        read_extension_reply(&mut client).unwrap(),
        ExtensionReply::CapabilityDenied { .. }
    ));

    // Re-identify as the granted extension on the same connection.
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
fn a_malformed_frame_after_handshake_ends_the_connection_rather_than_panicking() {
    let registry = ExtensionRegistry::minimal_slice();
    let (mut client, mut server) = UnixStream::pair().unwrap();
    let handle = thread::spawn(move || handle_extension_connection(&registry, &mut server));

    write_extension_request(&mut client, &hello(MINIMAL_SLICE_EXTENSION_ID)).unwrap();
    assert_eq!(
        read_extension_reply(&mut client).unwrap(),
        empty_hello_ack()
    );

    // A truncated frame: a length prefix promising more bytes than
    // are ever sent.
    client.write_all(&100u32.to_le_bytes()).unwrap();
    client.write_all(b"short").unwrap();
    drop(client);

    // Must return cleanly (not panic, not hang) -- see this fn's own
    // docs for why a malformed frame is treated the same as a plain
    // disconnect rather than propagated as a distinguishable error.
    handle.join().unwrap().unwrap();
}

#[test]
fn handles_two_connections_in_sequence() {
    // Proves the accept-loop-friendly shape (one call handles
    // exactly one connection, start to end, and returns) that
    // `src/main.rs`'s sequential accept loop depends on.
    for _ in 0..2 {
        let registry = ExtensionRegistry::minimal_slice();
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let handle = thread::spawn(move || handle_extension_connection(&registry, &mut server));
        write_extension_request(&mut client, &hello(MINIMAL_SLICE_EXTENSION_ID)).unwrap();
        assert_eq!(
            read_extension_reply(&mut client).unwrap(),
            empty_hello_ack()
        );
        drop(client);
        handle.join().unwrap().unwrap();
    }
}
