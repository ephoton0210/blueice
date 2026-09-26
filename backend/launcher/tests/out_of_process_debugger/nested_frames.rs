// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn public_socket_steps_and_resumes_one_real_bluets_nested_frame() {
    let gatekeeper_socket = clearing_gatekeeper();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let fixture = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0_u8; 1024];
        let _ = stream.read(&mut request);
        let source = "function inner(a: number, b: number): number { let first: number = a; let second: number = b; return first + second; } globalThis.answer = inner(2, 2) + 1;";
        let body = format!(
            "<main>nested-bluets</main><script type=\"application/x-blueice-typescript\">{source}</script>"
        );
        stream
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            )
            .unwrap();
    });
    let mut launcher = LauncherProcess::spawn(&gatekeeper_socket);
    let mut browser = launcher.connect_browser();
    blueice_ipc::client_handshake(&mut browser).unwrap();

    let mut debugger = UnixStream::connect(&launcher.debugger_socket).unwrap();
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::Hello {
                protocol_version: DEBUGGER_PROTOCOL_VERSION,
                requested_bounded_values: false,
                requested_metadata_capabilities: DebuggerMetadataCapabilityManifest::empty(),
            },
        ),
        DebuggerReply::HelloAck {
            protocol_version: DEBUGGER_PROTOCOL_VERSION,
            granted_bounded_values: false,
            granted_metadata_capabilities: DebuggerMetadataCapabilityManifest::empty(),
        }
    );
    navigate(&mut browser, &url);
    let realm = one_realm(debugger_request(
        &mut debugger,
        DebuggerRequest::ListPageRealms,
    ));
    let DebuggerReply::Capabilities(capabilities) = debugger_request(
        &mut debugger,
        DebuggerRequest::DescribeCapabilities { realm },
    ) else {
        panic!("expected public debugger capabilities");
    };
    assert!(capabilities.reports.iter().any(|report| {
        report.capability == DebuggerCapability::NestedFrames
            && report.state == DebuggerCapabilityState::Available
    }));
    for capability in [DebuggerCapability::Stack, DebuggerCapability::Scopes] {
        assert!(capabilities.reports.iter().any(|report| {
            report.capability == capability && report.state == DebuggerCapabilityState::Available
        }));
    }
    let program = one_program(
        debugger_request(&mut debugger, DebuggerRequest::ListPrograms { realm }),
        realm,
    );
    let target = safe_points(
        debugger_request(&mut debugger, DebuggerRequest::ListSafePoints { program }),
        program,
    )
    .into_iter()
    .find(|point| point.code_unit_ordinal == 1 && point.bytecode_offset == 0)
    .expect("BlueTS inner function must have its first safe point");
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::ArmNestedSafePointBreakpoint { safe_point: target },
        ),
        DebuggerReply::NestedSafePointBreakpointArmed { safe_point: target }
    );
    let (frame, first) = await_nested_paused_execution(&mut debugger, program);
    assert_eq!(first, target);
    assert!(frame.matches_safe_point(first));
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::StepRootInstruction { program }
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidExecutionState,
            ..
        }
    ));
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::StepNestedInstruction { frame },
        ),
        DebuggerReply::NestedStepRequested { frame }
    );
    let (successor_frame, successor) = await_nested_paused_execution(&mut debugger, program);
    assert_eq!(successor_frame, frame);
    assert_ne!(successor, first);
    assert_eq!(successor.code_unit_ordinal, first.code_unit_ordinal);

    let mut active_stack = None;
    for _ in 0..64 {
        let DebuggerReply::Stack(stack) = debugger_request(
            &mut debugger,
            DebuggerRequest::GetStack {
                program,
                frame: Some(frame),
                max_frames: 2,
            },
        ) else {
            panic!("the paused BlueTS child must expose a bounded stack");
        };
        assert_eq!(stack.program, program);
        assert_eq!(stack.frame, Some(frame));
        assert_eq!(stack.safe_points.len(), 2);
        assert_eq!(stack.safe_points[0].code_unit_ordinal, 1);
        assert_eq!(stack.safe_points[1].code_unit_ordinal, 0);
        assert!(!stack.stack_truncated);
        let DebuggerReply::Scopes(scopes) = debugger_request(
            &mut debugger,
            DebuggerRequest::GetScopes {
                program,
                frame: Some(frame),
                frame_index: 0,
                expected_safe_point: stack.safe_points[0],
                max_scope_entries: 256,
            },
        ) else {
            panic!("the exact paused BlueTS child must expose active lexical slots");
        };
        assert_eq!(scopes.safe_point, stack.safe_points[0]);
        assert!(!scopes.scope_truncated);
        if scopes.entries.len() >= 2 {
            active_stack = Some(stack);
            break;
        }
        assert_eq!(
            debugger_request(
                &mut debugger,
                DebuggerRequest::StepNestedInstruction { frame },
            ),
            DebuggerReply::NestedStepRequested { frame }
        );
        let (same_frame, _) = await_nested_paused_execution(&mut debugger, program);
        assert_eq!(same_frame, frame);
    }
    let stack = active_stack.expect("BlueTS child must enter a scope with two slots");
    let DebuggerReply::Stack(limited_stack) = debugger_request(
        &mut debugger,
        DebuggerRequest::GetStack {
            program,
            frame: Some(frame),
            max_frames: 1,
        },
    ) else {
        panic!("the smaller stack budget must return an explicit truncation");
    };
    assert_eq!(limited_stack.safe_points, vec![stack.safe_points[0]]);
    assert!(limited_stack.stack_truncated);
    let DebuggerReply::Scopes(limited_scopes) = debugger_request(
        &mut debugger,
        DebuggerRequest::GetScopes {
            program,
            frame: Some(frame),
            frame_index: 0,
            expected_safe_point: stack.safe_points[0],
            max_scope_entries: 1,
        },
    ) else {
        panic!("the smaller scope budget must return an explicit truncation");
    };
    assert_eq!(limited_scopes.entries.len(), 1);
    assert!(limited_scopes.scope_truncated);
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::GetScopes {
                program,
                frame: Some(frame),
                frame_index: 0,
                expected_safe_point: DebuggerSafePoint {
                    bytecode_offset: stack.safe_points[0].bytecode_offset + 1,
                    ..stack.safe_points[0]
                },
                max_scope_entries: 1,
            },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidExecutionState,
            ..
        }
    ));
    for max_frames in [0, 65] {
        assert!(matches!(
            debugger_request(
                &mut debugger,
                DebuggerRequest::GetStack {
                    program,
                    frame: Some(frame),
                    max_frames,
                },
            ),
            DebuggerReply::Error {
                code: DebuggerErrorCode::ResourceLimit,
                ..
            }
        ));
    }
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::GetScopes {
                program,
                frame: Some(frame),
                frame_index: 0,
                expected_safe_point: stack.safe_points[0],
                max_scope_entries: 257,
            },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::ResourceLimit,
            ..
        }
    ));
    let DebuggerReply::Scopes(parent_scopes) = debugger_request(
        &mut debugger,
        DebuggerRequest::GetScopes {
            program,
            frame: Some(frame),
            frame_index: 1,
            expected_safe_point: stack.safe_points[1],
            max_scope_entries: 256,
        },
    ) else {
        panic!("the waiting root must remain the exact second stack frame");
    };
    assert_eq!(parent_scopes.frame_index, 1);
    assert_eq!(parent_scopes.safe_point, stack.safe_points[1]);

    assert!(matches!(
        debugger_request(&mut debugger, DebuggerRequest::ResumeExecution { program }),
        DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidExecutionState,
            ..
        }
    ));
    let mut wrong_frame = frame;
    wrong_frame.frame_handle += 1;
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::GetStack {
                program,
                frame: Some(wrong_frame),
                max_frames: 2,
            },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidExecutionState,
            ..
        }
    ));
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::ResumeNestedExecution { frame: wrong_frame },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidExecutionState,
            ..
        }
    ));
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::ResumeNestedExecution { frame },
        ),
        DebuggerReply::NestedResumeRequested { frame }
    );
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::GetExecutionState { program },
        ),
        DebuggerReply::ExecutionState {
            program,
            state: DebuggerExecutionState::NestedResuming { frame },
        }
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match debugger_request(
            &mut debugger,
            DebuggerRequest::GetExecutionState { program },
        ) {
            DebuggerReply::ExecutionState {
                state: DebuggerExecutionState::Paused { safe_point },
                ..
            } => {
                assert_eq!(safe_point.code_unit_ordinal, 0);
                break;
            }
            DebuggerReply::ExecutionState {
                state: DebuggerExecutionState::NestedResuming { frame: same },
                ..
            } if same == frame && Instant::now() < deadline => {
                thread::sleep(Duration::from_millis(10));
            }
            reply => panic!("nested resume must rejoin its root: {reply:?}"),
        }
    }
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::ResumeNestedExecution { frame }
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidExecutionState,
            ..
        }
    ));
    let DebuggerReply::Stack(root_stack) = debugger_request(
        &mut debugger,
        DebuggerRequest::GetStack {
            program,
            frame: None,
            max_frames: 2,
        },
    ) else {
        panic!("the waiting root must be inspectable after its child returns");
    };
    assert_eq!(root_stack.safe_points.len(), 1);
    assert_eq!(root_stack.safe_points[0].code_unit_ordinal, 0);
    assert!(!root_stack.stack_truncated);
    assert_eq!(
        debugger_request(&mut debugger, DebuggerRequest::ResumeExecution { program }),
        DebuggerReply::ExecutionResumed { program }
    );
    await_completed_execution(&mut debugger, program);
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::GetStack {
                program,
                frame: None,
                max_frames: 2,
            },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidExecutionState,
            ..
        }
    ));
    write_client_message(&mut browser, &ClientMessage::GetBlueTsScriptReports).unwrap();
    assert!(matches!(
        read_server_message(&mut browser).unwrap(),
        ServerMessage::BlueTsScriptReports(reports)
            if reports.len() == 1
                && reports[0].kind == blueice_ipc::BlueTsScriptKind::Classic
                && reports[0].outcome == blueice_ipc::BlueTsScriptExecutionOutcome::Executed
    ));

    fixture.join().unwrap();
    launcher.shutdown();
    let _ = std::fs::remove_file(gatekeeper_socket);
}

