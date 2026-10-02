// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;
use std::io::Cursor;
use std::net::{TcpListener, TcpStream};

#[test]
fn native_text_actions_round_trip_with_document_and_focus_fences() {
    use crate::input::{PageKey, TextInputAction, TextInputContext, TextMovement, TextRange};
    let range = TextRange {
        location: 1,
        length: 2,
    };
    let context = TextInputContext {
        version: 1,
        frame_source: 19,
        document_generation: 7,
        focus_generation: 3,
    };
    let actions = [
        TextInputAction::Key {
            key: PageKey::Tab,
            shift: true,
        },
        TextInputAction::Replace {
            text: "中文😀".into(),
            replacement: Some(range),
        },
        TextInputAction::Compose {
            text: "שלום".into(),
            selection: TextRange {
                location: 4,
                length: 0,
            },
            replacement: None,
        },
        TextInputAction::FinishComposition,
        TextInputAction::CancelComposition,
        TextInputAction::Select { range },
        TextInputAction::SelectAll,
        TextInputAction::Move {
            direction: TextMovement::WordBackward,
            extend: true,
        },
        TextInputAction::Delete { forward: false },
        TextInputAction::Pointer {
            x: 12.5,
            y: 30.0,
            extend: false,
            click_count: 2,
        },
    ];
    for action in actions {
        let expected = ClientMessage::TextInput { context, action };
        let mut bytes = Vec::new();
        write_client_message(&mut bytes, &expected).unwrap();
        assert_eq!(
            read_client_message(&mut Cursor::new(bytes)).unwrap(),
            expected
        );
    }
    let mut bytes = Vec::new();
    write_client_message(&mut bytes, &ClientMessage::GetTextInputState).unwrap();
    assert_eq!(
        read_client_message(&mut Cursor::new(bytes)).unwrap(),
        ClientMessage::GetTextInputState
    );
}

#[test]
fn native_ranges_check_overflow_and_password_state_stays_redacted_on_wire() {
    use crate::input::{TextControlState, TextInputState, TextRange};
    assert_eq!(
        TextRange {
            location: u32::MAX,
            length: 1
        }
        .end(),
        None
    );
    let bounds = Bounds {
        x: 1.0,
        y: 2.0,
        width: 100.0,
        height: 20.0,
    };
    let state = TextInputState {
        version: 1,
        frame_source: 19,
        document_generation: 7,
        focus_generation: 3,
        frame_generation: 8,
        tab_id: 2,
        scroll_y: 0.0,
        focused_node: Some(5),
        focus_exit: None,
        focused: Some(TextControlState {
            node_id: 5,
            text: None,
            text_length: 9,
            protected: true,
            writable: true,
            multiline: false,
            selection: TextRange {
                location: 9,
                length: 0,
            },
            marked: None,
            bounds,
            caret: bounds,
            carets: Vec::new(),
            selection_rects: Vec::new(),
        }),
    };
    let expected = ServerMessage::TextInputState(state);
    let mut bytes = Vec::new();
    write_server_message(&mut bytes, &expected).unwrap();
    assert_eq!(
        read_server_message(&mut Cursor::new(bytes.clone())).unwrap(),
        expected
    );
    assert!(std::str::from_utf8(&bytes[4..])
        .unwrap()
        .contains("\"text\":null"));
}

#[test]
fn older_node_state_defaults_native_input_and_protected_capabilities() {
    let state: NodeState = serde_json::from_str(
        r#"{"checked":null,"disabled":false,"required":false,"selected":false,"hovered":false,"focused":false}"#,
    )
    .unwrap();
    assert!(!state.native_text_input);
    assert!(!state.native_focusable);
    assert!(!state.radio);
    assert!(!state.protected);
}

/// Exercises framing across a real OS socket on every supported platform.
/// TCP keeps this protocol-boundary seam available on Windows while the
/// Unix-domain-socket services themselves remain Unix-only.
fn tcp_pair() -> (TcpStream, TcpStream) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let client = TcpStream::connect(address).unwrap();
    let (server, _) = listener.accept().unwrap();
    (client, server)
}

