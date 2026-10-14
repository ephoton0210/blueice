// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn a_first_attempt_that_succeeds_is_not_retried() {
    let mut attempts = 0;
    let out = run_with_retries(
        |n| {
            attempts += 1;
            Ok::<_, AttemptFailure>(n)
        },
        no_pause,
    );
    assert_eq!(out, Ok(1));
    assert_eq!(attempts, 1);
}

#[test]
fn a_transient_failure_is_retried_until_it_clears() {
    let out = run_with_retries(
        |n| {
            if n < 3 {
                Err(AttemptFailure::Retry(format!("spawn failed on {n}")))
            } else {
                Ok(n)
            }
        },
        no_pause,
    );
    assert_eq!(out, Ok(3));
}

#[test]
fn retries_are_bounded_and_the_last_reason_is_reported() {
    let mut attempts = 0;
    let out: Result<(), String> = run_with_retries(
        |n| {
            attempts += 1;
            Err(AttemptFailure::Retry(format!("still failing on {n}")))
        },
        no_pause,
    );
    assert_eq!(attempts, MAX_CUTOVER_ATTEMPTS);
    let reason = out.unwrap_err();
    assert!(reason.contains("still failing on 3"), "{reason}");
    assert!(reason.contains("gave up after 3 attempts"), "{reason}");
}

#[test]
fn a_deterministic_failure_is_never_retried() {
    let mut attempts = 0;
    let out: Result<(), String> = run_with_retries(
        |_| {
            attempts += 1;
            Err(AttemptFailure::Final("v2's gatekeeper blocked it".into()))
        },
        no_pause,
    );
    assert_eq!(attempts, 1);
    assert!(out.unwrap_err().contains("not retryable"));
}

#[test]
fn a_failed_health_check_is_retried_once_and_a_second_is_final() {
    let mut attempts = 0;
    let out: Result<(), String> = run_with_retries(
        |_| {
            attempts += 1;
            Err(AttemptFailure::RetryOnce(
                "post-replay ListTabs differed".into(),
            ))
        },
        no_pause,
    );
    assert_eq!(attempts, 2, "one retry, then stop");
    assert!(out.unwrap_err().contains("failed the health check twice"));
    // A health failure followed by a transient one still leaves a budget.
    let mut seen = Vec::new();
    let _: Result<(), String> = run_with_retries(
        |n| {
            seen.push(n);
            if n == 1 {
                Err(AttemptFailure::RetryOnce("health".into()))
            } else {
                Err(AttemptFailure::Retry("spawn".into()))
            }
        },
        no_pause,
    );
    assert_eq!(seen, [1, 2, 3]);
}

#[test]
fn the_pause_grows_between_attempts_and_there_is_none_after_the_last() {
    let mut pauses = Vec::new();
    let _: Result<(), String> = run_with_retries(
        |_| Err(AttemptFailure::Retry("x".into())),
        |d| pauses.push(d),
    );
    assert_eq!(
        pauses,
        [CUTOVER_RETRY_BASE_PAUSE, CUTOVER_RETRY_BASE_PAUSE * 2]
    );
}

#[test]
fn every_replay_and_health_message_this_file_produces_is_classified_as_intended() {
    for (reason, expected) in [
        ("v2 rejected the replayed Navigate: bad url", "final"),
        ("v2 rejected the replayed OpenTab: bad url", "final"),
        ("v2's gatekeeper blocked the replayed Navigate: rule", "final"),
        ("v2's gatekeeper blocked the replayed OpenTab: rule", "final"),
        ("v2's post-replay ListTabs didn't match what was captured from v1", "once"),
        ("v2's post-replay representation of tab 0 is structurally different from v1's: v1 showed 9 node(s), v2 shows 0", "once"),
        ("failed to ask v2 to describe replayed tab 0: broken pipe", "retry"),
        ("failed reading v2's description of replayed tab 0: timed out", "retry"),
        ("v2 could not describe replayed tab 0: no such tab", "retry"),
        ("failed reading v2's reply while replaying: timed out", "retry"),
        ("failed reading v2's health-check reply: eof", "retry"),
        ("failed to send v2's health-check ListTabs: broken pipe", "retry"),
        ("failed to replay the default tab's Navigate into v2: broken pipe", "retry"),
        ("failed to replay tab 1's OpenTab into v2: broken pipe", "retry"),
    ] {
        let got = match classify_attempt_error(reason.to_string()) {
            AttemptFailure::Final(_) => "final",
            AttemptFailure::RetryOnce(_) => "once",
            AttemptFailure::Retry(_) => "retry",
        };
        assert_eq!(got, expected, "{reason}");
    }
}

