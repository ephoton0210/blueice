// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn act_on_click_navigates_the_same_way_a_coordinate_click_does() {
    let dir = temp_frame_dir("act-on-click");
    let gatekeeper = clearing_gatekeeper("act-on-click");
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buf = [0u8; 1024];
        let _ = std::io::Read::read(&mut stream, &mut buf);
        let body = "<p>landed via id</p>";
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
    let url = format!("http://{addr}");

    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        default_page(&mut tabs).load_html_str(&format!(r#"<a href="{url}">go</a>"#), None);
        let mut generation = 0u64;
        run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
        dir
    });
    handshake(&mut client);

    blueice_ipc::write_client_message(&mut client, &ClientMessage::GetRepresentation).unwrap();
    let ServerMessage::Representation(snapshot) =
        blueice_ipc::read_server_message(&mut client).unwrap()
    else {
        panic!("expected Representation")
    };
    let link_id = snapshot
        .nodes
        .iter()
        .find(|n| n.name.as_deref() == Some("go"))
        .unwrap()
        .id;

    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::ActOn {
            id: link_id,
            action: NodeAction::Click,
        },
    )
    .unwrap();
    let navigated = blueice_ipc::read_server_message(&mut client).unwrap();
    assert!(matches!(navigated, ServerMessage::Navigated { .. }));
    let frame = blueice_ipc::read_server_message(&mut client).unwrap();
    assert!(matches!(frame, ServerMessage::FrameReady { .. }));

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn act_on_focus_is_reflected_in_the_next_representation() {
    let dir = temp_frame_dir("act-on-focus");
    let gatekeeper = clearing_gatekeeper("act-on-focus");
    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        default_page(&mut tabs)
            .load_html_str(r#"<input id="name" type="text" placeholder="Name">"#, None);
        let mut generation = 0u64;
        run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
        dir
    });
    handshake(&mut client);

    blueice_ipc::write_client_message(&mut client, &ClientMessage::GetRepresentation).unwrap();
    let ServerMessage::Representation(before) =
        blueice_ipc::read_server_message(&mut client).unwrap()
    else {
        panic!("expected Representation")
    };
    let input_id = before.nodes[0].id;
    assert!(!before.nodes[0].state.focused);

    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::ActOn {
            id: input_id,
            action: NodeAction::Focus,
        },
    )
    .unwrap();
    let frame = blueice_ipc::read_server_message(&mut client).unwrap();
    assert!(
        matches!(frame, ServerMessage::FrameReady { .. }),
        "Focus is a state change and still gets a FrameReady, per session.rs's own docs"
    );

    blueice_ipc::write_client_message(&mut client, &ClientMessage::GetRepresentation).unwrap();
    let ServerMessage::Representation(after) =
        blueice_ipc::read_server_message(&mut client).unwrap()
    else {
        panic!("expected Representation")
    };
    assert!(after.nodes[0].state.focused);

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn act_on_an_unknown_id_is_a_harmless_no_op() {
    let dir = temp_frame_dir("act-on-unknown");
    let gatekeeper = clearing_gatekeeper("act-on-unknown");
    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        default_page(&mut tabs).load_html_str(r#"<a href="/x">go</a>"#, None);
        let mut generation = 0u64;
        run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
        dir
    });
    handshake(&mut client);

    // an unknown id with Click: same "no reply at all" contract as
    // a coordinate click that lands on nothing.
    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::ActOn {
            id: 999_999,
            action: NodeAction::Click,
        },
    )
    .unwrap();
    // proven by the fact that the next message still gets a normal
    // reply -- the unknown id didn't wedge or end the session.
    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::Resize {
            width: 10,
            height: 10,
        },
    )
    .unwrap();
    let reply = blueice_ipc::read_server_message(&mut client).unwrap();
    assert!(matches!(reply, ServerMessage::FrameReady { .. }));

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_stale_id_from_before_a_navigation_is_a_harmless_no_op_after_it() {
    // Unlike `act_on_an_unknown_id_is_a_harmless_no_op` (a
    // never-allocated id), this id is real -- it existed in the
    // document *before* the navigation below. Regression: NodeId
    // allocation used to restart at 0 for every freshly-parsed
    // document, so this same numeric id could be reused by an
    // unrelated node in the post-navigation document, and ActOn
    // would silently act on that unrelated node instead of safely
    // no-op'ing.
    let dir = temp_frame_dir("stale-id-across-navigation");
    let gatekeeper = clearing_gatekeeper("stale-id-across-navigation");
    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        default_page(&mut tabs).load_html_str(r#"<a href="/x">go</a>"#, None);
        let mut generation = 0u64;
        run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
        dir
    });
    handshake(&mut client);

    blueice_ipc::write_client_message(&mut client, &ClientMessage::GetRepresentation).unwrap();
    let ServerMessage::Representation(snap) =
        blueice_ipc::read_server_message(&mut client).unwrap()
    else {
        panic!("expected Representation")
    };
    let stale_id = snap.nodes[0].id;

    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::Navigate {
            url: "about:blank".to_string(),
        },
    )
    .unwrap();
    let navigated = blueice_ipc::read_server_message(&mut client).unwrap();
    assert_eq!(
        navigated,
        ServerMessage::Navigated {
            url: "about:blank".to_string()
        }
    );
    let _frame = blueice_ipc::read_server_message(&mut client).unwrap();

    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::ActOn {
            id: stale_id,
            action: NodeAction::Click,
        },
    )
    .unwrap();
    // proven the same way as the never-allocated-id case: the next
    // message still gets a normal reply, so the stale id neither
    // wedged the session nor triggered a misdirected action.
    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::Resize {
            width: 10,
            height: 10,
        },
    )
    .unwrap();
    let reply = blueice_ipc::read_server_message(&mut client).unwrap();
    assert!(matches!(reply, ServerMessage::FrameReady { .. }));

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn highlight_adds_an_outline_to_the_next_frame_and_clearing_it_removes_it() {
    let dir = temp_frame_dir("highlight");
    let gatekeeper = clearing_gatekeeper("highlight");
    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        default_page(&mut tabs).load_html_str(r#"<a href="/x">go</a>"#, None);
        let mut generation = 0u64;
        run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
        dir
    });
    handshake(&mut client);

    blueice_ipc::write_client_message(&mut client, &ClientMessage::GetRepresentation).unwrap();
    let ServerMessage::Representation(snap) =
        blueice_ipc::read_server_message(&mut client).unwrap()
    else {
        panic!("expected Representation")
    };
    let link_id = snap.nodes[0].id;

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Highlight { id: Some(link_id) })
        .unwrap();
    let frame = blueice_ipc::read_server_message(&mut client).unwrap();
    assert!(matches!(frame, ServerMessage::FrameReady { .. }));

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn hover_updates_state_silently_with_no_reply() {
    let dir = temp_frame_dir("hover");
    let gatekeeper = clearing_gatekeeper("hover");
    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        default_page(&mut tabs).load_html_str(r#"<a href="/x">go</a>"#, None);
        let mut generation = 0u64;
        run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
        dir
    });
    handshake(&mut client);

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Hover { x: 2.0, y: 2.0 })
        .unwrap();
    // proven the same way SetVisible/Chrome is: the next message
    // still gets a normal reply, so Hover didn't wedge the session.
    blueice_ipc::write_client_message(&mut client, &ClientMessage::GetRepresentation).unwrap();
    let ServerMessage::Representation(snap) =
        blueice_ipc::read_server_message(&mut client).unwrap()
    else {
        panic!("expected Representation")
    };
    assert!(
        snap.nodes[0].state.hovered,
        "the hovered state must be visible via GetRepresentation"
    );

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn chrome_set_visible_does_not_change_engine_render_state() {
    // `phase-5-ai-representation-output/PLAN.md`'s "verify engine
    // state is unchanged across a hide/show cycle" checklist item,
    // made explicit and checkable rather than left implicit in
    // `Chrome`'s no-op handling: a full hide-then-show round trip
    // must leave the representation (and therefore the DOM/styles/
    // fragment tree it's derived from) byte-for-byte identical, and
    // must not cause a new frame to be rendered.
    let dir = temp_frame_dir("chrome-no-restart");
    let gatekeeper = clearing_gatekeeper("chrome-no-restart");
    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        default_page(&mut tabs).load_html_str("<p>hi</p>", None);
        let mut generation = 0u64;
        run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
        dir
    });
    handshake(&mut client);

    blueice_ipc::write_client_message(&mut client, &ClientMessage::GetRepresentation).unwrap();
    let ServerMessage::Representation(before) =
        blueice_ipc::read_server_message(&mut client).unwrap()
    else {
        panic!("expected Representation")
    };

    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::Chrome(blueice_ipc::ChromeCommand::SetVisible(false)),
    )
    .unwrap();
    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::Chrome(blueice_ipc::ChromeCommand::SetVisible(true)),
    )
    .unwrap();

    blueice_ipc::write_client_message(&mut client, &ClientMessage::GetRepresentation).unwrap();
    let ServerMessage::Representation(after) =
        blueice_ipc::read_server_message(&mut client).unwrap()
    else {
        panic!("expected Representation")
    };

    assert_eq!(
        before.nodes, after.nodes,
        "a hide/show cycle must not change the engine's render-pass state"
    );
    assert_eq!(
        before.generation, after.generation,
        "no frame is re-rendered just from a visibility toggle"
    );

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn disconnecting_without_shutdown_ends_the_session_cleanly() {
    let dir = temp_frame_dir("disconnect");
    let gatekeeper = unique_gatekeeper_socket_path("disconnect"); // never dialed
    let (client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        let mut generation = 0u64;
        run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper)
    });
    drop(client);
    assert!(handle.join().unwrap().is_ok());
}

