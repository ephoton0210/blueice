// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn a_generic_dom_write_skips_review_but_a_missing_reviewer_blocks_network_causing_writes() {
    let registry = registry_with_dom_write_granted();
    let unavailable_socket = unique_gatekeeper_socket("unavailable");
    let _ = std::fs::remove_file(&unavailable_socket);
    let (mut client, mut server) = UnixStream::pair().unwrap();
    let socket_for_handler = unavailable_socket.clone();
    let handle = thread::spawn(move || {
        handle_extension_connection_with_gatekeeper(&registry, &socket_for_handler, &mut server)
    });

    write_extension_request(&mut client, &hello(MINIMAL_SLICE_EXTENSION_ID)).unwrap();
    assert_eq!(
        read_extension_reply(&mut client).unwrap(),
        empty_hello_ack()
    );

    // A non-triggering document write stays local and does not need
    // a live gatekeeper connection.
    write_extension_request(
        &mut client,
        &ExtensionRequest::DomWrite {
            value: "cosmetic text".to_string(),
            target: DomWriteTarget::Document,
        },
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut client).unwrap(),
        ExtensionReply::DomWriteAck
    );

    write_extension_request(
        &mut client,
        &ExtensionRequest::DomWrite {
            value: "submit".to_string(),
            target: DomWriteTarget::NetworkCausing {
                action: "form-submit".to_string(),
            },
        },
    )
    .unwrap();
    match read_extension_reply(&mut client).unwrap() {
        ExtensionReply::GatekeeperBlocked {
            capability,
            reason,
            category,
        } => {
            assert_eq!(capability, CAPABILITY_DOM_WRITE);
            assert_eq!(category, "gatekeeper-unavailable");
            assert!(reason.contains("could not connect"));
        }
        other => panic!("expected fail-closed GatekeeperBlocked, got {other:?}"),
    }

    drop(client);
    handle.join().unwrap().unwrap();
}

#[test]
fn a_network_intercept_registration_is_reviewed_before_it_is_acknowledged() {
    let registry = registry_with_network_intercept_granted();
    let (gatekeeper_socket, gatekeeper) =
        start_gatekeeper("clear-network-intercept", GatekeeperReply::Cleared);
    let (mut client, mut server) = UnixStream::pair().unwrap();
    let socket_for_handler = gatekeeper_socket.clone();
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
    assert_eq!(
        read_extension_reply(&mut client).unwrap(),
        ExtensionReply::NetworkInterceptAck
    );

    drop(client);
    handle.join().unwrap().unwrap();
    assert_eq!(
        gatekeeper.join().unwrap(),
        GatekeeperRequest::CheckExtensionAction {
            extension_id: MINIMAL_SLICE_EXTENSION_ID.to_string(),
            capability: CAPABILITY_NETWORK_INTERCEPT.to_string(),
            detail: "action=register-intercept".to_string(),
        }
    );
    let _ = std::fs::remove_file(gatekeeper_socket);
}

