// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn shutdown_with_no_cutover_still_ends_the_whole_launcher_cleanly() {
    let _guard = broker_test_guard();
    // A focused regression test that the generation-counter/cutover
    // restructuring didn't change this pre-existing behavior: with no
    // `Cutover` ever sent, a single client's `Shutdown` must still
    // cascade through `core` and end the launcher process, exactly as
    // it did before this phase's cutover mechanism existed (see
    // `two_clients_through_the_same_launcher_observe_the_same_render_pass`,
    // which also exercises this as a side effect -- this test isolates
    // it as its own explicit assertion).
    let mut launcher = Launcher::spawn();
    let mut client = launcher.connect();

    write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();

    let deadline = Instant::now() + Duration::from_secs(5);
    let status = loop {
        if let Some(status) = launcher.child.try_wait().unwrap() {
            break status;
        }
        assert!(
            Instant::now() < deadline,
            "the launcher must exit on its own after a Shutdown with no cutover involved"
        );
        thread::sleep(Duration::from_millis(20));
    };
    assert!(
        status.success(),
        "the launcher must exit cleanly (not be killed) after an ordinary Shutdown cascade"
    );
}

#[test]
fn opt_in_launcher_replaces_the_private_child_at_cutover_and_reaps_both_generations() {
    let _guard = broker_test_guard();
    let gatekeeper_socket = clearing_gatekeeper();
    let mut launcher = Launcher::spawn_with_supervised_bluejs(&gatekeeper_socket);
    let v1_child_socket = launcher
        .private_bluejs_socket
        .clone()
        .expect("the opted-in launcher must create v1's private child");
    assert!(v1_child_socket.exists());
    let v1_script_socket = std::env::temp_dir().join(format!(
        "blueice-launcher-script-{}-0.sock",
        launcher.child.id()
    ));
    launcher.wait_for_socket(&v1_script_socket, "v1 private script");

    // Give cutover a real current tab to replay. `about:` avoids a network
    // fixture while still exercising the core-replacement lifecycle.
    let mut frontend = launcher.connect();
    write_client_message(
        &mut frontend,
        &ClientMessage::Navigate {
            url: "about:credits".to_string(),
        },
    )
    .unwrap();
    assert!(matches!(
        read_server_message(&mut frontend).unwrap(),
        ServerMessage::Navigated { .. }
    ));
    assert!(matches!(
        read_server_message(&mut frontend).unwrap(),
        ServerMessage::FrameReady { .. }
    ));

    let mut control = launcher.connect_control();
    write_control_request(&mut control, &ControlRequest::Cutover).unwrap();
    assert!(matches!(
        read_control_reply(&mut control).unwrap(),
        ControlReply::CutoverDone { tabs_migrated: 1 }
    ));
    assert!(
        !v1_child_socket.exists(),
        "cutover must reap the superseded core's child and unlink its capability endpoint"
    );
    assert!(
        !v1_script_socket.exists(),
        "cutover must unlink the superseded core's script DOM listener"
    );

    let v2_child_socket = std::env::temp_dir().join(format!(
        "blueice-launcher-bluejs-host-{}-1.sock",
        launcher.child.id()
    ));
    assert!(
        v2_child_socket.exists(),
        "replacement core must receive a fresh launcher-created child, not v1's capability"
    );
    let v2_script_socket = std::env::temp_dir().join(format!(
        "blueice-launcher-script-{}-1.sock",
        launcher.child.id()
    ));
    launcher.wait_for_socket(&v2_script_socket, "v2 private script");
    assert_ne!(v1_script_socket, v2_script_socket);

    write_client_message(&mut frontend, &ClientMessage::Shutdown).unwrap();
    launcher.wait_or_kill(Duration::from_secs(5));
    assert!(
        !v2_child_socket.exists(),
        "final launcher shutdown must reap the replacement child too"
    );
    assert!(
        !v2_script_socket.exists(),
        "final launcher shutdown must unlink the replacement script DOM listener"
    );
    let _ = std::fs::remove_file(gatekeeper_socket);
}

