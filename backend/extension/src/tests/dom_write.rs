// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn v2_text_input_write_is_reviewed_and_delegated_with_its_explicit_ids() {
    let registry = registry_with_dom_write_granted();
    let (gatekeeper_socket, gatekeeper) =
        start_gatekeeper("clear-v2-text-input", GatekeeperReply::Cleared);
    let (seen_tx, seen_rx) = std::sync::mpsc::channel();
    let (mut client, mut server) = UnixStream::pair().unwrap();
    let socket_for_handler = gatekeeper_socket.clone();
    let handle = thread::spawn(move || {
        handle_extension_connection_with_actions(
            &registry,
            &socket_for_handler,
            &mut server,
            |_| Ok("unused in this test".to_string()),
            move |target, value, _, _| {
                seen_tx.send((target, value)).unwrap();
                Ok(())
            },
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
        &ExtensionRequest::SetTextInputValue {
            tab_id: 7,
            node_id: 11,
            value: "core-owned value".to_string(),
        },
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut client).unwrap(),
        ExtensionReply::DomWriteAck
    );
    assert_eq!(
        seen_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
        (Some((7, 11)), "core-owned value".to_string())
    );

    drop(client);
    handle.join().unwrap().unwrap();
    assert_eq!(
        gatekeeper.join().unwrap(),
        GatekeeperRequest::CheckExtensionAction {
            extension_id: MINIMAL_SLICE_EXTENSION_ID.to_string(),
            capability: CAPABILITY_DOM_WRITE.to_string(),
            detail: "action=set-text-input-value".to_string(),
        }
    );
    let _ = std::fs::remove_file(gatekeeper_socket);
}

#[test]
fn v2_text_input_write_a_gatekeeper_rejects_never_reaches_the_delegate() {
    let registry = registry_with_dom_write_granted();
    let (gatekeeper_socket, gatekeeper) = start_gatekeeper(
        "reject-v2-text-input",
        GatekeeperReply::Rejected {
            reason: "external input mutation needs confirmation".to_string(),
            category: "sensitive-extension-action".to_string(),
        },
    );
    let delegated = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let delegated_for_handler = std::sync::Arc::clone(&delegated);
    let (mut client, mut server) = UnixStream::pair().unwrap();
    let socket_for_handler = gatekeeper_socket.clone();
    let handle = thread::spawn(move || {
        handle_extension_connection_with_actions(
            &registry,
            &socket_for_handler,
            &mut server,
            |_| Ok("unused in this test".to_string()),
            move |_, _, _, _| {
                delegated_for_handler.store(true, std::sync::atomic::Ordering::SeqCst);
                Ok(())
            },
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
        &ExtensionRequest::SetTextInputValue {
            tab_id: 1,
            node_id: 2,
            value: "must not reach core".to_string(),
        },
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut client).unwrap(),
        ExtensionReply::GatekeeperBlocked {
            capability: CAPABILITY_DOM_WRITE.to_string(),
            reason: "external input mutation needs confirmation".to_string(),
            category: "sensitive-extension-action".to_string(),
        }
    );
    assert!(!delegated.load(std::sync::atomic::Ordering::SeqCst));

    drop(client);
    handle.join().unwrap().unwrap();
    assert_eq!(
        gatekeeper.join().unwrap(),
        GatekeeperRequest::CheckExtensionAction {
            extension_id: MINIMAL_SLICE_EXTENSION_ID.to_string(),
            capability: CAPABILITY_DOM_WRITE.to_string(),
            detail: "action=set-text-input-value".to_string(),
        }
    );
    let _ = std::fs::remove_file(gatekeeper_socket);
}

#[test]
fn v3_checkbox_write_is_reviewed_and_delegated_with_explicit_ids() {
    let registry = registry_with_dom_write_granted();
    let (gatekeeper_socket, gatekeeper) =
        start_gatekeeper("clear-v3-checkbox", GatekeeperReply::Cleared);
    let (seen_tx, seen_rx) = std::sync::mpsc::channel();
    let (mut client, mut server) = UnixStream::pair().unwrap();
    let socket_for_handler = gatekeeper_socket.clone();
    let handle = thread::spawn(move || {
        handle_extension_connection_with_actions(
            &registry,
            &socket_for_handler,
            &mut server,
            |_| Ok("unused in this test".to_string()),
            move |target, value, write_target, _| {
                seen_tx.send((target, value, write_target.clone())).unwrap();
                Ok(())
            },
            || Ok(()),
        )
    });

    write_extension_request(
        &mut client,
        &hello_with_capabilities(MINIMAL_SLICE_EXTENSION_ID, [(CAPABILITY_DOM_WRITE, 3)]),
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut client).unwrap(),
        empty_hello_ack()
    );
    write_extension_request(
        &mut client,
        &ExtensionRequest::SetCheckboxChecked {
            tab_id: 7,
            node_id: 12,
            checked: true,
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
            Some((7, 12)),
            "true".to_string(),
            DomWriteTarget::FormInput {
                input_type: "checkbox".to_string()
            }
        )
    );

    drop(client);
    handle.join().unwrap().unwrap();
    assert_eq!(
        gatekeeper.join().unwrap(),
        GatekeeperRequest::CheckExtensionAction {
            extension_id: MINIMAL_SLICE_EXTENSION_ID.to_string(),
            capability: CAPABILITY_DOM_WRITE.to_string(),
            detail: "action=set-checkbox-checked".to_string(),
        }
    );
    let _ = std::fs::remove_file(gatekeeper_socket);
}

