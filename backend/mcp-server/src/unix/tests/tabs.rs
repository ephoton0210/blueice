// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn list_tabs_returns_what_core_reports() {
    let (client, server) = UnixStream::pair().unwrap();
    fake_core(
        server,
        vec![Box::new(|msg, s| {
            assert!(matches!(msg, ClientMessage::ListTabs));
            reply(
                s,
                &ServerMessage::Tabs(vec![
                    TabSummary {
                        id: 1,
                        url: None,
                        group_id: None,
                    },
                    TabSummary {
                        id: 2,
                        url: Some("https://example.com".to_string()),
                        group_id: Some(3),
                    },
                ]),
            );
        })],
    );

    let mut conn = CoreConnection::new(client);
    let tabs = conn.list_tabs().unwrap();
    assert_eq!(
        tabs,
        vec![
            TabSummary {
                id: 1,
                url: None,
                group_id: None
            },
            TabSummary {
                id: 2,
                url: Some("https://example.com".to_string()),
                group_id: Some(3),
            }
        ]
    );
}

#[test]
fn tab_group_methods_round_trip_the_shared_core_state() {
    let (client, server) = UnixStream::pair().unwrap();
    fake_core(
        server,
        vec![
            Box::new(|msg, stream| {
                assert_eq!(
                    msg,
                    ClientMessage::CreateTabGroup {
                        name: "Research".to_string(),
                        color: "#4f8cff".to_string(),
                    }
                );
                reply(
                    stream,
                    &ServerMessage::TabGroupCreated(TabGroupSummary {
                        id: 3,
                        name: "Research".to_string(),
                        color: "#4f8cff".to_string(),
                        collapsed: false,
                    }),
                );
            }),
            Box::new(|msg, stream| {
                assert_eq!(msg, ClientMessage::SetTabGroup { group_id: Some(3) });
                reply_tab(
                    stream,
                    2,
                    &ServerMessage::TabGroupAssigned {
                        tab_id: 2,
                        group_id: Some(3),
                    },
                );
            }),
            Box::new(|msg, stream| {
                assert!(matches!(msg, ClientMessage::ListTabGroups));
                reply(
                    stream,
                    &ServerMessage::TabGroups(vec![TabGroupSummary {
                        id: 3,
                        name: "Research".to_string(),
                        color: "#4f8cff".to_string(),
                        collapsed: false,
                    }]),
                );
            }),
            Box::new(|msg, stream| {
                assert_eq!(msg, ClientMessage::CloseTabGroup { group_id: 3 });
                reply(stream, &ServerMessage::TabGroupClosed { group_id: 3 });
            }),
        ],
    );

    let mut conn = CoreConnection::new(client);
    assert!(matches!(
        conn.create_tab_group("Research", "#4f8cff").unwrap(),
        TabGroupOutcome::Group(TabGroupSummary { id: 3, .. })
    ));
    assert_eq!(
        conn.set_tab_group(2, Some(3)).unwrap(),
        TabGroupOutcome::Assigned {
            tab_id: 2,
            group_id: Some(3),
        }
    );
    assert!(matches!(
        conn.list_tab_groups().unwrap(),
        Ok(groups) if groups.len() == 1 && groups[0].id == 3
    ));
    assert_eq!(
        conn.close_tab_group(3).unwrap(),
        TabGroupOutcome::Closed { group_id: 3 }
    );
}

#[test]
fn open_tab_without_a_url_returns_the_new_blank_tab() {
    let (client, server) = UnixStream::pair().unwrap();
    fake_core(
        server,
        vec![Box::new(|msg, s| {
            assert!(matches!(msg, ClientMessage::OpenTab { url: None }));
            reply(
                s,
                &ServerMessage::TabOpened {
                    tab_id: 2,
                    url: None,
                },
            );
        })],
    );

    let mut conn = CoreConnection::new(client);
    assert_eq!(
        conn.open_tab(None).unwrap(),
        OpenTabOutcome::Opened {
            tab_id: 2,
            url: None
        }
    );
}

