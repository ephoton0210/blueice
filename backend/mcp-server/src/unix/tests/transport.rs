// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn navigate_waits_for_its_terminal_reply_before_requesting_a_snapshot() {
    // A real `http(s)` navigation completes asynchronously in core. The
    // old implementation sent GetRepresentation immediately, so core
    // processed it before this reply and MCP returned the prior page.
    let (client, mut server) = UnixStream::pair().unwrap();
    thread::spawn(move || {
        let (_, action_id, first) = blueice_ipc::read_client_message_with_ids(&mut server).unwrap();
        assert!(matches!(first, ClientMessage::Navigate { .. }));

        // There must not yet be a pipelined GetRepresentation. If there
        // is, emulate core's old-page reply; the assertion below then
        // proves the adapter did not accept it as the navigate result.
        server
            .set_read_timeout(Some(Duration::from_millis(50)))
            .unwrap();
        match blueice_ipc::read_client_message(&mut server) {
            Ok(ClientMessage::GetRepresentation) => {
                blueice_ipc::write_server_message_with_id(
                    &mut server,
                    action_id,
                    &ServerMessage::Representation(sample_snapshot(0)),
                )
                .unwrap();
            }
            Ok(other) => panic!("expected GetRepresentation, got {other:?}"),
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                ) =>
            {
                blueice_ipc::write_server_message_with_id(
                    &mut server,
                    action_id,
                    &ServerMessage::Navigated {
                        url: "https://example.com/new".to_string(),
                    },
                )
                .unwrap();
                blueice_ipc::write_server_message_with_ids(
                    &mut server,
                    Some(1),
                    action_id,
                    &ServerMessage::FrameReady {
                        shm_path: "/tmp/new".to_string(),
                        width: 10,
                        height: 10,
                        generation: 1,
                    },
                )
                .unwrap();
                server.set_read_timeout(None).unwrap();
                let (_, representation_id, second) =
                    blueice_ipc::read_client_message_with_ids(&mut server).unwrap();
                assert!(matches!(second, ClientMessage::GetRepresentation));
                blueice_ipc::write_server_message_with_id(
                    &mut server,
                    representation_id,
                    &ServerMessage::Representation(sample_snapshot(1)),
                )
                .unwrap();
            }
            Err(error) => panic!("unexpected read error: {error}"),
        }
    });

    let mut conn = CoreConnection::new(client);
    let outcome = conn.navigate("https://example.com/new", None).unwrap();
    assert_eq!(outcome.error, None);
    assert_eq!(outcome.snapshot.generation, 1);
    assert_eq!(conn.last_frame(Some(1)).unwrap().generation, 1);
}

#[test]
fn act_on_focus_gets_a_frame_reply_before_the_representation() {
    let (client, server) = UnixStream::pair().unwrap();
    fake_core(
        server,
        vec![
            Box::new(|msg, s| {
                assert!(matches!(
                    msg,
                    ClientMessage::ActOn {
                        action: NodeAction::Focus,
                        ..
                    }
                ));
                reply_tab(
                    s,
                    1,
                    &ServerMessage::FrameReady {
                        shm_path: "/tmp/y".to_string(),
                        width: 5,
                        height: 5,
                        generation: 2,
                    },
                );
            }),
            Box::new(|msg, s| {
                assert!(matches!(msg, ClientMessage::GetRepresentation));
                reply(s, &ServerMessage::Representation(sample_snapshot(2)));
            }),
        ],
    );

    let mut conn = CoreConnection::new(client);
    let outcome = conn.act(1, NodeAction::Focus, None).unwrap();
    assert_eq!(outcome.snapshot.generation, 2);
    assert_eq!(conn.last_frame(Some(1)).unwrap().shm_path, "/tmp/y");
}

#[test]
fn shutdown_sends_the_shutdown_message() {
    let (client, server) = UnixStream::pair().unwrap();
    fake_core(
        server,
        vec![Box::new(|msg, _s| {
            assert!(matches!(msg, ClientMessage::Shutdown));
        })],
    );

    let mut conn = CoreConnection::new(client);
    conn.shutdown().unwrap();
}

