// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn queued_network_registration_checks_original_grant_generation_at_session_commit() {
    let root = temp_frame_dir("stale-network-registration");
    std::fs::create_dir_all(&root).unwrap();
    let manifest = root.join("extension.json");
    std::fs::write(&manifest,
        r#"{"name":"Queued network","version":"1","blueice_api_version":1,"entry_point":"extension.wasm","capabilities":{"optional":["network:intercept"]}}"#
    ).unwrap();
    std::fs::write(root.join("extension.wasm"), b"\0asm\x01\0\0\0").unwrap();
    let installed = blueice_extension_host::load_installed_extension(&manifest).unwrap();
    let id = installed.extension_id().to_string();
    let registry = Arc::new(blueice_extension_host::registry_for_installed_extension(
        &installed,
    ));
    let mut tabs = TabManager::new(320.0, 200.0);
    tabs.set_extension_permission_registry(Arc::clone(&registry), id.clone());
    assert!(registry.grant_optional(&id, "network:intercept").unwrap());
    let old_generation = registry
        .capability_generation(&id, "network:intercept")
        .unwrap();
    assert!(registry.revoke_optional(&id, "network:intercept").unwrap());
    assert!(registry.grant_optional(&id, "network:intercept").unwrap());
    let new_generation = registry
        .capability_generation(&id, "network:intercept")
        .unwrap();
    assert_ne!(old_generation, new_generation);

    let mut wire = Vec::new();
    let mut frame_generation = 0;
    let mut toolbar = None;
    let mut popup = None;
    let mut submit = |tabs: &mut TabManager, grant_generation| {
        let (reply, result) = mpsc::channel();
        handle_extension_page_request(
            tabs,
            &mut wire,
            &root,
            &mut frame_generation,
            &mut toolbar,
            &mut popup,
            ExtensionPageRequest::RegisterNetworkBlockUrl {
                connection_id: 7,
                grant_generation,
                url: "https://example.test/blocked".into(),
                reply,
            },
        )
        .unwrap();
        result.recv().unwrap()
    };
    assert!(submit(&mut tabs, old_generation).is_err());
    assert!(!tabs.is_extension_navigation_blocked("https://example.test/blocked"));
    assert!(submit(&mut tabs, new_generation).is_ok());
    assert!(tabs.is_extension_navigation_blocked("https://example.test/blocked"));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn queued_dom_writes_check_original_grant_generation_at_session_commit() {
    let root = temp_frame_dir("stale-dom-write");
    std::fs::create_dir_all(&root).unwrap();
    let manifest = root.join("extension.json");
    std::fs::write(&manifest,
        r#"{"name":"Queued DOM write","version":"1","blueice_api_version":1,"entry_point":"extension.wasm","capabilities":{"optional":["dom:write"]}}"#
    ).unwrap();
    std::fs::write(root.join("extension.wasm"), b"\0asm\x01\0\0\0").unwrap();
    let installed = blueice_extension_host::load_installed_extension(&manifest).unwrap();
    let id = installed.extension_id().to_string();
    let registry = Arc::new(blueice_extension_host::registry_for_installed_extension(
        &installed,
    ));
    let mut tabs = TabManager::new(320.0, 200.0);
    let tab_id = tabs.default_tab();
    tabs.get_mut(tab_id).unwrap().load_html_str(
        "<input id='name' value='Before'><h1 id='title'>Original</h1>",
        Some("https://example.test/page".into()),
    );
    let input_id = tabs
        .get(tab_id)
        .unwrap()
        .script_get_element_by_id("name")
        .unwrap();
    let title_id = tabs
        .get(tab_id)
        .unwrap()
        .script_get_element_by_id("title")
        .unwrap();
    tabs.set_extension_permission_registry(Arc::clone(&registry), id.clone());
    registry.grant_optional(&id, "dom:write").unwrap();
    let old_generation = registry.capability_generation(&id, "dom:write").unwrap();
    registry.revoke_optional(&id, "dom:write").unwrap();
    registry.grant_optional(&id, "dom:write").unwrap();
    let new_generation = registry.capability_generation(&id, "dom:write").unwrap();
    assert_ne!(old_generation, new_generation);

    let before = tabs.get(tab_id).unwrap().dom_dump();
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
        ExtensionPageRequest::SetTextInputValue {
            tab_id: tab_id.as_u64(),
            node_id: input_id.as_u64(),
            value: "Stale".into(),
            grant_generation: old_generation,
            reply,
        },
    )
    .unwrap();
    assert!(result.recv().unwrap().is_err());
    let (reply, result) = mpsc::channel();
    handle_extension_page_request(
        &mut tabs,
        &mut wire,
        &root,
        &mut frame_generation,
        &mut toolbar,
        &mut popup,
        ExtensionPageRequest::SetVisibleTextContent {
            tab_id: tab_id.as_u64(),
            node_id: title_id.as_u64(),
            value: "Stale".into(),
            grant_generation: old_generation,
            reply,
        },
    )
    .unwrap();
    assert!(result.recv().unwrap().is_err());
    // Every other core-backed dom:write shape shares the final grant
    // check. An invalid node would produce a different error if one arm
    // accidentally bypassed it.
    for variant in 0..6 {
        let (reply, result) = mpsc::channel();
        let request = match variant {
            0 => ExtensionPageRequest::SetCheckboxChecked {
                tab_id: tab_id.as_u64(),
                node_id: 0,
                checked: true,
                grant_generation: old_generation,
                reply,
            },
            1 => ExtensionPageRequest::SetTextareaValue {
                tab_id: tab_id.as_u64(),
                node_id: 0,
                value: "Stale".into(),
                grant_generation: old_generation,
                reply,
            },
            2 => ExtensionPageRequest::SetVisibleLeafText {
                tab_id: tab_id.as_u64(),
                node_id: 0,
                value: "Stale".into(),
                grant_generation: old_generation,
                reply,
            },
            3 => ExtensionPageRequest::SetRangeInputValue {
                tab_id: tab_id.as_u64(),
                node_id: 0,
                value: 1,
                grant_generation: old_generation,
                reply,
            },
            4 => ExtensionPageRequest::SetRadioChecked {
                tab_id: tab_id.as_u64(),
                node_id: 0,
                grant_generation: old_generation,
                reply,
            },
            _ => ExtensionPageRequest::SelectOption {
                tab_id: tab_id.as_u64(),
                node_id: 0,
                grant_generation: old_generation,
                reply,
            },
        };
        handle_extension_page_request(
            &mut tabs,
            &mut wire,
            &root,
            &mut frame_generation,
            &mut toolbar,
            &mut popup,
            request,
        )
        .unwrap();
        assert!(
            result
                .recv()
                .unwrap()
                .unwrap_err()
                .contains("grant changed"),
            "every DOM write must reject the original revoked grant"
        );
    }
    assert_eq!(tabs.get(tab_id).unwrap().dom_dump(), before);
    assert!(wire.is_empty(), "a denied write must not publish a frame");

    let (reply, result) = mpsc::channel();
    handle_extension_page_request(
        &mut tabs,
        &mut wire,
        &root,
        &mut frame_generation,
        &mut toolbar,
        &mut popup,
        ExtensionPageRequest::SetTextInputValue {
            tab_id: tab_id.as_u64(),
            node_id: input_id.as_u64(),
            value: "Fresh".into(),
            grant_generation: new_generation,
            reply,
        },
    )
    .unwrap();
    result.recv().unwrap().unwrap();
    assert_ne!(tabs.get(tab_id).unwrap().dom_dump(), before);
    assert!(!wire.is_empty());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn internal_revoke_barrier_acks_only_after_ui_and_network_effects_are_removed() {
    let root = temp_frame_dir("optional-revoke-barrier");
    std::fs::create_dir_all(&root).unwrap();
    let manifest = root.join("extension.json");
    std::fs::write(&manifest,
        r#"{"name":"Optional effects","version":"1","blueice_api_version":1,"entry_point":"extension.wasm","capabilities":{"optional":["network:intercept","ui:inject"]}}"#
    ).unwrap();
    std::fs::write(root.join("extension.wasm"), b"\0asm\x01\0\0\0").unwrap();
    let installed = blueice_extension_host::load_installed_extension(&manifest).unwrap();
    let id = installed.extension_id().to_string();
    let registry = Arc::new(blueice_extension_host::registry_for_installed_extension(
        &installed,
    ));
    registry.grant_optional(&id, "network:intercept").unwrap();
    registry.grant_optional(&id, "ui:inject").unwrap();
    let network_generation = registry
        .capability_generation(&id, "network:intercept")
        .unwrap();
    let ui_generation = registry.capability_generation(&id, "ui:inject").unwrap();
    let mut tabs = TabManager::new(320.0, 200.0);
    tabs.set_extension_permission_registry(Arc::clone(&registry), id.clone());
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
        ExtensionPageRequest::RegisterNetworkBlockHost {
            connection_id: 7,
            grant_generation: network_generation,
            host: "old.example.test".into(),
            reply,
        },
    )
    .unwrap();
    result.recv().unwrap().unwrap();
    assert_eq!(tabs.extension_navigation_rule_owner_count(), 1);
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
            grant_generation: ui_generation,
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
            grant_generation: ui_generation,
            popup: shown.clone(),
            reply,
        },
    )
    .unwrap();
    result.recv().unwrap().unwrap();

    registry.revoke_optional(&id, "ui:inject").unwrap();
    let (session_tx, session_rx) = mpsc::channel();
    thread::scope(|scope| {
        let frame_root = &root;
        let tabs = &mut tabs;
        let wire = &mut wire;
        let frame_generation = &mut frame_generation;
        let toolbar = &mut toolbar;
        let popup = &mut popup;
        let worker = scope.spawn(move || {
            let request = session_rx.recv_timeout(Duration::from_secs(2)).unwrap();
            handle_extension_page_request(
                tabs,
                wire,
                frame_root,
                frame_generation,
                toolbar,
                popup,
                request,
            )
            .unwrap();
        });
        assert!(revoke_optional_and_wait_for_cleanup(
            &registry,
            &id,
            "network:intercept",
            &session_tx,
        )
        .unwrap());
        worker.join().unwrap();
    });
    assert_eq!(tabs.extension_navigation_rule_owner_count(), 0);
    assert!(toolbar.is_none() && popup.is_none());
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
    registry.grant_optional(&id, "network:intercept").unwrap();
    let (unavailable_session, receiver) = mpsc::channel();
    drop(receiver);
    assert!(
        revoke_optional_and_wait_for_cleanup(
            &registry,
            &id,
            "network:intercept",
            &unavailable_session,
        )
        .is_err(),
        "a missing session acknowledgement must not look like successful cleanup"
    );
    assert_eq!(
        registry.capability_generation(&id, "network:intercept"),
        None,
        "an acknowledgement error must not restore the revoked grant"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn idle_session_physically_prunes_revoked_optional_navigation_rules() {
    let root = temp_frame_dir("idle-optional-network-revoke");
    std::fs::create_dir_all(&root).unwrap();
    let manifest = root.join("extension.json");
    std::fs::write(&manifest,
        r#"{"name":"Optional network","version":"1","blueice_api_version":1,"entry_point":"extension.wasm","capabilities":{"optional":["network:intercept"]}}"#
    ).unwrap();
    std::fs::write(root.join("extension.wasm"), b"\0asm\x01\0\0\0").unwrap();
    let installed = blueice_extension_host::load_installed_extension(&manifest).unwrap();
    let id = installed.extension_id().to_string();
    let registry = Arc::new(blueice_extension_host::registry_for_installed_extension(
        &installed,
    ));
    registry.grant_optional(&id, "network:intercept").unwrap();
    let grant_generation = registry
        .capability_generation(&id, "network:intercept")
        .unwrap();
    let (mut client, mut server) = client_pair();
    let (extension_tx, extension_rx) = mpsc::channel();
    let session_registry = Arc::clone(&registry);
    let session = thread::spawn({
        let id = id.clone();
        let root = root.clone();
        move || {
            let mut tabs = TabManager::new(320.0, 200.0);
            tabs.set_extension_permission_registry(session_registry, id);
            let mut generation = 0;
            let result = run_session_with_extension_requests(
                &mut tabs,
                &mut server,
                &root,
                &mut generation,
                Path::new("/not-used-for-optional-network"),
                &extension_rx,
            );
            (result, tabs.extension_navigation_rule_owner_count())
        }
    });
    handshake(&mut client);
    let (reply, result) = mpsc::channel();
    extension_tx
        .send(ExtensionPageRequest::RegisterNetworkBlockHost {
            connection_id: 7,
            grant_generation,
            host: "old.example.test".into(),
            reply,
        })
        .unwrap();
    result
        .recv_timeout(Duration::from_secs(2))
        .unwrap()
        .unwrap();
    registry.revoke_optional(&id, "network:intercept").unwrap();
    // No client or extension message follows. The 25 ms session poll
    // removes the now-inert rule before this client disconnects; the EOF
    // branch itself deliberately does not run cleanup.
    thread::sleep(Duration::from_millis(125));
    drop(client);
    let (result, remaining_rule_owners) = session.join().unwrap();
    result.unwrap();
    assert_eq!(remaining_rule_owners, 0);
    let _ = std::fs::remove_dir_all(root);
}