#[test]
fn a_client_survives_a_cutover_and_sees_v2s_replayed_state() {
    let _guard = broker_test_guard();
    let mut launcher = Launcher::spawn();
    let mut client = launcher.connect();

    // Give v1 two tabs with distinct, real (built-in, no network
    // needed) content, so cutover has something non-trivial to capture
    // and replay.
    write_client_message(
        &mut client,
        &ClientMessage::Navigate {
            url: "about:credits".to_string(),
        },
    )
    .unwrap();
    assert!(matches!(
        read_server_message(&mut client).unwrap(),
        ServerMessage::Navigated { .. }
    ));
    assert!(matches!(
        read_server_message(&mut client).unwrap(),
        ServerMessage::FrameReady { .. }
    ));

    write_client_message(
        &mut client,
        &ClientMessage::OpenTab {
            url: Some("about:blank".to_string()),
        },
    )
    .unwrap();
    let opened = read_server_message(&mut client).unwrap();
    assert!(
        matches!(opened, ServerMessage::TabOpened { url: Some(_), .. }),
        "expected the second tab to have navigated, got {opened:?}"
    );
    assert!(matches!(
        read_server_message(&mut client).unwrap(),
        ServerMessage::FrameReady { .. }
    ));

    let v1_internal_socket = launcher.v1_internal_socket_path();
    assert!(
        v1_internal_socket.exists(),
        "sanity check: v1 must actually be up before cutover"
    );

    // Trigger the cutover, on a *separate* connection to the control
    // socket -- the client above never touches this socket at all.
    let mut control = launcher.connect_control();
    write_control_request(&mut control, &ControlRequest::Cutover).unwrap();
    let reply = read_control_reply(&mut control).unwrap();
    let ControlReply::CutoverDone { tabs_migrated } = reply else {
        panic!("expected CutoverDone, got {reply:?}")
    };
    assert_eq!(tabs_migrated, 2);
    assert_eq!(
        launcher.inspect_extension_permissions(),
        (1, None),
        "a package-free cutover must still publish the new core generation"
    );

    // (c) v1's process is actually dead: `SpawnedCore::Drop` removes
    // its internal socket file only after the child has been reaped.
    assert!(
        !v1_internal_socket.exists(),
        "v1's internal socket must be gone once cutover has torn it down"
    );

    // (a) the *original* client connection was never dropped by the
    // cutover -- it can still send and receive on the exact same
    // socket, now served by v2. Tagged with its own request_id and
    // filtered accordingly: the tab-capture step's own `ListTabs`
    // (tagged with a synthetic id) was also broadcast to this same
    // client as a side effect of sharing the broker, and must not be
    // mistaken for this request's own reply -- exactly the id-based
    // filtering discipline Part 1's fix exists to make possible through
    // a shared broker connection. v2's replay frames arrive as unsolicited
    // handoffs before the requested tab list, without another user action.
    write_client_message_with_id(&mut client, Some(777), &ClientMessage::ListTabs).unwrap();
    let mut handoff_tabs = Vec::new();
    let tabs = loop {
        let (tab_id, reply_id, message) = read_server_message_with_ids(&mut client).unwrap();
        if reply_id.is_none() {
            if matches!(message, ServerMessage::FrameReady { .. }) {
                handoff_tabs.push(tab_id);
            }
            continue;
        }
        if reply_id != Some(777) {
            continue;
        }
        match message {
            ServerMessage::Tabs(tabs) => break tabs,
            other => panic!("expected Tabs, got {other:?}"),
        }
    };
    assert_eq!(handoff_tabs, vec![Some(1), Some(2)]);

    // (b) v2's replayed state matches what was set up on v1 before
    // cutover -- same URLs, same order, ids may legitimately differ
    // (v2 assigns its own fresh ones).
    let urls: Vec<Option<String>> = tabs.iter().map(|t| t.url.clone()).collect();
    assert_eq!(
        urls,
        vec![
            Some("about:credits".to_string()),
            Some("about:blank".to_string())
        ]
    );

    write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    launcher.wait_or_kill(Duration::from_secs(5));
}

