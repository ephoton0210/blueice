// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;
use blueice_ipc::*;

#[test]
fn bounded_values_policy_is_independent_of_static_metadata() {
    let options = CoreLaunchOptions::default()
        .with_debugger_endpoint(PathBuf::from("/tmp/debugger.sock"))
        .with_debugger_bounded_values();
    assert!(options.debugger_bounded_values);
    assert!(!options.debugger_static_metadata_inventory);
}

#[test]
fn forward_client_to_core_relays_one_message_then_stops_on_disconnect() {
    let (client_side, mut client_observed) = UnixStream::pair().unwrap();
    let (core_side, mut core_observed) = UnixStream::pair().unwrap();
    let core = Arc::new(Mutex::new(core_side));

    write_client_message(
        &mut client_observed,
        &ClientMessage::Resize {
            width: 10,
            height: 20,
        },
    )
    .unwrap();
    drop(client_observed); // triggers a clean disconnect after the one message

    forward_client_to_core(client_side, Arc::clone(&core));

    assert_eq!(
        read_client_message(&mut core_observed).unwrap(),
        ClientMessage::Resize {
            width: 10,
            height: 20
        }
    );
}

#[test]
fn forward_client_to_core_stops_once_the_core_connection_is_gone() {
    let (client_side, mut client_observed) = UnixStream::pair().unwrap();
    let (core_side, core_observed) = UnixStream::pair().unwrap();
    drop(core_observed); // core is "gone" before any message arrives
    let core = Arc::new(Mutex::new(core_side));

    write_client_message(&mut client_observed, &ClientMessage::Shutdown).unwrap();
    // Also close the client's own peer: writing to an already-
    // closed Unix domain socket peer isn't *always* an immediate
    // error (the kernel can let one small write through before it
    // fully propagates the peer's close) -- without this, a rare
    // timing race could let the write-to-`core` attempt below
    // spuriously succeed once, sending this loop around for a
    // second `read` that would then block forever with nothing
    // left to read. Closing this peer too guarantees a prompt
    // return via *that* read failing, regardless of which race
    // outcome the write hits.
    drop(client_observed);

    // Must return (not hang or panic) once the connection to
    // `core` and/or `client` is gone.
    forward_client_to_core(client_side, core);
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
fn broadcast_core_to_clients_relays_one_message_to_every_registered_client() {
    let (core_side, mut core_observed) = UnixStream::pair().unwrap();
    let (sender1, receiver1) = mpsc::channel();
    let (sender2, receiver2) = mpsc::channel();
    let clients = Arc::new(Mutex::new(vec![sender1, sender2]));

    write_server_message(
        &mut core_observed,
        &ServerMessage::Navigated {
            url: "about:blank".to_string(),
        },
    )
    .unwrap();
    drop(core_observed); // ends the broadcaster loop after the one message

    broadcast_core_to_clients(core_side, clients);

    let expected = TaggedServerMessage {
        tab_id: None,
        request_id: None,
        message: ServerMessage::Navigated {
            url: "about:blank".to_string(),
        },
    };
    assert_eq!(receiver1.recv().unwrap(), expected);
    assert_eq!(receiver2.recv().unwrap(), expected);
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
fn broadcast_core_to_clients_drops_a_client_whose_channel_is_gone_without_affecting_the_others() {
    let (core_side, mut core_observed) = UnixStream::pair().unwrap();
    let (dead_sender, dead_receiver) = mpsc::channel();
    drop(dead_receiver); // stands in for that client's writer thread having already exited
    let (live_sender, live_receiver) = mpsc::channel();
    let clients = Arc::new(Mutex::new(vec![dead_sender, live_sender]));

    write_server_message(
        &mut core_observed,
        &ServerMessage::Navigated {
            url: "about:blank".to_string(),
        },
    )
    .unwrap();
    drop(core_observed);

    broadcast_core_to_clients(core_side, Arc::clone(&clients));

    assert_eq!(
        live_receiver.recv().unwrap().message,
        ServerMessage::Navigated {
            url: "about:blank".to_string()
        }
    );
    // the dead client's sender must have been pruned from the list.
    assert_eq!(clients.lock().unwrap().len(), 1);
}

#[test]
fn register_client_forwards_its_messages_and_receives_broadcasts() {
    let (client_side, mut client_observed) = UnixStream::pair().unwrap();
    let (core_side, mut core_observed) = UnixStream::pair().unwrap();
    let core_writer = Arc::new(Mutex::new(core_side));
    let clients: Arc<Mutex<Vec<Sender<TaggedServerMessage>>>> = Arc::new(Mutex::new(Vec::new()));

    register_client(client_side, Arc::clone(&core_writer), Arc::clone(&clients)).unwrap();

    // fan-in: a message the "client" sends must reach core.
    write_client_message(&mut client_observed, &ClientMessage::GetRepresentation).unwrap();
    assert_eq!(
        read_client_message(&mut core_observed).unwrap(),
        ClientMessage::GetRepresentation
    );

    // fan-out: a message sent into the registered channel (standing
    // in for the broadcaster) must reach the client's real socket,
    // relayed by this client's own writer thread.
    let registered = clients.lock().unwrap().pop().unwrap();
    registered
        .send(TaggedServerMessage {
            tab_id: Some(1),
            request_id: Some(9),
            message: ServerMessage::Navigated {
                url: "x".to_string(),
            },
        })
        .unwrap();
    assert_eq!(
        read_server_message_with_ids(&mut client_observed).unwrap(),
        (
            Some(1),
            Some(9),
            ServerMessage::Navigated {
                url: "x".to_string()
            }
        )
    );
}

#[test]
fn client_write_half_gets_a_bounded_write_timeout() {
    // Regression coverage for a real deadlock this fixes: without a
    // write timeout, a client whose writer thread stalls (its
    // kernel socket buffer fills and nobody drains it) would never
    // notice the client is gone -- its channel would just queue up
    // forever instead of eventually being pruned. See
    // `a_slow_client_does_not_block_delivery_to_another_client` for
    // the actual "doesn't block other clients" property this and
    // the channel/writer-thread split together provide.
    let (client_side, _client_observed) = UnixStream::pair().unwrap();
    let write_half = client_write_half(&client_side).unwrap();
    assert_eq!(
        write_half.write_timeout().unwrap(),
        Some(CLIENT_WRITE_TIMEOUT)
    );
}

#[test]
fn a_slow_client_does_not_block_delivery_to_another_client() {
    // The actual bug this channel/writer-thread design fixes: a
    // single shared thread writing to every registered client's
    // socket directly, one after another, meant one client that
    // stalls (its kernel socket buffer fills, e.g. a large message
    // nobody drains) blocked delivery to every *other* client too,
    // for as long as that stalled write took to time out --
    // confirmed against a real launcher/core pair before this fix
    // existed. Proven here with a real filled socket buffer, not a
    // mock: `slow_client`'s peer end is never read; `live_client`'s
    // is read immediately after this call returns, and must have
    // its message waiting regardless of the slow client's state.
    let (core_side, mut core_observed) = UnixStream::pair().unwrap();
    let (slow_client_side, _never_read) = UnixStream::pair().unwrap();
    let (live_client_side, mut live_client_observed) = UnixStream::pair().unwrap();
    let (core_writer_side, _unused) = UnixStream::pair().unwrap();
    let core_writer = Arc::new(Mutex::new(core_writer_side));
    let clients: Arc<Mutex<Vec<Sender<TaggedServerMessage>>>> = Arc::new(Mutex::new(Vec::new()));

    register_client(
        slow_client_side,
        Arc::clone(&core_writer),
        Arc::clone(&clients),
    )
    .unwrap();
    register_client(
        live_client_side,
        Arc::clone(&core_writer),
        Arc::clone(&clients),
    )
    .unwrap();

    // Comfortably larger than any realistic default kernel socket
    // buffer, so the write to `slow_client`'s writer thread genuinely
    // blocks rather than merely being slow to observe. Written on
    // its own thread since *this* write can itself block until
    // `broadcast_core_to_clients` below is actively reading
    // `core_side` -- `core_observed`'s own send buffer is no bigger
    // than any other socket's here.
    let big_message = ServerMessage::Dom("x".repeat(4 * 1024 * 1024));
    let sent = big_message.clone();
    thread::spawn(move || {
        write_server_message(&mut core_observed, &sent).unwrap();
        drop(core_observed); // ends the broadcaster loop after the one message
    });

    let start = Instant::now();
    broadcast_core_to_clients(core_side, clients);
    assert!(start.elapsed() < Duration::from_secs(1), "fanning out must be a cheap non-blocking queue push regardless of any client's own writer-thread state");

    let received = read_server_message(&mut live_client_observed).unwrap();
    assert_eq!(
        received, big_message,
        "the live client must receive its own copy promptly, not stalled behind the slow one"
    );
}

#[test]
fn default_rendezvous_socket_path_is_per_user_not_system_wide() {
    let path = default_rendezvous_socket_path();
    assert_eq!(path.file_name().unwrap(), "core.sock");
    // must not resolve to a single fixed system-wide path regardless
    // of environment -- it has to vary by runtime dir or uid.
    assert_ne!(path, PathBuf::from("/core.sock"));
}

#[test]
fn sibling_core_binary_sits_next_to_the_launcher_binary() {
    let exe = PathBuf::from("/some/target/debug/blueice-launcher");
    assert_eq!(
        sibling_core_binary(&exe),
        PathBuf::from("/some/target/debug/blueice-core")
    );
}

#[test]
fn sibling_core_binary_steps_out_of_a_deps_directory_for_integration_tests() {
    let exe = PathBuf::from("/some/target/debug/deps/broker_end_to_end-abc123");
    assert_eq!(
        sibling_core_binary(&exe),
        PathBuf::from("/some/target/debug/blueice-core")
    );
}

#[test]
fn unique_internal_socket_path_stays_short_enough_for_af_unix() {
    assert!(unique_internal_socket_path().to_string_lossy().len() < 100);
    assert!(unique_internal_script_socket_path().to_string_lossy().len() < 100);
}

#[test]
fn unique_internal_socket_path_differs_across_multiple_calls_in_the_same_process() {
    // A cutover calls `SpawnedCore::spawn` a second time within the
    // same launcher process (v1 at startup, v2 during cutover) --
    // PID alone would collide, so this must also vary per call.
    assert_ne!(unique_internal_socket_path(), unique_internal_socket_path());
    assert_ne!(
        unique_internal_script_socket_path(),
        unique_internal_script_socket_path()
    );
}

fn unique_compiler_mcp_test_path(label: &str) -> PathBuf {
    PathBuf::from("/tmp").join(format!(
        "blueice-launcher-compiler-mcp-{label}-{}-{}.sock",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

#[test]
fn compiler_mcp_endpoint_validation_rejects_non_socket_paths_without_unlinking_them() {
    let path = unique_compiler_mcp_test_path("regular-file");
    std::fs::write(&path, b"do not remove").unwrap();

    let error = prepare_compiler_mcp_endpoint(&path).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
    assert_eq!(std::fs::read(&path).unwrap(), b"do not remove");

    let _ = std::fs::remove_file(path);
}

#[test]
fn compiler_mcp_endpoint_validation_reclaims_only_a_stale_socket_and_rejects_a_live_one() {
    let stale = unique_compiler_mcp_test_path("stale");
    let stale_listener = UnixListener::bind(&stale).unwrap();
    drop(stale_listener);
    // macOS can retain a just-closed local listener briefly while it
    // drains the final descriptor state.  The production path only
    // runs at startup, but give this deterministic stale-fixture a
    // moment to become observably refused.
    thread::sleep(Duration::from_millis(20));
    prepare_compiler_mcp_endpoint(&stale).unwrap();
    assert!(
        !stale.exists(),
        "a stale socket may be reclaimed before any child is spawned"
    );

    let live = unique_compiler_mcp_test_path("live");
    let _live_listener = UnixListener::bind(&live).unwrap();
    let error = prepare_compiler_mcp_endpoint(&live).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::AddrInUse);
    assert!(live.exists(), "a live endpoint must remain intact");

    let _ = std::fs::remove_file(live);
}

#[test]
fn compiler_mcp_endpoint_validation_happens_before_core_child_spawn() {
    let path = unique_compiler_mcp_test_path("preflight");
    std::fs::write(&path, b"must survive preflight").unwrap();
    let frame_dir = std::env::temp_dir().join(format!(
        "blueice-launcher-compiler-mcp-preflight-{}",
        std::process::id()
    ));
    let result = SpawnedCore::spawn_with_options(
        1.0,
        1.0,
        &frame_dir,
        CoreLaunchOptions::default().with_core_closed_compiler_mcp_endpoint(path.clone()),
    );
    let error = match result {
        Ok(_) => panic!("a non-socket compiler endpoint must reject before spawning core"),
        Err(error) => error,
    };
    assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
    assert_eq!(std::fs::read(&path).unwrap(), b"must survive preflight");
    assert!(
        !frame_dir.exists(),
        "a rejected compiler endpoint must not start a core that creates a frame directory"
    );

    let _ = std::fs::remove_file(path);
}

#[test]
fn wait_for_socket_returns_true_once_the_path_exists() {
    let path =
        std::env::temp_dir().join(format!("blueice-launcher-wait-test-{}", std::process::id()));
    let _ = std::fs::remove_file(&path);
    std::fs::write(&path, b"x").unwrap();
    assert!(wait_for_socket(&path, Duration::from_millis(50)));
    let _ = std::fs::remove_file(&path);
}

#[test]
fn wait_for_socket_times_out_if_the_path_never_appears() {
    let path = std::env::temp_dir().join(format!(
        "blueice-launcher-wait-test-missing-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);
    assert!(!wait_for_socket(&path, Duration::from_millis(50)));
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
fn generation_tagged_broadcast_signals_done_when_its_generation_is_still_current() {
    let (core_side, core_observed) = UnixStream::pair().unwrap();
    drop(core_observed); // "core" is already gone
    let clients: Arc<Mutex<Vec<Sender<TaggedServerMessage>>>> = Arc::new(Mutex::new(Vec::new()));
    let generation = Arc::new(AtomicU64::new(0));
    let (done_tx, done_rx) = mpsc::channel();

    spawn_generation_tagged_broadcast(core_side, clients, Arc::clone(&generation), 0, done_tx);

    done_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("an unsuperseded broadcast thread's death must signal done");
}

#[test]
fn generation_tagged_broadcast_stays_quiet_when_superseded() {
    let (core_side, core_observed) = UnixStream::pair().unwrap();
    drop(core_observed); // standing in for a cutover's deliberate close of v1's stream
    let clients: Arc<Mutex<Vec<Sender<TaggedServerMessage>>>> = Arc::new(Mutex::new(Vec::new()));
    let generation = Arc::new(AtomicU64::new(1)); // already bumped past this thread's own generation
    let (done_tx, done_rx) = mpsc::channel();

    spawn_generation_tagged_broadcast(core_side, clients, generation, 0, done_tx);

    assert!(
        done_rx.recv_timeout(Duration::from_millis(200)).is_err(),
        "a superseded broadcast thread's death must NOT signal done"
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
            url: Some("about:blank".to_string())
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
        },
        TabSummary {
            id: 2,
            url: Some("about:credits".to_string()),
        },
        TabSummary { id: 3, url: None },
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

    replay_tabs(&mut stream, &tabs).unwrap();
    responder.join().unwrap();
}

#[test]
fn replay_tabs_skips_the_first_tab_entirely_if_it_has_no_url() {
    let (mut stream, server) = UnixStream::pair().unwrap();
    let tabs = vec![TabSummary { id: 1, url: None }];

    replay_tabs(&mut stream, &tabs).unwrap();

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
        TabSummary { id: 1, url: None },
        TabSummary {
            id: 2,
            url: Some("http://bad".to_string()),
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
fn health_check_ignores_a_reply_carrying_a_mismatched_request_id_and_other_traffic() {
    let (mut stream, mut server) = UnixStream::pair().unwrap();
    let expected = vec![TabSummary {
        id: 1,
        url: Some("about:blank".to_string()),
    }];
    let responder = thread::spawn(move || {
        let (_, req, msg) = read_client_message_with_ids(&mut server).unwrap();
        assert!(matches!(msg, ClientMessage::ListTabs));
        write_server_message_with_id(
            &mut server,
            Some(999),
            &ServerMessage::Navigated {
                url: "unrelated".to_string(),
            },
        )
        .unwrap();
        write_server_message_with_id(
            &mut server,
            req,
            &ServerMessage::FrameReady {
                shm_path: "z".into(),
                width: 1,
                height: 1,
                generation: 1,
            },
        )
        .unwrap();
        write_server_message_with_id(
            &mut server,
            req,
            &ServerMessage::Tabs(vec![TabSummary {
                id: 99,
                url: Some("about:blank".to_string()),
            }]),
        )
        .unwrap();
    });

    health_check(&mut stream, &expected).unwrap();
    responder.join().unwrap();
}

#[test]
fn health_check_succeeds_when_urls_match() {
    let (mut stream, mut server) = UnixStream::pair().unwrap();
    let expected = vec![TabSummary {
        id: 1,
        url: Some("about:blank".to_string()),
    }];
    let responder = thread::spawn(move || {
        let (_, req, msg) = read_client_message_with_ids(&mut server).unwrap();
        assert!(matches!(msg, ClientMessage::ListTabs));
        // v2's own ids differ from v1's -- only urls must match.
        write_server_message_with_id(
            &mut server,
            req,
            &ServerMessage::Tabs(vec![TabSummary {
                id: 99,
                url: Some("about:blank".to_string()),
            }]),
        )
        .unwrap();
    });

    health_check(&mut stream, &expected).unwrap();
    responder.join().unwrap();
}

#[test]
fn health_check_fails_when_urls_dont_match() {
    let (mut stream, mut server) = UnixStream::pair().unwrap();
    let expected = vec![TabSummary {
        id: 1,
        url: Some("about:blank".to_string()),
    }];
    let responder = thread::spawn(move || {
        let (_, req, _msg) = read_client_message_with_ids(&mut server).unwrap();
        write_server_message_with_id(&mut server, req, &ServerMessage::Tabs(vec![])).unwrap();
    });

    assert!(health_check(&mut stream, &expected).is_err());
    responder.join().unwrap();
}

#[test]
fn v2_frame_dir_differs_from_v1s_and_varies_by_target_generation() {
    let v1 = PathBuf::from("/tmp/blueice-frames-123");
    let a = v2_frame_dir(&v1, 1);
    let b = v2_frame_dir(&v1, 2);
    assert_ne!(a, v1);
    assert_ne!(b, v1);
    assert_ne!(
        a, b,
        "a repeated cutover must not reuse the same v2 frame_dir"
    );
}