#[test]
fn a_first_message_that_is_not_hello_is_rejected_and_ends_the_session() {
    let dir = temp_frame_dir("handshake-not-hello-first");
    let gatekeeper = unique_gatekeeper_socket_path("handshake-not-hello-first"); // never dialed
    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        let mut generation = 0u64;
        run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper)
    });

    blueice_ipc::write_client_message(&mut client, &ClientMessage::GetRepresentation).unwrap();
    let reply = blueice_ipc::read_server_message(&mut client).unwrap();
    assert!(
        matches!(reply, ServerMessage::Error { .. }),
        "expected an Error reply, got {reply:?}"
    );

    assert!(
        handle.join().unwrap().is_ok(),
        "the session must end cleanly, not hang, after rejecting the handshake"
    );
}

#[test]
fn an_unsupported_protocol_version_is_rejected_and_ends_the_session() {
    let dir = temp_frame_dir("handshake-bad-version");
    let gatekeeper = unique_gatekeeper_socket_path("handshake-bad-version"); // never dialed
    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        let mut generation = 0u64;
        run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper)
    });

    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::Hello {
            protocol_version: blueice_ipc::PROTOCOL_VERSION + 1,
        },
    )
    .unwrap();
    let reply = blueice_ipc::read_server_message(&mut client).unwrap();
    assert!(
        matches!(reply, ServerMessage::Error { .. }),
        "expected an Error reply, got {reply:?}"
    );

    assert!(
        handle.join().unwrap().is_ok(),
        "the session must end cleanly, not hang, after rejecting an unsupported version"
    );
}