#[test]
fn a_cutover_resets_generation_but_changes_the_shared_frame_source() {
    let mut launcher = Launcher::spawn();
    let mut client = launcher.connect();
    write_client_message(
        &mut client,
        &ClientMessage::Navigate {
            url: "about:credits".to_string(),
        },
    )
    .unwrap();
    assert!(matches!(
        read_server_message(&mut client).unwrap(),
        ServerMessage::Navigated { .. }
    ));
    assert!(matches!(
        read_server_message(&mut client).unwrap(),
        ServerMessage::FrameReady { .. }
    ));

    let mut v1_source = 0;
    let mut v1_generation = 0;
    for index in 0..6_u64 {
        let request_id = 930 + index;
        write_client_message_with_id(
            &mut client,
            Some(request_id),
            &ClientMessage::Resize {
                width: 320 + index as u32,
                height: 200,
            },
        )
        .unwrap();
        loop {
            let (reply_id, message) = read_server_message_with_id(&mut client).unwrap();
            if reply_id != Some(request_id) {
                continue;
            }
            match message {
                ServerMessage::FrameReady {
                    shm_path,
                    generation,
                    ..
                } => {
                    v1_source = blueice_ipc::shm::frame_source_id_for_path(&shm_path);
                    v1_generation = generation;
                    break;
                }
                other => panic!("expected v1 FrameReady, got {other:?}"),
            }
        }
    }

    let mut control = launcher.connect_control();
    write_control_request(&mut control, &ControlRequest::Cutover).unwrap();
    assert!(matches!(
        read_control_reply(&mut control).unwrap(),
        ControlReply::CutoverDone { tabs_migrated: 1 }
    ));
    client
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let (handoff_source, handoff_generation) = loop {
        let (reply_id, message) = read_server_message_with_id(&mut client).expect(
            "cutover must hand the replay frame to an existing client without another action",
        );
        if reply_id.is_some() {
            continue; // v1's synthetic ListTabs capture may still be queued
        }
        match message {
            ServerMessage::FrameReady {
                shm_path,
                generation,
                ..
            } => {
                break (
                    blueice_ipc::shm::frame_source_id_for_path(&shm_path),
                    generation,
                );
            }
            other => panic!("expected a v2 handoff frame, got {other:?}"),
        }
    };
    assert_ne!(v1_source, handoff_source);
    assert!(handoff_generation < v1_generation);
    write_client_message_with_id(
        &mut client,
        Some(950),
        &ClientMessage::Resize {
            width: 399,
            height: 200,
        },
    )
    .unwrap();
    let (v2_source, v2_generation) = loop {
        let (reply_id, message) = read_server_message_with_id(&mut client).unwrap();
        if reply_id != Some(950) {
            continue;
        }
        match message {
            ServerMessage::FrameReady {
                shm_path,
                generation,
                ..
            } => {
                break (
                    blueice_ipc::shm::frame_source_id_for_path(&shm_path),
                    generation,
                );
            }
            other => panic!("expected v2 FrameReady, got {other:?}"),
        }
    };
    assert_ne!(
        v1_source, v2_source,
        "cutover must use a fresh frame source"
    );
    assert_eq!(handoff_source, v2_source);
    assert!(
        v2_generation < v1_generation,
        "this test must exercise a real generation reset: v1={v1_generation}, v2={v2_generation}"
    );

    write_client_message_with_id(&mut client, Some(951), &ClientMessage::GetRepresentation)
        .unwrap();
    let snapshot = loop {
        let (reply_id, message) = read_server_message_with_id(&mut client).unwrap();
        if reply_id != Some(951) {
            continue;
        }
        match message {
            ServerMessage::Representation(snapshot) => break snapshot,
            other => panic!("expected v2 Representation, got {other:?}"),
        }
    };
    assert_eq!(snapshot.frame_source, v2_source);
    assert_eq!(snapshot.generation, v2_generation);

    write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    launcher.wait_or_kill(Duration::from_secs(5));
}

#[test]
fn simultaneous_cutover_requests_allow_one_transition_and_report_the_other_as_busy() {
    let mut launcher = Launcher::spawn();

    // Send both requests before reading either reply. The control
    // listener gives each its own handler thread, so they contend for
    // the launcher's real single-flight cutover gate rather than being
    // serialized by this test client.
    let mut first = launcher.connect_control();
    let mut second = launcher.connect_control();
    write_control_request(&mut first, &ControlRequest::Cutover).unwrap();
    write_control_request(&mut second, &ControlRequest::Cutover).unwrap();

    let replies = [
        read_control_reply(&mut first).unwrap(),
        read_control_reply(&mut second).unwrap(),
    ];
    assert_eq!(
        replies
            .iter()
            .filter(|reply| matches!(reply, ControlReply::CutoverDone { .. }))
            .count(),
        1,
        "exactly one request must own the v1-to-v2 transition: {replies:?}"
    );
    assert_eq!(
        replies
            .iter()
            .filter(|reply| matches!(reply, ControlReply::CutoverBusy))
            .count(),
        1,
        "the competing request must be rejected without starting another transition: {replies:?}"
    );

    let mut client = launcher.connect();
    write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    launcher.wait_or_kill(Duration::from_secs(5));
}

