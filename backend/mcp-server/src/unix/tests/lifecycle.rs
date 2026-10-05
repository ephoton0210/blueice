// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn go_back_uses_the_same_navigation_completion_barrier_and_returns_restored_state() {
    let (client, server) = UnixStream::pair().unwrap();
    fake_core(
        server,
        vec![
            Box::new(|msg, s| {
                assert!(matches!(msg, ClientMessage::GoBack));
                reply_tab(
                    s,
                    7,
                    &ServerMessage::AccessibilityTextState(
                        blueice_ipc::accessibility::AccessibilityTextReply {
                            context: blueice_ipc::accessibility::AccessibilityTextContext {
                                version: 1,
                                frame_source: 9,
                                document_generation: 3,
                                frame_generation: 8,
                                node_id: 7,
                            },
                            result: blueice_ipc::accessibility::AccessibilityTextResult::Index(
                                None,
                            ),
                        },
                    ),
                );
                reply_tab(
                    s,
                    7,
                    &ServerMessage::PrintState(blueice_ipc::printing::PrintReply::Ended {
                        ticket: "a".repeat(32),
                    }),
                );
                reply_tab(
                    s,
                    7,
                    &ServerMessage::DisplayPreferencesState(
                        blueice_ipc::display::DisplayPreferencesState {
                            tab_id: 7,
                            frame_source: 19,
                            frame_generation: 3,
                            preferences: blueice_ipc::display::DisplayPreferences {
                                dark: true,
                                high_contrast: true,
                                reduced_motion: true,
                            },
                        },
                    ),
                );
                // Display metadata must not release the history completion barrier.
                reply_tab(
                    s,
                    7,
                    &ServerMessage::ViewportState(blueice_ipc::viewport::ViewportState {
                        tab_id: 7,
                        frame_source: 19,
                        frame_generation: 3,
                        width: 300.0,
                        height: 200.0,
                        device_scale: 2.0,
                        backing_scale: None,
                        zoom: 1.5,
                        css_width: 200.0,
                        css_height: 200.0 / 1.5,
                        pixel_width: 600,
                        pixel_height: 400,
                    }),
                );
                let context = blueice_ipc::context_menu::ContextMenuContext {
                    tab_id: 7,
                    frame_source: 19,
                    document_generation: 1,
                    frame_generation: 3,
                    x: 10.0,
                    y: 20.0,
                };
                // Native menu broadcasts also cannot complete MCP navigation.
                reply_tab(
                    s,
                    7,
                    &ServerMessage::ContextMenu(blueice_ipc::context_menu::ContextMenuState {
                        context,
                        link_url: Some("https://example.com/link".into()),
                        input: None,
                    }),
                );
                reply_tab(
                    s,
                    7,
                    &ServerMessage::ContextMenuLink {
                        context,
                        url: "https://example.com/link".into(),
                    },
                );
                // A browser search reply can be broadcast while MCP waits
                // for this tab's history completion. It must not satisfy that barrier.
                reply_tab(
                    s,
                    7,
                    &ServerMessage::FindState(blueice_ipc::find::FindState {
                        tab_id: 7,
                        frame_source: 19,
                        document_generation: 1,
                        revision: 1,
                        query: "previous".into(),
                        case_sensitive: false,
                        match_count: 1,
                        active_match: Some(1),
                        wrapped: false,
                        limited: false,
                        rects: vec![blueice_ipc::Bounds {
                            x: 0.0,
                            y: 0.0,
                            width: 60.0,
                            height: 20.0,
                        }],
                    }),
                );
                reply(
                    s,
                    &ServerMessage::Navigated {
                        url: "https://example.com/previous".to_string(),
                    },
                );
                reply_tab(
                    s,
                    7,
                    &ServerMessage::FrameReady {
                        shm_path: "/tmp/previous".to_string(),
                        width: 10,
                        height: 10,
                        generation: 4,
                    },
                );
            }),
            Box::new(|msg, s| {
                assert!(matches!(msg, ClientMessage::GetRepresentation));
                let mut snapshot = sample_snapshot(4);
                snapshot.tab_id = 7;
                snapshot.url = Some("https://example.com/previous".to_string());
                reply(s, &ServerMessage::Representation(snapshot));
            }),
        ],
    );

    let mut conn = CoreConnection::new(client);
    let outcome = conn.go_back(Some(7)).unwrap();
    assert_eq!(outcome.error, None);
    assert_eq!(outcome.snapshot.tab_id, 7);
    assert_eq!(
        outcome.snapshot.url.as_deref(),
        Some("https://example.com/previous")
    );
    assert_eq!(conn.last_frame(Some(7)).unwrap().generation, 4);
}

#[test]
fn attach_to_never_falls_back_to_a_private_core() {
    let rendezvous_path = std::env::temp_dir().join(format!(
        "blueice-mcp-test-required-rendezvous-missing-{}.sock",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&rendezvous_path);

    assert!(
        CoreProcess::attach_to(&rendezvous_path, 320, 200).is_err(),
        "a required launcher attachment must fail rather than spawn an unobserved core"
    );
}

#[test]
fn sibling_core_binary_sits_next_to_the_mcp_server_binary() {
    let exe = PathBuf::from("/some/target/debug/blueice-mcp-server");
    assert_eq!(
        sibling_core_binary(&exe),
        PathBuf::from("/some/target/debug/blueice-core")
    );
}

#[test]
fn sibling_core_binary_steps_out_of_a_deps_directory_for_integration_tests() {
    let exe = PathBuf::from("/some/target/debug/deps/core_process-abc123");
    assert_eq!(
        sibling_core_binary(&exe),
        PathBuf::from("/some/target/debug/blueice-core")
    );
}

#[test]
fn send_and_drain_reports_gatekeeper_blocked_as_an_error() {
    let (client, server) = UnixStream::pair().unwrap();
    fake_core(
        server,
        vec![
            Box::new(|msg, s| {
                assert!(matches!(msg, ClientMessage::Navigate { .. }));
                reply(
                    s,
                    &ServerMessage::GatekeeperBlocked {
                        reason: "denied".to_string(),
                        category: "policy".to_string(),
                        url: "https://bad.example".to_string(),
                    },
                );
            }),
            Box::new(|msg, s| {
                assert!(matches!(msg, ClientMessage::GetRepresentation));
                reply(s, &ServerMessage::Representation(sample_snapshot(0)));
            }),
        ],
    );

    let mut conn = CoreConnection::new(client);
    let outcome = conn.navigate("https://bad.example", None).unwrap();
    let error = outcome
        .error
        .expect("gatekeeper block must surface as an error");
    assert!(error.contains("denied"));
    assert!(error.contains("policy"));
    assert!(error.contains("https://bad.example"));
}

#[test]
fn post_history_returns_confirmation_notice_without_replaying_a_request() {
    let (client, server) = UnixStream::pair().unwrap();
    fake_core(
        server,
        vec![
            Box::new(|msg, s| {
                assert!(matches!(msg, ClientMessage::GoBack));
                reply(
                    s,
                    &ServerMessage::FormResubmission {
                        confirmation_id: 42,
                        url: "https://example.test/posted".into(),
                    },
                );
            }),
            Box::new(|msg, s| {
                assert!(matches!(msg, ClientMessage::GetRepresentation));
                reply(s, &ServerMessage::Representation(sample_snapshot(4)));
            }),
        ],
    );
    let outcome = CoreConnection::new(client).go_back(Some(7)).unwrap();
    assert!(outcome
        .error
        .unwrap()
        .contains("confirmation in the browser"));
}