#[test]
fn public_socket_rejects_cross_tab_nested_frames_with_two_live_bluets_pages() {
    let gatekeeper_socket = clearing_gatekeeper();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let fixture = thread::spawn(move || {
        for _ in 0..2 {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0_u8; 1024];
            let _ = stream.read(&mut request);
            let source = "function inner(): number { return 4; } globalThis.answer = inner() + 1;";
            let body = format!(
                "<main>two-nested-tabs</main><script type=\"application/x-blueice-typescript\">{source}</script>"
            );
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                )
                .unwrap();
        }
    });
    let mut launcher = LauncherProcess::spawn(&gatekeeper_socket);
    let mut browser = launcher.connect_browser();
    blueice_ipc::client_handshake(&mut browser).unwrap();
    let mut debugger = UnixStream::connect(&launcher.debugger_socket).unwrap();
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::Hello {
                protocol_version: DEBUGGER_PROTOCOL_VERSION,
                requested_bounded_values: false,
                requested_metadata_capabilities: DebuggerMetadataCapabilityManifest::empty(),
            },
        ),
        DebuggerReply::HelloAck { .. }
    ));
    navigate(&mut browser, &url);
    let first_realm = one_realm(debugger_request(
        &mut debugger,
        DebuggerRequest::ListPageRealms,
    ));
    let (first_program, first_frame) = arm_first_nested_frame(&mut debugger, first_realm);

    write_client_message(
        &mut browser,
        &ClientMessage::OpenTab {
            url: Some(url.clone()),
        },
    )
    .unwrap();
    let second_tab = match read_server_message(&mut browser).unwrap() {
        ServerMessage::TabOpened {
            tab_id,
            url: Some(opened_url),
        } => {
            assert_eq!(opened_url, url);
            tab_id
        }
        reply => panic!("expected second live BlueTS tab: {reply:?}"),
    };
    assert!(matches!(
        read_server_message(&mut browser).unwrap(),
        ServerMessage::FrameReady { .. }
    ));
    let second_realm = match debugger_request(&mut debugger, DebuggerRequest::ListPageRealms) {
        DebuggerReply::PageRealms(realms) => {
            assert_eq!(realms.len(), 2);
            realms
                .into_iter()
                .find(|realm| realm.tab_id == second_tab)
                .unwrap()
        }
        reply => panic!("expected two live debugger realms: {reply:?}"),
    };
    assert_ne!(second_realm.tab_id, first_realm.tab_id);
    let (second_program, second_frame) = arm_first_nested_frame(&mut debugger, second_realm);
    assert_ne!(first_frame, second_frame);

    for (frame, target_program) in [(first_frame, second_program), (second_frame, first_program)] {
        let wrong_tab = blueice_ipc::debugger::DebuggerFrame {
            program: target_program,
            ..frame
        };
        for request in [
            DebuggerRequest::StepNestedInstruction { frame: wrong_tab },
            DebuggerRequest::ResumeNestedExecution { frame: wrong_tab },
        ] {
            assert!(matches!(
                debugger_request(&mut debugger, request),
                DebuggerReply::Error {
                    code: DebuggerErrorCode::InvalidExecutionState,
                    ..
                }
            ));
        }
    }
    for (program, frame) in [(first_program, first_frame), (second_program, second_frame)] {
        assert!(matches!(
            debugger_request(&mut debugger, DebuggerRequest::GetExecutionState { program }),
            DebuggerReply::ExecutionState {
                state: DebuggerExecutionState::NestedPaused { frame: same, .. },
                ..
            } if same == frame
        ));
        assert_eq!(
            debugger_request(
                &mut debugger,
                DebuggerRequest::ResumeNestedExecution { frame },
            ),
            DebuggerReply::NestedResumeRequested { frame }
        );
    }

    fixture.join().unwrap();
    launcher.shutdown();
    let _ = std::fs::remove_file(gatekeeper_socket);
}