#[test]
fn a_render_is_comparable_within_half_to_double_and_never_blank_when_v1_was_not() {
    for (v1, v2, ok) in [
        (10, 10, true),
        (10, 5, true),   // exactly half
        (10, 4, false),  // less than half
        (10, 20, true),  // exactly double
        (10, 21, false), // more than double
        (10, 0, false),  // blank where v1 had content
        (1, 1, true),
        (1, 2, true),
        (1, 3, false),
        (0, 0, true), // a blank tab constrains nothing
        (0, 50, true),
    ] {
        assert_eq!(structure_comparable(v1, v2), ok, "v1={v1} v2={v2}");
    }
}

#[test]
fn a_blank_or_far_smaller_render_fails_the_structural_check_with_a_retry_once_reason() {
    for v2_nodes in [0, 2] {
        let (mut stream, v2) = fake_v2(vec![ServerMessage::Representation(snapshot_with_nodes(
            v2_nodes,
        ))]);
        let reason = structural_health_check(&mut stream, &[5], &[Some(10)]).unwrap_err();
        v2.join().unwrap();
        assert!(reason.contains("structurally different"), "{reason}");
        assert!(matches!(
            classify_attempt_error(reason),
            AttemptFailure::RetryOnce(_)
        ));
    }
}

#[test]
fn cutover_gate_allows_only_one_in_flight_cutover_and_releases_on_drop() {
    let gate = CutoverGate::new();
    let first = gate
        .try_acquire()
        .expect("the first cutover must acquire the gate");
    assert!(
        gate.try_acquire().is_none(),
        "a concurrent cutover must be rejected while the first owns the gate"
    );
    drop(first);
    assert!(
        gate.try_acquire().is_some(),
        "a later cutover must be able to proceed after the first returns"
    );
}

#[test]
fn generation_pinned_relay_fails_closed_before_any_generation_is_activated() {
    let path = unique_internal_socket_path();
    let relay = GenerationPinnedUnixRelay::bind(&path, "test", Arc::new(Mutex::new(()))).unwrap();
    let mut client = UnixStream::connect(&path).unwrap();
    let mut buf = [0u8; 1];
    // No target generation has been activated yet: the accept loop must
    // close the connection rather than hang or forward it anywhere.
    assert_eq!(std::io::Read::read(&mut client, &mut buf).unwrap(), 0);
    relay.close();
}

#[test]
fn generation_pinned_relay_close_is_idempotent_and_drop_calls_close() {
    let path = unique_internal_socket_path();
    let relay = GenerationPinnedUnixRelay::bind(&path, "test", Arc::new(Mutex::new(()))).unwrap();
    assert!(path.exists());
    relay.close();
    assert!(!path.exists());
    // A second close() must be a no-op, not a panic or a hang.
    relay.close();
    drop(relay);
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
fn superseded_generation_cannot_publish_a_late_frame() {
    let (mut core_side, mut core_observed) = UnixStream::pair().unwrap();
    let (sender, receiver) = mpsc::channel();
    let clients = Arc::new(Mutex::new(vec![sender]));
    let generation = AtomicU64::new(1);
    write_server_message_with_ids(
        &mut core_observed,
        Some(1),
        None,
        &ServerMessage::FrameReady {
            shm_path: "old-core-frame".into(),
            width: 1,
            height: 1,
            generation: 12,
        },
    )
    .unwrap();
    drop(core_observed);

    broadcast_core_to_clients_for_generation(&mut core_side, clients, Some((&generation, 0)));
    assert!(
        receiver.try_recv().is_err(),
        "a late v1 frame must not follow v2 handoff"
    );
}

#[test]
fn health_check_ignores_a_reply_carrying_a_mismatched_request_id_and_other_traffic() {
    let (mut stream, mut server) = UnixStream::pair().unwrap();
    let expected = vec![TabSummary {
        id: 1,
        url: Some("about:blank".to_string()),
        group_id: None,
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
                group_id: None,
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
        group_id: None,
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
                group_id: None,
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
        group_id: None,
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