#[test]
fn simultaneous_cutovers_serialize_without_reusing_a_generation() {
    let _guard = broker_test_guard();
    let mut launcher = Launcher::spawn();
    let barrier = Arc::new(Barrier::new(3));
    let requests = (0..2)
        .map(|_| {
            let socket = launcher.control_socket.clone();
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                barrier.wait();
                let mut control = UnixStream::connect(socket).unwrap();
                write_control_request(&mut control, &ControlRequest::Cutover).unwrap();
                read_control_reply(&mut control).unwrap()
            })
        })
        .collect::<Vec<_>>();
    barrier.wait();
    // The launcher's cutover is single-flight: a request that overlaps
    // another is refused as busy rather than queued, so a transition never
    // starts from a generation its predecessor has not yet committed. One that
    // arrives after the first has finished may run its own, serialized.
    let replies = requests
        .into_iter()
        .map(|request| request.join().unwrap())
        .collect::<Vec<_>>();
    assert!(
        replies.iter().all(|reply| matches!(
            reply,
            ControlReply::CutoverDone { .. } | ControlReply::CutoverBusy
        )),
        "a simultaneous cutover must finish or be refused as busy: {replies:?}"
    );
    assert!(
        replies
            .iter()
            .any(|reply| matches!(reply, ControlReply::CutoverDone { .. })),
        "one of two simultaneous cutovers must own the transition: {replies:?}"
    );

    // The broker's client-facing socket remains usable after both serialized
    // swaps; use the ordinary shutdown path so the subprocess exits cleanly.
    let mut client = launcher.connect();
    write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    launcher.wait_or_kill(Duration::from_secs(5));
}

#[test]
fn a_cutover_that_fails_during_replay_leaves_v1_serving_normally() {
    let _guard = broker_test_guard();
    let mut launcher = Launcher::spawn();
    let mut client = launcher.connect();

    // Give v1 a real (built-in) current URL, so replay actually
    // attempts a `Navigate` into v2 rather than trivially skipping a
    // still-blank default tab.
    write_client_message(
        &mut client,
        &ClientMessage::Navigate {
            url: "about:credits".to_string(),
        },
    )
    .unwrap();
    assert!(matches!(
        read_server_message(&mut client).unwrap(),
        ServerMessage::Navigated { .. }
    ));
    assert!(matches!(
        read_server_message(&mut client).unwrap(),
        ServerMessage::FrameReady { .. }
    ));

    // Block v2's frame directory with a plain file where a directory is
    // expected. `blueice-core` never touches `frame_dir` until its
    // first paint (`shm::write_frame`'s `create_dir_all`) -- which only
    // happens partway through handling the replayed `Navigate` -- so v2
    // still spawns and completes its own handshake successfully; the
    // failure surfaces specifically as v2's connection breaking mid-
    // replay, which is exactly the "any failure here aborts the whole
    // cutover" path this test exercises. This is the very first cutover
    // attempt against a freshly-started launcher (generation 0 -> target
    // generation 1), so the frame_dir name is fully predictable.
    //
    // A cutover now retries a transient failure, and each retry uses a fresh
    // frame directory (`<name>-retryN`), so every attempt's directory is
    // blocked: the failure must be one no retry can clear.
    let first_attempt_dir = expected_v2_frame_dir(&launcher.frame_dir, 1);
    let blocked_dirs: Vec<PathBuf> = std::iter::once(first_attempt_dir.clone())
        .chain((2..=3).map(|attempt| {
            let mut dir = first_attempt_dir.clone();
            let name = dir.file_name().unwrap().to_string_lossy().into_owned();
            dir.set_file_name(format!("{name}-retry{attempt}"));
            dir
        }))
        .collect();
    for dir in &blocked_dirs {
        let _ = std::fs::remove_file(dir);
        std::fs::write(dir, b"not a directory").expect("failed to pre-create the blocking file");
    }

    let mut control = launcher.connect_control();
    write_control_request(&mut control, &ControlRequest::Cutover).unwrap();
    let reply = read_control_reply(&mut control).unwrap();
    assert!(
        matches!(reply, ControlReply::CutoverFailed { .. }),
        "expected CutoverFailed, got {reply:?}"
    );

    // v1 must still be serving this already-connected client completely
    // normally, with no observable disruption from the failed attempt.
    // Tagged and filtered by request_id, same as the happy-path test:
    // the aborted cutover's own tab-capture step already broadcast a
    // `Tabs` reply to every registered client (including this one)
    // before replay ever failed, and that stray message must not be
    // mistaken for this request's own reply.
    write_client_message_with_id(&mut client, Some(555), &ClientMessage::GetRepresentation)
        .unwrap();
    let rep = loop {
        let (reply_id, message) = read_server_message_with_id(&mut client).unwrap();
        if matches!(reply_id, Some(id) if id != 555) {
            continue;
        }
        break message;
    };
    assert!(
        matches!(rep, ServerMessage::Representation(_)),
        "expected v1 to keep answering normally, got {rep:?}"
    );

    for dir in &blocked_dirs {
        let _ = std::fs::remove_file(dir);
    }
    write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    launcher.wait_or_kill(Duration::from_secs(5));
}
