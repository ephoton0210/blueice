// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn v1_node_counts_are_captured_per_tab_in_order_ignoring_other_traffic() {
    let (core_side, mut core_observed) = UnixStream::pair().unwrap();
    let core_writer = Arc::new(Mutex::new(core_side.try_clone().unwrap()));
    let clients: Arc<Mutex<Vec<Sender<TaggedServerMessage>>>> = Arc::new(Mutex::new(Vec::new()));
    let broadcast_clients = Arc::clone(&clients);
    let broadcaster =
        thread::spawn(move || broadcast_core_to_clients(core_side, broadcast_clients));

    let responder = thread::spawn(move || {
        // Two requests arrive, one per tab, each addressed to its own tab.
        let mut requests = Vec::new();
        for _ in 0..2 {
            let (tab_id, request_id, message) =
                read_client_message_with_ids(&mut core_observed).unwrap();
            assert!(matches!(message, ClientMessage::GetRepresentation));
            requests.push((tab_id.unwrap(), request_id.unwrap()));
        }
        assert_eq!(requests.iter().map(|r| r.0).collect::<Vec<_>>(), [7, 9]);
        // Unrelated traffic first, then the answers out of order.
        write_server_message_with_id(
            &mut core_observed,
            Some(1),
            &ServerMessage::Navigated { url: "x".into() },
        )
        .unwrap();
        write_server_message_with_id(
            &mut core_observed,
            Some(requests[1].1),
            &ServerMessage::Representation(snapshot_with_nodes(3)),
        )
        .unwrap();
        write_server_message_with_id(
            &mut core_observed,
            Some(requests[0].1),
            &ServerMessage::Representation(snapshot_with_nodes(12)),
        )
        .unwrap();
        drop(core_observed);
    });

    let counts = capture_v1_node_counts(
        &core_writer,
        &clients,
        &[tab(7), tab(9)],
        Duration::from_secs(5),
    );
    assert_eq!(counts, [Some(12), Some(3)]);
    responder.join().unwrap();
    broadcaster.join().unwrap();
}

#[test]
fn a_tab_v1_does_not_describe_in_time_is_unmeasured_not_a_failure() {
    let (core_side, mut core_observed) = UnixStream::pair().unwrap();
    let core_writer = Arc::new(Mutex::new(core_side.try_clone().unwrap()));
    let clients: Arc<Mutex<Vec<Sender<TaggedServerMessage>>>> = Arc::new(Mutex::new(Vec::new()));
    let broadcast_clients = Arc::clone(&clients);
    let _broadcaster =
        thread::spawn(move || broadcast_core_to_clients(core_side, broadcast_clients));
    let responder = thread::spawn(move || {
        let (_, first, _) = read_client_message_with_ids(&mut core_observed).unwrap();
        let _ = read_client_message_with_ids(&mut core_observed).unwrap(); // the second is never answered
        write_server_message_with_id(
            &mut core_observed,
            first,
            &ServerMessage::Representation(snapshot_with_nodes(5)),
        )
        .unwrap();
        core_observed // keep the connection open so the wait times out
    });
    let counts = capture_v1_node_counts(
        &core_writer,
        &clients,
        &[tab(1), tab(2)],
        Duration::from_millis(400),
    );
    assert_eq!(counts, [Some(5), None]);
    drop(responder.join().unwrap());
    // Nothing to ask for means nothing to wait for.
    assert!(capture_v1_node_counts(&core_writer, &clients, &[], Duration::from_secs(5)).is_empty());
}

#[test]
fn a_comparable_replay_passes_the_structural_check_and_each_tab_is_asked_about() {
    let (mut stream, v2) = fake_v2(vec![
        ServerMessage::Representation(snapshot_with_nodes(11)),
        ServerMessage::Representation(snapshot_with_nodes(2)),
    ]);
    structural_health_check(&mut stream, &[41, 42], &[Some(10), Some(3)]).unwrap();
    assert_eq!(v2.join().unwrap(), [Some(41), Some(42)]);
}