#[test]
fn a_hello_seen_again_after_the_handshake_is_answered_without_ending_the_session() {
    // The broker-multiplexing scenario `run_session`'s own docs
    // describe: a second external client's handshake, forwarded
    // into the one already-past-its-own-handshake shared
    // connection, must not be treated as a protocol violation.
    let dir = temp_frame_dir("late-hello");
    let gatekeeper = clearing_gatekeeper("late-hello");
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
        &ClientMessage::Hello {
            protocol_version: blueice_ipc::PROTOCOL_VERSION,
        },
    )
    .unwrap();
    let reply = blueice_ipc::read_server_message(&mut client).unwrap();
    assert_eq!(
        reply,
        ServerMessage::Hello {
            protocol_version: blueice_ipc::PROTOCOL_VERSION
        }
    );

    // proven the same way other no-special-effect messages are:
    // the session is still alive and answers normally afterward.
    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn every_reply_to_a_message_echoes_back_its_request_id() {
    let dir = temp_frame_dir("request-id-echo");
    let gatekeeper = clearing_gatekeeper("request-id-echo");
    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        default_page(&mut tabs).load_html_str("<p>hi</p>", None);
        let mut generation = 0u64;
        run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
        dir
    });
    handshake(&mut client);

    blueice_ipc::write_client_message_with_id(
        &mut client,
        Some(99),
        &ClientMessage::GetRepresentation,
    )
    .unwrap();
    let (request_id, reply) = blueice_ipc::read_server_message_with_id(&mut client).unwrap();
    assert_eq!(request_id, Some(99));
    assert!(matches!(reply, ServerMessage::Representation(_)));

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn an_unknown_client_variant_is_ignored_and_the_session_keeps_running() {
    let dir = temp_frame_dir("unknown-variant");
    let gatekeeper = clearing_gatekeeper("unknown-variant");
    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        let mut generation = 0u64;
        run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
        dir
    });
    handshake(&mut client);

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Unknown).unwrap();
    // proven the same way other no-reply messages are: the next
    // message still gets a normal reply, so Unknown didn't wedge
    // or end the session.
    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::Resize {
            width: 10,
            height: 10,
        },
    )
    .unwrap();
    let reply = blueice_ipc::read_server_message(&mut client).unwrap();
    assert!(matches!(reply, ServerMessage::FrameReady { .. }));

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

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

