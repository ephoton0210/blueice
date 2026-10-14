// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn navigate_replies_with_navigated_then_a_frame_reflecting_the_new_page() {
    let dir = temp_frame_dir("navigate");
    let gatekeeper = clearing_gatekeeper("navigate");
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buf = [0u8; 1024];
        let _ = std::io::Read::read(&mut stream, &mut buf);
        let body = "<p>fetched page</p>";
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
    blueice_ipc::write_client_message(&mut client, &ClientMessage::Navigate { url: url.clone() })
        .unwrap();
    let navigated = blueice_ipc::read_server_message(&mut client).unwrap();
    assert_eq!(navigated, ServerMessage::Navigated { url });
    let frame = blueice_ipc::read_server_message(&mut client).unwrap();
    let shm_path = match frame {
        ServerMessage::FrameReady {
            shm_path,
            generation: 1,
            ..
        } => shm_path,
        other => panic!("expected FrameReady, got {other:?}"),
    };
    assert!(
        shm::map_frame(std::path::Path::new(&shm_path)).is_ok(),
        "the frame-plane file must actually exist and be mappable"
    );

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn navigate_to_an_unreachable_host_replies_with_error_not_a_frame() {
    let dir = temp_frame_dir("navigate-error");
    let gatekeeper = clearing_gatekeeper("navigate-error");
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
        &ClientMessage::Navigate {
            url: "not-a-valid-url".to_string(),
        },
    )
    .unwrap();
    let reply = blueice_ipc::read_server_message(&mut client).unwrap();
    assert!(matches!(reply, ServerMessage::Error { .. }));

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn get_dom_returns_the_full_tree_unfiltered_by_the_ai_representation() {
    let dir = temp_frame_dir("get-dom");
    let gatekeeper = clearing_gatekeeper("get-dom");
    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        default_page(&mut tabs)
            .load_html_str(r#"<div style="background-color: red;">x</div>"#, None);
        let mut generation = 0u64;
        run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
        dir
    });
    handshake(&mut client);

    blueice_ipc::write_client_message(&mut client, &ClientMessage::GetDom).unwrap();
    let reply = blueice_ipc::read_server_message(&mut client).unwrap();
    let ServerMessage::Dom(dump) = reply else {
        panic!("expected Dom, got {reply:?}")
    };
    assert!(
        dump.contains("<div>"),
        "a bare div has no AI-representation role but must still appear in the full DOM dump: {dump}"
    );

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

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

    // The native frontend sends these narrower keyboard messages rather
    // than guessing a DOM node ID. They are accepted only because the
    // preceding focus action selected this supported text input.
    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::InsertText {
            text: "BlueIce".to_string(),
        },
    )
    .unwrap();
    assert!(matches!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::FrameReady { .. }
    ));
    blueice_ipc::write_client_message(&mut client, &ClientMessage::GetRepresentation).unwrap();
    let ServerMessage::Representation(with_text) =
        blueice_ipc::read_server_message(&mut client).unwrap()
    else {
        panic!("expected Representation")
    };
    assert_eq!(with_text.nodes[0].state.value.as_deref(), Some("BlueIce"));

    blueice_ipc::write_client_message(&mut client, &ClientMessage::DeleteBackward).unwrap();
    assert!(matches!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::FrameReady { .. }
    ));
    blueice_ipc::write_client_message(&mut client, &ClientMessage::GetRepresentation).unwrap();
    let ServerMessage::Representation(after_delete) =
        blueice_ipc::read_server_message(&mut client).unwrap()
    else {
        panic!("expected Representation")
    };
    assert_eq!(after_delete.nodes[0].state.value.as_deref(), Some("BlueIc"));

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn act_on_set_value_is_reflected_in_the_next_representation() {
    let dir = temp_frame_dir("act-on-set-value");
    let gatekeeper = clearing_gatekeeper("act-on-set-value");
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
    assert_eq!(before.nodes[0].state.value, None);

    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::ActOn {
            id: input_id,
            action: NodeAction::SetValue("BlueIce".to_string()),
        },
    )
    .unwrap();
    assert!(matches!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::FrameReady { .. }
    ));

    blueice_ipc::write_client_message(&mut client, &ClientMessage::GetRepresentation).unwrap();
    let ServerMessage::Representation(after) =
        blueice_ipc::read_server_message(&mut client).unwrap()
    else {
        panic!("expected Representation")
    };
    assert_eq!(after.nodes[0].state.value.as_deref(), Some("BlueIce"));

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
fn an_unchanged_list_does_not_keep_pushing_frames() {
    let dir = DownloadsScratch::new("sess-quiet");
    let state = FakeState {
        transfers: Arc::new(Mutex::new(vec![dl(
            1,
            "alpha.iso",
            TransferState::Paused,
            400,
        )])),
        ..FakeState::default()
    };
    let lists = state.lists.clone();
    let _server = fake_downloads_live(&dir.socket(), state, false, DOWNLOADS_PROTOCOL_VERSION);
    let (mut client, handle) = downloads_session("sess-quiet", dir.socket());
    let initial_lists = lists.load(Ordering::SeqCst);
    navigate_to(&mut client, "about:downloads");
    next_refresh(&mut client);
    assert!(lists.load(Ordering::SeqCst) > initial_lists);

    let polled_before = lists.load(Ordering::SeqCst);
    let deadline = Instant::now() + Duration::from_secs(5);
    while lists.load(Ordering::SeqCst) <= polled_before {
        assert!(
            next_pushed_frame(&mut client, Duration::from_millis(50)).is_none(),
            "nothing changed, so nothing should be pushed"
        );
        assert!(
            Instant::now() < deadline,
            "the refresher never made another list request"
        );
    }
    finish_session(client, handle);
}