#[test]
fn connect_to_attaches_to_a_reachable_rendezvous_socket_instead_of_spawning() {
    let rendezvous_path = std::env::temp_dir().join(format!(
        "blueice-mcp-test-rendezvous-{}.sock",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&rendezvous_path);
    let listener = std::os::unix::net::UnixListener::bind(&rendezvous_path).unwrap();
    let accepted = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        assert!(matches!(
            blueice_ipc::read_client_message(&mut stream).unwrap(),
            ClientMessage::Hello { .. }
        ));
        blueice_ipc::write_server_message(
            &mut stream,
            &ServerMessage::Hello {
                protocol_version: blueice_ipc::PROTOCOL_VERSION,
            },
        )
        .unwrap();
    });

    let core = CoreProcess::connect_to(&rendezvous_path, 320, 200)
        .expect("must attach to the reachable rendezvous socket");
    assert!(
        matches!(core.ownership, CoreOwnership::Shared),
        "a reachable rendezvous socket must produce Shared ownership, not a private spawn"
    );

    accepted.join().unwrap();
    let _ = std::fs::remove_file(&rendezvous_path);
}

#[test]
fn dropping_a_shared_core_process_does_not_send_shutdown() {
    let rendezvous_path = std::env::temp_dir().join(format!(
        "blueice-mcp-test-rendezvous-noshutdown-{}.sock",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&rendezvous_path);
    let listener = std::os::unix::net::UnixListener::bind(&rendezvous_path).unwrap();
    let accepted = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        assert!(matches!(
            blueice_ipc::read_client_message(&mut stream).unwrap(),
            ClientMessage::Hello { .. }
        ));
        blueice_ipc::write_server_message(
            &mut stream,
            &ServerMessage::Hello {
                protocol_version: blueice_ipc::PROTOCOL_VERSION,
            },
        )
        .unwrap();
        // If Drop ever sends Shutdown, this read succeeds with that
        // message; a plain disconnect makes it error instead (EOF)
        // -- assert the latter, proving no Shutdown was sent.
        blueice_ipc::read_client_message(&mut stream)
    });

    let core = CoreProcess::connect_to(&rendezvous_path, 320, 200).unwrap();
    drop(core);

    assert!(
        accepted.join().unwrap().is_err(),
        "a shared core's connection must just close, never receive an explicit Shutdown"
    );
    let _ = std::fs::remove_file(&rendezvous_path);
}

