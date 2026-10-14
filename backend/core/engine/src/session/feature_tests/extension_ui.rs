// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn extension_toolbar_is_broadcast_clickable_and_owned_by_its_connection() {
    let (mut client, mut server) = client_pair();
    let (extension_tx, extension_rx) = mpsc::channel();
    let (event_tx, event_rx) = mpsc::sync_channel(16);
    let dir = temp_frame_dir("extension-toolbar");
    let cleanup_dir = dir.clone();
    std::fs::create_dir_all(&dir).unwrap();
    let session = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        let mut generation = 0;
        run_session_with_extension_requests_and_events(
            &mut tabs,
            &mut server,
            &dir,
            &mut generation,
            Path::new("/not-used-for-native-ui"),
            &extension_rx,
            Some(&event_tx),
        )
    });
    handshake(&mut client);
    let (reply_tx, reply_rx) = mpsc::channel();
    extension_tx
        .send(ExtensionPageRequest::SetToolbarButton {
            connection_id: 4,
            grant_generation: 0,
            label: "\u{202e}spoof".to_string(),
            reply: reply_tx,
        })
        .unwrap();
    assert!(reply_rx
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .is_err());
    let (reply_tx, reply_rx) = mpsc::channel();
    extension_tx
        .send(ExtensionPageRequest::SetToolbarButton {
            connection_id: 4,
            grant_generation: 0,
            label: "Notes".to_string(),
            reply: reply_tx,
        })
        .unwrap();
    assert_eq!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::ExtensionToolbar {
            label: Some("Notes".to_string()),
        }
    );
    reply_rx
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap();
    blueice_ipc::write_client_message(&mut client, &ClientMessage::GetExtensionToolbar).unwrap();
    assert_eq!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::ExtensionToolbar {
            label: Some("Notes".to_string()),
        }
    );
    blueice_ipc::write_client_message_with_ids(
        &mut client,
        Some(1),
        None,
        &ClientMessage::ActivateExtensionToolbar,
    )
    .unwrap();
    assert_eq!(
        event_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
        ExtensionRuntimeEvent::ToolbarActivated {
            tab_id: 1,
            grant_generation: 0
        }
    );

    // A stale connection cannot clear a newer connection's button.
    let (reply_tx, reply_rx) = mpsc::channel();
    extension_tx
        .send(ExtensionPageRequest::SetToolbarButton {
            connection_id: 5,
            grant_generation: 0,
            label: "Tasks".to_string(),
            reply: reply_tx,
        })
        .unwrap();
    assert_eq!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::ExtensionToolbar {
            label: Some("Tasks".to_string()),
        }
    );
    reply_rx
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap();
    let popup = ExtensionPopup {
        id: 1,
        tab_id: 1,
        title: "Tasks".to_string(),
        body: "Saved locally".to_string(),
        action_label: None,
    };
    let (reply_tx, reply_rx) = mpsc::channel();
    extension_tx
        .send(ExtensionPageRequest::ShowPopup {
            connection_id: 5,
            grant_generation: 0,
            popup: popup.clone(),
            reply: reply_tx,
        })
        .unwrap();
    assert_eq!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::ExtensionPopup {
            popup: Some(popup.clone())
        }
    );
    reply_rx
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap();
    blueice_ipc::write_client_message(&mut client, &ClientMessage::GetExtensionPopup).unwrap();
    assert_eq!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::ExtensionPopup {
            popup: Some(popup.clone())
        }
    );
    blueice_ipc::write_client_message(&mut client, &ClientMessage::DismissExtensionPopup).unwrap();
    assert_eq!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::ExtensionPopup { popup: None }
    );
    let (reply_tx, reply_rx) = mpsc::channel();
    extension_tx
        .send(ExtensionPageRequest::ShowPopup {
            connection_id: 5,
            grant_generation: 0,
            popup: popup.clone(),
            reply: reply_tx,
        })
        .unwrap();
    assert_eq!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::ExtensionPopup { popup: Some(popup) }
    );
    reply_rx
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap();
    let action_popup = ExtensionPopup {
        id: 2,
        tab_id: 1,
        title: "Tasks".to_string(),
        body: "Ready to open".to_string(),
        action_label: Some("Open tasks".to_string()),
    };
    let (reply_tx, reply_rx) = mpsc::channel();
    extension_tx
        .send(ExtensionPageRequest::ShowPopup {
            connection_id: 5,
            grant_generation: 0,
            popup: action_popup.clone(),
            reply: reply_tx,
        })
        .unwrap();
    assert_eq!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::ExtensionPopup {
            popup: Some(action_popup.clone())
        }
    );
    reply_rx
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap();
    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::ActivateExtensionPopupAction { popup_id: 1 },
    )
    .unwrap();
    assert!(
        matches!(blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::Error { message } if message.contains("no matching live extension popup action"))
    );
    assert!(event_rx.try_recv().is_err());
    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::ActivateExtensionPopupAction { popup_id: 2 },
    )
    .unwrap();
    assert_eq!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::ExtensionPopup { popup: None }
    );
    assert_eq!(
        event_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
        ExtensionRuntimeEvent::PopupActionActivated {
            tab_id: 1,
            grant_generation: 0
        }
    );
    let (reply_tx, reply_rx) = mpsc::channel();
    extension_tx
        .send(ExtensionPageRequest::ShowPopup {
            connection_id: 5,
            grant_generation: 0,
            popup: action_popup,
            reply: reply_tx,
        })
        .unwrap();
    assert!(matches!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::ExtensionPopup { popup: Some(_) }
    ));
    reply_rx
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap();
    let (reply_tx, reply_rx) = mpsc::channel();
    extension_tx
        .send(ExtensionPageRequest::ClearToolbarButton {
            connection_id: 4,
            reply: reply_tx,
        })
        .unwrap();
    reply_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    blueice_ipc::write_client_message(&mut client, &ClientMessage::GetExtensionToolbar).unwrap();
    assert_eq!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::ExtensionToolbar {
            label: Some("Tasks".to_string()),
        }
    );
    let (reply_tx, reply_rx) = mpsc::channel();
    extension_tx
        .send(ExtensionPageRequest::ClearToolbarButton {
            connection_id: 5,
            reply: reply_tx,
        })
        .unwrap();
    assert_eq!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::ExtensionPopup { popup: None }
    );
    assert_eq!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::ExtensionToolbar { label: None }
    );
    reply_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    blueice_ipc::write_client_message(&mut client, &ClientMessage::ActivateExtensionToolbar)
        .unwrap();
    assert!(matches!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::Error { message } if message.contains("no extension toolbar")
    ));
    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    session.join().unwrap().unwrap();
    let _ = std::fs::remove_dir_all(cleanup_dir);
}