#[test]
fn client_message_round_trips_through_the_wire_format() {
    for msg in [
        ClientMessage::Hello {
            protocol_version: PROTOCOL_VERSION,
        },
        ClientMessage::Navigate {
            url: "https://example.com".to_string(),
        },
        ClientMessage::GoBack,
        ClientMessage::GoForward,
        ClientMessage::Resize {
            width: 800,
            height: 600,
        },
        ClientMessage::Click { x: 12.5, y: 30.0 },
        ClientMessage::Scroll { delta_y: -40.0 },
        ClientMessage::InsertText {
            text: "BlueIce".to_string(),
        },
        ClientMessage::DeleteBackward,
        ClientMessage::Chrome(ChromeCommand::SetVisible(false)),
        ClientMessage::Hover { x: 5.0, y: 6.0 },
        ClientMessage::GetRepresentation,
        ClientMessage::ActOn {
            id: 7,
            action: NodeAction::Click,
        },
        ClientMessage::Highlight { id: Some(7) },
        ClientMessage::Highlight { id: None },
        ClientMessage::GetDom,
        ClientMessage::GetBlueTsScriptReports,
        ClientMessage::GetBlueJsScriptReports,
        ClientMessage::OpenTab {
            url: Some("https://example.com".to_string()),
        },
        ClientMessage::OpenTab { url: None },
        ClientMessage::CloseTab,
        ClientMessage::ListTabs,
        ClientMessage::SetTranslationLanguage {
            target_language: Some("zh-TW".to_string()),
        },
        ClientMessage::SetTranslationLanguage {
            target_language: None,
        },
        ClientMessage::ShowTranslation { shown: false },
        ClientMessage::GetTranslationState,
        ClientMessage::SummarizePage,
        ClientMessage::OrganizePage {
            instruction: "make a table".to_string(),
        },
        ClientMessage::ActivateExtensionPopupAction { popup_id: 42 },
        ClientMessage::CreateTabGroup {
            name: "Research".to_string(),
            color: "#4f8cff".to_string(),
        },
        ClientMessage::SetTabGroup { group_id: Some(3) },
        ClientMessage::SetTabGroup { group_id: None },
        ClientMessage::RenameTabGroup {
            group_id: 3,
            name: "Reference".to_string(),
        },
        ClientMessage::SetTabGroupColor {
            group_id: 3,
            color: "#ff6600".to_string(),
        },
        ClientMessage::SetTabGroupCollapsed {
            group_id: 3,
            collapsed: true,
        },
        ClientMessage::CloseTabGroup { group_id: 3 },
        ClientMessage::ListTabGroups,
        ClientMessage::Shutdown,
        ClientMessage::Unknown,
    ] {
        let mut buf = Vec::new();
        write_client_message(&mut buf, &msg).unwrap();
        let mut cursor = Cursor::new(buf);
        assert_eq!(read_client_message(&mut cursor).unwrap(), msg);
    }
}