#[test]
fn unmeasured_tabs_are_skipped_and_never_asked_about() {
    // Only the middle tab has a v1 measurement, so v2 is asked exactly once.
    let (mut stream, v2) = fake_v2(vec![ServerMessage::Representation(snapshot_with_nodes(4))]);
    structural_health_check(&mut stream, &[1, 2, 3], &[None, Some(4)]).unwrap();
    assert_eq!(v2.join().unwrap(), [Some(2)]);
    // No measurements at all: nothing is asked.
    let (mut stream, _peer) = UnixStream::pair().unwrap();
    structural_health_check(&mut stream, &[1, 2], &[]).unwrap();
}

#[test]
fn a_v2_that_cannot_describe_a_tab_or_stops_answering_fails_the_check() {
    let (mut stream, v2) = fake_v2(vec![ServerMessage::Error {
        message: "no such tab".into(),
    }]);
    let reason = structural_health_check(&mut stream, &[9], &[Some(3)]).unwrap_err();
    v2.join().unwrap();
    assert!(reason.contains("could not describe"), "{reason}");

    let (mut stream, peer) = UnixStream::pair().unwrap();
    drop(peer);
    assert!(structural_health_check(&mut stream, &[9], &[Some(3)]).is_err());
}

#[test]
fn forward_client_to_core_preserves_tab_id_and_request_id() {
    // Direct regression coverage for Part 1's fix at the primitive
    // level: previously this used the plain (ids-discarding)
    // `read_client_message`/`write_client_message`, so a client's
    // `tab_id`/`request_id` never survived being forwarded into
    // `core`.
    let (client_side, mut client_observed) = UnixStream::pair().unwrap();
    let (core_side, mut core_observed) = UnixStream::pair().unwrap();
    let core = Arc::new(Mutex::new(core_side));

    write_client_message_with_ids(
        &mut client_observed,
        Some(3),
        Some(42),
        &ClientMessage::Navigate {
            url: "https://example.com".to_string(),
        },
    )
    .unwrap();
    drop(client_observed);

    forward_client_to_core(client_side, Arc::clone(&core));

    assert_eq!(
        read_client_message_with_ids(&mut core_observed).unwrap(),
        (
            Some(3),
            Some(42),
            ClientMessage::Navigate {
                url: "https://example.com".to_string()
            }
        )
    );
}

#[test]
fn broadcast_core_to_clients_preserves_tab_id_and_request_id() {
    let (core_side, mut core_observed) = UnixStream::pair().unwrap();
    let (sender, receiver) = mpsc::channel();
    let clients = Arc::new(Mutex::new(vec![sender]));

    write_server_message_with_ids(
        &mut core_observed,
        Some(3),
        Some(42),
        &ServerMessage::Navigated {
            url: "about:blank".to_string(),
        },
    )
    .unwrap();
    drop(core_observed);

    broadcast_core_to_clients(core_side, clients);

    assert_eq!(
        receiver.recv().unwrap(),
        TaggedServerMessage {
            tab_id: Some(3),
            request_id: Some(42),
            message: ServerMessage::Navigated {
                url: "about:blank".to_string()
            }
        }
    );
}