#[test]
fn v5_radio_selection_is_reviewed_and_delegated_without_group_metadata() {
    let registry = registry_with_dom_write_granted();
    let (gatekeeper_socket, gatekeeper) =
        start_gatekeeper("clear-v5-radio", GatekeeperReply::Cleared);
    let (seen_tx, seen_rx) = std::sync::mpsc::channel();
    let (mut client, mut server) = UnixStream::pair().unwrap();
    let socket_for_handler = gatekeeper_socket.clone();
    let handle = thread::spawn(move || {
        handle_extension_connection_with_actions(
            &registry,
            &socket_for_handler,
            &mut server,
            |_| Ok("unused in this test".to_string()),
            move |target, value, write_target, _| {
                seen_tx.send((target, value, write_target.clone())).unwrap();
                Ok(())
            },
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
        &ExtensionRequest::SetRadioChecked {
            tab_id: 7,
            node_id: 15,
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
            Some((7, 15)),
            "true".to_string(),
            DomWriteTarget::FormInput {
                input_type: "radio".to_string()
            }
        )
    );

    drop(client);
    handle.join().unwrap().unwrap();
    assert_eq!(
        gatekeeper.join().unwrap(),
        GatekeeperRequest::CheckExtensionAction {
            extension_id: MINIMAL_SLICE_EXTENSION_ID.to_string(),
            capability: CAPABILITY_DOM_WRITE.to_string(),
            detail: "action=set-radio-checked".to_string(),
        }
    );
    let _ = std::fs::remove_file(gatekeeper_socket);
}

#[test]
fn v7_range_write_is_reviewed_and_delegated_without_range_metadata() {
    let registry = registry_with_dom_write_granted();
    let (gatekeeper_socket, gatekeeper) =
        start_gatekeeper("clear-v7-range", GatekeeperReply::Cleared);
    let (seen_tx, seen_rx) = std::sync::mpsc::channel();
    let (mut client, mut server) = UnixStream::pair().unwrap();
    let socket_for_handler = gatekeeper_socket.clone();
    let handle = thread::spawn(move || {
        handle_extension_connection_with_actions(
            &registry,
            &socket_for_handler,
            &mut server,
            |_| Ok("unused in this test".to_string()),
            move |target, value, write_target, _| {
                seen_tx.send((target, value, write_target.clone())).unwrap();
                Ok(())
            },
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
        &ExtensionRequest::SetRangeInputValue {
            tab_id: 7,
            node_id: 17,
            value: -3,
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
            Some((7, 17)),
            "-3".to_string(),
            DomWriteTarget::FormInput {
                input_type: "range".to_string()
            }
        )
    );

    drop(client);
    handle.join().unwrap().unwrap();
    assert_eq!(
        gatekeeper.join().unwrap(),
        GatekeeperRequest::CheckExtensionAction {
            extension_id: MINIMAL_SLICE_EXTENSION_ID.to_string(),
            capability: CAPABILITY_DOM_WRITE.to_string(),
            detail: "action=set-range-input-value".to_string(),
        }
    );
    let _ = std::fs::remove_file(gatekeeper_socket);
}

#[test]
fn v6_select_option_is_reviewed_and_delegated_without_select_metadata() {
    let registry = registry_with_dom_write_granted();
    let (gatekeeper_socket, gatekeeper) =
        start_gatekeeper("clear-v6-select-option", GatekeeperReply::Cleared);
    let (seen_tx, seen_rx) = std::sync::mpsc::channel();
    let (mut client, mut server) = UnixStream::pair().unwrap();
    let socket_for_handler = gatekeeper_socket.clone();
    let handle = thread::spawn(move || {
        handle_extension_connection_with_actions(
            &registry,
            &socket_for_handler,
            &mut server,
            |_| Ok("unused in this test".to_string()),
            move |target, value, write_target, _| {
                seen_tx.send((target, value, write_target.clone())).unwrap();
                Ok(())
            },
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
        &ExtensionRequest::SelectOption {
            tab_id: 7,
            node_id: 16,
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
            Some((7, 16)),
            "true".to_string(),
            DomWriteTarget::FormInput {
                input_type: "select".to_string()
            }
        )
    );

    drop(client);
    handle.join().unwrap().unwrap();
    assert_eq!(
        gatekeeper.join().unwrap(),
        GatekeeperRequest::CheckExtensionAction {
            extension_id: MINIMAL_SLICE_EXTENSION_ID.to_string(),
            capability: CAPABILITY_DOM_WRITE.to_string(),
            detail: "action=select-option".to_string(),
        }
    );
    let _ = std::fs::remove_file(gatekeeper_socket);
}

#[test]
fn v4_textarea_write_is_reviewed_and_delegated_with_explicit_ids() {
    let registry = registry_with_dom_write_granted();
    let (gatekeeper_socket, gatekeeper) =
        start_gatekeeper("clear-v4-textarea", GatekeeperReply::Cleared);
    let (seen_tx, seen_rx) = std::sync::mpsc::channel();
    let (mut client, mut server) = UnixStream::pair().unwrap();
    let socket_for_handler = gatekeeper_socket.clone();
    let handle = thread::spawn(move || {
        handle_extension_connection_with_actions(
            &registry,
            &socket_for_handler,
            &mut server,
            |_| Ok("unused in this test".to_string()),
            move |target, value, write_target, _| {
                seen_tx.send((target, value, write_target.clone())).unwrap();
                Ok(())
            },
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
        &ExtensionRequest::SetTextareaValue {
            tab_id: 7,
            node_id: 14,
            value: "core-owned\nnotes".to_string(),
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
            Some((7, 14)),
            "core-owned\nnotes".to_string(),
            DomWriteTarget::FormInput {
                input_type: "textarea".to_string()
            }
        )
    );

    drop(client);
    handle.join().unwrap().unwrap();
    assert_eq!(
        gatekeeper.join().unwrap(),
        GatekeeperRequest::CheckExtensionAction {
            extension_id: MINIMAL_SLICE_EXTENSION_ID.to_string(),
            capability: CAPABILITY_DOM_WRITE.to_string(),
            detail: "action=set-textarea-value".to_string(),
        }
    );
    let _ = std::fs::remove_file(gatekeeper_socket);
}

#[test]
fn oversized_v2_and_v4_text_writes_are_rejected_before_review_or_delegation() {
    let registry = registry_with_dom_write_granted();
    let (mut client, mut server) = UnixStream::pair().unwrap();
    let handle = thread::spawn(move || {
        handle_extension_connection_with_actions(
            &registry,
            Path::new("/not-reached-for-oversized-text-writes.sock"),
            &mut server,
            |_| Ok("unused in this test".to_string()),
            |_, _, _, _| panic!("an oversized write must not reach core"),
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
    let oversized = "x".repeat(blueice_ipc::extension::MAX_TEXT_WRITE_BYTES + 1);
    for request in [
        ExtensionRequest::SetTextInputValue {
            tab_id: 1,
            node_id: 2,
            value: oversized.clone(),
        },
        ExtensionRequest::SetTextareaValue {
            tab_id: 1,
            node_id: 3,
            value: oversized.clone(),
        },
    ] {
        write_extension_request(&mut client, &request).unwrap();
        match read_extension_reply(&mut client).unwrap() {
            ExtensionReply::OperationUnavailable { capability, reason } => {
                assert_eq!(capability, CAPABILITY_DOM_WRITE);
                assert!(reason.contains("4096 bytes"));
            }
            other => panic!("expected oversized value rejection, got {other:?}"),
        }
    }

    drop(client);
    handle.join().unwrap().unwrap();
}

#[test]
fn a_form_input_write_is_reviewed_and_a_gatekeeper_rejection_blocks_it() {
    let registry = registry_with_dom_write_granted();
    let (gatekeeper_socket, gatekeeper) = start_gatekeeper(
        "reject-form-input",
        GatekeeperReply::Rejected {
            reason: "credential-shaped field requires confirmation".to_string(),
            category: "sensitive-extension-action".to_string(),
        },
    );
    let (mut client, mut server) = UnixStream::pair().unwrap();
    let socket_for_handler = gatekeeper_socket.clone();
    let handle = thread::spawn(move || {
        handle_extension_connection_with_gatekeeper(&registry, &socket_for_handler, &mut server)
    });

    write_extension_request(&mut client, &hello(MINIMAL_SLICE_EXTENSION_ID)).unwrap();
    assert_eq!(
        read_extension_reply(&mut client).unwrap(),
        empty_hello_ack()
    );
    write_extension_request(
        &mut client,
        &ExtensionRequest::DomWrite {
            value: "a secret the host must not inspect".to_string(),
            target: DomWriteTarget::FormInput {
                input_type: "password".to_string(),
            },
        },
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut client).unwrap(),
        ExtensionReply::GatekeeperBlocked {
            capability: CAPABILITY_DOM_WRITE.to_string(),
            reason: "credential-shaped field requires confirmation".to_string(),
            category: "sensitive-extension-action".to_string(),
        }
    );

    drop(client);
    handle.join().unwrap().unwrap();
    assert_eq!(
        gatekeeper.join().unwrap(),
        GatekeeperRequest::CheckExtensionAction {
            extension_id: MINIMAL_SLICE_EXTENSION_ID.to_string(),
            capability: CAPABILITY_DOM_WRITE.to_string(),
            detail: "target=form-input; input_type=password".to_string(),
        }
    );
    let _ = std::fs::remove_file(gatekeeper_socket);
}