#[test]
fn server_message_round_trips_through_the_wire_format() {
    for msg in [
        ServerMessage::Hello {
            protocol_version: PROTOCOL_VERSION,
        },
        ServerMessage::FrameReady {
            shm_path: "/dev/shm/blueice-1".to_string(),
            width: 800,
            height: 600,
            generation: 42,
        },
        ServerMessage::Navigated {
            url: "https://example.com/".to_string(),
        },
        ServerMessage::AssistantResult {
            kind: AssistantTaskKind::Organized,
            text: "| a | 1 |".to_string(),
        },
        ServerMessage::TranslationState {
            language: Some("zh-TW".to_string()),
            available: true,
            shown: false,
        },
        ServerMessage::HistoryState {
            can_go_back: true,
            can_go_forward: false,
        },
        ServerMessage::Representation(AiSnapshot {
            frame_source: 0,
            generation: 42,
            tab_id: 1,
            url: Some("https://example.com/".to_string()),
            scroll_y: 10.0,
            nodes: vec![AiNode {
                id: 3,
                parent: None,
                children: vec![],
                role: Role::Link,
                name: Some("Example".to_string()),
                name_from: Some(NameFrom::Contents),
                original_name: None,
                state: NodeState {
                    hovered: true,
                    ..Default::default()
                },
                bounds: Bounds {
                    x: 0.0,
                    y: 0.0,
                    width: 10.0,
                    height: 5.0,
                },
                opacity: 1.0,
                occluded: false,
                occluded_by: None,
                occluded_fraction: 0.0,
            }],
        }),
        ServerMessage::Dom("| <html>\n".to_string()),
        ServerMessage::BlueTsScriptReports(vec![BlueTsScriptExecutionReport {
            tab_id: 2,
            document_generation: 42,
            ordinal: 0,
            kind: BlueTsScriptKind::Classic,
            policy: BlueTsScriptRuntimePolicy::Checked,
            source_position: None,
            outcome: BlueTsScriptExecutionOutcome::Executed,
        }]),
        ServerMessage::BlueJsScriptReports(vec![BlueJsScriptExecutionReport {
            tab_id: 2,
            document_generation: 42,
            ordinal: 0,
            kind: BlueJsScriptKind::Classic,
            outcome: BlueJsScriptExecutionOutcome::Executed,
        }]),
        ServerMessage::BlueJsScriptReports(vec![BlueJsScriptExecutionReport {
            tab_id: 2,
            document_generation: 42,
            ordinal: 1,
            kind: BlueJsScriptKind::Module,
            outcome: BlueJsScriptExecutionOutcome::Rejected {
                category: "BlueJS compilation rejected the page script".to_string(),
            },
        }]),
        ServerMessage::BlueTsScriptReports(vec![BlueTsScriptExecutionReport {
            tab_id: 2,
            document_generation: 42,
            ordinal: 1,
            kind: BlueTsScriptKind::Module,
            policy: BlueTsScriptRuntimePolicy::StrictRuntime,
            source_position: Some(BlueTsScriptSourcePosition { start: 3, end: 8 }),
            outcome: BlueTsScriptExecutionOutcome::Rejected {
                category: "BlueTS compilation rejected the page script".to_string(),
            },
        }]),
        ServerMessage::TabOpened {
            tab_id: 2,
            url: Some("https://example.com/".to_string()),
        },
        ServerMessage::TabOpened {
            tab_id: 2,
            url: None,
        },
        ServerMessage::TabClosed { tab_id: 2 },
        ServerMessage::ExtensionPopup {
            popup: Some(ExtensionPopup {
                id: 42,
                tab_id: 2,
                title: "Notes".to_string(),
                body: "Ready".to_string(),
                action_label: Some("Open".to_string()),
            }),
        },
        ServerMessage::Tabs(vec![
            TabSummary {
                id: 1,
                url: None,
                group_id: None,
            },
            TabSummary {
                id: 2,
                url: Some("https://example.com/".to_string()),
                group_id: Some(3),
            },
        ]),
        ServerMessage::TabGroupCreated(TabGroupSummary {
            id: 3,
            name: "Research".to_string(),
            color: "#4f8cff".to_string(),
            collapsed: false,
        }),
        ServerMessage::TabGroupUpdated(TabGroupSummary {
            id: 3,
            name: "Reference".to_string(),
            color: "#ff6600".to_string(),
            collapsed: true,
        }),
        ServerMessage::TabGroupAssigned {
            tab_id: 2,
            group_id: Some(3),
        },
        ServerMessage::TabGroupClosed { group_id: 3 },
        ServerMessage::TabGroups(vec![TabGroupSummary {
            id: 3,
            name: "Reference".to_string(),
            color: "#ff6600".to_string(),
            collapsed: true,
        }]),
        ServerMessage::Error {
            message: "oops".to_string(),
        },
        ServerMessage::GatekeeperBlocked {
            reason: "hidden instruction-shaped text".to_string(),
            category: "prompt-injection".to_string(),
            url: "https://example.com/".to_string(),
        },
        ServerMessage::Unknown,
    ] {
        let mut buf = Vec::new();
        write_server_message(&mut buf, &msg).unwrap();
        let mut cursor = Cursor::new(buf);
        assert_eq!(read_server_message(&mut cursor).unwrap(), msg);
    }
}

#[test]
fn multiple_messages_can_be_written_and_read_in_sequence_on_one_stream() {
    let mut buf = Vec::new();
    write_client_message(
        &mut buf,
        &ClientMessage::Resize {
            width: 1,
            height: 2,
        },
    )
    .unwrap();
    write_client_message(&mut buf, &ClientMessage::Shutdown).unwrap();
    let mut cursor = Cursor::new(buf);
    assert_eq!(
        read_client_message(&mut cursor).unwrap(),
        ClientMessage::Resize {
            width: 1,
            height: 2
        }
    );
    assert_eq!(
        read_client_message(&mut cursor).unwrap(),
        ClientMessage::Shutdown
    );
}

#[test]
fn reading_past_a_truncated_stream_is_an_io_error_not_a_panic() {
    let mut buf = Vec::new();
    write_client_message(&mut buf, &ClientMessage::Shutdown).unwrap();
    buf.truncate(buf.len() - 1); // cut off the last byte of the payload
    let mut cursor = Cursor::new(buf);
    assert!(read_client_message::<_>(&mut cursor).is_err());
}