#[test]
fn optional_ui_revocation_clears_published_surfaces_and_rejects_stale_publication() {
    let root = temp_frame_dir("stale-optional-ui");
    std::fs::create_dir_all(&root).unwrap();
    let manifest = root.join("extension.json");
    std::fs::write(&manifest,
        r#"{"name":"Optional UI","version":"1","blueice_api_version":1,"entry_point":"extension.wasm","capabilities":{"optional":["ui:inject"]}}"#
    ).unwrap();
    std::fs::write(root.join("extension.wasm"), b"\0asm\x01\0\0\0").unwrap();
    let installed = blueice_extension_host::load_installed_extension(&manifest).unwrap();
    let id = installed.extension_id().to_string();
    let registry = Arc::new(blueice_extension_host::registry_for_installed_extension(
        &installed,
    ));
    let mut tabs = TabManager::new(320.0, 200.0);
    tabs.set_extension_permission_registry(Arc::clone(&registry), id.clone());
    registry.grant_optional(&id, "ui:inject").unwrap();
    let old_generation = registry.capability_generation(&id, "ui:inject").unwrap();
    let mut wire = Vec::new();
    let mut frame_generation = 0;
    let mut toolbar = None;
    let mut popup = None;
    let (reply, result) = mpsc::channel();
    handle_extension_page_request(
        &mut tabs,
        &mut wire,
        &root,
        &mut frame_generation,
        &mut toolbar,
        &mut popup,
        ExtensionPageRequest::SetToolbarButton {
            connection_id: 7,
            grant_generation: old_generation,
            label: "Notes".into(),
            reply,
        },
    )
    .unwrap();
    result.recv().unwrap().unwrap();
    let shown = ExtensionPopup {
        id: 1,
        tab_id: 1,
        title: "Notes".into(),
        body: "Saved".into(),
        action_label: None,
    };
    let (reply, result) = mpsc::channel();
    handle_extension_page_request(
        &mut tabs,
        &mut wire,
        &root,
        &mut frame_generation,
        &mut toolbar,
        &mut popup,
        ExtensionPageRequest::ShowPopup {
            connection_id: 7,
            grant_generation: old_generation,
            popup: shown.clone(),
            reply,
        },
    )
    .unwrap();
    result.recv().unwrap().unwrap();

    registry.revoke_optional(&id, "ui:inject").unwrap();
    prune_stale_extension_effects(&mut tabs, &mut wire, &mut toolbar, &mut popup).unwrap();
    assert!(toolbar.is_none() && popup.is_none());
    registry.grant_optional(&id, "ui:inject").unwrap();
    let new_generation = registry.capability_generation(&id, "ui:inject").unwrap();
    assert_ne!(old_generation, new_generation);
    let (reply, result) = mpsc::channel();
    handle_extension_page_request(
        &mut tabs,
        &mut wire,
        &root,
        &mut frame_generation,
        &mut toolbar,
        &mut popup,
        ExtensionPageRequest::SetToolbarButton {
            connection_id: 7,
            grant_generation: old_generation,
            label: "Old".into(),
            reply,
        },
    )
    .unwrap();
    assert!(result.recv().unwrap().is_err());
    assert!(toolbar.is_none());
    let (reply, result) = mpsc::channel();
    handle_extension_page_request(
        &mut tabs,
        &mut wire,
        &root,
        &mut frame_generation,
        &mut toolbar,
        &mut popup,
        ExtensionPageRequest::SetToolbarButton {
            connection_id: 7,
            grant_generation: new_generation,
            label: "New".into(),
            reply,
        },
    )
    .unwrap();
    result.recv().unwrap().unwrap();
    let mut cursor = std::io::Cursor::new(wire);
    assert_eq!(
        blueice_ipc::read_server_message(&mut cursor).unwrap(),
        ServerMessage::ExtensionToolbar {
            label: Some("Notes".into())
        }
    );
    assert_eq!(
        blueice_ipc::read_server_message(&mut cursor).unwrap(),
        ServerMessage::ExtensionPopup { popup: Some(shown) }
    );
    assert_eq!(
        blueice_ipc::read_server_message(&mut cursor).unwrap(),
        ServerMessage::ExtensionPopup { popup: None }
    );
    assert_eq!(
        blueice_ipc::read_server_message(&mut cursor).unwrap(),
        ServerMessage::ExtensionToolbar { label: None }
    );
    assert_eq!(
        blueice_ipc::read_server_message(&mut cursor).unwrap(),
        ServerMessage::ExtensionToolbar {
            label: Some("New".into())
        }
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn live_session_broadcasts_optional_ui_revocation_without_client_traffic() {
    let root = temp_frame_dir("live-optional-ui-revoke");
    std::fs::create_dir_all(&root).unwrap();
    let manifest = root.join("extension.json");
    std::fs::write(&manifest,
        r#"{"name":"Live optional UI","version":"1","blueice_api_version":1,"entry_point":"extension.wasm","capabilities":{"optional":["ui:inject"]}}"#
    ).unwrap();
    std::fs::write(root.join("extension.wasm"), b"\0asm\x01\0\0\0").unwrap();
    let installed = blueice_extension_host::load_installed_extension(&manifest).unwrap();
    let id = installed.extension_id().to_string();
    let registry = Arc::new(blueice_extension_host::registry_for_installed_extension(
        &installed,
    ));
    registry.grant_optional(&id, "ui:inject").unwrap();
    let old_generation = registry.capability_generation(&id, "ui:inject").unwrap();
    let (mut client, mut server) = client_pair();
    client
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let (extension_tx, extension_rx) = mpsc::channel();
    let (event_tx, event_rx) = mpsc::sync_channel(16);
    let session_registry = Arc::clone(&registry);
    let session = thread::spawn({
        let id = id.clone();
        let root = root.clone();
        move || {
            let mut tabs = TabManager::new(320.0, 200.0);
            tabs.set_extension_permission_registry(session_registry, id);
            let mut generation = 0;
            run_session_with_extension_requests_and_events(
                &mut tabs,
                &mut server,
                &root,
                &mut generation,
                Path::new("/not-used-for-optional-ui"),
                &extension_rx,
                Some(&event_tx),
            )
        }
    });
    handshake(&mut client);
    let (reply, result) = mpsc::channel();
    extension_tx
        .send(ExtensionPageRequest::SetToolbarButton {
            connection_id: 7,
            grant_generation: old_generation,
            label: "Notes".into(),
            reply,
        })
        .unwrap();
    assert_eq!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::ExtensionToolbar {
            label: Some("Notes".into())
        }
    );
    result
        .recv_timeout(Duration::from_secs(2))
        .unwrap()
        .unwrap();
    let shown = ExtensionPopup {
        id: 1,
        tab_id: 1,
        title: "Notes".into(),
        body: "Saved".into(),
        action_label: Some("Open".into()),
    };
    let (reply, result) = mpsc::channel();
    extension_tx
        .send(ExtensionPageRequest::ShowPopup {
            connection_id: 7,
            grant_generation: old_generation,
            popup: shown.clone(),
            reply,
        })
        .unwrap();
    assert_eq!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::ExtensionPopup { popup: Some(shown) }
    );
    result
        .recv_timeout(Duration::from_secs(2))
        .unwrap()
        .unwrap();

    assert!(
        revoke_optional_and_wait_for_cleanup(&registry, &id, "ui:inject", &extension_tx,).unwrap()
    );
    // No client command follows the revoke. The private completion
    // barrier acknowledges only after both removals are published.
    assert_eq!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::ExtensionPopup { popup: None }
    );
    assert_eq!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::ExtensionToolbar { label: None }
    );
    blueice_ipc::write_client_message(&mut client, &ClientMessage::ActivateExtensionToolbar)
        .unwrap();
    assert!(
        matches!(blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::Error { message } if message.contains("no extension toolbar"))
    );
    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::ActivateExtensionPopupAction { popup_id: 1 },
    )
    .unwrap();
    assert!(
        matches!(blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::Error { message } if message.contains("no matching live extension popup action"))
    );
    assert!(event_rx.try_recv().is_err());

    registry.grant_optional(&id, "ui:inject").unwrap();
    let new_generation = registry.capability_generation(&id, "ui:inject").unwrap();
    assert_ne!(old_generation, new_generation);
    let (reply, result) = mpsc::channel();
    extension_tx
        .send(ExtensionPageRequest::SetToolbarButton {
            connection_id: 7,
            grant_generation: old_generation,
            label: "Old".into(),
            reply,
        })
        .unwrap();
    assert!(result
        .recv_timeout(Duration::from_secs(2))
        .unwrap()
        .is_err());
    let (reply, result) = mpsc::channel();
    extension_tx
        .send(ExtensionPageRequest::SetToolbarButton {
            connection_id: 7,
            grant_generation: new_generation,
            label: "New".into(),
            reply,
        })
        .unwrap();
    assert_eq!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::ExtensionToolbar {
            label: Some("New".into())
        }
    );
    result
        .recv_timeout(Duration::from_secs(2))
        .unwrap()
        .unwrap();
    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    session.join().unwrap().unwrap();
    let _ = std::fs::remove_dir_all(root);
}
