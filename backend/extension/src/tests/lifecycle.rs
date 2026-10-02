// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn v8_visible_leaf_text_rejection_never_reaches_core() {
    let registry = registry_with_dom_write_granted();
    let (gatekeeper_socket, gatekeeper) = start_gatekeeper(
        "block-v8-visible-leaf",
        GatekeeperReply::Rejected {
            reason: "unsafe text".to_string(),
            category: "extension-visible-text-social-engineering".to_string(),
        },
    );
    let (mut client, mut server) = UnixStream::pair().unwrap();
    let socket_for_handler = gatekeeper_socket.clone();
    let handle = thread::spawn(move || {
        handle_extension_connection_with_actions(
            &registry,
            &socket_for_handler,
            &mut server,
            |_| Ok("unused".to_string()),
            |_, _, _, _| panic!("a gatekeeper rejection must not mutate core"),
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
            value: "Enter your password".to_string(),
        },
    )
    .unwrap();
    assert!(
        matches!(read_extension_reply(&mut client).unwrap(), ExtensionReply::GatekeeperBlocked { category, .. } if category == "extension-visible-text-social-engineering")
    );
    drop(client);
    handle.join().unwrap().unwrap();
    assert!(
        matches!(gatekeeper.join().unwrap(), GatekeeperRequest::CheckExtensionAction { detail, .. } if detail.contains("Enter your password"))
    );
    let _ = std::fs::remove_file(gatekeeper_socket);
}

#[test]
fn a_core_action_delegate_failure_is_reported_instead_of_acknowledged() {
    let registry = ExtensionRegistry::minimal_slice();
    let (mut client, mut server) = UnixStream::pair().unwrap();
    let handle = thread::spawn(move || {
        handle_extension_connection_with_actions(
            &registry,
            Path::new("/not-used-by-dom-read"),
            &mut server,
            |_| Err("the core session has ended".to_string()),
            |_, _, _, _| Ok(()),
            || Ok(()),
        )
    });

    write_extension_request(&mut client, &hello(MINIMAL_SLICE_EXTENSION_ID)).unwrap();
    assert_eq!(
        read_extension_reply(&mut client).unwrap(),
        empty_hello_ack()
    );
    write_extension_request(&mut client, &ExtensionRequest::DomRead).unwrap();
    assert_eq!(
        read_extension_reply(&mut client).unwrap(),
        ExtensionReply::OperationUnavailable {
            capability: CAPABILITY_DOM_READ.to_string(),
            reason: "the core session has ended".to_string(),
        }
    );

    drop(client);
    handle.join().unwrap().unwrap();
}

#[test]
fn v5_path_prefix_is_reviewed_then_delegated_without_exposing_guest_fields() {
    let registry = registry_with_network_intercept_granted();
    let (gatekeeper_socket, gatekeeper) =
        start_gatekeeper("clear-path-prefix-block", GatekeeperReply::Cleared);
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
                |_, _| panic!("a path request must not register an exact URL"),
                || Ok(()),
            )
            .with_network_block_path_prefix(move |host, path_prefix, _generation| {
                seen_tx.send((host, path_prefix)).unwrap();
                Ok(())
            }),
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
        &ExtensionRequest::RegisterNetworkBlockPathPrefix {
            host: "Example.test".into(),
            path_prefix: "/private".into(),
        },
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut client).unwrap(),
        ExtensionReply::NetworkInterceptAck
    );
    assert_eq!(
        seen_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
        ("Example.test".to_string(), "/private".to_string())
    );
    drop(client);
    handle.join().unwrap().unwrap();
    assert_eq!(
        gatekeeper.join().unwrap(),
        GatekeeperRequest::CheckExtensionAction {
            extension_id: MINIMAL_SLICE_EXTENSION_ID.to_string(),
            capability: CAPABILITY_NETWORK_INTERCEPT.to_string(),
            detail: "action=register-path-prefix-navigation-block".to_string(),
        }
    );
    let _ = std::fs::remove_file(gatekeeper_socket);
}

#[test]
fn core_spawned_mode_requires_the_one_time_credential_before_acknowledging() {
    let registry = ExtensionRegistry::minimal_slice();
    let (mut client, mut server) = UnixStream::pair().unwrap();
    let expected = "a-core-generated-credential".to_string();
    let handle = thread::spawn(move || {
        handle_extension_connection_with_actions_and_authentication(
            &registry,
            Path::new("/not-used-before-a-dom-action.sock"),
            &mut server,
            ExtensionConnectionAuthentication::required(&expected),
            |_| Ok(PLACEHOLDER_DOM_READ_VALUE.to_string()),
            |_, _, _, _| Ok(()),
            || Ok(()),
        )
    });

    // A package-derived identity by itself is intentionally not
    // credentials in host-spawned mode.
    write_extension_request(&mut client, &hello(MINIMAL_SLICE_EXTENSION_ID)).unwrap();
    handle.join().unwrap().unwrap();
    assert!(read_extension_reply(&mut client).is_err());
}
