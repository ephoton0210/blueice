// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn extension_request_reads_the_default_tabs_real_ai_representation() {
    let (mut client, mut server) = client_pair();
    let (extension_tx, extension_rx) = mpsc::channel();
    let dir = temp_frame_dir("extension-default-tab-read");
    let cleanup_dir = dir.clone();
    std::fs::create_dir_all(&dir).unwrap();
    let gatekeeper = PathBuf::from("/not-used-for-built-in-navigation");
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        let mut generation = 0;
        run_session_with_extension_requests(
            &mut tabs,
            &mut server,
            &dir,
            &mut generation,
            &gatekeeper,
            &extension_rx,
        )
    });

    blueice_ipc::client_handshake(&mut client).unwrap();
    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::Navigate {
            url: "about:credits".to_string(),
        },
    )
    .unwrap();
    assert_eq!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::Navigated {
            url: "about:credits".to_string(),
        }
    );
    assert!(matches!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::FrameReady { .. }
    ));

    let (reply_tx, reply_rx) = mpsc::channel();
    extension_tx
        .send(ExtensionPageRequest::ReadRepresentation {
            tab_id: None,
            reply: reply_tx,
        })
        .unwrap();
    let encoded = reply_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("the live session must answer the extension read")
        .expect("a live default tab must serialize");
    let snapshot: blueice_ipc::AiSnapshot = serde_json::from_str(&encoded).unwrap();
    assert_eq!(snapshot.tab_id, 1);
    assert_eq!(snapshot.url.as_deref(), Some("about:credits"));
    assert!(
        !snapshot.nodes.is_empty(),
        "the core-backed snapshot must be from the navigated credits page, not the empty initial tab"
    );

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    handle.join().unwrap().unwrap();
    let _ = std::fs::remove_dir_all(cleanup_dir);
}

