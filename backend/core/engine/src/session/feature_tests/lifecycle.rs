// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

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
            url: "about:blank".to_string(),
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
fn content_stage_rejection_blocks_navigation_and_leaves_the_page_unchanged() {
    let dir = temp_frame_dir("content-stage-block");
    let gatekeeper_path = unique_gatekeeper_socket_path("content-stage-block");
    let _ = std::fs::remove_file(&gatekeeper_path);
    let listener = UnixListener::bind(&gatekeeper_path).unwrap();
    thread::spawn(move || {
        for incoming in listener.incoming() {
            let Ok(mut stream) = incoming else { break };
            let Ok(req) = blueice_ipc::gatekeeper::read_gatekeeper_request(&mut stream) else {
                continue;
            };
            let reply = match req {
                blueice_ipc::gatekeeper::GatekeeperRequest::CheckUrl { .. } => {
                    blueice_ipc::gatekeeper::GatekeeperReply::Cleared
                }
                blueice_ipc::gatekeeper::GatekeeperRequest::CheckContent { .. } => {
                    blueice_ipc::gatekeeper::GatekeeperReply::Rejected {
                        reason: "hidden instruction-shaped text".to_string(),
                        category: "prompt-injection".to_string(),
                    }
                }
                blueice_ipc::gatekeeper::GatekeeperRequest::CheckDownload { .. } => {
                    unreachable!(
                        "navigation never sends a download check; that stage belongs to the downloads process"
                    )
                }
                blueice_ipc::gatekeeper::GatekeeperRequest::CheckExtensionAction { .. } => {
                    unreachable!(
                        "navigation never sends an extension action check; that stage belongs to the extension host"
                    )
                }
            };
            let _ = blueice_ipc::gatekeeper::write_gatekeeper_reply(&mut stream, &reply);
        }
    });

    let http = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = http.local_addr().unwrap();
    thread::spawn(move || {
        let (mut stream, _) = http.accept().unwrap();
        let mut buf = [0u8; 1024];
        let _ = std::io::Read::read(&mut stream, &mut buf);
        let body = "<p>malicious page</p>";
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

    let url = format!("http://{addr}");
    blueice_ipc::write_client_message(&mut client, &ClientMessage::Navigate { url: url.clone() })
        .unwrap();
    let reply = blueice_ipc::read_server_message(&mut client).unwrap();
    assert_eq!(
        reply,
        ServerMessage::GatekeeperBlocked {
            reason: "hidden instruction-shaped text".to_string(),
            category: "prompt-injection".to_string(),
            url: url.clone()
        }
    );

    // The page must not have changed: a follow-up GetRepresentation
    // shows no trace of the blocked page's content (no `FrameReady`
    // was ever produced for it either, since the only reply so far
    // was the GatekeeperBlocked above).
    blueice_ipc::write_client_message(&mut client, &ClientMessage::GetRepresentation).unwrap();
    let ServerMessage::Representation(snap) =
        blueice_ipc::read_server_message(&mut client).unwrap()
    else {
        panic!("expected Representation")
    };
    assert!(!snap
        .nodes
        .iter()
        .any(|n| n.name.as_deref() == Some("malicious page")));

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn navigation_fails_closed_when_the_gatekeeper_is_unreachable() {
    let dir = temp_frame_dir("gatekeeper-unreachable");
    let gatekeeper_path = unique_gatekeeper_socket_path("gatekeeper-unreachable"); // nothing listens here
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
        "an unreachable gatekeeper must fail closed, got {reply:?}"
    );

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn an_absent_service_shows_the_not_running_page_and_the_list_appears_when_it_starts() {
    let dir = DownloadsScratch::new("sess-late");
    let (mut client, handle) = downloads_session("sess-late", dir.socket());
    navigate_to(&mut client, "about:downloads");
    assert!(dom_text(&mut client).contains("The downloads service is not running"));

    // The service comes up later; the open page notices without being reloaded.
    let state = FakeState {
        transfers: Arc::new(Mutex::new(vec![dl(
            3,
            "gamma.bin",
            TransferState::Active,
            10,
        )])),
        ..FakeState::default()
    };
    let _server = fake_downloads_live(&dir.socket(), state, false, DOWNLOADS_PROTOCOL_VERSION);
    let deadline = Instant::now() + Duration::from_secs(6);
    loop {
        let _ = next_pushed_frame(&mut client, Duration::from_millis(50));
        let dom = dom_text(&mut client);
        if dom.contains("gamma.bin") && !dom.contains("not running") {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the open page never showed the recovered service: {dom}"
        );
    }
    finish_session(client, handle);
}