#[test]
fn open_tab_with_a_url_returns_the_navigated_tab_and_caches_its_frame() {
    let (client, server) = UnixStream::pair().unwrap();
    fake_core(
        server,
        vec![Box::new(|msg, s| {
            assert_eq!(
                msg,
                ClientMessage::OpenTab {
                    url: Some("https://example.com".to_string())
                }
            );
            reply_tab(
                s,
                2,
                &ServerMessage::TabOpened {
                    tab_id: 2,
                    url: Some("https://example.com".to_string()),
                },
            );
            reply_tab(
                s,
                2,
                &ServerMessage::FrameReady {
                    shm_path: "/tmp/newtab".to_string(),
                    width: 8,
                    height: 8,
                    generation: 3,
                },
            );
        })],
    );

    let mut conn = CoreConnection::new(client);
    let outcome = conn.open_tab(Some("https://example.com")).unwrap();
    assert_eq!(
        outcome,
        OpenTabOutcome::Opened {
            tab_id: 2,
            url: Some("https://example.com".to_string())
        }
    );
    assert_eq!(conn.last_frame(Some(2)).unwrap().generation, 3);
}

#[test]
fn open_tab_surfaces_a_navigation_failure_as_an_error_outcome() {
    let (client, server) = UnixStream::pair().unwrap();
    fake_core(
        server,
        vec![Box::new(|msg, s| {
            assert!(matches!(msg, ClientMessage::OpenTab { .. }));
            reply(
                s,
                &ServerMessage::Error {
                    message: "unreachable host".to_string(),
                },
            );
        })],
    );

    let mut conn = CoreConnection::new(client);
    assert_eq!(
        conn.open_tab(Some("http://bad")).unwrap(),
        OpenTabOutcome::Error("unreachable host".to_string())
    );
}

#[test]
fn close_tab_returns_closed_on_success() {
    let (client, server) = UnixStream::pair().unwrap();
    fake_core(
        server,
        vec![Box::new(|msg, s| {
            assert!(matches!(msg, ClientMessage::CloseTab));
            reply(s, &ServerMessage::TabClosed { tab_id: 2 });
        })],
    );

    let mut conn = CoreConnection::new(client);
    assert_eq!(conn.close_tab(2).unwrap(), CloseTabOutcome::Closed);
}

#[test]
fn close_tab_returns_error_for_an_unknown_tab() {
    let (client, server) = UnixStream::pair().unwrap();
    fake_core(
        server,
        vec![Box::new(|msg, s| {
            assert!(matches!(msg, ClientMessage::CloseTab));
            reply(
                s,
                &ServerMessage::Error {
                    message: "unknown tab 999".to_string(),
                },
            );
        })],
    );

    let mut conn = CoreConnection::new(client);
    assert_eq!(
        conn.close_tab(999).unwrap(),
        CloseTabOutcome::Error("unknown tab 999".to_string())
    );
}

#[test]
fn navigate_addresses_the_given_tab_id_on_the_wire() {
    // Direct wire-level check that `tab_id` and request correlation
    // both reach the envelope, not just the in-process struct field.
    let (client, mut server) = UnixStream::pair().unwrap();
    let handle = thread::spawn(move || {
        let (tab_id, request_id, msg) =
            blueice_ipc::read_client_message_with_ids(&mut server).unwrap();
        assert_eq!(tab_id, Some(7));
        assert!(matches!(msg, ClientMessage::Navigate { .. }));
        blueice_ipc::write_server_message_with_ids(
            &mut server,
            Some(7),
            request_id,
            &ServerMessage::Navigated {
                url: "https://example.com".to_string(),
            },
        )
        .unwrap();
        // A successful core navigation always renders a frame after its
        // `Navigated` acknowledgement. `CoreConnection::navigate`
        // deliberately waits for that frame before it asks for the
        // representation, avoiding a snapshot of the old page.
        blueice_ipc::write_server_message_with_ids(
            &mut server,
            Some(7),
            request_id,
            &ServerMessage::FrameReady {
                shm_path: "/tmp/example".to_string(),
                width: 10,
                height: 10,
                generation: 1,
            },
        )
        .unwrap();
        let (tab_id, representation_id, msg) =
            blueice_ipc::read_client_message_with_ids(&mut server).unwrap();
        assert_eq!(
            tab_id,
            Some(7),
            "the follow-up GetRepresentation must be addressed to the same tab"
        );
        assert!(matches!(msg, ClientMessage::GetRepresentation));
        blueice_ipc::write_server_message_with_ids(
            &mut server,
            Some(7),
            representation_id,
            &ServerMessage::Representation(sample_snapshot(1)),
        )
        .unwrap();
    });

    let mut conn = CoreConnection::new(client);
    conn.navigate("https://example.com", Some(7)).unwrap();
    handle.join().unwrap();
}