#[test]
fn prepare_stable_endpoint_rejects_relative_oversized_and_parentless_or_missing_paths() {
    assert_eq!(
        prepare_stable_endpoint(Path::new("relative/path.sock"), "test")
            .unwrap_err()
            .kind(),
        io::ErrorKind::InvalidInput
    );

    let oversized = PathBuf::from("/").join("a".repeat(MAX_STABLE_ENDPOINT_SOCKET_PATH_BYTES + 1));
    assert_eq!(
        prepare_stable_endpoint(&oversized, "test")
            .unwrap_err()
            .kind(),
        io::ErrorKind::InvalidInput
    );

    // The filesystem root has no parent directory at all.
    assert_eq!(
        prepare_stable_endpoint(Path::new("/"), "test")
            .unwrap_err()
            .kind(),
        io::ErrorKind::InvalidInput
    );

    let missing_parent = PathBuf::from("/tmp")
        .join(format!(
            "blueice-launcher-test-missing-parent-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
        .join("socket.sock");
    assert_eq!(
        prepare_stable_endpoint(&missing_parent, "test")
            .unwrap_err()
            .kind(),
        io::ErrorKind::NotFound
    );
}

#[test]
fn prepare_stable_endpoint_surfaces_a_non_not_found_stat_error_verbatim() {
    use std::os::unix::ffi::OsStrExt;

    // An interior NUL byte makes the path un-stat-able with a non-`NotFound`
    // `InvalidInput` error, distinct from a merely-absent socket.
    let path = PathBuf::from("/tmp").join(std::ffi::OsStr::from_bytes(b"blueice-nul-\0-test"));
    assert_eq!(
        prepare_stable_endpoint(&path, "test").unwrap_err().kind(),
        io::ErrorKind::InvalidInput
    );
}

#[test]
fn tab_id_and_request_id_survive_a_full_relay_through_the_broker() {
    // The end-to-end regression test for Part 1's fix, driven over
    // the real broker primitives (`register_client`/
    // `forward_client_to_core`/`broadcast_core_to_clients`), not
    // just one function in isolation: a client's tagged message
    // must reach `core` with its ids intact, and `core`'s tagged
    // reply must reach the client's own socket with its ids intact
    // too.
    let (client_side, mut client_observed) = UnixStream::pair().unwrap();
    let (core_side, mut core_observed) = UnixStream::pair().unwrap();
    let core_writer = Arc::new(Mutex::new(core_side.try_clone().unwrap()));
    let clients: Arc<Mutex<Vec<Sender<TaggedServerMessage>>>> = Arc::new(Mutex::new(Vec::new()));

    register_client(client_side, Arc::clone(&core_writer), Arc::clone(&clients)).unwrap();

    write_client_message_with_ids(
        &mut client_observed,
        Some(3),
        Some(42),
        &ClientMessage::Navigate {
            url: "https://example.com".to_string(),
        },
    )
    .unwrap();

    let (tab_id, request_id, msg) = read_client_message_with_ids(&mut core_observed).unwrap();
    assert_eq!(
        tab_id,
        Some(3),
        "the broker must not silently drop tab_id on the forwarding path"
    );
    assert_eq!(
        request_id,
        Some(42),
        "the broker must not silently drop request_id on the forwarding path"
    );
    assert_eq!(
        msg,
        ClientMessage::Navigate {
            url: "https://example.com".to_string()
        }
    );

    write_server_message_with_ids(
        &mut core_observed,
        Some(3),
        Some(42),
        &ServerMessage::Navigated {
            url: "https://example.com".to_string(),
        },
    )
    .unwrap();
    drop(core_observed); // ends the broadcaster loop after the one message

    broadcast_core_to_clients(core_side, Arc::clone(&clients));

    let (tab_id, request_id, msg) = read_server_message_with_ids(&mut client_observed).unwrap();
    assert_eq!(
        tab_id,
        Some(3),
        "the broker must not silently drop tab_id on the reply path"
    );
    assert_eq!(
        request_id,
        Some(42),
        "the broker must not silently drop request_id on the reply path"
    );
    assert_eq!(
        msg,
        ServerMessage::Navigated {
            url: "https://example.com".to_string()
        }
    );
}

#[test]
fn capture_v1_tabs_filters_for_the_matching_request_id_and_ignores_other_traffic() {
    let (core_side, mut core_observed) = UnixStream::pair().unwrap();
    let core_writer = Arc::new(Mutex::new(core_side.try_clone().unwrap()));
    let clients: Arc<Mutex<Vec<Sender<TaggedServerMessage>>>> = Arc::new(Mutex::new(Vec::new()));

    // `capture_v1_tabs` only ever *writes* into `core_writer` and
    // then waits on its own registered channel -- something else
    // (the real broker's own broadcast thread, in production) has
    // to actually read `core`'s replies and fan them out to
    // `clients`. Stands that in here.
    let broadcast_clients = Arc::clone(&clients);
    let broadcaster =
        thread::spawn(move || broadcast_core_to_clients(core_side, broadcast_clients));

    let responder = thread::spawn(move || {
        let (_, request_id, msg) = read_client_message_with_ids(&mut core_observed).unwrap();
        assert!(matches!(msg, ClientMessage::ListTabs));
        // Some unrelated broadcast traffic first (another client's
        // concurrent action) -- must be skipped, not mistaken for
        // this call's own reply.
        write_server_message_with_id(
            &mut core_observed,
            Some(999_999),
            &ServerMessage::Navigated {
                url: "https://unrelated.example".to_string(),
            },
        )
        .unwrap();
        write_server_message_with_id(
            &mut core_observed,
            request_id,
            &ServerMessage::Tabs(vec![TabSummary {
                id: 1,
                url: Some("about:blank".to_string()),
                group_id: None,
            }]),
        )
        .unwrap();
        drop(core_observed); // ends the broadcaster loop
    });

    let tabs = capture_v1_tabs(&core_writer, &clients, Duration::from_secs(5)).unwrap();
    assert_eq!(
        tabs,
        vec![TabSummary {
            id: 1,
            url: Some("about:blank".to_string()),
            group_id: None,
        }]
    );
    responder.join().unwrap();
    broadcaster.join().unwrap();
}

#[test]
fn capture_v1_tabs_fails_if_no_reply_arrives_within_the_timeout() {
    let (core_side, _core_observed) = UnixStream::pair().unwrap(); // nobody replies
    let core_writer = Arc::new(Mutex::new(core_side));
    let clients: Arc<Mutex<Vec<Sender<TaggedServerMessage>>>> = Arc::new(Mutex::new(Vec::new()));

    assert!(capture_v1_tabs(&core_writer, &clients, Duration::from_millis(100)).is_err());
}

#[test]
fn capture_v1_tabs_fails_if_its_own_registered_sender_is_dropped_while_waiting() {
    // Distinct from the broadcast-connection-ends case below: here the
    // initial `ListTabs` write to `core` succeeds (the pair stays open),
    // but the `Sender` `capture_v1_tabs` itself registered in `clients`
    // is dropped out from under it -- the `mpsc::RecvTimeoutError::
    // Disconnected` branch, not a write failure or the plain deadline.
    let (core_side, _core_observed) = UnixStream::pair().unwrap();
    let core_writer = Arc::new(Mutex::new(core_side));
    let clients: Arc<Mutex<Vec<Sender<TaggedServerMessage>>>> = Arc::new(Mutex::new(Vec::new()));

    let waiter_clients = Arc::clone(&clients);
    let waiter = thread::spawn(move || {
        capture_v1_tabs(&core_writer, &waiter_clients, Duration::from_secs(5))
    });

    loop {
        if clients
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .len()
            == 1
        {
            break;
        }
        thread::sleep(Duration::from_millis(1));
    }
    clients
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clear();

    let err = waiter.join().unwrap().unwrap_err();
    assert!(err.contains("ended while waiting"));
}

#[test]
fn capture_v1_tabs_fails_if_the_broadcast_connection_ends_before_a_reply_arrives() {
    let (core_side, core_observed) = UnixStream::pair().unwrap();
    drop(core_observed); // "core" is already gone before ever replying
    let core_writer = Arc::new(Mutex::new(core_side));
    let clients: Arc<Mutex<Vec<Sender<TaggedServerMessage>>>> = Arc::new(Mutex::new(Vec::new()));

    // A short timeout parameter, not the real multi-second
    // `TAB_CAPTURE_TIMEOUT`: whether the write itself fails
    // immediately (broken pipe) or this call has to fall back to
    // its own deadline depends on OS-level socket-teardown timing,
    // which isn't worth this test waiting several real seconds to
    // observe either way -- both paths return `Err`.
    assert!(capture_v1_tabs(&core_writer, &clients, Duration::from_millis(200)).is_err());
}

#[test]
fn capture_v1_tabs_registers_and_the_stale_sender_self_prunes_on_the_next_broadcast() {
    let (core_side, mut core_observed) = UnixStream::pair().unwrap();
    let core_writer = Arc::new(Mutex::new(core_side.try_clone().unwrap()));
    let clients: Arc<Mutex<Vec<Sender<TaggedServerMessage>>>> = Arc::new(Mutex::new(Vec::new()));

    let broadcast_clients = Arc::clone(&clients);
    let broadcaster =
        thread::spawn(move || broadcast_core_to_clients(core_side, broadcast_clients));

    // Explicit hand-off, not just "send two messages in a row": the
    // *second* message must not reach the broadcaster until
    // `capture_v1_tabs` below has actually returned (and so dropped
    // its own `receiver`) -- otherwise there's a real race where the
    // second `send` could land while that receiver is still alive
    // (the drop happening a few instructions later, as the call
    // stack unwinds), succeeding instead of triggering the prune
    // this test means to prove.
    let (capture_returned_tx, capture_returned_rx) = mpsc::channel::<()>();
    let responder = thread::spawn(move || {
        let (_, request_id, _) = read_client_message_with_ids(&mut core_observed).unwrap();
        write_server_message_with_id(&mut core_observed, request_id, &ServerMessage::Tabs(vec![]))
            .unwrap();
        let _ = capture_returned_rx.recv();
        // This second broadcast message, sent only once the caller
        // below has confirmed `capture_v1_tabs` already returned, is
        // what the stale, still-registered `Sender` fails to
        // deliver, triggering its self-prune.
        write_server_message(
            &mut core_observed,
            &ServerMessage::Navigated {
                url: "x".to_string(),
            },
        )
        .unwrap();
        drop(core_observed); // ends the broadcaster loop
    });

    capture_v1_tabs(&core_writer, &clients, Duration::from_secs(5)).unwrap();
    assert_eq!(
        clients.lock().unwrap().len(),
        1,
        "the synthetic client is still registered right after capture returns"
    );
    let _ = capture_returned_tx.send(());

    // Wait for the broadcaster to process the second message (and
    // then end, once `core_observed` is dropped) before checking
    // that the stale sender was pruned.
    broadcaster.join().unwrap();
    assert_eq!(
        clients.lock().unwrap().len(),
        0,
        "the stale synthetic sender must self-prune once its receiver has been dropped"
    );
    responder.join().unwrap();
}

#[test]
fn replay_tabs_navigates_the_first_tab_and_opens_the_rest() {
    let (mut stream, mut server) = UnixStream::pair().unwrap();
    let tabs = vec![
        TabSummary {
            id: 1,
            url: Some("about:blank".to_string()),
            group_id: None,
        },
        TabSummary {
            id: 2,
            url: Some("about:credits".to_string()),
            group_id: None,
        },
        TabSummary {
            id: 3,
            url: None,
            group_id: None,
        },
    ];
    let responder = thread::spawn(move || {
        let (_, req, msg) = read_client_message_with_ids(&mut server).unwrap();
        assert_eq!(
            msg,
            ClientMessage::Navigate {
                url: "about:blank".to_string()
            }
        );
        write_server_message_with_id(
            &mut server,
            req,
            &ServerMessage::Navigated {
                url: "about:blank".to_string(),
            },
        )
        .unwrap();
        write_server_message_with_id(
            &mut server,
            req,
            &ServerMessage::FrameReady {
                shm_path: "x".into(),
                width: 1,
                height: 1,
                generation: 1,
            },
        )
        .unwrap();

        let (_, req, msg) = read_client_message_with_ids(&mut server).unwrap();
        assert_eq!(
            msg,
            ClientMessage::OpenTab {
                url: Some("about:credits".to_string())
            }
        );
        write_server_message_with_id(
            &mut server,
            req,
            &ServerMessage::TabOpened {
                tab_id: 2,
                url: Some("about:credits".to_string()),
            },
        )
        .unwrap();
        write_server_message_with_id(
            &mut server,
            req,
            &ServerMessage::FrameReady {
                shm_path: "y".into(),
                width: 1,
                height: 1,
                generation: 2,
            },
        )
        .unwrap();

        let (_, req, msg) = read_client_message_with_ids(&mut server).unwrap();
        assert_eq!(msg, ClientMessage::OpenTab { url: None });
        write_server_message_with_id(
            &mut server,
            req,
            &ServerMessage::TabOpened {
                tab_id: 3,
                url: None,
            },
        )
        .unwrap();
        // no FrameReady expected, since url was None
    });

    let frames = replay_tabs(&mut stream, &tabs).unwrap();
    assert_eq!(frames.len(), 2, "only navigated tabs have replay frames");
    assert!(frames.iter().all(|frame| frame.request_id.is_none()));
    assert!(matches!(
        frames[0].message,
        ServerMessage::FrameReady { .. }
    ));
    assert!(matches!(
        frames[1].message,
        ServerMessage::FrameReady { .. }
    ));
    responder.join().unwrap();
}

#[test]
fn replay_tabs_skips_the_first_tab_entirely_if_it_has_no_url() {
    let (mut stream, server) = UnixStream::pair().unwrap();
    let tabs = vec![TabSummary {
        id: 1,
        url: None,
        group_id: None,
    }];

    assert!(replay_tabs(&mut stream, &tabs).unwrap().is_empty());

    // No message should ever have been sent for the blank default
    // tab -- dropping the peer without ever reading confirms
    // nothing was written (a write into a full/closed pipe would
    // otherwise still succeed locally without a reader, so the real
    // proof is simply that this returns `Ok` with no interaction).
    drop(server);
}

#[test]
fn replay_tabs_aborts_on_the_first_error_reply() {
    let (mut stream, mut server) = UnixStream::pair().unwrap();
    let tabs = vec![TabSummary {
        id: 1,
        url: Some("http://bad".to_string()),
        group_id: None,
    }];
    let responder = thread::spawn(move || {
        let (_, req, _msg) = read_client_message_with_ids(&mut server).unwrap();
        write_server_message_with_id(
            &mut server,
            req,
            &ServerMessage::Error {
                message: "boom".to_string(),
            },
        )
        .unwrap();
    });

    let err = replay_tabs(&mut stream, &tabs).unwrap_err();
    assert!(err.contains("boom"));
    responder.join().unwrap();
}

#[test]
fn replay_tabs_aborts_on_a_gatekeeper_blocked_reply_for_a_later_tab() {
    let (mut stream, mut server) = UnixStream::pair().unwrap();
    let tabs = vec![
        TabSummary {
            id: 1,
            url: None,
            group_id: None,
        },
        TabSummary {
            id: 2,
            url: Some("http://bad".to_string()),
            group_id: None,
        },
    ];
    let responder = thread::spawn(move || {
        let (_, req, msg) = read_client_message_with_ids(&mut server).unwrap();
        assert!(matches!(msg, ClientMessage::OpenTab { .. }));
        write_server_message_with_id(
            &mut server,
            req,
            &ServerMessage::GatekeeperBlocked {
                reason: "nope".to_string(),
                category: "test".to_string(),
                url: "http://bad".to_string(),
            },
        )
        .unwrap();
    });

    let err = replay_tabs(&mut stream, &tabs).unwrap_err();
    assert!(err.contains("nope"));
    responder.join().unwrap();
}

#[test]
fn replay_tabs_aborts_on_a_gatekeeper_blocked_reply_for_the_first_default_tab() {
    let (mut stream, mut server) = UnixStream::pair().unwrap();
    let tabs = vec![TabSummary {
        id: 1,
        url: Some("http://bad".to_string()),
        group_id: None,
    }];
    let responder = thread::spawn(move || {
        let (_, req, msg) = read_client_message_with_ids(&mut server).unwrap();
        assert!(matches!(msg, ClientMessage::Navigate { .. }));
        write_server_message_with_id(
            &mut server,
            req,
            &ServerMessage::GatekeeperBlocked {
                reason: "blocked".to_string(),
                category: "test".to_string(),
                url: "http://bad".to_string(),
            },
        )
        .unwrap();
    });

    let err = replay_tabs(&mut stream, &tabs).unwrap_err();
    assert!(err.contains("blocked"));
    responder.join().unwrap();
}

#[test]
fn replay_tabs_ignores_a_reply_carrying_a_mismatched_request_id() {
    // The same request-id-filtering discipline `capture_v1_tabs`
    // uses, exercised on the replay side: a reply tagged with a
    // different id than the one this call's own message was tagged
    // with must be skipped, not mistaken for the real reply.
    let (mut stream, mut server) = UnixStream::pair().unwrap();
    let tabs = vec![TabSummary {
        id: 1,
        url: Some("about:blank".to_string()),
        group_id: None,
    }];
    let responder = thread::spawn(move || {
        let (_, req, msg) = read_client_message_with_ids(&mut server).unwrap();
        assert!(matches!(msg, ClientMessage::Navigate { .. }));
        write_server_message_with_id(
            &mut server,
            Some(123_456),
            &ServerMessage::Error {
                message: "belongs to someone else".to_string(),
            },
        )
        .unwrap();
        write_server_message_with_id(
            &mut server,
            req,
            &ServerMessage::Navigated {
                url: "about:blank".to_string(),
            },
        )
        .unwrap();
        write_server_message_with_id(
            &mut server,
            req,
            &ServerMessage::FrameReady {
                shm_path: "x".into(),
                width: 1,
                height: 1,
                generation: 1,
            },
        )
        .unwrap();
    });

    replay_tabs(&mut stream, &tabs).unwrap();
    responder.join().unwrap();
}

#[test]
fn replay_tabs_first_tab_skips_a_premature_frame_ready_and_an_unrelated_reply() {
    // `expect_navigate_success` must not mistake a `FrameReady` that
    // arrives before `Navigated`, or any other reply shape entirely, for
    // the real completion signal -- both are simply noise to wait past.
    let (mut stream, mut server) = UnixStream::pair().unwrap();
    let tabs = vec![TabSummary {
        id: 1,
        url: Some("about:blank".to_string()),
        group_id: None,
    }];
    let responder = thread::spawn(move || {
        let (_, req, msg) = read_client_message_with_ids(&mut server).unwrap();
        assert!(matches!(msg, ClientMessage::Navigate { .. }));
        write_server_message_with_id(
            &mut server,
            req,
            &ServerMessage::FrameReady {
                shm_path: "premature".into(),
                width: 1,
                height: 1,
                generation: 0,
            },
        )
        .unwrap();
        write_server_message_with_id(&mut server, req, &ServerMessage::Tabs(Vec::new())).unwrap();
        write_server_message_with_id(
            &mut server,
            req,
            &ServerMessage::Navigated {
                url: "about:blank".to_string(),
            },
        )
        .unwrap();
        write_server_message_with_id(
            &mut server,
            req,
            &ServerMessage::FrameReady {
                shm_path: "x".into(),
                width: 1,
                height: 1,
                generation: 1,
            },
        )
        .unwrap();
    });

    replay_tabs(&mut stream, &tabs).unwrap();
    responder.join().unwrap();
}

#[test]
fn replay_tabs_later_tab_skips_a_mismatched_id_and_a_premature_frame_ready_then_reports_an_error() {
    // The same request-id filtering and premature-`FrameReady` tolerance
    // as the first-tab `Navigate` path above, exercised on the later-tab
    // `OpenTab` path (`expect_open_tab_success`), plus a reply shape
    // that matches neither of its named arms at all, ending in a plain
    // `Error` reply rather than `GatekeeperBlocked`.
    let (mut stream, mut server) = UnixStream::pair().unwrap();
    let tabs = vec![
        TabSummary {
            id: 1,
            url: None,
            group_id: None,
        },
        TabSummary {
            id: 2,
            url: Some("http://bad".to_string()),
            group_id: None,
        },
    ];
    let responder = thread::spawn(move || {
        let (_, req, msg) = read_client_message_with_ids(&mut server).unwrap();
        assert!(matches!(msg, ClientMessage::OpenTab { .. }));
        write_server_message_with_id(&mut server, req, &ServerMessage::Tabs(Vec::new())).unwrap();
        write_server_message_with_id(
            &mut server,
            Some(999_999),
            &ServerMessage::TabOpened {
                tab_id: 2,
                url: Some("http://bad".to_string()),
            },
        )
        .unwrap();
        write_server_message_with_id(
            &mut server,
            req,
            &ServerMessage::FrameReady {
                shm_path: "premature".into(),
                width: 1,
                height: 1,
                generation: 0,
            },
        )
        .unwrap();
        write_server_message_with_id(
            &mut server,
            req,
            &ServerMessage::Error {
                message: "second boom".to_string(),
            },
        )
        .unwrap();
    });

    let err = replay_tabs(&mut stream, &tabs).unwrap_err();
    assert!(err.contains("second boom"));
    responder.join().unwrap();
}
