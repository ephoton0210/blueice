// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn real_subprocess_pauses_and_resumes_a_non_entry_root_safe_point_without_debugger_leaks() {
    // This deliberately uses the public binary, its Unix debugger socket, and
    // the real core session scheduler. It is not an in-process approximation
    // of the v5 continuation seam. The secret is both source text and a
    // potential thrown completion value, so every debugger reply below proves
    // it cannot cross the debugger transport.
    const PAGE_SECRET: &str = "BLUEICE_DEBUGGER_SECRET_DO_NOT_DISCLOSE";
    let socket_path = unique_socket_path("debugger-v5-core");
    let debugger_socket_path = unique_socket_path("debugger-v5-host");
    let frame_dir = std::env::temp_dir().join(format!(
        "blueice-core-binary-test-debugger-v5-frames-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&socket_path);
    let _ = std::fs::remove_file(&debugger_socket_path);
    let _ = std::fs::remove_dir_all(&frame_dir);
    let gatekeeper_path = clearing_gatekeeper("dbg-v5-gk");

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let classic_body = format!(
        concat!(
            "<main>v5 classic debugger fixture</main>",
            "<script>function debuggerChildCodeUnit() {{ return \"{PAGE_SECRET}\"; }} ",
            "globalThis.debuggerSecretBefore = \"{PAGE_SECRET}\"; ",
            "throw \"{PAGE_SECRET}\";</script>"
        ),
        PAGE_SECRET = PAGE_SECRET
    );
    let module_body = format!(
        concat!(
            "<main>v5 module debugger fixture</main>",
            "<script type=\"module\">export const debuggerModuleSecret = ",
            "\"{PAGE_SECRET}\";</script>"
        ),
        PAGE_SECRET = PAGE_SECRET
    );
    thread::spawn(move || {
        for body in [classic_body, module_body] {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0u8; 1024];
            let _ = stream.read(&mut request);
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                )
                .unwrap();
        }
    });

    let mut child = Command::new(env!("CARGO_BIN_EXE_blueice-core"))
        .args([
            "--socket",
            socket_path.to_str().unwrap(),
            "--debugger-socket",
            debugger_socket_path.to_str().unwrap(),
            "--frame-dir",
            frame_dir.to_str().unwrap(),
            "--gatekeeper-socket",
            gatekeeper_path.to_str().unwrap(),
            "--inline-bluejs",
        ])
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn blueice-core with debugger v5 enabled");
    assert!(wait_for(&socket_path, Duration::from_secs(5)));
    assert!(wait_for(&debugger_socket_path, Duration::from_secs(5)));

    let mut frontend = connect_with_retry(&socket_path, Duration::from_secs(5)).unwrap();
    blueice_ipc::client_handshake(&mut frontend).unwrap();
    blueice_ipc::write_client_message(
        &mut frontend,
        &blueice_ipc::ClientMessage::Navigate {
            url: format!("http://{address}"),
        },
    )
    .unwrap();
    assert!(matches!(
        blueice_ipc::read_server_message(&mut frontend).unwrap(),
        blueice_ipc::ServerMessage::Navigated { .. }
    ));
    assert!(matches!(
        blueice_ipc::read_server_message(&mut frontend).unwrap(),
        blueice_ipc::ServerMessage::FrameReady { generation: 1, .. }
    ));

    let mut debugger = connect_with_retry(&debugger_socket_path, Duration::from_secs(5)).unwrap();
    assert_eq!(
        debugger_request(
            &mut debugger,
            &blueice_ipc::debugger::DebuggerRequest::Hello {
                protocol_version: blueice_ipc::debugger::DEBUGGER_PROTOCOL_VERSION,
                requested_bounded_values: false,
                requested_metadata_capabilities:
                    blueice_ipc::debugger::DebuggerMetadataCapabilityManifest::empty(),
            },
            PAGE_SECRET,
        ),
        blueice_ipc::debugger::DebuggerReply::HelloAck {
            protocol_version: blueice_ipc::debugger::DEBUGGER_PROTOCOL_VERSION,
            granted_bounded_values: false,
            granted_metadata_capabilities:
                blueice_ipc::debugger::DebuggerMetadataCapabilityManifest::empty(),
        }
    );
    let realm = match debugger_request(
        &mut debugger,
        &blueice_ipc::debugger::DebuggerRequest::ListPageRealms,
        PAGE_SECRET,
    ) {
        blueice_ipc::debugger::DebuggerReply::PageRealms(realms) => {
            assert_eq!(realms.len(), 1);
            realms[0]
        }
        other => panic!("expected one v5 debugger realm, got {other:?}"),
    };
    let program = match debugger_request(
        &mut debugger,
        &blueice_ipc::debugger::DebuggerRequest::ListPrograms { realm },
        PAGE_SECRET,
    ) {
        blueice_ipc::debugger::DebuggerReply::Programs(programs) => {
            assert_eq!(programs.len(), 1);
            programs[0]
        }
        other => panic!("expected one opaque classic debugger program, got {other:?}"),
    };
    let safe_points = match debugger_request(
        &mut debugger,
        &blueice_ipc::debugger::DebuggerRequest::ListSafePoints { program },
        PAGE_SECRET,
    ) {
        blueice_ipc::debugger::DebuggerReply::SafePoints(safe_points) => safe_points,
        other => panic!("expected compiler-verified debugger safe points, got {other:?}"),
    };
    let root_safe_point = *safe_points
        .iter()
        .find(|safe_point| safe_point.code_unit_ordinal == 0 && safe_point.bytecode_offset != 0)
        .expect("classic fixture must expose a non-entry root boundary");
    let child_safe_point = *safe_points
        .iter()
        .find(|safe_point| safe_point.code_unit_ordinal != 0)
        .expect("fixture function must expose a child code-unit boundary");

    // Discovery is source-free but the program remains genuinely pending.
    // This begins the public v5 transition acceptance: no request below may
    // make the script run unless it successfully changes that exact state.
    assert_eq!(
        debugger_request(
            &mut debugger,
            &blueice_ipc::debugger::DebuggerRequest::GetExecutionState { program },
            PAGE_SECRET,
        ),
        blueice_ipc::debugger::DebuggerReply::ExecutionState {
            program,
            state: blueice_ipc::debugger::DebuggerExecutionState::Pending,
        }
    );

    // A resume for a different opaque program cannot consume this pending
    // declaration's scheduling turn. It is a real socket-level cross-program
    // failure rather than an in-process approximation.
    let cross_program = blueice_ipc::debugger::DebuggerProgram {
        program_handle: program
            .program_handle
            .checked_add(1)
            .expect("fixture program handle leaves a distinct invalid handle"),
        ..program
    };
    assert!(matches!(
        debugger_request(
            &mut debugger,
            &blueice_ipc::debugger::DebuggerRequest::ResumeExecution {
                program: cross_program,
            },
            PAGE_SECRET,
        ),
        blueice_ipc::debugger::DebuggerReply::Error {
            code: blueice_ipc::debugger::DebuggerErrorCode::InvalidTarget,
            ..
        }
    ));
    assert_eq!(
        debugger_request(
            &mut debugger,
            &blueice_ipc::debugger::DebuggerRequest::GetExecutionState { program },
            PAGE_SECRET,
        ),
        blueice_ipc::debugger::DebuggerReply::ExecutionState {
            program,
            state: blueice_ipc::debugger::DebuggerExecutionState::Pending,
        }
    );

    // A resume for the exact but not-yet-paused program also fails closed and
    // cannot begin ordinary script execution.
    assert!(matches!(
        debugger_request(
            &mut debugger,
            &blueice_ipc::debugger::DebuggerRequest::ResumeExecution { program },
            PAGE_SECRET,
        ),
        blueice_ipc::debugger::DebuggerReply::Error {
            code: blueice_ipc::debugger::DebuggerErrorCode::InvalidExecutionState,
            ..
        }
    ));
    assert_eq!(
        debugger_request(
            &mut debugger,
            &blueice_ipc::debugger::DebuggerRequest::GetExecutionState { program },
            PAGE_SECRET,
        ),
        blueice_ipc::debugger::DebuggerReply::ExecutionState {
            program,
            state: blueice_ipc::debugger::DebuggerExecutionState::Pending,
        }
    );

    // The target is verified before execution, but v5 intentionally does not
    // claim continuation support for child code units.
    assert!(matches!(
        debugger_request(
            &mut debugger,
            &blueice_ipc::debugger::DebuggerRequest::ArmRootSafePointBreakpoint {
                safe_point: child_safe_point,
            },
            PAGE_SECRET,
        ),
        blueice_ipc::debugger::DebuggerReply::Error {
            code: blueice_ipc::debugger::DebuggerErrorCode::InvalidSafePoint,
            ..
        }
    ));
    assert_eq!(
        debugger_request(
            &mut debugger,
            &blueice_ipc::debugger::DebuggerRequest::GetExecutionState { program },
            PAGE_SECRET,
        ),
        blueice_ipc::debugger::DebuggerReply::ExecutionState {
            program,
            state: blueice_ipc::debugger::DebuggerExecutionState::Pending,
        }
    );
    assert_eq!(
        debugger_request(
            &mut debugger,
            &blueice_ipc::debugger::DebuggerRequest::ArmRootSafePointBreakpoint {
                safe_point: root_safe_point,
            },
            PAGE_SECRET,
        ),
        blueice_ipc::debugger::DebuggerReply::RootSafePointBreakpointArmed {
            safe_point: root_safe_point,
        }
    );
    wait_for_debugger_state(
        &mut debugger,
        program,
        blueice_ipc::debugger::DebuggerExecutionState::Paused {
            safe_point: root_safe_point,
        },
        PAGE_SECRET,
    );
    // Once BlueJS has paused, a second arm cannot change the target or create
    // loop-hit/re-arm behavior. It fails before another bytecode run.
    assert!(matches!(
        debugger_request(
            &mut debugger,
            &blueice_ipc::debugger::DebuggerRequest::ArmRootSafePointBreakpoint {
                safe_point: root_safe_point,
            },
            PAGE_SECRET,
        ),
        blueice_ipc::debugger::DebuggerReply::Error {
            code: blueice_ipc::debugger::DebuggerErrorCode::InvalidExecutionState,
            ..
        }
    ));
    assert_eq!(
        debugger_request(
            &mut debugger,
            &blueice_ipc::debugger::DebuggerRequest::GetExecutionState { program },
            PAGE_SECRET,
        ),
        blueice_ipc::debugger::DebuggerReply::ExecutionState {
            program,
            state: blueice_ipc::debugger::DebuggerExecutionState::Paused {
                safe_point: root_safe_point,
            },
        }
    );
    assert_eq!(
        debugger_request(
            &mut debugger,
            &blueice_ipc::debugger::DebuggerRequest::ResumeExecution { program },
            PAGE_SECRET,
        ),
        blueice_ipc::debugger::DebuggerReply::ExecutionResumed { program }
    );
    assert_eq!(
        debugger_request(
            &mut debugger,
            &blueice_ipc::debugger::DebuggerRequest::GetExecutionState { program },
            PAGE_SECRET,
        ),
        blueice_ipc::debugger::DebuggerReply::ExecutionState {
            program,
            state: blueice_ipc::debugger::DebuggerExecutionState::Resuming,
        }
    );
    // `GetExecutionState` holds a transition for its reply, then an idle
    // session tick resumes the retained BlueJS frame without a debugger lease.
    thread::sleep(Duration::from_millis(100));
    assert_eq!(
        debugger_request(
            &mut debugger,
            &blueice_ipc::debugger::DebuggerRequest::GetExecutionState { program },
            PAGE_SECRET,
        ),
        blueice_ipc::debugger::DebuggerReply::ExecutionState {
            program,
            state: blueice_ipc::debugger::DebuggerExecutionState::Completed,
        }
    );

    // A replacement realm must make every old target stale before it can
    // affect the pending module declaration in the successor document.
    blueice_ipc::write_client_message(
        &mut frontend,
        &blueice_ipc::ClientMessage::Navigate {
            url: format!("http://{address}"),
        },
    )
    .unwrap();
    assert!(matches!(
        blueice_ipc::read_server_message(&mut frontend).unwrap(),
        blueice_ipc::ServerMessage::Navigated { .. }
    ));
    assert!(matches!(
        blueice_ipc::read_server_message(&mut frontend).unwrap(),
        blueice_ipc::ServerMessage::FrameReady { generation: 2, .. }
    ));
    assert!(matches!(
        debugger_request(
            &mut debugger,
            &blueice_ipc::debugger::DebuggerRequest::GetExecutionState { program },
            PAGE_SECRET,
        ),
        blueice_ipc::debugger::DebuggerReply::Error {
            code: blueice_ipc::debugger::DebuggerErrorCode::StaleRealm,
            ..
        }
    ));
    let module_realm = match debugger_request(
        &mut debugger,
        &blueice_ipc::debugger::DebuggerRequest::ListPageRealms,
        PAGE_SECRET,
    ) {
        blueice_ipc::debugger::DebuggerReply::PageRealms(realms) => {
            assert_eq!(realms.len(), 1);
            assert_ne!(realms[0], realm);
            realms[0]
        }
        other => panic!("expected replacement debugger realm, got {other:?}"),
    };
    let module_program = match debugger_request(
        &mut debugger,
        &blueice_ipc::debugger::DebuggerRequest::ListPrograms {
            realm: module_realm,
        },
        PAGE_SECRET,
    ) {
        blueice_ipc::debugger::DebuggerReply::Programs(programs) => {
            assert_eq!(programs.len(), 1);
            programs[0]
        }
        other => panic!("expected one opaque module debugger program, got {other:?}"),
    };
    let module_safe_point = match debugger_request(
        &mut debugger,
        &blueice_ipc::debugger::DebuggerRequest::ListSafePoints {
            program: module_program,
        },
        PAGE_SECRET,
    ) {
        blueice_ipc::debugger::DebuggerReply::SafePoints(safe_points) => *safe_points
            .iter()
            .find(|safe_point| safe_point.code_unit_ordinal == 0)
            .expect("module fixture must expose a root safe-point inventory"),
        other => panic!("expected module debugger safe points, got {other:?}"),
    };
    assert!(matches!(
        debugger_request(
            &mut debugger,
            &blueice_ipc::debugger::DebuggerRequest::ArmRootSafePointBreakpoint {
                safe_point: module_safe_point,
            },
            PAGE_SECRET,
        ),
        blueice_ipc::debugger::DebuggerReply::Error {
            code: blueice_ipc::debugger::DebuggerErrorCode::InvalidSafePoint,
            ..
        }
    ));

    blueice_ipc::write_client_message(&mut frontend, &blueice_ipc::ClientMessage::Shutdown)
        .unwrap();
    assert!(child.wait().unwrap().success());
    assert!(!socket_path.exists());
    assert!(!debugger_socket_path.exists());
    assert!(!frame_dir.exists());
}