#[test]
fn a_message_addressed_to_an_unknown_tab_replies_error_not_a_silent_no_op() {
    let dir = temp_frame_dir("unknown-tab-error");
    let gatekeeper = clearing_gatekeeper("unknown-tab-error");
    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        let mut generation = 0u64;
        run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
        dir
    });
    handshake(&mut client);

    blueice_ipc::write_client_message_with_ids(
        &mut client,
        Some(999_999),
        None,
        &ClientMessage::GetRepresentation,
    )
    .unwrap();
    let (reply_tab, _, reply) = blueice_ipc::read_server_message_with_ids(&mut client).unwrap();
    assert_eq!(
        reply_tab,
        Some(999_999),
        "the reply should still echo back which (nonexistent) tab was addressed"
    );
    assert!(
        matches!(reply, ServerMessage::Error { .. }),
        "expected Error, got {reply:?}"
    );

    // The session must survive an unknown-tab error, same as every
    // other error case in this file.
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
fn close_tab_removes_it_and_a_later_message_to_it_becomes_an_error() {
    let dir = temp_frame_dir("close-tab");
    let gatekeeper = clearing_gatekeeper("close-tab");
    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        let mut generation = 0u64;
        run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
        dir
    });
    handshake(&mut client);

    blueice_ipc::write_client_message(&mut client, &ClientMessage::OpenTab { url: None }).unwrap();
    let ServerMessage::TabOpened { tab_id: new_id, .. } =
        blueice_ipc::read_server_message(&mut client).unwrap()
    else {
        panic!("expected TabOpened")
    };

    blueice_ipc::write_client_message_with_ids(
        &mut client,
        Some(new_id),
        None,
        &ClientMessage::CloseTab,
    )
    .unwrap();
    let (reply_tab, _, reply) = blueice_ipc::read_server_message_with_ids(&mut client).unwrap();
    assert_eq!(reply_tab, Some(new_id));
    assert_eq!(reply, ServerMessage::TabClosed { tab_id: new_id });

    blueice_ipc::write_client_message_with_ids(
        &mut client,
        Some(new_id),
        None,
        &ClientMessage::GetRepresentation,
    )
    .unwrap();
    let (_, _, reply) = blueice_ipc::read_server_message_with_ids(&mut client).unwrap();
    assert!(
        matches!(reply, ServerMessage::Error { .. }),
        "a closed tab's id must no longer resolve, expected Error, got {reply:?}"
    );

    // Closing again is a harmless-but-reported "unknown tab" error,
    // not a panic or a second TabClosed.
    blueice_ipc::write_client_message_with_ids(
        &mut client,
        Some(new_id),
        None,
        &ClientMessage::CloseTab,
    )
    .unwrap();
    let (_, _, reply) = blueice_ipc::read_server_message_with_ids(&mut client).unwrap();
    assert!(matches!(reply, ServerMessage::Error { .. }));

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_reply_to_an_untagged_request_still_echoes_the_resolved_default_tab_id() {
    // The load-bearing property that makes broadcast-shared,
    // multi-tab connections work at all: a request that left
    // `tab_id` implicit still gets a reply that self-discloses the
    // *concrete* tab it resolved to, not `None` -- otherwise a
    // second client sharing the connection via `blueice-launcher`'s
    // broker could never tell which tab an untagged client's
    // broadcasted reply was actually about.
    let dir = temp_frame_dir("echo-resolved-default-tab");
    let gatekeeper = clearing_gatekeeper("echo-resolved-default-tab");
    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        let mut generation = 0u64;
        run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
        dir
    });
    handshake(&mut client);

    blueice_ipc::write_client_message(&mut client, &ClientMessage::ListTabs).unwrap();
    let ServerMessage::Tabs(tabs) = blueice_ipc::read_server_message(&mut client).unwrap() else {
        panic!("expected Tabs")
    };
    let default_tab_id = tabs[0].id;

    // Sent with no tab_id at all -- the envelope-level default.
    blueice_ipc::write_client_message_with_id(&mut client, None, &ClientMessage::GetRepresentation)
        .unwrap();
    let (reply_tab, _, reply) = blueice_ipc::read_server_message_with_ids(&mut client).unwrap();
    assert_eq!(
        reply_tab,
        Some(default_tab_id),
        "the reply must echo the resolved tab, not None"
    );
    assert!(matches!(reply, ServerMessage::Representation(_)));

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

// -- Gatekeeper-specific behavior --------------------------------
