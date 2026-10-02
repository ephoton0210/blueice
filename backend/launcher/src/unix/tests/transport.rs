// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn waiting_for_a_socket_gives_up_at_once_when_the_child_exits() {
    let mut child = Command::new("true").spawn().unwrap();
    let started = Instant::now();
    let path = std::env::temp_dir().join(format!("never-{}.sock", std::process::id()));
    assert!(!wait_for_socket_or_exit(
        &path,
        &mut child,
        Duration::from_secs(20)
    ));
    assert!(started.elapsed() < Duration::from_secs(5));
    let _ = child.wait();
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
fn default_rendezvous_socket_path_is_per_user_not_system_wide() {
    let path = default_rendezvous_socket_path();
    assert_eq!(path.file_name().unwrap(), "core.sock");
    assert_eq!(
        path,
        blueice_ipc::local_socket::default_socket_dir().join("core.sock")
    );
    // must not resolve to a single fixed system-wide path regardless
    // of environment -- it has to vary by runtime dir or uid.
    assert_ne!(path, PathBuf::from("/core.sock"));
}

#[test]
fn rendezvous_socket_dir_prefers_xdg_runtime_dir_when_set() {
    // SAFETY: no other test in this crate reads or writes `XDG_RUNTIME_DIR`,
    // and this crate's own binaries never run in-process during unit tests.
    let previous = std::env::var_os("XDG_RUNTIME_DIR");
    unsafe {
        std::env::set_var("XDG_RUNTIME_DIR", "/tmp/blueice-xdg-test-runtime-dir");
    }
    let dir = rendezvous_socket_dir();
    match previous {
        Some(value) => unsafe { std::env::set_var("XDG_RUNTIME_DIR", value) },
        None => unsafe { std::env::remove_var("XDG_RUNTIME_DIR") },
    }
    assert_eq!(
        dir,
        PathBuf::from("/tmp/blueice-xdg-test-runtime-dir").join("blueice")
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
fn connect_when_listening_waits_for_a_socket_that_is_not_yet_bound() {
    let path = std::env::temp_dir().join(format!(
        "blueice-connect-wait-test-{}.sock",
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
fn connect_when_listening_gives_up_after_the_timeout() {
    let path = std::env::temp_dir().join(format!(
        "blueice-connect-wait-test-missing-{}.sock",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);
    assert!(connect_when_listening(&path, Duration::from_millis(50)).is_err());
}