#[test]
fn real_subprocess_serves_navigate_resize_and_shutdown_over_a_real_socket() {
    let socket_path = unique_socket_path("full-session");
    let frame_dir = std::env::temp_dir().join(format!(
        "blueice-core-binary-test-frames-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&socket_path);
    let _ = std::fs::remove_dir_all(&frame_dir);
    let gatekeeper_path = clearing_gatekeeper("fs-gk"); // short: Unix socket paths are capped at ~100 bytes total

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buf = [0u8; 1024];
        let _ = stream.read(&mut buf);
        let body = "<p>from a real subprocess</p>";
        stream
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            )
            .unwrap();
    });

    let mut child = Command::new(env!("CARGO_BIN_EXE_blueice-core"))
        .args([
            "--socket",
            socket_path.to_str().unwrap(),
            "--width",
            "300",
            "--height",
            "150",
            "--frame-dir",
            frame_dir.to_str().unwrap(),
            "--gatekeeper-socket",
            gatekeeper_path.to_str().unwrap(),
        ])
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn blueice-core");

    assert!(
        wait_for(&socket_path, Duration::from_secs(5)),
        "blueice-core never created its socket"
    );
    let mut stream = connect_with_retry(&socket_path, Duration::from_secs(5))
        .expect("failed to connect to the real subprocess");
    blueice_ipc::client_handshake(&mut stream)
        .expect("the real subprocess must complete the protocol_version handshake");

    let url = format!("http://{addr}");
    blueice_ipc::write_client_message(
        &mut stream,
        &blueice_ipc::ClientMessage::Navigate { url: url.clone() },
    )
    .unwrap();
    let navigated = blueice_ipc::read_server_message(&mut stream).unwrap();
    assert_eq!(navigated, blueice_ipc::ServerMessage::Navigated { url });

    let frame = blueice_ipc::read_server_message(&mut stream).unwrap();
    let (shm_path, width, height) = match frame {
        blueice_ipc::ServerMessage::FrameReady {
            shm_path,
            width,
            height,
            generation: 1,
        } => (shm_path, width, height),
        other => panic!("expected the first FrameReady, got {other:?}"),
    };
    assert_eq!((width, height), (300, 150));
    let mapped = blueice_ipc::shm::map_frame(std::path::Path::new(&shm_path))
        .expect("the real subprocess's frame file must be mappable");
    assert_eq!(
        mapped.len() as u32,
        width * height * 4,
        "RGBA8 frame bytes must match the requested viewport size"
    );

    blueice_ipc::write_client_message(
        &mut stream,
        &blueice_ipc::ClientMessage::Resize {
            width: 100,
            height: 80,
        },
    )
    .unwrap();
    let resized = blueice_ipc::read_server_message(&mut stream).unwrap();
    assert!(matches!(
        resized,
        blueice_ipc::ServerMessage::FrameReady {
            width: 100,
            height: 80,
            generation: 2,
            ..
        }
    ));

    blueice_ipc::write_client_message(&mut stream, &blueice_ipc::ClientMessage::Shutdown).unwrap();
    let status = child
        .wait()
        .expect("failed to wait for blueice-core to exit");
    assert!(
        status.success(),
        "blueice-core must exit cleanly after Shutdown"
    );

    assert!(
        !socket_path.exists(),
        "blueice-core must remove its own socket file on exit"
    );
    assert!(
        !frame_dir.exists(),
        "blueice-core must remove its own frame directory on exit"
    );
}