#[test]
fn last_frame_is_tracked_independently_per_tab() {
    let (client, server) = UnixStream::pair().unwrap();
    fake_core(
        server,
        vec![
            Box::new(|msg, s| {
                assert!(matches!(msg, ClientMessage::Navigate { .. }));
                reply_tab(
                    s,
                    1,
                    &ServerMessage::Navigated {
                        url: "https://a.example".to_string(),
                    },
                );
                reply_tab(
                    s,
                    1,
                    &ServerMessage::FrameReady {
                        shm_path: "/tmp/a".to_string(),
                        width: 1,
                        height: 1,
                        generation: 1,
                    },
                );
            }),
            Box::new(|msg, s| {
                assert!(matches!(msg, ClientMessage::GetRepresentation));
                reply_tab(s, 1, &ServerMessage::Representation(sample_snapshot(1)));
            }),
            Box::new(|msg, s| {
                assert!(matches!(msg, ClientMessage::Navigate { .. }));
                reply_tab(
                    s,
                    2,
                    &ServerMessage::Navigated {
                        url: "https://b.example".to_string(),
                    },
                );
                reply_tab(
                    s,
                    2,
                    &ServerMessage::FrameReady {
                        shm_path: "/tmp/b".to_string(),
                        width: 1,
                        height: 1,
                        generation: 2,
                    },
                );
            }),
            Box::new(|msg, s| {
                assert!(matches!(msg, ClientMessage::GetRepresentation));
                reply_tab(s, 2, &ServerMessage::Representation(sample_snapshot(2)));
            }),
        ],
    );

    let mut conn = CoreConnection::new(client);
    conn.navigate("https://a.example", Some(1)).unwrap();
    conn.navigate("https://b.example", Some(2)).unwrap();

    assert_eq!(
        conn.last_frame(Some(1)).unwrap().shm_path,
        "/tmp/a",
        "tab 1's own frame must still be retrievable after tab 2 renders"
    );
    assert_eq!(conn.last_frame(Some(2)).unwrap().shm_path, "/tmp/b");
    assert_eq!(
        conn.last_frame(None).unwrap().shm_path,
        "/tmp/b",
        "the unqualified lookup follows this MCP connection's most recent request"
    );
    let (tab_id, frame) = conn.last_frame_with_tab_id(None).unwrap();
    assert_eq!((tab_id, frame.shm_path.as_str()), (2, "/tmp/b"));
    let (tab_id, frame) = conn.last_frame_with_tab_id(Some(1)).unwrap();
    assert_eq!((tab_id, frame.shm_path.as_str()), (1, "/tmp/a"));
}

#[test]
fn an_unsolicited_frame_cannot_change_the_default_screenshot_tab() {
    let (client, mut server) = UnixStream::pair().unwrap();
    let handle = thread::spawn(move || {
        let (_, action_id, message) =
            blueice_ipc::read_client_message_with_ids(&mut server).unwrap();
        assert!(matches!(message, ClientMessage::Navigate { .. }));
        blueice_ipc::write_server_message_with_ids(
            &mut server,
            Some(1),
            action_id,
            &ServerMessage::Navigated {
                url: "https://ai.example".to_string(),
            },
        )
        .unwrap();
        // A shared launcher's broadcast from the human's live
        // downloads tab has no request id for this MCP connection.
        blueice_ipc::write_server_message_with_ids(
            &mut server,
            Some(2),
            None,
            &ServerMessage::FrameReady {
                shm_path: "/tmp/human-downloads".to_string(),
                width: 1,
                height: 1,
                generation: 9,
            },
        )
        .unwrap();
        blueice_ipc::write_server_message_with_ids(
            &mut server,
            Some(1),
            action_id,
            &ServerMessage::FrameReady {
                shm_path: "/tmp/ai-page".to_string(),
                width: 1,
                height: 1,
                generation: 1,
            },
        )
        .unwrap();
        let (_, representation_id, message) =
            blueice_ipc::read_client_message_with_ids(&mut server).unwrap();
        assert!(matches!(message, ClientMessage::GetRepresentation));
        blueice_ipc::write_server_message_with_ids(
            &mut server,
            Some(1),
            representation_id,
            &ServerMessage::Representation(sample_snapshot(1)),
        )
        .unwrap();
    });

    let mut conn = CoreConnection::new(client);
    conn.navigate("https://ai.example", None).unwrap();
    assert_eq!(conn.last_frame(None).unwrap().shm_path, "/tmp/ai-page");
    assert_eq!(conn.last_frame_with_tab_id(None).unwrap().0, 1);
    assert!(
        conn.last_frame(Some(2)).is_none(),
        "a broadcast frame is not accepted as an MCP reply"
    );
    handle.join().unwrap();
}

