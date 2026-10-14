// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn open_tab_creates_a_second_tab_visible_in_list_tabs() {
    let dir = temp_frame_dir("open-tab-list");
    let gatekeeper = clearing_gatekeeper("open-tab-list");
    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        let mut generation = 0u64;
        run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
        dir
    });
    handshake(&mut client);

    blueice_ipc::write_client_message(&mut client, &ClientMessage::ListTabs).unwrap();
    let ServerMessage::Tabs(before) = blueice_ipc::read_server_message(&mut client).unwrap() else {
        panic!("expected Tabs")
    };
    assert_eq!(
        before.len(),
        1,
        "a fresh core starts with exactly one tab, same as before Phase 16"
    );

    blueice_ipc::write_client_message(&mut client, &ClientMessage::OpenTab { url: None }).unwrap();
    let ServerMessage::TabOpened {
        tab_id: new_id,
        url,
        ..
    } = blueice_ipc::read_server_message(&mut client).unwrap()
    else {
        panic!("expected TabOpened")
    };
    assert_eq!(url, None);
    assert_ne!(new_id, before[0].id);

    blueice_ipc::write_client_message(&mut client, &ClientMessage::ListTabs).unwrap();
    let ServerMessage::Tabs(after) = blueice_ipc::read_server_message(&mut client).unwrap() else {
        panic!("expected Tabs")
    };
    assert_eq!(
        after.iter().map(|t| t.id).collect::<Vec<_>>(),
        vec![before[0].id, new_id]
    );

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn back_and_forward_restore_only_the_addressed_tabs_history() {
    let dir = temp_frame_dir("per-tab-history");
    let gatekeeper = clearing_gatekeeper("per-tab-history");
    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        let mut generation = 0u64;
        run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
        dir
    });
    handshake(&mut client);

    // Two visits in tab 1 create a real back stack. Built-in pages keep
    // this test deterministic while exercising the same history commit
    // path a cleared network navigation uses.
    for url in ["about:credits", "about:downloads"] {
        blueice_ipc::write_client_message(
            &mut client,
            &ClientMessage::Navigate {
                url: url.to_string(),
            },
        )
        .unwrap();
        assert_eq!(
            blueice_ipc::read_server_message(&mut client).unwrap(),
            ServerMessage::Navigated {
                url: url.to_string()
            }
        );
        assert!(matches!(
            blueice_ipc::read_server_message(&mut client).unwrap(),
            ServerMessage::FrameReady { .. }
        ));
    }

    blueice_ipc::write_client_message(&mut client, &ClientMessage::OpenTab { url: None }).unwrap();
    let ServerMessage::TabOpened {
        tab_id: tab_two, ..
    } = blueice_ipc::read_server_message(&mut client).unwrap()
    else {
        panic!("expected TabOpened")
    };
    blueice_ipc::write_client_message_with_ids(
        &mut client,
        Some(tab_two),
        None,
        &ClientMessage::Navigate {
            url: "about:blank".to_string(),
        },
    )
    .unwrap();
    let (reply_tab, _, navigated) = blueice_ipc::read_server_message_with_ids(&mut client).unwrap();
    assert_eq!(reply_tab, Some(tab_two));
    assert_eq!(
        navigated,
        ServerMessage::Navigated {
            url: "about:blank".to_string()
        }
    );
    assert!(matches!(
        blueice_ipc::read_server_message_with_ids(&mut client)
            .unwrap()
            .2,
        ServerMessage::FrameReady { .. }
    ));

    // Going back in tab 1 must leave tab 2's distinct visit untouched.
    blueice_ipc::write_client_message(&mut client, &ClientMessage::GoBack).unwrap();
    assert_eq!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::Navigated {
            url: "about:credits".to_string()
        }
    );
    assert!(matches!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::FrameReady { .. }
    ));

    blueice_ipc::write_client_message_with_ids(
        &mut client,
        Some(tab_two),
        None,
        &ClientMessage::GetHistoryState,
    )
    .unwrap();
    let (reply_tab, _, state) = blueice_ipc::read_server_message_with_ids(&mut client).unwrap();
    assert_eq!(reply_tab, Some(tab_two));
    assert_eq!(
        state,
        ServerMessage::HistoryState {
            can_go_back: true,
            can_go_forward: false,
        },
        "tab 1's Back must not alter tab 2's history position"
    );

    blueice_ipc::write_client_message(&mut client, &ClientMessage::GetHistoryState).unwrap();
    assert_eq!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::HistoryState {
            can_go_back: true,
            can_go_forward: true,
        }
    );

    blueice_ipc::write_client_message(&mut client, &ClientMessage::GoForward).unwrap();
    assert_eq!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::Navigated {
            url: "about:downloads".to_string()
        }
    );
    assert!(matches!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::FrameReady { .. }
    ));

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn default_history_reload_fetches_the_url_again_and_uses_fresh_content() {
    let dir = temp_frame_dir("history-reload");
    let gatekeeper = clearing_gatekeeper("history-reload");
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let http = thread::spawn(move || {
        for body in ["first version", "updated version"] {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0u8; 1024];
            let _ = std::io::Read::read(&mut stream, &mut request);
            let body = format!("<button>{body}</button>");
            std::io::Write::write_all(
                &mut stream,
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            )
            .unwrap();
        }
    });

    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        let mut generation = 0u64;
        run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
        dir
    });
    handshake(&mut client);

    let url = format!("http://{addr}");
    blueice_ipc::write_client_message(&mut client, &ClientMessage::Navigate { url: url.clone() })
        .unwrap();
    assert_eq!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::Navigated { url: url.clone() }
    );
    assert!(matches!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::FrameReady { .. }
    ));

    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::Navigate {
            url: "about:credits".to_string(),
        },
    )
    .unwrap();
    let _ = blueice_ipc::read_server_message(&mut client).unwrap();
    let _ = blueice_ipc::read_server_message(&mut client).unwrap();

    blueice_ipc::write_client_message(&mut client, &ClientMessage::GoBack).unwrap();
    assert_eq!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::Navigated { url: url.clone() }
    );
    assert!(matches!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::FrameReady { .. }
    ));
    blueice_ipc::write_client_message(&mut client, &ClientMessage::GetRepresentation).unwrap();
    let ServerMessage::Representation(snapshot) =
        blueice_ipc::read_server_message(&mut client).unwrap()
    else {
        panic!("expected a representation after history reload")
    };
    assert!(
        snapshot
            .nodes
            .iter()
            .any(|node| node.name.as_deref() == Some("updated version")),
        "Back must fetch the URL again instead of displaying the first visit's in-memory page"
    );

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    http.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn default_history_reload_rechecks_the_live_gatekeeper_before_a_second_fetch() {
    use blueice_ipc::gatekeeper::GatekeeperSettingsChange;

    let dir = temp_frame_dir("history-live-policy");
    let gatekeeper = unique_gatekeeper_socket_path("history-live-policy");
    let _ = std::fs::remove_file(&gatekeeper);
    let listener = UnixListener::bind(&gatekeeper).unwrap();
    let service = Arc::new(blueice_ai_gatekeeper::GatekeeperService::new(None).unwrap());
    let reviewer = thread::spawn({
        let service = Arc::clone(&service);
        move || {
            // First visit: URL + content. Then one settings update. The
            // history reload must ask the same live service about its URL.
            for _ in 0..4 {
                let (mut stream, _) = listener.accept().unwrap();
                service.handle_connection(&mut stream).unwrap();
            }
        }
    });

    let http_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = http_listener.local_addr().unwrap();
    let http_peer = http_listener.try_clone().unwrap();
    let http = thread::spawn(move || {
        let (mut stream, _) = http_peer.accept().unwrap();
        let mut request = [0u8; 1024];
        let _ = std::io::Read::read(&mut stream, &mut request);
        let body = "<p>original page</p>";
        std::io::Write::write_all(
            &mut stream,
            format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .as_bytes(),
        )
        .unwrap();
    });

    let (mut client, mut server) = client_pair();
    let session_gatekeeper = gatekeeper.clone();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        let mut generation = 0u64;
        run_session(
            &mut tabs,
            &mut server,
            &dir,
            &mut generation,
            &session_gatekeeper,
        )
        .unwrap();
        dir
    });
    handshake(&mut client);
    let url = format!("http://{address}/history");
    blueice_ipc::write_client_message(&mut client, &ClientMessage::Navigate { url: url.clone() })
        .unwrap();
    assert_eq!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::Navigated { url: url.clone() }
    );
    assert!(matches!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::FrameReady { .. }
    ));
    http.join().unwrap();
    http_listener.set_nonblocking(true).unwrap();

    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::Navigate {
            url: "about:credits".into(),
        },
    )
    .unwrap();
    assert_eq!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::Navigated {
            url: "about:credits".into()
        }
    );
    assert!(matches!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::FrameReady { .. }
    ));

    let source = crate::gatekeeper_settings_page::GatekeeperSettingsSource::at(gatekeeper.clone());
    let effective = source
        .update(GatekeeperSettingsChange::AddBlockedHost {
            host: "127.0.0.1".into(),
        })
        .unwrap();
    assert_eq!(effective.custom_blocked_hosts, ["127.0.0.1"]);
    blueice_ipc::write_client_message(&mut client, &ClientMessage::GoBack).unwrap();
    assert!(
        matches!(blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::GatekeeperBlocked { url: blocked_url, category, .. }
            if blocked_url == url && category == "custom-blocked-domain")
    );
    assert_eq!(
        http_listener.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock,
        "history reload must be blocked before a second network connection"
    );

    blueice_ipc::write_client_message(&mut client, &ClientMessage::ListTabs).unwrap();
    let ServerMessage::Tabs(tabs) = blueice_ipc::read_server_message(&mut client).unwrap() else {
        panic!("expected tab state after the blocked history reload")
    };
    assert_eq!(
        tabs[0].url.as_deref(),
        Some("about:credits"),
        "a rejected reload must keep the current document visible"
    );
    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    reviewer.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
    let _ = std::fs::remove_file(gatekeeper);
}