#[test]
fn public_socket_rejects_predecessor_frame_after_supervised_child_cutover() {
    let gatekeeper_socket = clearing_gatekeeper();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let fixture = thread::spawn(move || {
        for _ in 0..4 {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0_u8; 1024];
            let _ = stream.read(&mut request);
            let source = "function inner(): number { return 4; } globalThis.answer = inner() + 1;";
            let body = format!(
                "<main>cutover-nested-frame</main><script type=\"application/x-blueice-typescript\">{source}</script>"
            );
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                )
                .unwrap();
        }
    });
    let mut launcher = LauncherProcess::spawn(&gatekeeper_socket);
    let mut browser = launcher.connect_browser();
    blueice_ipc::client_handshake(&mut browser).unwrap();
    navigate(&mut browser, &url);
    navigate(&mut browser, &url);
    let mut predecessor_debugger = UnixStream::connect(&launcher.debugger_socket).unwrap();
    assert!(matches!(
        debugger_request(
            &mut predecessor_debugger,
            DebuggerRequest::Hello {
                protocol_version: DEBUGGER_PROTOCOL_VERSION,
                requested_bounded_values: false,
                requested_metadata_capabilities: DebuggerMetadataCapabilityManifest::empty(),
            },
        ),
        DebuggerReply::HelloAck { .. }
    ));
    let predecessor_realm = one_realm(debugger_request(
        &mut predecessor_debugger,
        DebuggerRequest::ListPageRealms,
    ));
    let (predecessor_program, predecessor_frame) =
        arm_first_nested_frame(&mut predecessor_debugger, predecessor_realm);

    let mut control = UnixStream::connect(&launcher.control_socket).unwrap();
    write_control_request(&mut control, &ControlRequest::Cutover).unwrap();
    assert_eq!(
        read_control_reply(&mut control).unwrap(),
        ControlReply::CutoverDone { tabs_migrated: 1 }
    );
    drop(predecessor_debugger);
    let mut successor_browser = launcher.connect_browser();
    blueice_ipc::client_handshake(&mut successor_browser).unwrap();
    navigate(&mut successor_browser, &url);
    let mut successor_debugger = UnixStream::connect(&launcher.debugger_socket).unwrap();
    assert!(matches!(
        debugger_request(
            &mut successor_debugger,
            DebuggerRequest::Hello {
                protocol_version: DEBUGGER_PROTOCOL_VERSION,
                requested_bounded_values: false,
                requested_metadata_capabilities: DebuggerMetadataCapabilityManifest::empty(),
            },
        ),
        DebuggerReply::HelloAck { .. }
    ));
    let successor_realm = one_realm(debugger_request(
        &mut successor_debugger,
        DebuggerRequest::ListPageRealms,
    ));
    let (successor_program, successor_frame) =
        arm_first_nested_frame(&mut successor_debugger, successor_realm);
    assert_eq!(successor_realm, predecessor_realm);
    assert_eq!(successor_program, predecessor_program);
    assert_eq!(successor_frame.frame_handle, predecessor_frame.frame_handle);
    assert_ne!(
        successor_frame.core_instance,
        predecessor_frame.core_instance
    );
    for request in [
        DebuggerRequest::StepNestedInstruction {
            frame: predecessor_frame,
        },
        DebuggerRequest::ResumeNestedExecution {
            frame: predecessor_frame,
        },
        DebuggerRequest::GetStack {
            program: predecessor_program,
            frame: Some(predecessor_frame),
            max_frames: 2,
        },
        DebuggerRequest::GetScopes {
            program: predecessor_program,
            frame: Some(predecessor_frame),
            frame_index: 0,
            expected_safe_point: DebuggerSafePoint {
                program: predecessor_program,
                code_unit_ordinal: 1,
                bytecode_offset: 0,
            },
            max_scope_entries: 1,
        },
    ] {
        assert!(matches!(
            debugger_request(&mut successor_debugger, request),
            DebuggerReply::Error {
                code: DebuggerErrorCode::InvalidExecutionState,
                ..
            }
        ));
    }
    assert!(matches!(
        debugger_request(
            &mut successor_debugger,
            DebuggerRequest::GetExecutionState {
                program: successor_program,
            },
        ),
        DebuggerReply::ExecutionState {
            state: DebuggerExecutionState::NestedPaused { frame: same, .. },
            ..
        } if same == successor_frame
    ));
    assert_eq!(
        debugger_request(
            &mut successor_debugger,
            DebuggerRequest::ResumeNestedExecution {
                frame: successor_frame,
            },
        ),
        DebuggerReply::NestedResumeRequested {
            frame: successor_frame,
        }
    );

    fixture.join().unwrap();
    launcher.shutdown();
    let _ = std::fs::remove_file(gatekeeper_socket);
}

