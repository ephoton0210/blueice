// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

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
#[cfg(target_os = "macos")]
#[test]
fn bundled_macos_permissions_use_the_fixed_native_child_without_a_path_override() {
    assert_eq!(
        sibling_frontend_binary(Path::new("/Applications/BlueIce.app/Contents/MacOS/blueice-launcher")),
        PathBuf::from("/Applications/BlueIce.app/Contents/MacOS/BlueIcePanels.app/Contents/MacOS/BlueIcePanels")
    );
    for (launcher, expected) in [
        ("/tmp/debug/blueice-launcher", "/tmp/debug/blueice-frontend"),
        (
            "/tmp/debug/deps/test-launcher",
            "/tmp/debug/blueice-frontend",
        ),
        (
            "/tmp/other/Contents/MacOS/blueice-launcher",
            "/tmp/other/Contents/MacOS/blueice-frontend",
        ),
        (
            "/tmp/BlueIce.app/Contents/MacOS/other",
            "/tmp/BlueIce.app/Contents/MacOS/blueice-frontend",
        ),
    ] {
        assert_eq!(
            sibling_frontend_binary(Path::new(launcher)),
            PathBuf::from(expected)
        );
    }
}
