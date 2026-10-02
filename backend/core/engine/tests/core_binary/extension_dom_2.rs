// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn installed_extension_v9_writes_controls_and_reviewed_visible_text_after_gatekeeper_review() {
    let _guard = core_process_test_guard();
    use blueice_ipc::extension::{
        read_extension_reply, write_extension_request, ExtensionReply, ExtensionRequest,
    };
    use std::collections::BTreeMap;

    // macOS leaves little room below its long per-user temporary root; the
    // PID in `unique_socket_path` still keeps these concise leaves unique.
    let core_socket = unique_socket_path("ev2c");
    let extension_socket = unique_private_extension_socket_path("write");
    let frame_dir = std::env::temp_dir().join(format!(
        "blueice-core-extension-v5-frames-{}",
        std::process::id()
    ));
    let (package_root, manifest, extension_id) =
        extension_manifest_package("real-write", &["dom:read", "dom:write"]);
    let gatekeeper_socket = clearing_gatekeeper("ev2g");
    let _ = std::fs::remove_file(&core_socket);
    let _ = std::fs::remove_file(&extension_socket);
    let _ = std::fs::remove_dir_all(&frame_dir);

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buf = [0u8; 1024];
        let _ = stream.read(&mut buf);
        let body = r#"<h1 id="headline">Before</h1><p id="formatted">Before <strong>bold <em>and italic</em></strong></p><p id="linked">Before <a href="/next">link</a></p><label for="shared">Shared value</label><input id="shared" type="text" value="before"><label for="agree">Agree</label><input id="agree" type="checkbox"><label for="notes">Notes</label><textarea id="notes">before</textarea><label for="volume">Volume</label><input id="volume" type="range" min="0" max="10" step="2" value="0"><label for="first-priority">First priority</label><input id="first-priority" type="radio" name="priority" checked><label for="second-priority">Second priority</label><input id="second-priority" type="radio" name="priority"><label for="urgency">Urgency</label><select id="urgency"><option id="first-option" selected>First option</option><option id="second-option">Second option</option></select>"#;
        stream
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            )
            .unwrap();
    });

    let mut core = Command::new(env!("CARGO_BIN_EXE_blueice-core"))
        .args([
            "--socket",
            core_socket.to_str().unwrap(),
            "--extension-socket",
            extension_socket.to_str().unwrap(),
            "--extension-manifest",
            manifest.to_str().unwrap(),
            "--gatekeeper-socket",
            gatekeeper_socket.to_str().unwrap(),
            "--frame-dir",
            frame_dir.to_str().unwrap(),
        ])
        .spawn()
        .expect("failed to spawn core with a v6 installed extension");

    assert!(wait_for(&core_socket, Duration::from_secs(5)));
    assert!(wait_for(&extension_socket, Duration::from_secs(5)));
    let mut frontend = UnixStream::connect(&core_socket).unwrap();
    blueice_ipc::client_handshake(&mut frontend).unwrap();
    blueice_ipc::write_client_message(
        &mut frontend,
        &blueice_ipc::ClientMessage::Navigate {
            url: format!("http://{addr}"),
        },
    )
    .unwrap();
    assert!(matches!(
        blueice_ipc::read_server_message(&mut frontend).unwrap(),
        blueice_ipc::ServerMessage::Navigated { .. }
    ));
    assert!(matches!(
        blueice_ipc::read_server_message(&mut frontend).unwrap(),
        blueice_ipc::ServerMessage::FrameReady { .. }
    ));
    blueice_ipc::write_client_message(
        &mut frontend,
        &blueice_ipc::ClientMessage::GetRepresentation,
    )
    .unwrap();
    let (
        input_id,
        checkbox_id,
        textarea_id,
        first_radio_id,
        second_radio_id,
        first_option_id,
        second_option_id,
        range_id,
        headline_id,
        formatted_id,
        linked_id,
    ) = match blueice_ipc::read_server_message(&mut frontend).unwrap() {
        blueice_ipc::ServerMessage::Representation(snapshot) => {
            let input_id = snapshot
                .nodes
                .iter()
                .find(|node| matches!(node.role, blueice_ipc::Role::TextBox))
                .expect("the navigated form must expose its text input")
                .id;
            let checkbox_id = snapshot
                .nodes
                .iter()
                .find(|node| matches!(node.role, blueice_ipc::Role::CheckBox))
                .expect("the navigated form must expose its checkbox")
                .id;
            let textarea_id = snapshot
                .nodes
                .iter()
                .find(|node| {
                    matches!(node.role, blueice_ipc::Role::TextBox)
                        && node.name.as_deref() == Some("Notes")
                })
                .expect("the navigated form must expose its textarea")
                .id;
            let first_radio_id = snapshot
                .nodes
                .iter()
                .find(|node| node.name.as_deref() == Some("First priority"))
                .expect("the navigated form must expose its first radio")
                .id;
            let second_radio_id = snapshot
                .nodes
                .iter()
                .find(|node| node.name.as_deref() == Some("Second priority"))
                .expect("the navigated form must expose its second radio")
                .id;
            let first_option_id = snapshot
                .nodes
                .iter()
                .find(|node| {
                    matches!(node.role, blueice_ipc::Role::Option)
                        && node.name.as_deref() == Some("First option")
                })
                .expect("the navigated form must expose its first select option")
                .id;
            let second_option_id = snapshot
                .nodes
                .iter()
                .find(|node| {
                    matches!(node.role, blueice_ipc::Role::Option)
                        && node.name.as_deref() == Some("Second option")
                })
                .expect("the navigated form must expose its second select option")
                .id;
            let range_id = snapshot
                .nodes
                .iter()
                .find(|node| {
                    matches!(node.role, blueice_ipc::Role::Slider)
                        && node.name.as_deref() == Some("Volume")
                })
                .expect("the navigated form must expose its integer range input")
                .id;
            let headline_id = snapshot
                .nodes
                .iter()
                .find(|node| {
                    matches!(node.role, blueice_ipc::Role::Heading { .. })
                        && node.name.as_deref() == Some("Before")
                })
                .expect("the page must expose its heading")
                .id;
            let formatted_id = snapshot
                .nodes
                .iter()
                .find(|node| {
                    matches!(node.role, blueice_ipc::Role::Paragraph)
                        && node
                            .name
                            .as_deref()
                            .is_some_and(|name| name.starts_with("Before bold"))
                })
                .expect("the page must expose its formatted paragraph")
                .id;
            let linked_id = snapshot
                .nodes
                .iter()
                .rfind(|node| matches!(node.role, blueice_ipc::Role::Paragraph))
                .expect("the page must expose its linked paragraph")
                .id;
            (
                input_id,
                checkbox_id,
                textarea_id,
                first_radio_id,
                second_radio_id,
                first_option_id,
                second_option_id,
                range_id,
                headline_id,
                formatted_id,
                linked_id,
            )
        }
        other => panic!("expected the input representation, got {other:?}"),
    };

    let mut extension = UnixStream::connect(&extension_socket).unwrap();
    write_extension_request(
        &mut extension,
        &ExtensionRequest::Hello {
            extension_id,
            capability_versions: BTreeMap::from([
                ("dom:read".to_string(), 2),
                ("dom:write".to_string(), 9),
            ]),
        },
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut extension).unwrap(),
        ExtensionReply::HelloAck {
            unsupported_capabilities: BTreeMap::new(),
        }
    );
    write_extension_request(
        &mut extension,
        &ExtensionRequest::SetTextInputValue {
            tab_id: 1,
            node_id: input_id,
            value: "from extension v2".to_string(),
        },
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut extension).unwrap(),
        ExtensionReply::DomWriteAck
    );
    let (reply_tab, request_id, frame) =
        blueice_ipc::read_server_message_with_ids(&mut frontend).unwrap();
    assert_eq!(reply_tab, Some(1));
    assert_eq!(request_id, None);
    assert!(matches!(
        frame,
        blueice_ipc::ServerMessage::FrameReady { .. }
    ));

    write_extension_request(
        &mut extension,
        &ExtensionRequest::SetVisibleLeafText {
            tab_id: 1,
            node_id: headline_id,
            value: "After".to_string(),
        },
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut extension).unwrap(),
        ExtensionReply::DomWriteAck
    );
    let (reply_tab, request_id, frame) =
        blueice_ipc::read_server_message_with_ids(&mut frontend).unwrap();
    assert_eq!(reply_tab, Some(1));
    assert_eq!(request_id, None);
    assert!(matches!(
        frame,
        blueice_ipc::ServerMessage::FrameReady { .. }
    ));

    write_extension_request(
        &mut extension,
        &ExtensionRequest::SetVisibleTextContent {
            tab_id: 1,
            node_id: formatted_id,
            value: "After format".to_string(),
        },
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut extension).unwrap(),
        ExtensionReply::DomWriteAck
    );
    let (reply_tab, request_id, frame) =
        blueice_ipc::read_server_message_with_ids(&mut frontend).unwrap();
    assert_eq!(reply_tab, Some(1));
    assert_eq!(request_id, None);
    assert!(matches!(
        frame,
        blueice_ipc::ServerMessage::FrameReady { .. }
    ));

    write_extension_request(
        &mut extension,
        &ExtensionRequest::SetVisibleTextContent {
            tab_id: 1,
            node_id: linked_id,
            value: "Must not remove link".to_string(),
        },
    )
    .unwrap();
    assert!(matches!(read_extension_reply(&mut extension).unwrap(),
        ExtensionReply::OperationUnavailable { capability, reason }
            if capability == "dom:write" && reason.contains("inline formatting")));
    write_extension_request(
        &mut extension,
        &ExtensionRequest::SetVisibleTextContent {
            tab_id: 1,
            node_id: formatted_id,
            value: "Enter your password".to_string(),
        },
    )
    .unwrap();
    assert!(matches!(read_extension_reply(&mut extension).unwrap(),
        ExtensionReply::GatekeeperBlocked { capability, category, .. }
            if capability == "dom:write" && category == "extension-visible-text-social-engineering"));

    write_extension_request(
        &mut extension,
        &ExtensionRequest::SetRangeInputValue {
            tab_id: 1,
            node_id: range_id,
            value: 6,
        },
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut extension).unwrap(),
        ExtensionReply::DomWriteAck
    );
    let (reply_tab, request_id, frame) =
        blueice_ipc::read_server_message_with_ids(&mut frontend).unwrap();
    assert_eq!(reply_tab, Some(1));
    assert_eq!(request_id, None);
    assert!(matches!(
        frame,
        blueice_ipc::ServerMessage::FrameReady { .. }
    ));

    write_extension_request(
        &mut extension,
        &ExtensionRequest::SelectOption {
            tab_id: 1,
            node_id: second_option_id,
        },
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut extension).unwrap(),
        ExtensionReply::DomWriteAck
    );
    let (reply_tab, request_id, frame) =
        blueice_ipc::read_server_message_with_ids(&mut frontend).unwrap();
    assert_eq!(reply_tab, Some(1));
    assert_eq!(request_id, None);
    assert!(matches!(
        frame,
        blueice_ipc::ServerMessage::FrameReady { .. }
    ));

    write_extension_request(
        &mut extension,
        &ExtensionRequest::SetRadioChecked {
            tab_id: 1,
            node_id: second_radio_id,
        },
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut extension).unwrap(),
        ExtensionReply::DomWriteAck
    );
    let (reply_tab, request_id, frame) =
        blueice_ipc::read_server_message_with_ids(&mut frontend).unwrap();
    assert_eq!(reply_tab, Some(1));
    assert_eq!(request_id, None);
    assert!(matches!(
        frame,
        blueice_ipc::ServerMessage::FrameReady { .. }
    ));

    write_extension_request(
        &mut extension,
        &ExtensionRequest::SetCheckboxChecked {
            tab_id: 1,
            node_id: checkbox_id,
            checked: true,
        },
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut extension).unwrap(),
        ExtensionReply::DomWriteAck
    );
    let (reply_tab, request_id, frame) =
        blueice_ipc::read_server_message_with_ids(&mut frontend).unwrap();
    assert_eq!(reply_tab, Some(1));
    assert_eq!(request_id, None);
    assert!(matches!(
        frame,
        blueice_ipc::ServerMessage::FrameReady { .. }
    ));

    write_extension_request(
        &mut extension,
        &ExtensionRequest::SetTextareaValue {
            tab_id: 1,
            node_id: textarea_id,
            value: "from extension v4\nwith detail".to_string(),
        },
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut extension).unwrap(),
        ExtensionReply::DomWriteAck
    );
    let (reply_tab, request_id, frame) =
        blueice_ipc::read_server_message_with_ids(&mut frontend).unwrap();
    assert_eq!(reply_tab, Some(1));
    assert_eq!(request_id, None);
    assert!(matches!(
        frame,
        blueice_ipc::ServerMessage::FrameReady { .. }
    ));

    write_extension_request(&mut extension, &ExtensionRequest::DomReadTab { tab_id: 1 }).unwrap();
    let snapshot = match read_extension_reply(&mut extension).unwrap() {
        ExtensionReply::DomReadResult { value } => {
            serde_json::from_str::<blueice_ipc::AiSnapshot>(&value).unwrap()
        }
        other => panic!("expected the written core snapshot, got {other:?}"),
    };
    assert_eq!(snapshot.tab_id, 1);
    assert_eq!(
        snapshot
            .nodes
            .iter()
            .find(|node| node.id == input_id)
            .and_then(|node| node.state.value.as_deref()),
        Some("from extension v2")
    );
    assert_eq!(
        snapshot
            .nodes
            .iter()
            .find(|node| node.id == checkbox_id)
            .and_then(|node| node.state.checked),
        Some(true)
    );
    assert_eq!(
        snapshot
            .nodes
            .iter()
            .find(|node| node.id == first_option_id)
            .map(|node| node.state.selected),
        Some(false)
    );
    assert_eq!(
        snapshot
            .nodes
            .iter()
            .find(|node| node.id == second_option_id)
            .map(|node| node.state.selected),
        Some(true)
    );
    assert_eq!(
        snapshot
            .nodes
            .iter()
            .find(|node| node.id == textarea_id)
            .and_then(|node| node.state.value.as_deref()),
        Some("from extension v4 with detail")
    );
    assert_eq!(
        snapshot
            .nodes
            .iter()
            .find(|node| node.id == range_id)
            .and_then(|node| node.state.value.as_deref()),
        Some("6")
    );
    assert_eq!(
        snapshot
            .nodes
            .iter()
            .find(|node| node.id == headline_id)
            .and_then(|node| node.name.as_deref()),
        Some("After")
    );
    assert_eq!(
        snapshot
            .nodes
            .iter()
            .find(|node| node.id == formatted_id)
            .and_then(|node| node.name.as_deref()),
        Some("After format")
    );
    assert_eq!(
        snapshot
            .nodes
            .iter()
            .find(|node| node.id == linked_id)
            .and_then(|node| node.name.as_deref()),
        Some("Before")
    );
    assert_eq!(
        snapshot
            .nodes
            .iter()
            .find(|node| node.id == first_radio_id)
            .and_then(|node| node.state.checked),
        Some(false)
    );
    assert_eq!(
        snapshot
            .nodes
            .iter()
            .find(|node| node.id == second_radio_id)
            .and_then(|node| node.state.checked),
        Some(true)
    );

    blueice_ipc::write_client_message(&mut frontend, &blueice_ipc::ClientMessage::Shutdown)
        .unwrap();
    assert!(core.wait().unwrap().success());
    assert!(!core_socket.exists());
    assert!(!extension_socket.exists());
    assert!(!frame_dir.exists());
    let _ = std::fs::remove_dir_all(package_root);
    let _ = std::fs::remove_file(gatekeeper_socket);
}