#[test]
fn connect_to_falls_back_to_spawning_when_nothing_is_listening() {
    let rendezvous_path = std::env::temp_dir().join(format!(
        "blueice-mcp-test-rendezvous-missing-{}.sock",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&rendezvous_path);

    let core = CoreProcess::connect_to(&rendezvous_path, 320, 200)
        .expect("blueice-core must spawn and accept a connection");
    assert!(
        matches!(core.ownership, CoreOwnership::PrivatelySpawned { .. }),
        "an unreachable rendezvous socket must fall back to a private spawn"
    );
    drop(core); // tears down the real spawned subprocess
}

#[test]
fn unique_socket_path_stays_short_enough_for_af_unix() {
    assert!(unique_socket_path().to_string_lossy().len() < 100);
}

#[test]
fn wait_for_socket_returns_true_once_the_path_exists() {
    let path = std::env::temp_dir().join(format!("blueice-mcp-wait-test-{}", std::process::id()));
    let _ = std::fs::remove_file(&path);
    std::fs::write(&path, b"x").unwrap();
    assert!(wait_for_socket(&path, Duration::from_millis(50)));
    let _ = std::fs::remove_file(&path);
}

#[test]
fn wait_for_socket_times_out_if_the_path_never_appears() {
    let path = std::env::temp_dir().join("blueice-mcp-never-appears.sock");
    let _ = std::fs::remove_file(&path);
    assert!(!wait_for_socket(&path, Duration::from_millis(50)));
}

#[test]
fn handshake_succeeds_against_a_matching_hello_reply() {
    let (client, mut server) = UnixStream::pair().unwrap();
    thread::spawn(move || {
        assert!(matches!(
            blueice_ipc::read_client_message(&mut server).unwrap(),
            ClientMessage::Hello { .. }
        ));
        reply(
            &mut server,
            &ServerMessage::Hello {
                protocol_version: blueice_ipc::PROTOCOL_VERSION,
            },
        );
    });

    let mut conn = CoreConnection::new(client);
    conn.handshake().unwrap();
}

#[test]
fn handshake_surfaces_an_unsupported_version_error() {
    let (client, mut server) = UnixStream::pair().unwrap();
    thread::spawn(move || {
        let _ = blueice_ipc::read_client_message(&mut server).unwrap();
        reply(
            &mut server,
            &ServerMessage::Error {
                message: "unsupported protocol_version".to_string(),
            },
        );
    });

    let mut conn = CoreConnection::new(client);
    assert!(conn.handshake().is_err());
}

#[test]
fn send_and_drain_ignores_a_reply_carrying_a_different_requests_id() {
    // The exact regression this correlation exists to fix
    // (`phase-8-live-core-hotswap/PLAN.md`'s flagged broadcast-
    // misattribution gap): sharing a `core` connection through
    // `blueice-launcher`'s broker means an `Error` from another
    // client's concurrent, unrelated action can arrive interleaved
    // with this call's own replies. It must be skipped, not
    // mistaken for this call's own error.
    let (client, server) = UnixStream::pair().unwrap();
    fake_core(
        server,
        vec![
            Box::new(|msg, s| {
                assert!(matches!(msg, ClientMessage::Navigate { .. }));
                // A stray reply tagged with a request_id that
                // belongs to neither of this call's own two
                // outgoing messages -- stands in for another
                // client's concurrently-broadcast traffic.
                blueice_ipc::write_server_message_with_id(
                    s,
                    Some(9_999),
                    &ServerMessage::Error {
                        message: "unrelated client's failure".to_string(),
                    },
                )
                .unwrap();
                reply(
                    s,
                    &ServerMessage::Navigated {
                        url: "https://example.com".to_string(),
                    },
                );
                reply_tab(
                    s,
                    1,
                    &ServerMessage::FrameReady {
                        shm_path: "/tmp/example".to_string(),
                        width: 10,
                        height: 10,
                        generation: 1,
                    },
                );
            }),
            Box::new(|msg, s| {
                assert!(matches!(msg, ClientMessage::GetRepresentation));
                reply(s, &ServerMessage::Representation(sample_snapshot(1)));
            }),
        ],
    );

    let mut conn = CoreConnection::new(client);
    let outcome = conn.navigate("https://example.com", None).unwrap();
    assert_eq!(
        outcome.error, None,
        "the stray, differently-tagged Error must not be attributed to this call"
    );
    assert_eq!(outcome.snapshot.generation, 1);
}

#[test]
fn connect_when_listening_gives_up_after_the_timeout() {
    let path = std::env::temp_dir().join(format!(
        "blueice-mcp-connect-wait-missing-{}.sock",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);
    assert!(connect_when_listening(&path, Duration::from_millis(50)).is_err());
}

#[test]
fn connect_when_listening_waits_for_a_socket_that_is_not_yet_bound() {
    let path = std::env::temp_dir().join(format!(
        "blueice-mcp-connect-wait-{}.sock",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);
    let binder = {
        let path = path.clone();
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(100));
            let listener = UnixListener::bind(&path).unwrap();
            listener.accept().map(|_| ()).unwrap();
        })
    };
    connect_when_listening(&path, Duration::from_secs(5)).unwrap();
    binder.join().unwrap();
    let _ = std::fs::remove_file(&path);
}

#[test]
fn dom_ignores_a_mismatched_id_and_an_unrelated_reply() {
    let (client, server) = UnixStream::pair().unwrap();
    fake_core(
        server,
        vec![Box::new(|msg, s| {
            assert!(matches!(msg, ClientMessage::GetDom));
            blueice_ipc::write_server_message_with_id(
                s,
                Some(9_999),
                &ServerMessage::Error {
                    message: "unrelated client's failure".to_string(),
                },
            )
            .unwrap();
            reply(s, &ServerMessage::Tabs(Vec::new()));
            reply(s, &ServerMessage::Dom("| <html>\n".to_string()));
        })],
    );

    let mut conn = CoreConnection::new(client);
    assert_eq!(conn.dom(None).unwrap(), "| <html>\n");
}

#[test]
fn representation_ignores_a_mismatched_id_and_an_unrelated_reply() {
    let (client, server) = UnixStream::pair().unwrap();
    fake_core(
        server,
        vec![Box::new(|msg, s| {
            assert!(matches!(msg, ClientMessage::GetRepresentation));
            blueice_ipc::write_server_message_with_id(
                s,
                Some(9_999),
                &ServerMessage::Error {
                    message: "unrelated client's failure".to_string(),
                },
            )
            .unwrap();
            reply(s, &ServerMessage::Tabs(Vec::new()));
            reply(s, &ServerMessage::Representation(sample_snapshot(2)));
        })],
    );

    let mut conn = CoreConnection::new(client);
    let snap = conn.representation(None).unwrap();
    assert_eq!(snap.generation, 2);
}