#[test]
fn public_socket_keeps_unsupported_deeper_bluets_call_shape_unavailable() {
    let gatekeeper_socket = clearing_gatekeeper();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let fixture = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0_u8; 1024];
        let _ = stream.read(&mut request);
        let source = "function inner(): number { return 4; } function outer(): number { return inner(); } outer();";
        let body = format!(
            "<main>unsupported-nested-bluets</main><script type=\"application/x-blueice-typescript\">{source}</script>"
        );
        stream
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            )
            .unwrap();
    });
    let mut launcher = LauncherProcess::spawn(&gatekeeper_socket);
    let mut browser = launcher.connect_browser();
    blueice_ipc::client_handshake(&mut browser).unwrap();
    navigate(&mut browser, &url);

    let mut debugger = UnixStream::connect(&launcher.debugger_socket).unwrap();
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::Hello {
                protocol_version: DEBUGGER_PROTOCOL_VERSION,
                requested_bounded_values: false,
                requested_metadata_capabilities: DebuggerMetadataCapabilityManifest::empty(),
            },
        ),
        DebuggerReply::HelloAck { .. }
    ));
    let realm = one_realm(debugger_request(
        &mut debugger,
        DebuggerRequest::ListPageRealms,
    ));
    let program = one_program(
        debugger_request(&mut debugger, DebuggerRequest::ListPrograms { realm }),
        realm,
    );
    let target = safe_points(
        debugger_request(&mut debugger, DebuggerRequest::ListSafePoints { program }),
        program,
    )
    .into_iter()
    .find(|point| point.code_unit_ordinal == 1 && point.bytecode_offset == 0)
    .expect("nested BlueTS function has an exact static point");
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::ArmNestedSafePointBreakpoint { safe_point: target },
        ),
        DebuggerReply::NestedSafePointBreakpointArmed { safe_point: target }
    );

    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match debugger_request(
            &mut debugger,
            DebuggerRequest::GetExecutionState { program },
        ) {
            DebuggerReply::ExecutionState {
                state: DebuggerExecutionState::Completed,
                ..
            } => break,
            DebuggerReply::ExecutionState {
                state: DebuggerExecutionState::Pending,
                ..
            } if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
            reply => panic!("unsupported deeper call must not expose a frame: {reply:?}"),
        }
    }
    write_client_message(&mut browser, &ClientMessage::GetBlueTsScriptReports).unwrap();
    assert!(matches!(
        read_server_message(&mut browser).unwrap(),
        ServerMessage::BlueTsScriptReports(reports)
            if reports.len() == 1
                && reports[0].kind == blueice_ipc::BlueTsScriptKind::Classic
                && matches!(reports[0].outcome, blueice_ipc::BlueTsScriptExecutionOutcome::Rejected { .. })
    ));

    fixture.join().unwrap();
    launcher.shutdown();
    let _ = std::fs::remove_file(gatekeeper_socket);
}