#[test]
fn close_tab_still_caches_a_frame_ready_seen_along_the_way() {
    let (client, server) = UnixStream::pair().unwrap();
    fake_core(
        server,
        vec![Box::new(|msg, s| {
            assert!(matches!(msg, ClientMessage::CloseTab));
            blueice_ipc::write_server_message_with_id(
                s,
                Some(9_999),
                &ServerMessage::Error {
                    message: "unrelated client's failure".to_string(),
                },
            )
            .unwrap();
            reply_tab(
                s,
                1,
                &ServerMessage::FrameReady {
                    shm_path: "/tmp/close".to_string(),
                    width: 1,
                    height: 1,
                    generation: 9,
                },
            );
            reply(s, &ServerMessage::TabClosed { tab_id: 2 });
        })],
    );

    let mut conn = CoreConnection::new(client);
    assert_eq!(conn.close_tab(2).unwrap(), CloseTabOutcome::Closed);
    assert_eq!(conn.last_frame(None).unwrap().generation, 9);
}

#[test]
fn list_tabs_still_caches_a_frame_ready_seen_along_the_way() {
    let (client, server) = UnixStream::pair().unwrap();
    fake_core(
        server,
        vec![Box::new(|msg, s| {
            assert!(matches!(msg, ClientMessage::ListTabs));
            reply_tab(
                s,
                1,
                &ServerMessage::FrameReady {
                    shm_path: "/tmp/list".to_string(),
                    width: 1,
                    height: 1,
                    generation: 11,
                },
            );
            reply(
                s,
                &ServerMessage::Tabs(vec![TabSummary {
                    id: 1,
                    url: None,
                    group_id: None,
                }]),
            );
        })],
    );

    let mut conn = CoreConnection::new(client);
    let tabs = conn.list_tabs().unwrap();
    assert_eq!(
        tabs,
        vec![TabSummary {
            id: 1,
            url: None,
            group_id: None
        }]
    );
    assert_eq!(conn.last_frame(None).unwrap().generation, 11);
}

#[test]
fn open_tab_keeps_waiting_past_a_frame_ready_seen_before_tab_opened() {
    let (client, server) = UnixStream::pair().unwrap();
    fake_core(
        server,
        vec![Box::new(|msg, s| {
            assert!(matches!(msg, ClientMessage::OpenTab { .. }));
            blueice_ipc::write_server_message_with_id(
                s,
                Some(9_999),
                &ServerMessage::Error {
                    message: "unrelated client's failure".to_string(),
                },
            )
            .unwrap();
            reply(s, &ServerMessage::Tabs(Vec::new()));
            reply_tab(
                s,
                1,
                &ServerMessage::FrameReady {
                    shm_path: "/tmp/premature".to_string(),
                    width: 1,
                    height: 1,
                    generation: 3,
                },
            );
            reply(
                s,
                &ServerMessage::TabOpened {
                    tab_id: 2,
                    url: None,
                },
            );
        })],
    );

    let mut conn = CoreConnection::new(client);
    assert_eq!(
        conn.open_tab(None).unwrap(),
        OpenTabOutcome::Opened {
            tab_id: 2,
            url: None
        }
    );
    assert_eq!(conn.last_frame(None).unwrap().generation, 3);
}

#[test]
fn open_tab_reports_gatekeeper_blocked_as_an_error() {
    let (client, server) = UnixStream::pair().unwrap();
    fake_core(
        server,
        vec![Box::new(|msg, s| {
            assert!(matches!(msg, ClientMessage::OpenTab { .. }));
            reply(
                s,
                &ServerMessage::GatekeeperBlocked {
                    reason: "denied".to_string(),
                    category: "policy".to_string(),
                    url: "https://bad.example".to_string(),
                },
            );
        })],
    );

    let mut conn = CoreConnection::new(client);
    let outcome = conn.open_tab(Some("https://bad.example")).unwrap();
    let OpenTabOutcome::Error(message) = outcome else {
        panic!("expected an error outcome, got {outcome:?}");
    };
    assert!(message.contains("denied"));
    assert!(message.contains("policy"));
    assert!(message.contains("https://bad.example"));
}
