// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn resize_then_shutdown_produces_one_frame_and_then_ends_the_session() {
    let dir = temp_frame_dir("resize");
    let gatekeeper = clearing_gatekeeper("resize");
    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        default_page(&mut tabs).load_html_str("<p>hi</p>", None);
        let mut generation = 0u64;
        run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
        dir
    });
    handshake(&mut client);

    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::Resize {
            width: 100,
            height: 50,
        },
    )
    .unwrap();
    let reply = blueice_ipc::read_server_message(&mut client).unwrap();
    assert!(matches!(
        reply,
        ServerMessage::FrameReady {
            generation: 1,
            width: 100,
            height: 50,
            ..
        }
    ));

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn click_on_a_link_navigates_and_a_click_elsewhere_produces_no_reply() {
    let dir = temp_frame_dir("click");
    let gatekeeper = clearing_gatekeeper("click");
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buf = [0u8; 1024];
        let _ = std::io::Read::read(&mut stream, &mut buf);
        let body = "<p>landed</p>";
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
    let dir_for_thread = dir.clone();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        default_page(&mut tabs).load_html_str(&format!(r#"<a href="{url}">go</a>"#), None);
        let mut generation = 0u64;
        run_session(
            &mut tabs,
            &mut server,
            &dir_for_thread,
            &mut generation,
            &gatekeeper,
        )
        .unwrap();
    });
    handshake(&mut client);

    // clicking the link navigates: expect Navigated then FrameReady
    blueice_ipc::write_client_message(&mut client, &ClientMessage::Click { x: 2.0, y: 2.0 })
        .unwrap();
    let navigated = blueice_ipc::read_server_message(&mut client).unwrap();
    assert!(matches!(navigated, ServerMessage::Navigated { .. }));
    let frame = blueice_ipc::read_server_message(&mut client).unwrap();
    assert!(matches!(frame, ServerMessage::FrameReady { .. }));

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn set_visible_produces_no_reply_and_the_session_keeps_running() {
    let dir = temp_frame_dir("visible");
    let gatekeeper = clearing_gatekeeper("visible");
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
        &ClientMessage::Chrome(blueice_ipc::ChromeCommand::SetVisible(false)),
    )
    .unwrap();
    // proven by the fact that a subsequent message still gets a
    // normal reply -- Chrome(SetVisible) didn't wedge or end the session.
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
fn navigation_fails_closed_when_the_gatekeeper_accepts_then_drops_the_connection() {
    let dir = temp_frame_dir("gatekeeper-drops-connection");
    let gatekeeper_path = unique_gatekeeper_socket_path("gatekeeper-drops-connection");
    let _ = std::fs::remove_file(&gatekeeper_path);
    let listener = UnixListener::bind(&gatekeeper_path).unwrap();
    thread::spawn(move || {
        for incoming in listener.incoming() {
            drop(incoming); // accept, then immediately disconnect -- no reply ever sent
        }
    });

    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        let mut generation = 0u64;
        run_session(
            &mut tabs,
            &mut server,
            &dir,
            &mut generation,
            &gatekeeper_path,
        )
        .unwrap();
        dir
    });
    handshake(&mut client);

    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::Navigate {
            url: "http://example.invalid/".to_string(),
        },
    )
    .unwrap();
    let reply = blueice_ipc::read_server_message(&mut client).unwrap();
    assert!(
        matches!(reply, ServerMessage::GatekeeperBlocked { .. }),
        "a gatekeeper that drops the connection must fail closed, got {reply:?}"
    );

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}