#[test]
fn reading_from_an_empty_stream_is_an_error() {
    let mut cursor = Cursor::new(Vec::<u8>::new());
    let result: io::Result<ClientMessage> = read_client_message(&mut cursor);
    assert!(result.is_err());
}

#[test]
fn reading_malformed_json_payload_is_an_error() {
    let mut buf = Vec::new();
    let bad_payload = b"not json";
    buf.extend_from_slice(&(bad_payload.len() as u32).to_le_bytes());
    buf.extend_from_slice(bad_payload);
    let mut cursor = Cursor::new(buf);
    let result: io::Result<ClientMessage> = read_client_message(&mut cursor);
    assert!(result.is_err());
}

#[test]
fn an_oversized_length_prefix_is_refused_without_allocating_it() {
    let mut cursor = Cursor::new((MAX_FRAME_BYTES as u32 + 1).to_le_bytes().to_vec());
    let error = read_client_message::<_>(&mut cursor).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert!(error.to_string().contains("maximum"));
}

#[test]
fn real_unix_domain_socket_round_trip() {
    // TEST_PLAN.md's "UI testing strategy": core<->frontend IPC-
    // boundary tests should drive the real protocol with a test
    // client, not just an in-memory buffer -- this is that test,
    // now actionable since Phase 4 is underway.
    let (mut a, mut b) = tcp_pair();

    let sent = ClientMessage::Navigate {
        url: "https://example.com".to_string(),
    };
    write_client_message(&mut a, &sent).unwrap();
    let received = read_client_message(&mut b).unwrap();
    assert_eq!(received, sent);

    let sent = ServerMessage::FrameReady {
        shm_path: "/tmp/x".to_string(),
        width: 10,
        height: 20,
        generation: 1,
    };
    write_server_message(&mut b, &sent).unwrap();
    let received = read_server_message(&mut a).unwrap();
    assert_eq!(received, sent);
}

#[test]
fn a_message_written_without_a_request_id_reads_back_as_none() {
    let mut buf = Vec::new();
    write_client_message(&mut buf, &ClientMessage::Shutdown).unwrap();
    let mut cursor = Cursor::new(buf);
    assert_eq!(
        read_client_message_with_id(&mut cursor).unwrap(),
        (None, ClientMessage::Shutdown)
    );
}

#[test]
fn a_request_id_round_trips_for_both_a_unit_and_a_struct_variant() {
    // Regression coverage for the envelope shape specifically: unit
    // variants (`Shutdown`) serialize as a bare JSON string under
    // the default externally-tagged representation, unlike struct
    // variants (`Navigate`) -- both must still carry a request_id
    // through the same envelope.
    for msg in [
        ClientMessage::Shutdown,
        ClientMessage::Navigate {
            url: "https://example.com".to_string(),
        },
    ] {
        let mut buf = Vec::new();
        write_client_message_with_id(&mut buf, Some(42), &msg).unwrap();
        let mut cursor = Cursor::new(buf);
        assert_eq!(
            read_client_message_with_id(&mut cursor).unwrap(),
            (Some(42), msg)
        );
    }

    let mut buf = Vec::new();
    let reply = ServerMessage::Representation(AiSnapshot {
        frame_source: 0,
        generation: 1,
        tab_id: 1,
        url: None,
        scroll_y: 0.0,
        nodes: vec![],
    });
    write_server_message_with_id(&mut buf, Some(7), &reply).unwrap();
    let mut cursor = Cursor::new(buf);
    assert_eq!(
        read_server_message_with_id(&mut cursor).unwrap(),
        (Some(7), reply)
    );
}

#[test]
fn a_tab_id_round_trips_alongside_a_request_id() {
    let msg = ClientMessage::Navigate {
        url: "https://example.com".to_string(),
    };
    let mut buf = Vec::new();
    write_client_message_with_ids(&mut buf, Some(3), Some(42), &msg).unwrap();
    let mut cursor = Cursor::new(buf);
    assert_eq!(
        read_client_message_with_ids(&mut cursor).unwrap(),
        (Some(3), Some(42), msg)
    );

    let reply = ServerMessage::Navigated {
        url: "https://example.com".to_string(),
    };
    let mut buf = Vec::new();
    write_server_message_with_ids(&mut buf, Some(3), Some(42), &reply).unwrap();
    let mut cursor = Cursor::new(buf);
    assert_eq!(
        read_server_message_with_ids(&mut cursor).unwrap(),
        (Some(3), Some(42), reply)
    );
}