#[test]
fn real_subprocess_routes_the_bounded_oop_root_safe_point_lifecycle() {
    // This crosses both real sockets: the public debugger reaches the core
    // session, which in turn reaches the separately owned BlueJS child over
    // its private capability-authenticated page-host transport. The explicit
    // debugger listener activates bounded root-classic control and source-free
    // stack/scope inspection, never generic child interruption or VM values.
    const PAGE_SECRET: &str = "OOP_DEBUGGER_BREAKPOINT_SECRET_DO_NOT_DISCLOSE";
    let socket_path = unique_socket_path("oop-debugger-core");
    let debugger_socket_path = unique_socket_path("oop-debugger-host");
    let frame_dir = std::env::temp_dir().join(format!(
        "blueice-core-binary-test-oop-debugger-frames-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&socket_path);
    let _ = std::fs::remove_file(&debugger_socket_path);
    let _ = std::fs::remove_dir_all(&frame_dir);
    let gatekeeper_path = clearing_gatekeeper("oop-debugger-gk");
    let (host_socket, host_token, host) = spawn_private_bluejs_host("oop-debugger-host");

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0u8; 1024];
        let _ = stream.read(&mut request);
        let body = format!(
            concat!(
                "<main>private page host debugger</main><script>const secret = '{}'; ",
                "let checkpoint = secret; globalThis.finished = checkpoint;</script>"
            ),
            PAGE_SECRET,
        );
        stream
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            )
            .unwrap();
    });

    let mut core = Command::new(env!("CARGO_BIN_EXE_blueice-core"))
        .args([
            "--socket",
            socket_path.to_str().unwrap(),
            "--debugger-socket",
            debugger_socket_path.to_str().unwrap(),
            "--frame-dir",
            frame_dir.to_str().unwrap(),
            "--gatekeeper-socket",
            gatekeeper_path.to_str().unwrap(),
            "--out-of-process-bluejs-socket",
            host_socket.to_str().unwrap(),
            "--out-of-process-bluejs-token",
            &host_token,
        ])
        .stderr(Stdio::piped())
        .spawn()
        .expect("must spawn core with child page host and debugger socket");
    assert!(wait_for(&socket_path, Duration::from_secs(5)));
    assert!(wait_for(&debugger_socket_path, Duration::from_secs(5)));

    let mut frontend = connect_with_retry(&socket_path, Duration::from_secs(5)).unwrap();
    blueice_ipc::client_handshake(&mut frontend).unwrap();
    blueice_ipc::write_client_message(
        &mut frontend,
        &blueice_ipc::ClientMessage::Navigate {
            url: format!("http://{addr}"),
        },
    )
    .unwrap();
    assert!(matches!(
        blueice_ipc::read_server_message(&mut frontend).unwrap(),
        blueice_ipc::ServerMessage::Navigated { .. }
    ));
    assert!(matches!(
        blueice_ipc::read_server_message(&mut frontend).unwrap(),
        blueice_ipc::ServerMessage::FrameReady { generation: 1, .. }
    ));

    let mut debugger = connect_with_retry(&debugger_socket_path, Duration::from_secs(5)).unwrap();
    assert_eq!(
        debugger_request(
            &mut debugger,
            &blueice_ipc::debugger::DebuggerRequest::Hello {
                protocol_version: blueice_ipc::debugger::DEBUGGER_PROTOCOL_VERSION,
                requested_bounded_values: false,
                requested_metadata_capabilities:
                    blueice_ipc::debugger::DebuggerMetadataCapabilityManifest::empty(),
            },
            PAGE_SECRET,
        ),
        blueice_ipc::debugger::DebuggerReply::HelloAck {
            protocol_version: blueice_ipc::debugger::DEBUGGER_PROTOCOL_VERSION,
            granted_bounded_values: false,
            granted_metadata_capabilities:
                blueice_ipc::debugger::DebuggerMetadataCapabilityManifest::empty(),
        }
    );
    let realm = match debugger_request(
        &mut debugger,
        &blueice_ipc::debugger::DebuggerRequest::ListPageRealms,
        PAGE_SECRET,
    ) {
        blueice_ipc::debugger::DebuggerReply::PageRealms(realms) => {
            assert_eq!(realms.len(), 1);
            realms[0]
        }
        reply => panic!("expected OOP page realm, got {reply:?}"),
    };
    let capabilities = match debugger_request(
        &mut debugger,
        &blueice_ipc::debugger::DebuggerRequest::DescribeCapabilities { realm },
        PAGE_SECRET,
    ) {
        blueice_ipc::debugger::DebuggerReply::Capabilities(capabilities) => capabilities,
        reply => panic!("expected OOP debugger capabilities, got {reply:?}"),
    };
    assert!(capabilities.reports.iter().any(|report| {
        report.capability == blueice_ipc::debugger::DebuggerCapability::ProgramLocations
            && report.state == blueice_ipc::debugger::DebuggerCapabilityState::Available
    }));
    assert!(capabilities.reports.iter().any(|report| {
        report.capability == blueice_ipc::debugger::DebuggerCapability::BreakpointConfiguration
            && report.state == blueice_ipc::debugger::DebuggerCapabilityState::Available
            && report.detail.contains("does not interrupt execution")
    }));
    assert!(capabilities.reports.iter().any(|report| {
        report.capability == blueice_ipc::debugger::DebuggerCapability::PauseResume
            && report.state == blueice_ipc::debugger::DebuggerCapabilityState::Available
            && report.detail.contains("root frame")
    }));
    assert!(capabilities.reports.iter().any(|report| {
        report.capability == blueice_ipc::debugger::DebuggerCapability::Breakpoints
            && report.state == blueice_ipc::debugger::DebuggerCapabilityState::Available
            && report.detail.contains("root-code-unit")
    }));
    assert!(capabilities.reports.iter().any(|report| {
        report.capability == blueice_ipc::debugger::DebuggerCapability::Stepping
            && report.state == blueice_ipc::debugger::DebuggerCapabilityState::Available
    }));
    for capability in [
        blueice_ipc::debugger::DebuggerCapability::Stack,
        blueice_ipc::debugger::DebuggerCapability::Scopes,
    ] {
        assert!(capabilities.reports.iter().any(|report| {
            report.capability == capability
                && report.state == blueice_ipc::debugger::DebuggerCapabilityState::Available
        }));
    }
    for capability in [
        blueice_ipc::debugger::DebuggerCapability::BoundedValues,
        blueice_ipc::debugger::DebuggerCapability::StaticScopeRelation,
    ] {
        assert!(capabilities.reports.iter().any(|report| {
            report.capability == capability
                && report.state == blueice_ipc::debugger::DebuggerCapabilityState::Planned
        }));
    }

    let program = match debugger_request(
        &mut debugger,
        &blueice_ipc::debugger::DebuggerRequest::ListPrograms { realm },
        PAGE_SECRET,
    ) {
        blueice_ipc::debugger::DebuggerReply::Programs(programs) => {
            assert_eq!(programs.len(), 1);
            programs[0]
        }
        reply => panic!("expected opaque OOP program, got {reply:?}"),
    };
    assert!(
        program.program_handle >= (1 << 63),
        "the public debugger must never receive the child-private program ID"
    );
    let safe_points = match debugger_request(
        &mut debugger,
        &blueice_ipc::debugger::DebuggerRequest::ListSafePoints { program },
        PAGE_SECRET,
    ) {
        blueice_ipc::debugger::DebuggerReply::SafePoints(safe_points) => safe_points,
        reply => panic!("expected OOP safe points, got {reply:?}"),
    };
    let safe_point = *safe_points
        .iter()
        .find(|safe_point| safe_point.code_unit_ordinal == 0 && safe_point.bytecode_offset == 0)
        .expect("OOP program must expose its exact root entry safe point");
    let root_safe_point = *safe_points
        .iter()
        .find(|safe_point| safe_point.code_unit_ordinal == 0 && safe_point.bytecode_offset != 0)
        .expect("OOP program must expose a resumable non-entry root safe point");
    assert_eq!(
        debugger_request(
            &mut debugger,
            &blueice_ipc::debugger::DebuggerRequest::ArmRootSafePointBreakpoint {
                safe_point: root_safe_point,
            },
            PAGE_SECRET,
        ),
        blueice_ipc::debugger::DebuggerReply::RootSafePointBreakpointArmed {
            safe_point: root_safe_point,
        }
    );
    // The supervised child may need another owner turn after the arm reply.
    wait_for_debugger_state(
        &mut debugger,
        program,
        blueice_ipc::debugger::DebuggerExecutionState::Paused {
            safe_point: root_safe_point,
        },
        PAGE_SECRET,
    );
    assert_eq!(
        debugger_request(
            &mut debugger,
            &blueice_ipc::debugger::DebuggerRequest::SetBreakpoint { safe_point },
            PAGE_SECRET,
        ),
        blueice_ipc::debugger::DebuggerReply::BreakpointSet { safe_point }
    );
    assert_eq!(
        debugger_request(
            &mut debugger,
            &blueice_ipc::debugger::DebuggerRequest::ListBreakpoints { realm },
            PAGE_SECRET,
        ),
        blueice_ipc::debugger::DebuggerReply::Breakpoints(vec![safe_point, root_safe_point])
    );
    assert_eq!(
        debugger_request(
            &mut debugger,
            &blueice_ipc::debugger::DebuggerRequest::ClearBreakpoint { safe_point },
            PAGE_SECRET,
        ),
        blueice_ipc::debugger::DebuggerReply::BreakpointCleared {
            safe_point,
            was_present: true,
        }
    );
    assert_eq!(
        debugger_request(
            &mut debugger,
            &blueice_ipc::debugger::DebuggerRequest::ClearBreakpoint { safe_point },
            PAGE_SECRET,
        ),
        blueice_ipc::debugger::DebuggerReply::BreakpointCleared {
            safe_point,
            was_present: false,
        }
    );
    assert!(matches!(
        debugger_request(
            &mut debugger,
            &blueice_ipc::debugger::DebuggerRequest::ArmEntryBreakpoint { safe_point },
            PAGE_SECRET,
        ),
        blueice_ipc::debugger::DebuggerReply::Error {
            code: blueice_ipc::debugger::DebuggerErrorCode::CapabilityUnavailable,
            ..
        }
    ));
    assert!(matches!(
        debugger_request(
            &mut debugger,
            &blueice_ipc::debugger::DebuggerRequest::StepRootInstruction {
                program: blueice_ipc::debugger::DebuggerProgram {
                    program_generation: program.program_generation + 1,
                    ..program
                },
            },
            PAGE_SECRET,
        ),
        blueice_ipc::debugger::DebuggerReply::Error {
            code: blueice_ipc::debugger::DebuggerErrorCode::InvalidTarget,
            ..
        }
    ));
    assert_eq!(
        debugger_request(
            &mut debugger,
            &blueice_ipc::debugger::DebuggerRequest::StepRootInstruction { program },
            PAGE_SECRET,
        ),
        blueice_ipc::debugger::DebuggerReply::ExecutionStepRequested { program }
    );
    assert_eq!(
        debugger_request(
            &mut debugger,
            &blueice_ipc::debugger::DebuggerRequest::GetExecutionState { program },
            PAGE_SECRET,
        ),
        blueice_ipc::debugger::DebuggerReply::ExecutionState {
            program,
            state: blueice_ipc::debugger::DebuggerExecutionState::Stepping,
        }
    );
    let mut stepped_state = None;
    for _ in 0..20 {
        let reply = debugger_request(
            &mut debugger,
            &blueice_ipc::debugger::DebuggerRequest::GetExecutionState { program },
            PAGE_SECRET,
        );
        if let blueice_ipc::debugger::DebuggerReply::ExecutionState {
            program: reply_program,
            state: blueice_ipc::debugger::DebuggerExecutionState::Paused { safe_point },
        } = reply
        {
            if reply_program == program {
                stepped_state = Some(safe_point);
                break;
            }
        }
        thread::sleep(Duration::from_millis(50));
    }
    let stepped_safe_point = stepped_state.expect("one isolated child root step must pause again");
    assert_ne!(stepped_safe_point, root_safe_point);
    assert_eq!(stepped_safe_point.program, program);
    assert_eq!(
        debugger_request(
            &mut debugger,
            &blueice_ipc::debugger::DebuggerRequest::ResumeExecution { program },
            PAGE_SECRET,
        ),
        blueice_ipc::debugger::DebuggerReply::ExecutionResumed { program }
    );
    assert_eq!(
        debugger_request(
            &mut debugger,
            &blueice_ipc::debugger::DebuggerRequest::GetExecutionState { program },
            PAGE_SECRET,
        ),
        blueice_ipc::debugger::DebuggerReply::ExecutionState {
            program,
            state: blueice_ipc::debugger::DebuggerExecutionState::Resuming,
        }
    );
    wait_for_debugger_state(
        &mut debugger,
        program,
        blueice_ipc::debugger::DebuggerExecutionState::Completed,
        PAGE_SECRET,
    );

    blueice_ipc::write_client_message(&mut frontend, &blueice_ipc::ClientMessage::Shutdown)
        .unwrap();
    assert!(core.wait().unwrap().success());
    shutdown_private_bluejs_host(&host_socket, &host_token);
    host.join().unwrap();
    let _ = std::fs::remove_file(&host_socket);
    assert!(!socket_path.exists());
    assert!(!debugger_socket_path.exists());
    assert!(!frame_dir.exists());
}
