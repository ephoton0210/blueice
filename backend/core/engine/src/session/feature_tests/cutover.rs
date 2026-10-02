// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn get_representation_shares_the_current_generation_across_every_send_frame_call_site() {
    // `get_representation_shares_the_current_generation_with_the_last_frame`
    // below proves the "same render pass" invariant for `Resize`
    // alone; this extends the same proof to `Scroll`, `Highlight`,
    // and a non-navigating `ActOn` (`Focus`) -- the other distinct
    // `send_frame` call sites in `run_session` (`Click`/`ActOn`'s
    // Click variant only ever reach `send_frame` via the same
    // navigate path `Navigate` itself already exercises, so they add
    // no new coverage here). `send_frame` is a single choke point
    // every one of these routes through, so this is expected to
    // hold structurally -- but the invariant is central enough to
    // this project's premise to prove per call site, not infer from
    // one example.
    let dir = temp_frame_dir("representation-generation-all-sites");
    let gatekeeper = clearing_gatekeeper("representation-generation-all-sites");
    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        default_page(&mut tabs).load_html_str(r#"<input type="text">"#, None);
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
    let input_id = snap.nodes[0].id;

    let assert_matching_generation = |client: &mut UnixStream, send: ClientMessage| {
        blueice_ipc::write_client_message(client, &send).unwrap();
        let frame = blueice_ipc::read_server_message(client).unwrap();
        let ServerMessage::FrameReady {
            generation: frame_generation,
            ..
        } = frame
        else {
            panic!("expected FrameReady, got {frame:?}")
        };

        blueice_ipc::write_client_message(client, &ClientMessage::GetRepresentation).unwrap();
        let reply = blueice_ipc::read_server_message(client).unwrap();
        let ServerMessage::Representation(snapshot) = reply else {
            panic!("expected Representation, got {reply:?}")
        };
        assert_eq!(snapshot.generation, frame_generation);
    };

    assert_matching_generation(&mut client, ClientMessage::Scroll { delta_y: 10.0 });
    assert_matching_generation(&mut client, ClientMessage::Highlight { id: Some(input_id) });
    assert_matching_generation(
        &mut client,
        ClientMessage::ActOn {
            id: input_id,
            action: NodeAction::Focus,
        },
    );

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn get_representation_shares_the_current_generation_with_the_last_frame() {
    // the concrete, checkable "same render pass" proof
    // `phase-5-ai-representation-output/PLAN.md` asks for: a
    // Representation and the FrameReady sent alongside a prior
    // state change carry the identical generation number.
    let dir = temp_frame_dir("representation-generation");
    let gatekeeper = clearing_gatekeeper("representation-generation");
    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        default_page(&mut tabs).load_html_str(r#"<a href="/x">Go</a>"#, None);
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
    let frame = blueice_ipc::read_server_message(&mut client).unwrap();
    let ServerMessage::FrameReady {
        generation: frame_generation,
        ..
    } = frame
    else {
        panic!("expected FrameReady, got {frame:?}")
    };

    blueice_ipc::write_client_message(&mut client, &ClientMessage::GetRepresentation).unwrap();
    let reply = blueice_ipc::read_server_message(&mut client).unwrap();
    let ServerMessage::Representation(snapshot) = reply else {
        panic!("expected Representation, got {reply:?}")
    };
    assert_eq!(snapshot.generation, frame_generation);
    assert!(snapshot
        .nodes
        .iter()
        .any(|n| n.name.as_deref() == Some("Go")));

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_second_navigation_supersedes_a_still_pending_first_one() {
    let dir = temp_frame_dir("supersede");
    let gatekeeper_path = unique_gatekeeper_socket_path("supersede");
    let _ = std::fs::remove_file(&gatekeeper_path);
    let listener = UnixListener::bind(&gatekeeper_path).unwrap();
    thread::spawn(move || {
        for incoming in listener.incoming() {
            let Ok(mut stream) = incoming else { break };
            thread::spawn(move || {
                if let Ok(req) = blueice_ipc::gatekeeper::read_gatekeeper_request(&mut stream) {
                    if matches!(&req, blueice_ipc::gatekeeper::GatekeeperRequest::CheckUrl { url } if url.contains("first"))
                    {
                        thread::sleep(Duration::from_millis(300));
                    }
                    let _ = blueice_ipc::gatekeeper::write_gatekeeper_reply(
                        &mut stream,
                        &blueice_ipc::gatekeeper::GatekeeperReply::Cleared,
                    );
                }
            });
        }
    });

    let second_http = TcpListener::bind("127.0.0.1:0").unwrap();
    let second_addr = second_http.local_addr().unwrap();
    thread::spawn(move || {
        let (mut stream, _) = second_http.accept().unwrap();
        let mut buf = [0u8; 1024];
        let _ = std::io::Read::read(&mut stream, &mut buf);
        let body = "<p>second page</p>";
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

    // First navigation: stalls 300ms on its own CheckUrl stage, and
    // even once cleared points nowhere reachable -- must never
    // become visible.
    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::Navigate {
            url: "http://127.0.0.1:1/first-slow".to_string(),
        },
    )
    .unwrap();
    // Second navigation to the same (default) tab, sent immediately
    // after, well before the first's gatekeeper check resolves.
    let second_url = format!("http://{second_addr}");
    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::Navigate {
            url: second_url.clone(),
        },
    )
    .unwrap();

    let navigated = blueice_ipc::read_server_message(&mut client).unwrap();
    assert_eq!(navigated, ServerMessage::Navigated { url: second_url });
    let frame = blueice_ipc::read_server_message(&mut client).unwrap();
    assert!(matches!(frame, ServerMessage::FrameReady { .. }));

    // No further reply ever arrives for the stale first navigation,
    // even after waiting past its stall -- proven the same way
    // every other "harmless no-op" case in this file is: the next
    // real message still gets exactly one, normal reply.
    thread::sleep(Duration::from_millis(400));
    blueice_ipc::write_client_message(&mut client, &ClientMessage::GetRepresentation).unwrap();
    let ServerMessage::Representation(snap) =
        blueice_ipc::read_server_message(&mut client).unwrap()
    else {
        panic!("expected Representation")
    };
    assert!(snap
        .nodes
        .iter()
        .any(|n| n.name.as_deref() == Some("second page")));

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}