#[test]
fn v2_network_block_url_is_reviewed_then_delegated_without_exposing_the_url_to_review() {
    let registry = registry_with_network_intercept_granted();
    let (gatekeeper_socket, gatekeeper) =
        start_gatekeeper("clear-exact-navigation-block", GatekeeperReply::Cleared);
    let (seen_tx, seen_rx) = std::sync::mpsc::channel();
    let (mut client, mut server) = UnixStream::pair().unwrap();
    let socket_for_handler = gatekeeper_socket.clone();
    let handle = thread::spawn(move || {
        handle_extension_connection_with_actions_and_authentication_and_network_rules(
            &registry,
            &socket_for_handler,
            &mut server,
            ExtensionConnectionAuthentication::unauthenticated(),
            ExtensionActionDelegates::new(
                |_| Ok("unused in this test".to_string()),
                unused_write_delegate,
                || Ok(()),
                move |url, _generation| {
                    seen_tx.send(url).unwrap();
                    Ok(())
                },
                || panic!("a v2 request must not clear v3 rules"),
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
    write_extension_request(
        &mut client,
        &ExtensionRequest::RegisterNetworkBlockUrl {
            url: "https://example.test/private".to_string(),
        },
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut client).unwrap(),
        ExtensionReply::NetworkInterceptAck
    );
    assert_eq!(
        seen_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
        "https://example.test/private"
    );

    drop(client);
    handle.join().unwrap().unwrap();
    assert_eq!(
        gatekeeper.join().unwrap(),
        GatekeeperRequest::CheckExtensionAction {
            extension_id: MINIMAL_SLICE_EXTENSION_ID.to_string(),
            capability: CAPABILITY_NETWORK_INTERCEPT.to_string(),
            detail: "action=register-exact-navigation-block".to_string(),
        }
    );
    let _ = std::fs::remove_file(gatekeeper_socket);
}

#[test]
fn v4_network_block_host_is_reviewed_then_delegated_without_exposing_the_host() {
    let registry = registry_with_network_intercept_granted();
    let (gatekeeper_socket, gatekeeper) =
        start_gatekeeper("clear-host-navigation-block", GatekeeperReply::Cleared);
    let (seen_tx, seen_rx) = std::sync::mpsc::channel();
    let (mut client, mut server) = UnixStream::pair().unwrap();
    let socket_for_handler = gatekeeper_socket.clone();
    let handle = thread::spawn(move || {
        handle_extension_connection_with_actions_and_authentication_and_network_rules(
            &registry,
            &socket_for_handler,
            &mut server,
            ExtensionConnectionAuthentication::unauthenticated(),
            ExtensionActionDelegates::new(
                |_| Ok("unused in this test".to_string()),
                unused_write_delegate,
                || Ok(()),
                |_, _| panic!("a host request must not register an exact URL"),
                || Ok(()),
            )
            .with_network_block_host(move |host, _generation| {
                seen_tx.send(host).unwrap();
                Ok(())
            }),
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
        &ExtensionRequest::RegisterNetworkBlockHost {
            host: "Example.test".to_string(),
        },
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut client).unwrap(),
        ExtensionReply::NetworkInterceptAck
    );
    assert_eq!(
        seen_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
        "Example.test"
    );

    drop(client);
    handle.join().unwrap().unwrap();
    assert_eq!(
        gatekeeper.join().unwrap(),
        GatekeeperRequest::CheckExtensionAction {
            extension_id: MINIMAL_SLICE_EXTENSION_ID.to_string(),
            capability: CAPABILITY_NETWORK_INTERCEPT.to_string(),
            detail: "action=register-host-navigation-block".to_string(),
        }
    );
    let _ = std::fs::remove_file(gatekeeper_socket);
}

#[test]
fn v6_same_origin_redirect_is_reviewed_with_fixed_metadata_before_core() {
    let registry = registry_with_network_intercept_granted();
    let (gatekeeper_socket, gatekeeper) =
        start_gatekeeper("clear-navigation-redirect", GatekeeperReply::Cleared);
    let (seen_tx, seen_rx) = std::sync::mpsc::channel();
    let (mut client, mut server) = UnixStream::pair().unwrap();
    let socket_for_handler = gatekeeper_socket.clone();
    let handle = thread::spawn(move || {
        handle_extension_connection_with_actions_and_authentication_and_network_rules(
            &registry,
            &socket_for_handler,
            &mut server,
            ExtensionConnectionAuthentication::unauthenticated(),
            ExtensionActionDelegates::new(
                |_| Ok("unused".into()),
                unused_write_delegate,
                || Ok(()),
                |_, _| Ok(()),
                || Ok(()),
            )
            .with_network_redirect_url(move |source_url, target_url, _generation| {
                seen_tx.send((source_url, target_url)).unwrap();
                Ok(())
            }),
        )
    });
    write_extension_request(
        &mut client,
        &hello_with_capabilities(
            MINIMAL_SLICE_EXTENSION_ID,
            [(CAPABILITY_NETWORK_INTERCEPT, 6)],
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
    assert_eq!(
        read_extension_reply(&mut client).unwrap(),
        ExtensionReply::NetworkInterceptAck
    );
    assert_eq!(
        seen_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
        (
            "https://example.test/old".to_string(),
            "https://example.test/new".to_string()
        )
    );
    drop(client);
    handle.join().unwrap().unwrap();
    assert_eq!(
        gatekeeper.join().unwrap(),
        GatekeeperRequest::CheckExtensionAction {
            extension_id: MINIMAL_SLICE_EXTENSION_ID.to_string(),
            capability: CAPABILITY_NETWORK_INTERCEPT.to_string(),
            detail: "action=register-same-origin-navigation-redirect".to_string(),
        }
    );
    let _ = std::fs::remove_file(gatekeeper_socket);
}

#[test]
fn v3_network_rule_clear_is_delegated_without_gatekeeper_review() {
    let registry = registry_with_network_intercept_granted();
    let (cleared_tx, cleared_rx) = std::sync::mpsc::channel();
    let (mut client, mut server) = UnixStream::pair().unwrap();
    let handle = thread::spawn(move || {
        handle_extension_connection_with_actions_and_authentication_and_network_rules(
            &registry,
            Path::new("/not-reached-for-safe-network-rule-clear.sock"),
            &mut server,
            ExtensionConnectionAuthentication::unauthenticated(),
            ExtensionActionDelegates::new(
                |_| Ok("unused in this test".to_string()),
                unused_write_delegate,
                || Ok(()),
                |_, _| panic!("a v3 clear must not register a rule"),
                move || {
                    cleared_tx.send(()).unwrap();
                    Ok(())
                },
            ),
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
    write_extension_request(&mut client, &ExtensionRequest::ClearNetworkBlockUrls).unwrap();
    assert_eq!(
        read_extension_reply(&mut client).unwrap(),
        ExtensionReply::NetworkInterceptAck
    );
    cleared_rx.recv_timeout(Duration::from_secs(1)).unwrap();

    drop(client);
    handle.join().unwrap().unwrap();
}

#[test]
fn oversized_v2_network_block_url_is_rejected_before_review_or_delegate() {
    let registry = registry_with_network_intercept_granted();
    let (mut client, mut server) = UnixStream::pair().unwrap();
    let handle = thread::spawn(move || {
        handle_extension_connection_with_actions_and_authentication_and_network_rules(
            &registry,
            Path::new("/not-reached-for-oversized-network-rule.sock"),
            &mut server,
            ExtensionConnectionAuthentication::unauthenticated(),
            ExtensionActionDelegates::new(
                |_| Ok("unused in this test".to_string()),
                unused_write_delegate,
                || Ok(()),
                |_, _| panic!("an oversized network rule must not reach core"),
                || panic!("an oversized network rule must not clear v3 rules"),
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
    write_extension_request(
        &mut client,
        &ExtensionRequest::RegisterNetworkBlockUrl {
            url: "x".repeat(blueice_ipc::extension::MAX_NETWORK_BLOCK_URL_BYTES + 1),
        },
    )
    .unwrap();
    match read_extension_reply(&mut client).unwrap() {
        ExtensionReply::OperationUnavailable { capability, reason } => {
            assert_eq!(capability, CAPABILITY_NETWORK_INTERCEPT);
            assert!(reason.contains("2048 bytes"));
        }
        other => panic!("expected an oversized-rule rejection, got {other:?}"),
    }

    drop(client);
    handle.join().unwrap().unwrap();
}
