// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;
use blueice_ipc::permission_control::{
    read_permission_control_request, write_permission_control_reply,
};
use blueice_ipc::*;

#[test]
fn private_permission_worker_serializes_inspect_grant_and_revoke() {
    let (parent_input, mut child_input) = UnixStream::pair().unwrap();
    let (mut child_output, parent_output) = UnixStream::pair().unwrap();
    let (requests, pending) = mpsc::sync_channel(1);
    let worker = thread::spawn(move || {
        serve_permission_control_worker(parent_input, parent_output, pending)
    });
    let core = thread::spawn(move || {
        for expected in [
            PermissionControlRequest::Inspect,
            PermissionControlRequest::InspectDocument { tab_id: 7 },
            PermissionControlRequest::Grant {
                capability: "storage".into(),
            },
            PermissionControlRequest::Revoke {
                capability: "storage".into(),
            },
        ] {
            assert_eq!(
                read_permission_control_request(&mut child_input).unwrap(),
                Some(expected.clone())
            );
            let reply = match expected {
                PermissionControlRequest::Inspect => PermissionControlReply::State {
                    extension_id: "sha256:installed".into(),
                    name: "Notes".into(),
                    version: "1".into(),
                    optional: vec![],
                    runtime_ephemeral: vec![],
                },
                PermissionControlRequest::InspectDocument { tab_id } => {
                    PermissionControlReply::Document {
                        tab_id,
                        document_epoch: 3,
                        url: Some("https://example.test/".into()),
                    }
                }
                PermissionControlRequest::ArmEphemeral { .. } => PermissionControlReply::Rejected {
                    reason: "the test worker has no ephemeral declaration".into(),
                },
                PermissionControlRequest::Grant { capability } => PermissionControlReply::Updated {
                    capability,
                    granted: true,
                    changed: true,
                },
                PermissionControlRequest::Revoke { capability } => {
                    PermissionControlReply::Updated {
                        capability,
                        granted: false,
                        changed: true,
                    }
                }
            };
            write_permission_control_reply(&mut child_output, &reply).unwrap();
        }
        assert!(read_permission_control_request(&mut child_input)
            .unwrap()
            .is_none());
    });
    let channel = PermissionControlChannel { requests };
    assert!(matches!(
        channel.inspect().unwrap(),
        PermissionControlReply::State { .. }
    ));
    assert!(matches!(
        channel
            .exchange(
                PermissionControlRequest::InspectDocument { tab_id: 7 },
                PERMISSION_INSPECT_TIMEOUT
            )
            .unwrap(),
        PermissionControlReply::Document {
            tab_id: 7,
            document_epoch: 3,
            ..
        }
    ));
    assert!(matches!(
        channel
            .exchange(
                PermissionControlRequest::Grant {
                    capability: "storage".into()
                },
                PERMISSION_CHANGE_TIMEOUT
            )
            .unwrap(),
        PermissionControlReply::Updated { granted: true, .. }
    ));
    assert!(matches!(
        channel
            .exchange(
                PermissionControlRequest::Revoke {
                    capability: "storage".into()
                },
                PERMISSION_CHANGE_TIMEOUT
            )
            .unwrap(),
        PermissionControlReply::Updated { granted: false, .. }
    ));
    drop(channel);
    worker.join().unwrap();
    core.join().unwrap();
}

