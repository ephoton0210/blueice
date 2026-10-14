// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn native_toolbar_rejection_or_unavailable_gatekeeper_never_reaches_core() {
    let mut registry = ExtensionRegistry::minimal_slice();
    registry.grant(MINIMAL_SLICE_EXTENSION_ID, CAPABILITY_UI_INJECT);
    let (gatekeeper, reviewed) = start_gatekeeper(
        "toolbar-blocked",
        GatekeeperReply::Rejected {
            reason: "unsafe toolbar label".to_string(),
            category: "extension-toolbar-social-engineering".to_string(),
        },
    );
    let (published_tx, published_rx) = mpsc::channel();
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
            label: "Enter password".to_string(),
        },
    )
    .unwrap();
    assert!(matches!(read_extension_reply(&mut client).unwrap(),
        ExtensionReply::GatekeeperBlocked { category, .. }
            if category == "extension-toolbar-social-engineering"));
    assert!(matches!(reviewed.join().unwrap(),
        GatekeeperRequest::CheckExtensionAction { detail, .. }
            if detail.contains("action=set-native-toolbar-button") && detail.contains("Enter password")));
    assert!(published_rx.try_recv().is_err());

    // The one-shot reviewer is gone. The next label must fail closed too.
    let _ = std::fs::remove_file(&gatekeeper);
    write_extension_request(
        &mut client,
        &ExtensionRequest::SetToolbarButton {
            label: "Notes".to_string(),
        },
    )
    .unwrap();
    assert!(matches!(read_extension_reply(&mut client).unwrap(),
        ExtensionReply::GatekeeperBlocked { category, .. }
            if category == "gatekeeper-unavailable"));
    assert!(published_rx.try_recv().is_err());
    drop(client);
    handle.join().unwrap().unwrap();
}

#[test]
fn native_popup_requires_v2_toolbar_and_review_of_its_actual_text() {
    let mut registry = ExtensionRegistry::minimal_slice();
    registry.grant(MINIMAL_SLICE_EXTENSION_ID, CAPABILITY_UI_INJECT);
    let (gatekeeper, reviewed) =
        start_gatekeeper_replies("popup-clear", vec![GatekeeperReply::Cleared; 2]);
    let (shown_tx, shown_rx) = mpsc::channel();
    let (cleared_tx, cleared_rx) = mpsc::channel();
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
            .with_toolbar_button(|_, _| Ok(()))
            .with_toolbar_clearer(|| Ok(()))
            .with_popup(
                move |tab_id, title, body, _| {
                    shown_tx.send((tab_id, title, body)).unwrap();
                    Ok(())
                },
                move || {
                    cleared_tx.send(()).unwrap();
                    Ok(())
                },
            ),
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
        &ExtensionRequest::ShowPopup {
            tab_id: 1,
            title: "Notes".to_string(),
            body: "Saved locally".to_string(),
        },
    )
    .unwrap();
    assert!(matches!(
        read_extension_reply(&mut client).unwrap(),
        ExtensionReply::CapabilityDenied { .. }
    ));
    write_extension_request(
        &mut client,
        &hello_with_capabilities(MINIMAL_SLICE_EXTENSION_ID, [(CAPABILITY_UI_INJECT, 2)]),
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut client).unwrap(),
        empty_hello_ack()
    );
    write_extension_request(
        &mut client,
        &ExtensionRequest::ShowPopup {
            tab_id: 1,
            title: "Notes".to_string(),
            body: "Saved locally".to_string(),
        },
    )
    .unwrap();
    assert!(matches!(
        read_extension_reply(&mut client).unwrap(),
        ExtensionReply::OperationUnavailable { .. }
    ));
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
    write_extension_request(
        &mut client,
        &ExtensionRequest::ShowPopup {
            tab_id: 1,
            title: "Notes".to_string(),
            body: "bad\ntext".to_string(),
        },
    )
    .unwrap();
    assert!(matches!(
        read_extension_reply(&mut client).unwrap(),
        ExtensionReply::OperationUnavailable { .. }
    ));
    write_extension_request(
        &mut client,
        &ExtensionRequest::ShowPopup {
            tab_id: 1,
            title: "Notes".to_string(),
            body: "Saved locally".to_string(),
        },
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut client).unwrap(),
        ExtensionReply::UiInjectAck
    );
    assert_eq!(
        shown_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
        (1, "Notes".to_string(), "Saved locally".to_string())
    );
    let requests = reviewed.join().unwrap();
    assert_eq!(requests.len(), 2);
    assert!(
        matches!(&requests[0], GatekeeperRequest::CheckExtensionAction { capability, detail, .. }
        if capability == CAPABILITY_UI_INJECT && detail.contains("action=set-native-toolbar-button"))
    );
    assert!(
        matches!(&requests[1], GatekeeperRequest::CheckExtensionAction { capability, detail, .. }
        if capability == CAPABILITY_UI_INJECT && detail.contains("Saved locally"))
    );
    write_extension_request(&mut client, &ExtensionRequest::ClearPopup).unwrap();
    assert_eq!(
        read_extension_reply(&mut client).unwrap(),
        ExtensionReply::UiInjectAck
    );
    cleared_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    drop(client);
    handle.join().unwrap().unwrap();
    let _ = std::fs::remove_file(gatekeeper);
}