#[test]
fn opted_in_history_snapshot_restores_when_the_original_url_is_unavailable() {
    let dir = temp_frame_dir("history-snapshot");
    let gatekeeper = clearing_gatekeeper("history-snapshot");
    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new_with_history_snapshot_mode(
            320.0,
            200.0,
            crate::HistorySnapshotMode::Snapshot,
        );
        let tab = tabs.default_tab();
        tabs.get_mut(tab).unwrap().load_html_str(
            "<button>saved historical version</button>",
            Some("https://unavailable.example.test/archive-me".to_string()),
        );
        let mut generation = 0u64;
        run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
        dir
    });
    handshake(&mut client);

    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::Navigate {
            url: "about:credits".to_string(),
        },
    )
    .unwrap();
    let _ = blueice_ipc::read_server_message(&mut client).unwrap();
    let _ = blueice_ipc::read_server_message(&mut client).unwrap();

    blueice_ipc::write_client_message(&mut client, &ClientMessage::GoBack).unwrap();
    assert_eq!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::Navigated {
            url: "https://unavailable.example.test/archive-me".to_string()
        }
    );
    assert!(matches!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::FrameReady { .. }
    ));
    blueice_ipc::write_client_message(&mut client, &ClientMessage::GetRepresentation).unwrap();
    let ServerMessage::Representation(snapshot) =
        blueice_ipc::read_server_message(&mut client).unwrap()
    else {
        panic!("expected a representation after snapshot restoration")
    };
    assert!(snapshot
        .nodes
        .iter()
        .any(|node| node.name.as_deref() == Some("saved historical version")));

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn failed_default_history_reload_keeps_the_current_page_and_cursor() {
    let dir = temp_frame_dir("history-reload-failure");
    let gatekeeper = clearing_gatekeeper("history-reload-failure");
    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        let tab = tabs.default_tab();
        tabs.get_mut(tab).unwrap().load_html_str(
            "<button>unavailable historical page</button>",
            Some("http://127.0.0.1:1/history-unavailable".to_string()),
        );
        let mut generation = 0u64;
        run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
        dir
    });
    handshake(&mut client);

    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::Navigate {
            url: "about:credits".to_string(),
        },
    )
    .unwrap();
    let _ = blueice_ipc::read_server_message(&mut client).unwrap();
    let _ = blueice_ipc::read_server_message(&mut client).unwrap();

    blueice_ipc::write_client_message(&mut client, &ClientMessage::GoBack).unwrap();
    assert!(matches!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::Error { .. }
    ));
    blueice_ipc::write_client_message(&mut client, &ClientMessage::GetRepresentation).unwrap();
    let ServerMessage::Representation(snapshot) =
        blueice_ipc::read_server_message(&mut client).unwrap()
    else {
        panic!("expected the still-current page representation")
    };
    assert_eq!(snapshot.url.as_deref(), Some("about:credits"));

    blueice_ipc::write_client_message(&mut client, &ClientMessage::GetHistoryState).unwrap();
    assert_eq!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::HistoryState {
            can_go_back: true,
            can_go_forward: false,
        },
        "a failed reload must not advance the history cursor"
    );

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn tab_groups_are_shared_session_state_and_closing_one_ungroups_its_tabs() {
    let dir = temp_frame_dir("tab-groups");
    let gatekeeper = clearing_gatekeeper("tab-groups");
    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        let mut generation = 0u64;
        run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
        dir
    });
    handshake(&mut client);

    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::CreateTabGroup {
            name: "  Research  ".to_string(),
            color: "#4F8cFf".to_string(),
        },
    )
    .unwrap();
    assert_eq!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::TabGroupCreated(TabGroupSummary {
            id: 1,
            name: "Research".to_string(),
            color: "#4f8cff".to_string(),
            collapsed: false,
        })
    );

    blueice_ipc::write_client_message_with_ids(
        &mut client,
        Some(1),
        None,
        &ClientMessage::SetTabGroup { group_id: Some(1) },
    )
    .unwrap();
    assert_eq!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::TabGroupAssigned {
            tab_id: 1,
            group_id: Some(1),
        }
    );

    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::RenameTabGroup {
            group_id: 1,
            name: "Reference".to_string(),
        },
    )
    .unwrap();
    assert_eq!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::TabGroupUpdated(TabGroupSummary {
            id: 1,
            name: "Reference".to_string(),
            color: "#4f8cff".to_string(),
            collapsed: false,
        })
    );

    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::SetTabGroupColor {
            group_id: 1,
            color: "#ff6600".to_string(),
        },
    )
    .unwrap();
    let recolored = blueice_ipc::read_server_message(&mut client).unwrap();
    assert!(matches!(
        recolored,
        ServerMessage::TabGroupUpdated(TabGroupSummary { ref color, .. }) if color == "#ff6600"
    ));

    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::SetTabGroupCollapsed {
            group_id: 1,
            collapsed: true,
        },
    )
    .unwrap();
    let collapsed = blueice_ipc::read_server_message(&mut client).unwrap();
    assert!(matches!(
        collapsed,
        ServerMessage::TabGroupUpdated(TabGroupSummary {
            collapsed: true,
            ..
        })
    ));

    blueice_ipc::write_client_message(&mut client, &ClientMessage::ListTabs).unwrap();
    assert!(matches!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::Tabs(ref tabs) if tabs == &vec![TabSummary {
            id: 1,
            url: None,
            group_id: Some(1),
        }]
    ));
    blueice_ipc::write_client_message(&mut client, &ClientMessage::ListTabGroups).unwrap();
    assert!(matches!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::TabGroups(ref groups) if groups.len() == 1 && groups[0].collapsed
    ));

    blueice_ipc::write_client_message(&mut client, &ClientMessage::CloseTabGroup { group_id: 1 })
        .unwrap();
    assert_eq!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::TabGroupClosed { group_id: 1 }
    );
    blueice_ipc::write_client_message(&mut client, &ClientMessage::ListTabs).unwrap();
    assert!(matches!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::Tabs(ref tabs) if tabs[0].group_id.is_none()
    ));

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn resize_eagerly_reflows_background_tabs_without_creating_an_active_tab() {
    let dir = temp_frame_dir("resize-background-tabs");
    let gatekeeper = clearing_gatekeeper("resize-background-tabs");
    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        let mut generation = 0u64;
        run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
        dir
    });
    handshake(&mut client);

    blueice_ipc::write_client_message(&mut client, &ClientMessage::OpenTab { url: None }).unwrap();
    assert!(matches!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::TabOpened { tab_id: 2, .. }
    ));
    blueice_ipc::write_client_message_with_ids(
        &mut client,
        Some(1),
        None,
        &ClientMessage::Resize {
            width: 640,
            height: 480,
        },
    )
    .unwrap();
    let first = blueice_ipc::read_server_message_with_ids(&mut client).unwrap();
    let second = blueice_ipc::read_server_message_with_ids(&mut client).unwrap();
    let mut resized = [first, second];
    resized.sort_by_key(|(tab_id, _, _)| *tab_id);
    assert!(matches!(
        &resized[..],
        [
            (Some(1), _, ServerMessage::FrameReady { .. }),
            (Some(2), _, ServerMessage::FrameReady { .. }),
        ]
    ));

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn open_tab_with_a_url_navigates_it_and_sends_a_frame() {
    let dir = temp_frame_dir("open-tab-with-url");
    let gatekeeper = clearing_gatekeeper("open-tab-with-url");
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buf = [0u8; 1024];
        let _ = std::io::Read::read(&mut stream, &mut buf);
        let body = "<p>opened via url</p>";
        std::io::Write::write_all(
            &mut stream,
            format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .as_bytes(),
        )
        .unwrap();
    });

    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        let mut generation = 0u64;
        run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
        dir
    });
    handshake(&mut client);

    let url = format!("http://{addr}");
    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::OpenTab {
            url: Some(url.clone()),
        },
    )
    .unwrap();
    let ServerMessage::TabOpened {
        tab_id: new_id,
        url: opened_url,
        ..
    } = blueice_ipc::read_server_message(&mut client).unwrap()
    else {
        panic!("expected TabOpened")
    };
    assert_eq!(opened_url, Some(url));
    let frame = blueice_ipc::read_server_message(&mut client).unwrap();
    assert!(
        matches!(frame, ServerMessage::FrameReady { .. }),
        "expected FrameReady, got {frame:?}"
    );

    // The new tab's content must actually be addressable afterward.
    blueice_ipc::write_client_message_with_ids(
        &mut client,
        Some(new_id),
        None,
        &ClientMessage::GetRepresentation,
    )
    .unwrap();
    let (reply_tab, _, reply) = blueice_ipc::read_server_message_with_ids(&mut client).unwrap();
    assert_eq!(reply_tab, Some(new_id));
    let ServerMessage::Representation(snapshot) = reply else {
        panic!("expected Representation, got {reply:?}")
    };
    assert_eq!(snapshot.tab_id, new_id);
    assert!(snapshot
        .nodes
        .iter()
        .any(|n| n.name.as_deref() == Some("opened via url")));

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn open_tab_with_a_failing_url_replies_error_not_tab_opened() {
    let dir = temp_frame_dir("open-tab-failing-url");
    let gatekeeper = clearing_gatekeeper("open-tab-failing-url");
    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        let mut generation = 0u64;
        run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
        dir
    });
    handshake(&mut client);

    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::OpenTab {
            url: Some("not-a-valid-url".to_string()),
        },
    )
    .unwrap();
    let reply = blueice_ipc::read_server_message(&mut client).unwrap();
    assert!(
        matches!(reply, ServerMessage::Error { .. }),
        "expected Error, got {reply:?}"
    );

    // The session must still be alive and taking new commands
    // afterward -- proven the same way every other no-crash case
    // in this file is.
    blueice_ipc::write_client_message(&mut client, &ClientMessage::ListTabs).unwrap();
    assert!(matches!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::Tabs(_)
    ));

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn an_action_addressed_to_one_tab_never_affects_another_tabs_state() {
    let dir = temp_frame_dir("tab-isolation");
    let gatekeeper = clearing_gatekeeper("tab-isolation");
    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        default_page(&mut tabs).load_html_str("<p>tab one</p>", None);
        let mut generation = 0u64;
        run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
        dir
    });
    handshake(&mut client);

    blueice_ipc::write_client_message(&mut client, &ClientMessage::ListTabs).unwrap();
    let ServerMessage::Tabs(initial) = blueice_ipc::read_server_message(&mut client).unwrap()
    else {
        panic!("expected Tabs")
    };
    let tab_one = initial[0].id;

    blueice_ipc::write_client_message(&mut client, &ClientMessage::OpenTab { url: None }).unwrap();
    let ServerMessage::TabOpened {
        tab_id: tab_two, ..
    } = blueice_ipc::read_server_message(&mut client).unwrap()
    else {
        panic!("expected TabOpened")
    };

    // Scroll only tab_two.
    blueice_ipc::write_client_message_with_ids(
        &mut client,
        Some(tab_two),
        None,
        &ClientMessage::Scroll { delta_y: 500.0 },
    )
    .unwrap();
    let frame = blueice_ipc::read_server_message(&mut client).unwrap();
    assert!(matches!(frame, ServerMessage::FrameReady { .. }));

    // tab_one's representation must be completely unaffected --
    // still showing its own content, scroll untouched.
    blueice_ipc::write_client_message_with_ids(
        &mut client,
        Some(tab_one),
        None,
        &ClientMessage::GetRepresentation,
    )
    .unwrap();
    let (_, _, reply) = blueice_ipc::read_server_message_with_ids(&mut client).unwrap();
    let ServerMessage::Representation(snap) = reply else {
        panic!("expected Representation, got {reply:?}")
    };
    assert_eq!(snap.tab_id, tab_one);
    assert_eq!(
        snap.scroll_y, 0.0,
        "scrolling tab_two must not move tab_one's scroll position"
    );
    assert!(snap
        .nodes
        .iter()
        .any(|n| n.name.as_deref() == Some("tab one")));

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_gated_navigation_addressed_to_one_tab_never_affects_another_tabs_state() {
    // Extends `an_action_addressed_to_one_tab_never_affects_another_
    // tabs_state` (which only covers `Scroll`) to a gated `Navigate`
    // specifically, now that navigation is asynchronous: `tab_two`
    // fully navigating must leave `tab_one`'s content, generation
    // relationship, and addressability completely untouched.
    let dir = temp_frame_dir("tab-isolation-gated-navigate");
    let gatekeeper = clearing_gatekeeper("tab-isolation-gated-navigate");
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buf = [0u8; 1024];
        let _ = std::io::Read::read(&mut stream, &mut buf);
        let body = "<p>tab two content</p>";
        std::io::Write::write_all(
            &mut stream,
            format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .as_bytes(),
        )
        .unwrap();
    });

    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        default_page(&mut tabs).load_html_str("<p>tab one</p>", None);
        let mut generation = 0u64;
        run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
        dir
    });
    handshake(&mut client);

    blueice_ipc::write_client_message(&mut client, &ClientMessage::ListTabs).unwrap();
    let ServerMessage::Tabs(initial) = blueice_ipc::read_server_message(&mut client).unwrap()
    else {
        panic!("expected Tabs")
    };
    let tab_one = initial[0].id;

    blueice_ipc::write_client_message(&mut client, &ClientMessage::OpenTab { url: None }).unwrap();
    let ServerMessage::TabOpened {
        tab_id: tab_two, ..
    } = blueice_ipc::read_server_message(&mut client).unwrap()
    else {
        panic!("expected TabOpened")
    };

    let url = format!("http://{addr}");
    blueice_ipc::write_client_message_with_ids(
        &mut client,
        Some(tab_two),
        None,
        &ClientMessage::Navigate { url: url.clone() },
    )
    .unwrap();
    let (reply_tab, _, navigated) = blueice_ipc::read_server_message_with_ids(&mut client).unwrap();
    assert_eq!(reply_tab, Some(tab_two));
    assert_eq!(navigated, ServerMessage::Navigated { url });
    let (_, _, frame) = blueice_ipc::read_server_message_with_ids(&mut client).unwrap();
    assert!(matches!(frame, ServerMessage::FrameReady { .. }));

    blueice_ipc::write_client_message_with_ids(
        &mut client,
        Some(tab_one),
        None,
        &ClientMessage::GetRepresentation,
    )
    .unwrap();
    let (_, _, reply) = blueice_ipc::read_server_message_with_ids(&mut client).unwrap();
    let ServerMessage::Representation(snap) = reply else {
        panic!("expected Representation, got {reply:?}")
    };
    assert_eq!(snap.tab_id, tab_one);
    assert!(
        snap.nodes
            .iter()
            .any(|n| n.name.as_deref() == Some("tab one")),
        "tab one's content must be untouched by tab two's gated navigation"
    );

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}