#[test]
fn a_message_written_without_a_tab_id_reads_back_as_none() {
    // The load-bearing backward-compatibility property Milestone C
    // depends on: a client that never addresses a specific tab (or
    // sent before Phase 16 existed) must resolve to the default tab
    // transparently -- proven at the wire level here, and at the
    // `session.rs` dispatch level in `blueice_engine`'s own tests.
    let mut buf = Vec::new();
    write_client_message(&mut buf, &ClientMessage::GetRepresentation).unwrap();
    let mut cursor = Cursor::new(buf);
    assert_eq!(
        read_client_message_with_ids(&mut cursor).unwrap(),
        (None, None, ClientMessage::GetRepresentation)
    );
}

#[test]
fn an_older_tabs_reply_without_additive_fields_defaults_safely() {
    // Group membership and history availability are additive Phase-16
    // fields, so a newer frontend/MCP client must still accept a reply
    // from a core that predates either addition.
    let payload = br#"{"message":{"Tabs":[{"id":1,"url":null}]}}"#;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    bytes.extend_from_slice(payload);
    let mut cursor = Cursor::new(bytes);
    assert_eq!(
        read_server_message(&mut cursor).unwrap(),
        ServerMessage::Tabs(vec![TabSummary {
            id: 1,
            url: None,
            group_id: None,
        }])
    );
}

#[test]
fn tab_id_and_request_id_are_independent_of_each_other() {
    // A message can carry either, neither, or both -- proven with
    // tab_id present but request_id absent, the case the two other
    // round-trip tests above don't cover on its own.
    let mut buf = Vec::new();
    write_client_message_with_ids(&mut buf, Some(5), None, &ClientMessage::ListTabs).unwrap();
    let mut cursor = Cursor::new(buf);
    assert_eq!(
        read_client_message_with_ids(&mut cursor).unwrap(),
        (Some(5), None, ClientMessage::ListTabs)
    );
}

#[test]
fn an_unrecognized_client_variant_deserializes_as_unknown_rather_than_erroring() {
    let mut buf = Vec::new();
    let bad_payload = br#"{"message":"SomeFutureVariant"}"#;
    buf.extend_from_slice(&(bad_payload.len() as u32).to_le_bytes());
    buf.extend_from_slice(bad_payload);
    let mut cursor = Cursor::new(buf);
    assert_eq!(
        read_client_message(&mut cursor).unwrap(),
        ClientMessage::Unknown
    );
}

#[test]
fn an_unrecognized_server_variant_deserializes_as_unknown_rather_than_erroring() {
    let mut buf = Vec::new();
    let bad_payload = br#"{"message":{"SomeFutureVariant":{"x":1}}}"#;
    buf.extend_from_slice(&(bad_payload.len() as u32).to_le_bytes());
    buf.extend_from_slice(bad_payload);
    let mut cursor = Cursor::new(buf);
    assert_eq!(
        read_server_message(&mut cursor).unwrap(),
        ServerMessage::Unknown
    );
}

#[test]
fn client_handshake_succeeds_against_a_matching_hello_reply() {
    let (mut client, mut server) = tcp_pair();
    let responder = std::thread::spawn(move || {
        assert_eq!(
            read_client_message(&mut server).unwrap(),
            ClientMessage::Hello {
                protocol_version: PROTOCOL_VERSION
            }
        );
        write_server_message(
            &mut server,
            &ServerMessage::Hello {
                protocol_version: PROTOCOL_VERSION,
            },
        )
        .unwrap();
    });
    client_handshake(&mut client).unwrap();
    responder.join().unwrap();
}

#[test]
fn client_handshake_surfaces_an_error_reply_as_an_io_error() {
    let (mut client, mut server) = tcp_pair();
    let responder = std::thread::spawn(move || {
        let _ = read_client_message(&mut server).unwrap();
        write_server_message(
            &mut server,
            &ServerMessage::Error {
                message: "unsupported protocol_version".to_string(),
            },
        )
        .unwrap();
    });
    let err = client_handshake(&mut client).unwrap_err();
    assert!(err.to_string().contains("unsupported protocol_version"));
    responder.join().unwrap();
}

#[test]
fn client_handshake_rejects_an_unexpected_reply_type() {
    let (mut client, mut server) = tcp_pair();
    let responder = std::thread::spawn(move || {
        let _ = read_client_message(&mut server).unwrap();
        write_server_message(
            &mut server,
            &ServerMessage::Navigated {
                url: "about:blank".to_string(),
            },
        )
        .unwrap();
    });
    assert!(client_handshake(&mut client).is_err());
    responder.join().unwrap();
}