#[test]
fn popup_action_requires_v3_and_reviews_the_button_label_before_publication() {
    let mut registry = ExtensionRegistry::minimal_slice();
    registry.grant(MINIMAL_SLICE_EXTENSION_ID, CAPABILITY_UI_INJECT);
    let (gatekeeper, reviewed) =
        start_gatekeeper_replies("popup-action-clear", vec![GatekeeperReply::Cleared; 3]);
    let (shown_tx, shown_rx) = mpsc::channel();
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
            .with_toolbar_button(|_, _| Ok(()))
            .with_toolbar_clearer(|| Ok(()))
            .with_popup_action(move |tab_id, title, body, label, _| {
                shown_tx.send((tab_id, title, body, label)).unwrap();
                Ok(())
            }),
        )
    });
    write_extension_request(
        &mut client,
        &hello_with_capabilities(MINIMAL_SLICE_EXTENSION_ID, [(CAPABILITY_UI_INJECT, 2)]),
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
    assert_eq!(
        read_extension_reply(&mut client).unwrap(),
        ExtensionReply::UiInjectAck
    );
    let action = || ExtensionRequest::ShowPopupAction {
        tab_id: 1,
        title: "Notes".to_string(),
        body: "Saved locally".to_string(),
        action_label: "Open notes".to_string(),
    };
    write_extension_request(&mut client, &action()).unwrap();
    assert!(matches!(
        read_extension_reply(&mut client).unwrap(),
        ExtensionReply::CapabilityDenied { .. }
    ));
    assert!(shown_rx.try_recv().is_err());
    write_extension_request(
        &mut client,
        &hello_with_capabilities(MINIMAL_SLICE_EXTENSION_ID, [(CAPABILITY_UI_INJECT, 3)]),
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut client).unwrap(),
        empty_hello_ack()
    );
    write_extension_request(&mut client, &action()).unwrap();
    assert!(matches!(
        read_extension_reply(&mut client).unwrap(),
        ExtensionReply::OperationUnavailable { .. }
    ));
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
    write_extension_request(
        &mut client,
        &ExtensionRequest::ShowPopupAction {
            tab_id: 1,
            title: "Notes".to_string(),
            body: "Saved locally".to_string(),
            action_label: "Bad\nLabel".to_string(),
        },
    )
    .unwrap();
    assert!(matches!(
        read_extension_reply(&mut client).unwrap(),
        ExtensionReply::OperationUnavailable { .. }
    ));
    assert!(shown_rx.try_recv().is_err());
    write_extension_request(&mut client, &action()).unwrap();
    assert_eq!(
        read_extension_reply(&mut client).unwrap(),
        ExtensionReply::UiInjectAck
    );
    assert_eq!(
        shown_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
        (
            1,
            "Notes".to_string(),
            "Saved locally".to_string(),
            "Open notes".to_string()
        )
    );
    let requests = reviewed.join().unwrap();
    assert_eq!(requests.len(), 3);
    assert!(
        matches!(&requests[2], GatekeeperRequest::CheckExtensionAction { capability, detail, .. }
        if capability == CAPABILITY_UI_INJECT && detail.contains("action=show-native-popup")
            && detail.contains("body=\"Saved locally\"")
            && detail.contains("action_label=\"Open notes\""))
    );
    drop(client);
    handle.join().unwrap().unwrap();
    let _ = std::fs::remove_file(gatekeeper);
}

#[test]
fn native_popup_rejection_never_calls_the_core_delegate() {
    let mut registry = ExtensionRegistry::minimal_slice();
    registry.grant(MINIMAL_SLICE_EXTENSION_ID, CAPABILITY_UI_INJECT);
    let (gatekeeper, reviewed) = start_gatekeeper_replies(
        "popup-rejected",
        vec![
            GatekeeperReply::Cleared,
            GatekeeperReply::Rejected {
                reason: "unsafe popup".to_string(),
                category: "extension-popup-social-engineering".to_string(),
            },
        ],
    );
    let (shown_tx, shown_rx) = mpsc::channel();
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
            .with_toolbar_button(|_, _| Ok(()))
            .with_popup(
                move |_, _, _, _| {
                    shown_tx.send(()).unwrap();
                    Ok(())
                },
                || Ok(()),
            ),
        )
    });
    write_extension_request(
        &mut client,
        &hello_with_capabilities(MINIMAL_SLICE_EXTENSION_ID, [(CAPABILITY_UI_INJECT, 2)]),
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
    assert_eq!(
        read_extension_reply(&mut client).unwrap(),
        ExtensionReply::UiInjectAck
    );
    write_extension_request(
        &mut client,
        &ExtensionRequest::ShowPopup {
            tab_id: 1,
            title: "Notes".to_string(),
            body: "Enter your password".to_string(),
        },
    )
    .unwrap();
    assert!(
        matches!(read_extension_reply(&mut client).unwrap(), ExtensionReply::GatekeeperBlocked { category, .. } if category == "extension-popup-social-engineering")
    );
    assert!(shown_rx.try_recv().is_err());
    let requests = reviewed.join().unwrap();
    assert_eq!(requests.len(), 2);
    assert!(
        matches!(&requests[1], GatekeeperRequest::CheckExtensionAction { detail, .. } if detail.contains("Enter your password"))
    );
    drop(client);
    handle.join().unwrap().unwrap();
    let _ = std::fs::remove_file(gatekeeper);
}