#[test]
fn native_pipe_scopes_optional_grants_and_one_shot_reads_outside_cutover() {
    use std::sync::atomic::AtomicBool;
    let root = std::env::temp_dir().join(format!(
        "blueice-native-permission-unit-{}-{}",
        std::process::id(),
        synthetic_request_id(),
    ));
    std::fs::create_dir(&root).unwrap();
    let (parent_input, mut child_input) = UnixStream::pair().unwrap();
    let (mut child_output, parent_output) = UnixStream::pair().unwrap();
    let (requests, pending) = mpsc::sync_channel(1);
    let worker = thread::spawn(move || {
        serve_permission_control_worker(parent_input, parent_output, pending)
    });
    let granted = Arc::new(AtomicBool::new(false));
    let grant_count = Arc::new(AtomicU64::new(0));
    let document_epoch = Arc::new(AtomicU64::new(12));
    let arm_count = Arc::new(AtomicU64::new(0));
    let simulated_core = thread::spawn({
        let granted = Arc::clone(&granted);
        let grant_count = Arc::clone(&grant_count);
        let document_epoch = Arc::clone(&document_epoch);
        let arm_count = Arc::clone(&arm_count);
        move || {
            while let Some(request) = read_permission_control_request(&mut child_input).unwrap() {
                let reply = match request {
                    PermissionControlRequest::Inspect => PermissionControlReply::State {
                        extension_id: "sha256:installed".into(),
                        name: "Notes".into(),
                        version: "1".into(),
                        optional: vec![blueice_ipc::permission_control::OptionalCapabilityInfo {
                            capability: "storage".into(),
                            granted: granted.load(Ordering::SeqCst),
                            origins: vec!["https://example.test".into()],
                        }],
                        runtime_ephemeral: vec![
                            blueice_ipc::permission_control::EphemeralCapabilityInfo {
                                capability: "dom:read".into(),
                                origins: vec!["https://example.test".into()],
                            },
                        ],
                    },
                    PermissionControlRequest::InspectDocument { tab_id } => {
                        PermissionControlReply::Document {
                            tab_id,
                            document_epoch: document_epoch.load(Ordering::SeqCst),
                            url: Some("https://example.test/page".into()),
                        }
                    }
                    PermissionControlRequest::ArmEphemeral {
                        capability,
                        tab_id,
                        document_epoch: expected,
                    } => {
                        if expected != document_epoch.load(Ordering::SeqCst) {
                            PermissionControlReply::Rejected {
                                reason: "stale document".into(),
                            }
                        } else {
                            arm_count.fetch_add(1, Ordering::SeqCst);
                            PermissionControlReply::EphemeralArmed {
                                capability,
                                tab_id,
                                document_epoch: expected,
                                ticket: "a".repeat(64),
                            }
                        }
                    }
                    PermissionControlRequest::Grant { capability } => {
                        grant_count.fetch_add(1, Ordering::SeqCst);
                        let changed = !granted.swap(true, Ordering::SeqCst);
                        PermissionControlReply::Updated {
                            capability,
                            granted: true,
                            changed,
                        }
                    }
                    PermissionControlRequest::Revoke { capability } => {
                        let changed = granted.swap(false, Ordering::SeqCst);
                        PermissionControlReply::Updated {
                            capability,
                            granted: false,
                            changed,
                        }
                    }
                };
                write_permission_control_reply(&mut child_output, &reply).unwrap();
            }
        }
    });
    let (core_stream, _core_peer) = UnixStream::pair().unwrap();
    let fake_child = || Command::new("true").spawn().unwrap();
    let core = SpawnedCore {
        child: fake_child(),
        internal_socket_path: root.join("core.sock"),
        extension_socket_path: None,
        script_private_socket_path: None,
        compiler_private_socket_path: None,
        debugger_private_socket_path: None,
        options: CoreLaunchOptions::default(),
        bluejs_host: None,
        route_gate: Arc::new(Mutex::new(())),
        compiler_mcp_relay: None,
        debugger_relay: None,
        permission_control: Some(PermissionControlChannel { requests }),
        frame_dir: root.join("frames"),
        stream: core_stream,
    };
    let (done, done_rx) = mpsc::channel();
    let broker = Arc::new(Broker {
        core_writer: Arc::new(Mutex::new(core.stream.try_clone().unwrap())),
        clients: Arc::new(Mutex::new(Vec::new())),
        generation: Arc::new(AtomicU64::new(3)),
        cutover_gate: CutoverGate::new(),
        active_core: Mutex::new(Some(core)),
        width: 320.0,
        height: 200.0,
        frame_dir: root.join("frames"),
        gatekeeper_socket: root.join("gate.sock"),
        extension_manifest: None,
        assistant: None,
        assistant_settings: None,
        core_options: CoreLaunchOptions::default(),
        route_gate: Arc::new(Mutex::new(())),
        compiler_mcp_relay: None,
        debugger_relay: None,
        done,
    });
    let change = |generation, id: &str, capability: &str, action| {
        trusted_window::TrustedWindowRequest::Change {
            expected_core_generation: generation,
            expected_extension_id: id.into(),
            capability: capability.into(),
            action,
        }
    };
    let initial =
        handle_trusted_window_request(trusted_window::TrustedWindowRequest::Inspect, &broker);
    assert!(matches!(
        initial,
        trusted_window::TrustedWindowReply::State {
            core_generation: 3,
            installed: Some(_),
        }
    ));
    for invalid in [
        change(
            2,
            "sha256:installed",
            "storage",
            trusted_window::PermissionAction::Grant,
        ),
        change(
            3,
            "sha256:wrong",
            "storage",
            trusted_window::PermissionAction::Grant,
        ),
        change(
            3,
            "sha256:installed",
            "dom:read",
            trusted_window::PermissionAction::Grant,
        ),
    ] {
        assert!(matches!(
            handle_trusted_window_request(invalid, &broker),
            trusted_window::TrustedWindowReply::Rejected { .. }
        ));
    }
    let cutover = broker.cutover_gate.try_acquire().unwrap();
    assert!(matches!(
        handle_trusted_window_request(
            change(
                3,
                "sha256:installed",
                "storage",
                trusted_window::PermissionAction::Grant
            ),
            &broker,
        ),
        trusted_window::TrustedWindowReply::Rejected { .. }
    ));
    drop(cutover);
    assert_eq!(grant_count.load(Ordering::SeqCst), 0);
    let granted_reply = handle_trusted_window_request(
        change(
            3,
            "sha256:installed",
            "storage",
            trusted_window::PermissionAction::Grant,
        ),
        &broker,
    );
    assert!(
        matches!(granted_reply, trusted_window::TrustedWindowReply::State {
        core_generation: 3, installed: Some(ref installed),
    } if installed.optional[0].granted)
    );
    assert_eq!(grant_count.load(Ordering::SeqCst), 1);
    let revoked_reply = handle_trusted_window_request(
        change(
            3,
            "sha256:installed",
            "storage",
            trusted_window::PermissionAction::Revoke,
        ),
        &broker,
    );
    assert!(
        matches!(revoked_reply, trusted_window::TrustedWindowReply::State {
        core_generation: 3, installed: Some(ref installed),
    } if !installed.optional[0].granted)
    );
    let review =
        |generation, id: &str, tab_id| trusted_window::TrustedWindowRequest::InspectEphemeral {
            expected_core_generation: generation,
            expected_extension_id: id.into(),
            capability: "dom:read".into(),
            tab_id,
        };
    for invalid in [
        review(2, "sha256:installed", 7),
        review(3, "sha256:other", 7),
        review(3, "sha256:installed", 0),
    ] {
        assert!(matches!(
            handle_trusted_window_request(invalid, &broker),
            trusted_window::TrustedWindowReply::Rejected { .. }
        ));
    }
    let reviewed = handle_trusted_window_request(review(3, "sha256:installed", 7), &broker);
    assert!(
        matches!(reviewed, trusted_window::TrustedWindowReply::EphemeralReview {
        core_generation: 3, tab_id: 7, document_epoch: 12,
        ref url, ref installed, ..
    } if url == "https://example.test/page"
        && installed.runtime_ephemeral[0].origins == ["https://example.test"])
    );
    let arm = |epoch| trusted_window::TrustedWindowRequest::ArmEphemeral {
        expected_core_generation: 3,
        expected_extension_id: "sha256:installed".into(),
        capability: "dom:read".into(),
        tab_id: 7,
        document_epoch: epoch,
    };
    document_epoch.store(13, Ordering::SeqCst);
    assert!(matches!(
        handle_trusted_window_request(arm(12), &broker),
        trusted_window::TrustedWindowReply::Rejected { .. }
    ));
    assert_eq!(
        arm_count.load(Ordering::SeqCst),
        0,
        "navigation after review must prevent arming"
    );
    let cutover = broker.cutover_gate.try_acquire().unwrap();
    assert!(matches!(
        handle_trusted_window_request(arm(13), &broker),
        trusted_window::TrustedWindowReply::Rejected { .. }
    ));
    drop(cutover);
    assert_eq!(arm_count.load(Ordering::SeqCst), 0);
    assert!(matches!(
        handle_trusted_window_request(review(3, "sha256:installed", 7), &broker),
        trusted_window::TrustedWindowReply::EphemeralReview {
            document_epoch: 13,
            ..
        }
    ));
    assert!(matches!(
        handle_trusted_window_request(arm(13), &broker),
        trusted_window::TrustedWindowReply::EphemeralArmed {
            core_generation: 3,
            tab_id: 7,
            document_epoch: 13,
            ..
        }
    ));
    assert_eq!(arm_count.load(Ordering::SeqCst), 1);
    let mut inbound = Vec::new();
    for request in [
        trusted_window::TrustedWindowRequest::Inspect,
        arm(13), // no review on this trusted pipe
        review(3, "sha256:installed", 7),
        arm(12), // mismatched review consumes it
        arm(13), // the matching arm can no longer reuse it
        review(3, "sha256:installed", 7),
        arm(13), // exactly one authorized arm
        arm(13), // duplicate confirmation
        review(3, "sha256:installed", 7),
        trusted_window::TrustedWindowRequest::Inspect, // cancels review
        arm(13),
    ] {
        trusted_window::write_request(&mut inbound, &request).unwrap();
    }
    let mut outbound = Vec::new();
    let (ready, ready_rx) = mpsc::channel();
    serve_trusted_window_pipe(inbound.as_slice(), &mut outbound, &broker, ready).unwrap();
    ready_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    let mut replies = outbound.as_slice();
    for expected in [
        "state", "rejected", "review", "rejected", "rejected", "review", "armed", "rejected",
        "review", "state", "rejected",
    ] {
        let reply = trusted_window::read_reply(&mut replies).unwrap().unwrap();
        assert!(
            match expected {
                "state" => matches!(&reply, trusted_window::TrustedWindowReply::State { .. }),
                "review" => matches!(
                    &reply,
                    trusted_window::TrustedWindowReply::EphemeralReview { .. }
                ),
                "armed" => matches!(
                    &reply,
                    trusted_window::TrustedWindowReply::EphemeralArmed { .. }
                ),
                _ => matches!(&reply, trusted_window::TrustedWindowReply::Rejected { .. }),
            },
            "expected {expected} from the trusted-window session, got {reply:?}"
        );
    }
    assert!(replies.is_empty());
    assert_eq!(
        arm_count.load(Ordering::SeqCst),
        2,
        "only a fresh matching review can reach the core's one-shot arm"
    );
    done_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    let (ready, _ready_rx) = mpsc::channel();
    assert!(serve_trusted_window_pipe([1_u8].as_slice(), &mut Vec::new(), &broker, ready).is_err());
    done_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    broker.active_core.lock().unwrap().take();
    worker.join().unwrap();
    simulated_core.join().unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
#[ignore = "requires compiled sibling core, BlueJS, and extension-host binaries"]
fn real_installed_core_confirms_private_grant_and_revoke() {
    let root = std::env::temp_dir().join(format!(
        "blueice-real-optional-permission-{}-{}",
        std::process::id(),
        synthetic_request_id(),
    ));
    std::fs::create_dir(&root).unwrap();
    let manifest = root.join("extension.json");
    std::fs::write(&manifest,
        r#"{"name":"Private permission proof","version":"1","blueice_api_version":1,"entry_point":"extension.wasm","capabilities":{"declared":["storage"],"optional":["dom:read"]},"capability_origins":{"dom:read":["https://example.test"]}}"#,
    ).unwrap();
    std::fs::write(
        root.join("extension.wasm"),
        wat::parse_str(r#"(module (func (export "blueice_start")))"#).unwrap(),
    )
    .unwrap();
    let core = SpawnedCore::spawn_with_gatekeeper_and_extension(
        320.0,
        200.0,
        &root.join("frames"),
        &root.join("unused-gatekeeper.sock"),
        Some(&manifest),
    )
    .unwrap();
    let initial = core.inspect_installed_extension().unwrap().unwrap();
    assert_eq!(initial.optional.len(), 1);
    assert_eq!(initial.optional[0].capability, "dom:read");
    assert_eq!(initial.optional[0].origins, ["https://example.test"]);
    assert!(!initial.optional[0].granted);
    let (done, done_rx) = mpsc::channel();
    let broker = Arc::new(Broker {
        core_writer: Arc::new(Mutex::new(core.stream.try_clone().unwrap())),
        clients: Arc::new(Mutex::new(Vec::new())),
        generation: Arc::new(AtomicU64::new(0)),
        cutover_gate: CutoverGate::new(),
        active_core: Mutex::new(Some(core)),
        width: 320.0,
        height: 200.0,
        frame_dir: root.join("frames"),
        gatekeeper_socket: root.join("unused-gatekeeper.sock"),
        extension_manifest: Some(manifest),
        assistant: None,
        assistant_settings: None,
        core_options: CoreLaunchOptions::default(),
        route_gate: Arc::new(Mutex::new(())),
        compiler_mcp_relay: None,
        debugger_relay: None,
        done,
    });
    let mut requests = Vec::new();
    for request in [
        trusted_window::TrustedWindowRequest::Inspect,
        trusted_window::TrustedWindowRequest::Change {
            expected_core_generation: 0,
            expected_extension_id: initial.extension_id.clone(),
            capability: "dom:read".into(),
            action: trusted_window::PermissionAction::Grant,
        },
        trusted_window::TrustedWindowRequest::Change {
            expected_core_generation: 0,
            expected_extension_id: initial.extension_id.clone(),
            capability: "dom:read".into(),
            action: trusted_window::PermissionAction::Revoke,
        },
    ] {
        trusted_window::write_request(&mut requests, &request).unwrap();
    }
    let mut replies = Vec::new();
    let (ready, ready_rx) = mpsc::channel();
    serve_trusted_window_pipe(requests.as_slice(), &mut replies, &broker, ready).unwrap();
    ready_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    let mut replies = replies.as_slice();
    for expected_granted in [false, true, false] {
        let reply = trusted_window::read_reply(&mut replies).unwrap().unwrap();
        assert!(matches!(reply, trusted_window::TrustedWindowReply::State {
            core_generation: 0, installed: Some(ref package),
        } if package.extension_id == initial.extension_id
            && package.optional[0].granted == expected_granted));
    }
    done_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    broker.active_core.lock().unwrap().take();
    drop(broker);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn malformed_mutation_reply_terminates_the_possibly_granted_core() {
    let root = std::env::temp_dir().join(format!(
        "blueice-uncertain-grant-unit-{}-{}",
        std::process::id(),
        synthetic_request_id(),
    ));
    std::fs::create_dir(&root).unwrap();
    let (parent_input, mut child_input) = UnixStream::pair().unwrap();
    let (mut child_output, parent_output) = UnixStream::pair().unwrap();
    let (requests, pending) = mpsc::sync_channel(1);
    let worker = thread::spawn(move || {
        serve_permission_control_worker(parent_input, parent_output, pending)
    });
    let fake_core = thread::spawn(move || {
        assert_eq!(
            read_permission_control_request(&mut child_input).unwrap(),
            Some(PermissionControlRequest::Grant {
                capability: "storage".into()
            })
        );
        child_output.write_all(&[1, 0, 0, 0, b'{']).unwrap();
    });
    let (stream, _peer) = UnixStream::pair().unwrap();
    let mut core = SpawnedCore {
        child: Command::new("sleep").arg("30").spawn().unwrap(),
        internal_socket_path: root.join("core.sock"),
        extension_socket_path: None,
        script_private_socket_path: None,
        compiler_private_socket_path: None,
        debugger_private_socket_path: None,
        options: CoreLaunchOptions::default(),
        bluejs_host: None,
        route_gate: Arc::new(Mutex::new(())),
        compiler_mcp_relay: None,
        debugger_relay: None,
        permission_control: Some(PermissionControlChannel { requests }),
        frame_dir: root.join("frames"),
        stream,
    };
    assert!(core
        .apply_optional_change(trusted_window::PermissionAction::Grant, "storage",)
        .is_err());
    let deadline = Instant::now() + Duration::from_secs(1);
    while core.child.try_wait().unwrap().is_none() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }
    assert!(
        core.child.try_wait().unwrap().is_some(),
        "an uncertain Grant reply cannot leave its core process alive"
    );
    drop(core);
    worker.join().unwrap();
    fake_core.join().unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

// ---- bounded retry (`phase-8-live-core-hotswap/PLAN.md`) ----

fn no_pause(_: Duration) {}

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

// ---- structural per-tab health diff ----

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

/// A snapshot with `n` nodes, for counting.
fn snapshot_with_nodes(n: usize) -> blueice_ipc::AiSnapshot {
    use blueice_ipc::{AiNode, Bounds, NodeState, Role};
    blueice_ipc::AiSnapshot {
        frame_source: 0,
        generation: 1,
        tab_id: 1,
        url: None,
        scroll_y: 0.0,
        nodes: (0..n as u64)
            .map(|id| AiNode {
                id,
                parent: None,
                children: vec![],
                role: Role::Paragraph,
                name: None,
                name_from: None,
                original_name: None,
                state: NodeState::default(),
                bounds: Bounds {
                    x: 0.0,
                    y: 0.0,
                    width: 1.0,
                    height: 1.0,
                },
                opacity: 1.0,
                occluded: false,
                occluded_by: None,
                occluded_fraction: 0.0,
            })
            .collect(),
    }
}

fn tab(id: u64) -> TabSummary {
    TabSummary {
        id,
        url: Some(format!("about:tab{id}")),
        group_id: None,
    }
}

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

/// A v2 stand-in answering each `GetRepresentation` with the next canned
/// reply, recording which tab each was addressed to.
fn fake_v2(replies: Vec<ServerMessage>) -> (UnixStream, thread::JoinHandle<Vec<Option<u64>>>) {
    let (client, mut server) = UnixStream::pair().unwrap();
    let handle = thread::spawn(move || {
        let mut asked = Vec::new();
        for reply in replies {
            let (tab_id, request_id, message) = read_client_message_with_ids(&mut server).unwrap();
            assert!(matches!(message, ClientMessage::GetRepresentation));
            asked.push(tab_id);
            write_server_message_with_id(&mut server, request_id, &reply).unwrap();
        }
        asked
    });
    (client, handle)
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

// ---- assistant settings over the trusted-window pipe ----

/// A broker around a fake core, with or without an assistant settings owner.
/// Assistant requests never touch the core, so a stand-in is enough.
fn broker_with_settings(
    service: Option<Arc<assistant_settings_service::AssistantSettingsService>>,
) -> Arc<Broker> {
    let root = std::env::temp_dir().join(format!(
        "trusted-assistant-{}-{}",
        std::process::id(),
        synthetic_request_id()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let (core_stream, _peer) = UnixStream::pair().unwrap();
    let fake_child = || Command::new("true").spawn().unwrap();
    let core = SpawnedCore {
        child: fake_child(),
        internal_socket_path: root.join("core.sock"),
        extension_socket_path: None,
        script_private_socket_path: None,
        compiler_private_socket_path: None,
        debugger_private_socket_path: None,
        options: CoreLaunchOptions::default(),
        bluejs_host: None,
        route_gate: Arc::new(Mutex::new(())),
        compiler_mcp_relay: None,
        debugger_relay: None,
        permission_control: None,
        frame_dir: root.join("frames"),
        stream: core_stream,
    };
    let (done, _done_rx) = mpsc::channel();
    Arc::new(Broker {
        core_writer: Arc::new(Mutex::new(core.stream.try_clone().unwrap())),
        clients: Arc::new(Mutex::new(Vec::new())),
        generation: Arc::new(AtomicU64::new(1)),
        cutover_gate: CutoverGate::new(),
        active_core: Mutex::new(Some(core)),
        width: 320.0,
        height: 200.0,
        frame_dir: root.join("frames"),
        gatekeeper_socket: root.join("gate.sock"),
        extension_manifest: None,
        assistant: None,
        assistant_settings: service,
        core_options: CoreLaunchOptions::default(),
        route_gate: Arc::new(Mutex::new(())),
        compiler_mcp_relay: None,
        debugger_relay: None,
        done,
    })
}

fn assistant_service() -> Arc<assistant_settings_service::AssistantSettingsService> {
    let dir = std::env::temp_dir().join(format!(
        "trusted-assistant-svc-{}-{}",
        std::process::id(),
        synthetic_request_id()
    ));
    Arc::new(
        assistant_settings_service::AssistantSettingsService::with_environment(
            dir.join("assistant-settings.json"),
            blueice_assistant_settings::AssistantSettings::default(),
            std::sync::Weak::new(),
            dir.join("models"),
            32 * 1024,
        ),
    )
}

#[test]
fn without_a_supervised_assistant_every_assistant_request_is_rejected() {
    let broker = broker_with_settings(None);
    for request in [
        trusted_window::TrustedWindowRequest::InspectAssistantSettings,
        trusted_window::TrustedWindowRequest::ApproveAssistantProposal {
            id: 1,
            digest: "x".into(),
        },
        trusted_window::TrustedWindowRequest::DenyAssistantProposal { id: 1 },
        trusted_window::TrustedWindowRequest::EditAssistantSettings {
            settings: blueice_assistant_settings::AssistantSettings::default(),
        },
    ] {
        let reply = handle_trusted_window_session_request(request, &broker, &mut None);
        assert!(
            matches!(&reply, trusted_window::TrustedWindowReply::Rejected { reason } if reason.contains("no assistant")),
            "{reply:?}"
        );
    }
}

#[test]
fn assistant_requests_are_answered_without_inspecting_the_core_and_cancel_an_ephemeral_review() {
    let broker = broker_with_settings(Some(assistant_service()));
    let mut review = Some(ReviewedEphemeral {
        core_generation: 1,
        extension_id: "sha256:x".into(),
        capability: "dom:read".into(),
        tab_id: 1,
        document_epoch: 1,
    });
    // The fake core has no permission pipe at all, so a reply proves the
    // core was never asked.
    let reply = handle_trusted_window_session_request(
        trusted_window::TrustedWindowRequest::InspectAssistantSettings,
        &broker,
        &mut review,
    );
    assert!(
        matches!(
            reply,
            trusted_window::TrustedWindowReply::AssistantSettingsState { pending: None, .. }
        ),
        "{reply:?}"
    );
    assert!(
        review.is_none(),
        "any intervening request cancels an ephemeral review"
    );
}

#[test]
fn a_direct_edit_over_the_pipe_takes_effect_and_the_next_inspect_shows_it() {
    let broker = broker_with_settings(Some(assistant_service()));
    let edited = blueice_assistant_settings::AssistantSettings {
        nice: 15,
        ..Default::default()
    };
    let reply = handle_trusted_window_session_request(
        trusted_window::TrustedWindowRequest::EditAssistantSettings {
            settings: edited.clone(),
        },
        &broker,
        &mut None,
    );
    assert!(
        matches!(&reply, trusted_window::TrustedWindowReply::AssistantSettingsState { current, .. } if **current == edited)
    );
    let again = handle_trusted_window_session_request(
        trusted_window::TrustedWindowRequest::InspectAssistantSettings,
        &broker,
        &mut None,
    );
    assert!(
        matches!(&again, trusted_window::TrustedWindowReply::AssistantSettingsState { current, .. } if **current == edited)
    );
}

#[test]
fn the_new_trusted_window_messages_round_trip_over_the_pipe_format() {
    let settings = blueice_assistant_settings::AssistantSettings::default();
    for request in [
        trusted_window::TrustedWindowRequest::InspectAssistantSettings,
        trusted_window::TrustedWindowRequest::ApproveAssistantProposal {
            id: 3,
            digest: "d".into(),
        },
        trusted_window::TrustedWindowRequest::DenyAssistantProposal { id: 3 },
        trusted_window::TrustedWindowRequest::EditAssistantSettings {
            settings: settings.clone(),
        },
    ] {
        let json = serde_json::to_string(&request).unwrap();
        assert_eq!(
            serde_json::from_str::<trusted_window::TrustedWindowRequest>(&json).unwrap(),
            request
        );
    }
    let reply = trusted_window::TrustedWindowReply::AssistantSettingsState {
        current: Box::new(settings.clone()),
        pending: Some(Box::new(trusted_window::PendingAssistantProposal {
            id: 1,
            digest: "d".into(),
            diff: vec!["a".into()],
            proposed: settings,
            seconds_left: 9,
        })),
    };
    let json = serde_json::to_string(&reply).unwrap();
    assert_eq!(
        serde_json::from_str::<trusted_window::TrustedWindowReply>(&json).unwrap(),
        reply
    );
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
fn bounded_values_policy_is_independent_of_static_metadata() {
    let options = CoreLaunchOptions::default()
        .with_debugger_endpoint(PathBuf::from("/tmp/debugger.sock"))
        .with_debugger_bounded_values();
    assert!(options.debugger_bounded_values);
    assert!(!options.debugger_static_metadata_inventory);
}

#[test]
fn supervise_out_of_process_bluejs_builders_set_their_own_fields() {
    let limits = BlueJsHostRuntimeLimits::default();
    let options =
        CoreLaunchOptions::default().supervise_out_of_process_bluejs_with_runtime_limits(limits);
    assert!(options.supervise_out_of_process_bluejs);
    assert_eq!(options.bluejs_host_runtime_limits, limits);

    let options =
        CoreLaunchOptions::default().supervise_out_of_process_bluejs_with_core_http_fixture();
    assert!(options.supervise_out_of_process_bluejs);
    assert!(options.core_http_page_script_fixture);
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
fn spawn_rejects_a_fixed_and_owner_selected_http_policy_before_spawning_anything() {
    use blueice_ipc::owner_bootstrap::{OwnerHttpOriginRule, OwnerHttpResource};
    let policy = OwnerHttpPolicyBootstrap {
        origin_rule: OwnerHttpOriginRule::SameDocumentOrigin,
        resources: vec![OwnerHttpResource {
            canonical_url: "https://example.test/app.js".to_string(),
            integrity: format!("sha256:{}", "a".repeat(64)),
        }],
    };
    let options = CoreLaunchOptions::default()
        .supervise_out_of_process_bluejs_with_owner_http_policy(policy)
        .unwrap()
        .supervise_out_of_process_bluejs_with_core_http_fixture();
    let Err(error) = SpawnedCore::spawn_with_options(320.0, 200.0, &std::env::temp_dir(), options)
    else {
        panic!("a mutually exclusive HTTP page policy combination must be rejected");
    };
    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    assert_eq!(
        error.to_string(),
        "fixed and owner-selected HTTP page policies are mutually exclusive"
    );
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
    assert!(
        start.elapsed() < Duration::from_secs(1),
        "fanning out must be a cheap non-blocking queue push regardless of any client's own writer-thread state"
    );

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