#[test]
fn extension_v2_text_write_updates_the_addressed_input_and_pushes_a_frame() {
    let (mut client, mut server) = client_pair();
    let (extension_tx, extension_rx) = mpsc::channel();
    let dir = temp_frame_dir("extension-v2-text-write");
    let cleanup_dir = dir.clone();
    std::fs::create_dir_all(&dir).unwrap();
    let mut tabs = TabManager::new(320.0, 200.0);
    let tab_id = tabs.default_tab();
    tabs.get_mut(tab_id).unwrap().load_html_str(
        r#"<label for="shared">Shared field</label><input id="shared" type="text" value="before">"#,
        Some("https://example.test/form".to_string()),
    );
    let input_id = tabs
        .get(tab_id)
        .unwrap()
        .script_get_element_by_id("shared")
        .unwrap();
    let gatekeeper = PathBuf::from("/not-used-after-host-review");
    let handle = thread::spawn(move || {
        let mut generation = 0;
        run_session_with_extension_requests(
            &mut tabs,
            &mut server,
            &dir,
            &mut generation,
            &gatekeeper,
            &extension_rx,
        )
    });

    blueice_ipc::client_handshake(&mut client).unwrap();
    let (reply_tx, reply_rx) = mpsc::channel();
    extension_tx
        .send(ExtensionPageRequest::SetTextInputValue {
            tab_id: tab_id.as_u64(),
            node_id: input_id.as_u64(),
            value: "from extension".to_string(),
            grant_generation: 0,
            reply: reply_tx,
        })
        .unwrap();
    reply_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("the live session must answer the extension write")
        .expect("the addressed text input must accept the value");
    let (reply_tab, request_id, frame) =
        blueice_ipc::read_server_message_with_ids(&mut client).unwrap();
    assert_eq!(reply_tab, Some(tab_id.as_u64()));
    assert_eq!(request_id, None);
    assert!(matches!(frame, ServerMessage::FrameReady { .. }));

    let (read_tx, read_rx) = mpsc::channel();
    extension_tx
        .send(ExtensionPageRequest::ReadRepresentation {
            tab_id: Some(tab_id.as_u64()),
            reply: read_tx,
        })
        .unwrap();
    let snapshot: blueice_ipc::AiSnapshot = serde_json::from_str(
        &read_rx
            .recv_timeout(Duration::from_secs(1))
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(snapshot.tab_id, tab_id.as_u64());
    assert_eq!(
        snapshot
            .nodes
            .iter()
            .find(|node| node.id == input_id.as_u64())
            .and_then(|node| node.state.value.as_deref()),
        Some("from extension")
    );

    let (reply_tx, reply_rx) = mpsc::channel();
    extension_tx
        .send(ExtensionPageRequest::SetTextInputValue {
            tab_id: tab_id.as_u64(),
            node_id: input_id.as_u64(),
            value: "x".repeat(blueice_ipc::extension::MAX_TEXT_WRITE_BYTES + 1),
            grant_generation: 0,
            reply: reply_tx,
        })
        .unwrap();
    let error = reply_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("the live session must answer an oversized text-input write")
        .expect_err("the core must reject oversized text-control values");
    assert!(error.contains("4096 bytes"));

    let (read_tx, read_rx) = mpsc::channel();
    extension_tx
        .send(ExtensionPageRequest::ReadRepresentation {
            tab_id: Some(tab_id.as_u64()),
            reply: read_tx,
        })
        .unwrap();
    let snapshot: blueice_ipc::AiSnapshot = serde_json::from_str(
        &read_rx
            .recv_timeout(Duration::from_secs(1))
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        snapshot
            .nodes
            .iter()
            .find(|node| node.id == input_id.as_u64())
            .and_then(|node| node.state.value.as_deref()),
        Some("from extension")
    );

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    handle.join().unwrap().unwrap();
    let _ = std::fs::remove_dir_all(cleanup_dir);
}

#[test]
fn extension_v3_checkbox_write_updates_the_addressed_control_and_pushes_a_frame() {
    let (mut client, mut server) = client_pair();
    let (extension_tx, extension_rx) = mpsc::channel();
    let dir = temp_frame_dir("extension-v3-checkbox-write");
    let cleanup_dir = dir.clone();
    std::fs::create_dir_all(&dir).unwrap();
    let mut tabs = TabManager::new(320.0, 200.0);
    let tab_id = tabs.default_tab();
    tabs.get_mut(tab_id).unwrap().load_html_str(
        r#"<label for="agree">Agree</label><input id="agree" type="checkbox">"#,
        Some("https://example.test/form".to_string()),
    );
    let checkbox_id = tabs
        .get(tab_id)
        .unwrap()
        .script_get_element_by_id("agree")
        .unwrap();
    let gatekeeper = PathBuf::from("/not-used-after-host-review");
    let handle = thread::spawn(move || {
        let mut generation = 0;
        run_session_with_extension_requests(
            &mut tabs,
            &mut server,
            &dir,
            &mut generation,
            &gatekeeper,
            &extension_rx,
        )
    });

    blueice_ipc::client_handshake(&mut client).unwrap();
    let (reply_tx, reply_rx) = mpsc::channel();
    extension_tx
        .send(ExtensionPageRequest::SetCheckboxChecked {
            tab_id: tab_id.as_u64(),
            node_id: checkbox_id.as_u64(),
            checked: true,
            grant_generation: 0,
            reply: reply_tx,
        })
        .unwrap();
    reply_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("the live session must answer the extension checkbox write")
        .expect("the addressed checkbox must accept its checked state");
    let (reply_tab, request_id, frame) =
        blueice_ipc::read_server_message_with_ids(&mut client).unwrap();
    assert_eq!(reply_tab, Some(tab_id.as_u64()));
    assert_eq!(request_id, None);
    assert!(matches!(frame, ServerMessage::FrameReady { .. }));

    let (read_tx, read_rx) = mpsc::channel();
    extension_tx
        .send(ExtensionPageRequest::ReadRepresentation {
            tab_id: Some(tab_id.as_u64()),
            reply: read_tx,
        })
        .unwrap();
    let snapshot: blueice_ipc::AiSnapshot = serde_json::from_str(
        &read_rx
            .recv_timeout(Duration::from_secs(1))
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        snapshot
            .nodes
            .iter()
            .find(|node| node.id == checkbox_id.as_u64())
            .and_then(|node| node.state.checked),
        Some(true)
    );

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    handle.join().unwrap().unwrap();
    let _ = std::fs::remove_dir_all(cleanup_dir);
}

#[test]
fn extension_v4_textarea_write_updates_the_addressed_control_and_pushes_a_frame() {
    let (mut client, mut server) = client_pair();
    let (extension_tx, extension_rx) = mpsc::channel();
    let dir = temp_frame_dir("extension-v4-textarea-write");
    let cleanup_dir = dir.clone();
    std::fs::create_dir_all(&dir).unwrap();
    let mut tabs = TabManager::new(320.0, 200.0);
    let tab_id = tabs.default_tab();
    tabs.get_mut(tab_id).unwrap().load_html_str(
        r#"<label for="notes">Notes</label><textarea id="notes">before</textarea>"#,
        Some("https://example.test/form".to_string()),
    );
    let textarea_id = tabs
        .get(tab_id)
        .unwrap()
        .script_get_element_by_id("notes")
        .unwrap();
    let gatekeeper = PathBuf::from("/not-used-after-host-review");
    let handle = thread::spawn(move || {
        let mut generation = 0;
        run_session_with_extension_requests(
            &mut tabs,
            &mut server,
            &dir,
            &mut generation,
            &gatekeeper,
            &extension_rx,
        )
    });

    blueice_ipc::client_handshake(&mut client).unwrap();
    let (reply_tx, reply_rx) = mpsc::channel();
    extension_tx
        .send(ExtensionPageRequest::SetTextareaValue {
            tab_id: tab_id.as_u64(),
            node_id: textarea_id.as_u64(),
            value: "from extension\nwith detail".to_string(),
            grant_generation: 0,
            reply: reply_tx,
        })
        .unwrap();
    reply_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("the live session must answer the extension textarea write")
        .expect("the addressed textarea must accept its value");
    let (reply_tab, request_id, frame) =
        blueice_ipc::read_server_message_with_ids(&mut client).unwrap();
    assert_eq!(reply_tab, Some(tab_id.as_u64()));
    assert_eq!(request_id, None);
    assert!(matches!(frame, ServerMessage::FrameReady { .. }));

    let (read_tx, read_rx) = mpsc::channel();
    extension_tx
        .send(ExtensionPageRequest::ReadRepresentation {
            tab_id: Some(tab_id.as_u64()),
            reply: read_tx,
        })
        .unwrap();
    let snapshot: blueice_ipc::AiSnapshot = serde_json::from_str(
        &read_rx
            .recv_timeout(Duration::from_secs(1))
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        snapshot
            .nodes
            .iter()
            .find(|node| node.id == textarea_id.as_u64())
            .and_then(|node| node.state.value.as_deref()),
        Some("from extension\nwith detail")
    );

    let (reply_tx, reply_rx) = mpsc::channel();
    extension_tx
        .send(ExtensionPageRequest::SetTextareaValue {
            tab_id: tab_id.as_u64(),
            node_id: textarea_id.as_u64(),
            value: "x".repeat(blueice_ipc::extension::MAX_TEXT_WRITE_BYTES + 1),
            grant_generation: 0,
            reply: reply_tx,
        })
        .unwrap();
    let error = reply_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("the live session must answer an oversized textarea write")
        .expect_err("the core must reject oversized text-control values");
    assert!(error.contains("4096 bytes"));

    let (read_tx, read_rx) = mpsc::channel();
    extension_tx
        .send(ExtensionPageRequest::ReadRepresentation {
            tab_id: Some(tab_id.as_u64()),
            reply: read_tx,
        })
        .unwrap();
    let snapshot: blueice_ipc::AiSnapshot = serde_json::from_str(
        &read_rx
            .recv_timeout(Duration::from_secs(1))
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        snapshot
            .nodes
            .iter()
            .find(|node| node.id == textarea_id.as_u64())
            .and_then(|node| node.state.value.as_deref()),
        Some("from extension\nwith detail")
    );

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    handle.join().unwrap().unwrap();
    let _ = std::fs::remove_dir_all(cleanup_dir);
}

#[test]
fn extension_v8_visible_leaf_write_updates_the_shared_frame_and_representation() {
    let (mut client, mut server) = client_pair();
    let (extension_tx, extension_rx) = mpsc::channel();
    let dir = temp_frame_dir("extension-v8-visible-leaf");
    let cleanup_dir = dir.clone();
    std::fs::create_dir_all(&dir).unwrap();
    let mut tabs = TabManager::new(320.0, 200.0);
    let tab_id = tabs.default_tab();
    tabs.get_mut(tab_id).unwrap().load_html_str(
        "<h1 id=\"headline\">Before</h1>",
        Some("https://example.test/".to_string()),
    );
    let node_id = tabs
        .get(tab_id)
        .unwrap()
        .script_get_element_by_id("headline")
        .unwrap();
    let gatekeeper = PathBuf::from("/not-used-after-host-review");
    let handle = thread::spawn(move || {
        let mut generation = 0;
        run_session_with_extension_requests(
            &mut tabs,
            &mut server,
            &dir,
            &mut generation,
            &gatekeeper,
            &extension_rx,
        )
    });
    blueice_ipc::client_handshake(&mut client).unwrap();
    let (reply_tx, reply_rx) = mpsc::channel();
    extension_tx
        .send(ExtensionPageRequest::SetVisibleLeafText {
            tab_id: tab_id.as_u64(),
            node_id: node_id.as_u64(),
            value: "After".to_string(),
            grant_generation: 0,
            reply: reply_tx,
        })
        .unwrap();
    reply_rx
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap();
    let (frame_tab, frame_request, frame) =
        blueice_ipc::read_server_message_with_ids(&mut client).unwrap();
    assert_eq!(frame_tab, Some(tab_id.as_u64()));
    assert_eq!(frame_request, None);
    assert!(matches!(frame, ServerMessage::FrameReady { .. }));
    let (read_tx, read_rx) = mpsc::channel();
    extension_tx
        .send(ExtensionPageRequest::ReadRepresentation {
            tab_id: Some(tab_id.as_u64()),
            reply: read_tx,
        })
        .unwrap();
    let snapshot: blueice_ipc::AiSnapshot = serde_json::from_str(
        &read_rx
            .recv_timeout(Duration::from_secs(1))
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        snapshot
            .nodes
            .iter()
            .find(|node| node.id == node_id.as_u64())
            .and_then(|node| node.name.as_deref()),
        Some("After")
    );
    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    handle.join().unwrap().unwrap();
    let _ = std::fs::remove_dir_all(cleanup_dir);
}

#[test]
fn extension_origin_scopes_are_checked_against_each_live_tab_before_reads_or_writes() {
    use std::collections::{BTreeMap, BTreeSet};
    let (mut client, mut server) = client_pair();
    let (extension_tx, extension_rx) = mpsc::channel();
    let dir = temp_frame_dir("extension-origin-scopes");
    let cleanup_dir = dir.clone();
    std::fs::create_dir_all(&dir).unwrap();
    let mut tabs = TabManager::new(320.0, 200.0);
    let allowed_tab = tabs.default_tab();
    let blocked_tab = tabs.open_tab();
    tabs.get_mut(allowed_tab).unwrap().load_html_str(
        "<h1 id=\"title\">Allowed</h1>",
        Some("https://allowed.test/page".to_string()),
    );
    tabs.get_mut(blocked_tab).unwrap().load_html_str(
        "<h1 id=\"title\">Blocked</h1>",
        Some("https://blocked.test/page".to_string()),
    );
    let allowed_node = tabs
        .get(allowed_tab)
        .unwrap()
        .script_get_element_by_id("title")
        .unwrap();
    let blocked_node = tabs
        .get(blocked_tab)
        .unwrap()
        .script_get_element_by_id("title")
        .unwrap();
    tabs.set_extension_capability_origins(BTreeMap::from([
        (
            "dom:read".to_string(),
            BTreeSet::from(["https://allowed.test".to_string()]),
        ),
        (
            "dom:write".to_string(),
            BTreeSet::from(["https://allowed.test".to_string()]),
        ),
        (
            "network:observe".to_string(),
            BTreeSet::from(["https://allowed.test".to_string()]),
        ),
    ]));
    let gatekeeper = PathBuf::from("/not-used-after-host-review");
    let handle = thread::spawn(move || {
        let mut generation = 0;
        run_session_with_extension_requests(
            &mut tabs,
            &mut server,
            &dir,
            &mut generation,
            &gatekeeper,
            &extension_rx,
        )
    });
    blueice_ipc::client_handshake(&mut client).unwrap();
    for (tab_id, permitted) in [(blocked_tab, false), (allowed_tab, true)] {
        let (reply_tx, reply_rx) = mpsc::channel();
        extension_tx
            .send(ExtensionPageRequest::ReadRepresentation {
                tab_id: Some(tab_id.as_u64()),
                reply: reply_tx,
            })
            .unwrap();
        assert_eq!(
            reply_rx
                .recv_timeout(Duration::from_secs(1))
                .unwrap()
                .is_ok(),
            permitted
        );
        let (reply_tx, reply_rx) = mpsc::channel();
        extension_tx
            .send(ExtensionPageRequest::ReadNetworkResponse {
                tab_id: tab_id.as_u64(),
                reply: reply_tx,
            })
            .unwrap();
        assert_eq!(
            reply_rx
                .recv_timeout(Duration::from_secs(1))
                .unwrap()
                .is_ok(),
            permitted
        );
    }
    let (reply_tx, reply_rx) = mpsc::channel();
    extension_tx
        .send(ExtensionPageRequest::SetVisibleLeafText {
            tab_id: blocked_tab.as_u64(),
            node_id: blocked_node.as_u64(),
            value: "must not change".to_string(),
            grant_generation: 0,
            reply: reply_tx,
        })
        .unwrap();
    assert!(reply_rx
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap_err()
        .contains("not granted"));
    let (reply_tx, reply_rx) = mpsc::channel();
    extension_tx
        .send(ExtensionPageRequest::SetVisibleLeafText {
            tab_id: allowed_tab.as_u64(),
            node_id: allowed_node.as_u64(),
            value: "Updated".to_string(),
            grant_generation: 0,
            reply: reply_tx,
        })
        .unwrap();
    reply_rx
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap();
    let (frame_tab, _, frame) = blueice_ipc::read_server_message_with_ids(&mut client).unwrap();
    assert_eq!(
        frame_tab,
        Some(allowed_tab.as_u64()),
        "a rejected write must not publish a frame"
    );
    assert!(matches!(frame, ServerMessage::FrameReady { .. }));
    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    handle.join().unwrap().unwrap();
    let _ = std::fs::remove_dir_all(cleanup_dir);
}
