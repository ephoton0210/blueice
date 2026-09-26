// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn launcher_supervised_child_debugger_execution_is_opaque_and_expires_after_http_reload() {
    let gatekeeper_socket = clearing_gatekeeper();
    let listener = TcpListener::bind("127.0.0.1:0").expect("local HTTP fixture must bind");
    let url = format!("http://{}", listener.local_addr().unwrap());
    let fixture = serve_two_classic_documents(listener);
    let mut launcher = LauncherProcess::spawn(&gatekeeper_socket);

    let mut browser = launcher.connect_browser();
    blueice_ipc::client_handshake(&mut browser).expect("public browser handshake must succeed");
    navigate(&mut browser, &url);

    let mut debugger = UnixStream::connect(&launcher.debugger_socket)
        .expect("launcher public debugger endpoint must accept a peer");
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
    let first_realm = one_realm(debugger_request(
        &mut debugger,
        DebuggerRequest::ListPageRealms,
    ));
    let first_program = one_program(
        debugger_request(
            &mut debugger,
            DebuggerRequest::ListPrograms { realm: first_realm },
        ),
        first_realm,
    );
    let first_safe_points = safe_points(
        debugger_request(
            &mut debugger,
            DebuggerRequest::ListSafePoints {
                program: first_program,
            },
        ),
        first_program,
    );
    let first_safe_point = first_safe_points[0];
    let root_safe_point = *first_safe_points
        .iter()
        .find(|safe_point| safe_point.code_unit_ordinal == 0 && safe_point.bytecode_offset != 0)
        .expect("classic program must expose a resumable non-entry root safe point");
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::ArmRootSafePointBreakpoint {
                safe_point: root_safe_point,
            },
        ),
        DebuggerReply::RootSafePointBreakpointArmed {
            safe_point: root_safe_point,
        }
    );
    await_paused_execution(&mut debugger, first_program, root_safe_point);
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::ValidateSafePoint {
                safe_point: first_safe_point,
            },
        ),
        DebuggerReply::SafePointValidated {
            safe_point: first_safe_point,
        }
    );
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::ResumeExecution {
                program: first_program,
            },
        ),
        DebuggerReply::ExecutionResumed {
            program: first_program,
        }
    );
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::GetExecutionState {
                program: first_program,
            },
        ),
        DebuggerReply::ExecutionState {
            program: first_program,
            state: blueice_ipc::debugger::DebuggerExecutionState::Resuming,
        }
    );
    await_completed_execution(&mut debugger, first_program);

    // Reload through the same public browser connection. The fixture accepts
    // exactly two requests so this is a real HTTP replacement, not a direct
    // page-host document injection.
    navigate(&mut browser, &url);
    fixture
        .join()
        .expect("local HTTP fixture must serve both documents");

    for request in [
        DebuggerRequest::ListPrograms { realm: first_realm },
        DebuggerRequest::ListSafePoints {
            program: first_program,
        },
        DebuggerRequest::ValidateSafePoint {
            safe_point: first_safe_point,
        },
        DebuggerRequest::ValidateSafePoint {
            safe_point: root_safe_point,
        },
    ] {
        assert!(matches!(
            debugger_request(&mut debugger, request),
            DebuggerReply::Error {
                code: DebuggerErrorCode::StaleRealm,
                ..
            }
        ));
    }

    let successor_realm = one_realm(debugger_request(
        &mut debugger,
        DebuggerRequest::ListPageRealms,
    ));
    assert_eq!(
        successor_realm.browser_context_id,
        first_realm.browser_context_id
    );
    assert_eq!(successor_realm.tab_id, first_realm.tab_id);
    assert_ne!(
        successor_realm.realm_generation, first_realm.realm_generation,
        "a public debugger realm cannot survive an HTTP document replacement"
    );
    let successor_program = one_program(
        debugger_request(
            &mut debugger,
            DebuggerRequest::ListPrograms {
                realm: successor_realm,
            },
        ),
        successor_realm,
    );
    let _ = safe_points(
        debugger_request(
            &mut debugger,
            DebuggerRequest::ListSafePoints {
                program: successor_program,
            },
        ),
        successor_program,
    );

    launcher.shutdown();
    assert!(
        !launcher.debugger_socket.exists(),
        "launcher shutdown must remove its public debugger endpoint"
    );
    assert!(
        !launcher.frame_dir.exists(),
        "launcher shutdown must remove its generation frame state"
    );
    let _ = std::fs::remove_file(gatekeeper_socket);
}

#[test]
fn launcher_reads_granted_classic_and_module_root_and_nested_bluets_values() {
    for (mime, slug) in [
        ("application/x-blueice-typescript", "classic"),
        ("application/x-blueice-typescript-module", "module"),
    ] {
        let gatekeeper_socket = clearing_gatekeeper();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let fixture = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0_u8; 1024];
            let _ = stream.read(&mut request);
            let body = format!(
                "<main>{slug}-bounded-values</main><script type=\"{mime}\">{BOUNDED_VALUE_BLUETS_SOURCE}</script>"
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
        let mut launcher = LauncherProcess::spawn_with_static_metadata_policy(
            &gatekeeper_socket,
            StaticMetadataPolicy {
                bounded_values: true,
                ..StaticMetadataPolicy::default()
            },
        );
        let mut browser = launcher.connect_browser();
        blueice_ipc::client_handshake(&mut browser).unwrap();
        navigate(&mut browser, &url);
        fixture.join().unwrap();

        let mut debugger = UnixStream::connect(&launcher.debugger_socket).unwrap();
        assert_eq!(
            debugger_request(
                &mut debugger,
                DebuggerRequest::Hello {
                    protocol_version: DEBUGGER_PROTOCOL_VERSION,
                    requested_bounded_values: true,
                    requested_metadata_capabilities: DebuggerMetadataCapabilityManifest::empty(),
                },
            ),
            DebuggerReply::HelloAck {
                protocol_version: DEBUGGER_PROTOCOL_VERSION,
                granted_bounded_values: true,
                granted_metadata_capabilities: DebuggerMetadataCapabilityManifest::empty(),
            }
        );
        let realm = one_realm(debugger_request(
            &mut debugger,
            DebuggerRequest::ListPageRealms,
        ));
        let DebuggerReply::Capabilities(capabilities) = debugger_request(
            &mut debugger,
            DebuggerRequest::DescribeCapabilities { realm },
        ) else {
            panic!("{slug} must report public debugger capabilities");
        };
        assert!(capabilities.reports.iter().any(|report| {
            report.capability == DebuggerCapability::BoundedValues
                && report.state == DebuggerCapabilityState::Available
        }));
        let (program, frame) = arm_first_nested_frame(&mut debugger, realm);
        let unavailable_source = DebuggerReply::Error {
            code: DebuggerErrorCode::CapabilityUnavailable,
            message: "debugger operation is unavailable".to_string(),
        };
        assert_eq!(
            debugger_request(&mut debugger, DebuggerRequest::GetSourceText { program }),
            unavailable_source
        );
        let foreign_program = DebuggerProgram {
            realm: DebuggerPageRealm {
                tab_id: realm.tab_id + 1,
                ..realm
            },
            ..program
        };
        assert_eq!(
            debugger_request(
                &mut debugger,
                DebuggerRequest::GetSourceText {
                    program: foreign_program,
                },
            ),
            unavailable_source
        );
        assert_eq!(
            debugger_request(&mut debugger, DebuggerRequest::Unknown),
            unavailable_source
        );
        assert_eq!(
            one_realm(debugger_request(
                &mut debugger,
                DebuggerRequest::ListPageRealms,
            )),
            realm,
            "denied source and unknown probes must preserve the public stream"
        );
        let mut nested_target = None;
        let mut parent_target = None;
        for _ in 0..96 {
            let DebuggerReply::Stack(stack) = debugger_request(
                &mut debugger,
                DebuggerRequest::GetStack {
                    program,
                    frame: Some(frame),
                    max_frames: 2,
                },
            ) else {
                panic!("{slug} nested frame must expose a bounded stack");
            };
            assert_eq!(stack.safe_points.len(), 2);
            let child_value = find_number_value(
                &mut debugger,
                program,
                Some(frame),
                0,
                stack.safe_points[0],
                3.0,
            );
            let caller_value = find_number_value(
                &mut debugger,
                program,
                Some(frame),
                1,
                stack.safe_points[1],
                9.0,
            );
            if child_value.is_some() && caller_value.is_some() {
                nested_target = child_value;
                parent_target = caller_value;
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
        let nested_target = nested_target.expect("nested and caller-root values must be readable");
        let parent_target = parent_target.expect("caller-root value must be readable");
        let static_target = blueice_ipc::debugger::DebuggerStaticScopeTarget::Ordinary {
            metadata: blueice_ipc::debugger::DebuggerStaticMetadataHandle {
                program,
                metadata_handle: 1,
                metadata_generation: 1,
            },
            target: parent_target,
        };
        assert!(static_target.is_well_formed());
        assert!(matches!(
            debugger_request(
                &mut debugger,
                DebuggerRequest::GetStaticScopeRelation {
                    target: static_target,
                },
            ),
            DebuggerReply::Error {
                code: DebuggerErrorCode::CapabilityUnavailable,
                ..
            }
        ));

        for guessed_program in [
            DebuggerProgram {
                program_handle: program.program_handle + 1,
                ..program
            },
            foreign_program,
        ] {
            let guessed = DebuggerValueTarget {
                program: guessed_program,
                frame: Some(blueice_ipc::debugger::DebuggerFrame {
                    program: guessed_program,
                    ..frame
                }),
                safe_point: DebuggerSafePoint {
                    program: guessed_program,
                    ..nested_target.safe_point
                },
                ..nested_target
            };
            assert!(guessed.is_well_formed());
            assert!(matches!(
                debugger_request(&mut debugger, DebuggerRequest::GetValue { target: guessed }),
                DebuggerReply::Error {
                    code: DebuggerErrorCode::InvalidTarget,
                    ..
                }
            ));
        }

        assert!(matches!(
            debugger_request(
                &mut debugger,
                DebuggerRequest::GetValue {
                    target: DebuggerValueTarget {
                        scope_entry: blueice_ipc::debugger::DebuggerScopeEntry {
                            slot_ordinal: nested_target.scope_entry.slot_ordinal + 1_000_000,
                            ..nested_target.scope_entry
                        },
                        ..nested_target
                    }
                },
            ),
            DebuggerReply::Error {
                code: DebuggerErrorCode::InvalidTarget,
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
        assert!(matches!(
            debugger_request(
                &mut debugger,
                DebuggerRequest::GetValue {
                    target: nested_target
                },
            ),
            DebuggerReply::Error {
                code: DebuggerErrorCode::InvalidTarget,
                ..
            }
        ));
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match debugger_request(
                &mut debugger,
                DebuggerRequest::GetExecutionState { program },
            ) {
                DebuggerReply::ExecutionState {
                    state: DebuggerExecutionState::Paused { .. },
                    ..
                } => break,
                DebuggerReply::ExecutionState {
                    state: DebuggerExecutionState::NestedResuming { frame: same },
                    ..
                } if same == frame && Instant::now() < deadline => {
                    thread::sleep(Duration::from_millis(10));
                }
                other => panic!("{slug} nested return must rejoin paused root: {other:?}"),
            }
        }
        let DebuggerReply::Stack(root_stack) = debugger_request(
            &mut debugger,
            DebuggerRequest::GetStack {
                program,
                frame: None,
                max_frames: 1,
            },
        ) else {
            panic!("{slug} returned root must expose its paused stack");
        };
        assert_eq!(root_stack.safe_points.len(), 1);
        let root_target = find_number_value(
            &mut debugger,
            program,
            None,
            0,
            root_stack.safe_points[0],
            9.0,
        )
        .expect("returned root must retain its own binding");
        drop(debugger);

        let mut separate = UnixStream::connect(&launcher.debugger_socket).unwrap();
        assert_eq!(
            debugger_request(
                &mut separate,
                DebuggerRequest::Hello {
                    protocol_version: DEBUGGER_PROTOCOL_VERSION,
                    requested_bounded_values: true,
                    requested_metadata_capabilities: DebuggerMetadataCapabilityManifest::empty(),
                },
            ),
            DebuggerReply::HelloAck {
                protocol_version: DEBUGGER_PROTOCOL_VERSION,
                granted_bounded_values: true,
                granted_metadata_capabilities: DebuggerMetadataCapabilityManifest::empty(),
            }
        );
        assert!(matches!(
            debugger_request(
                &mut separate,
                DebuggerRequest::GetExecutionState { program },
            ),
            DebuggerReply::ExecutionState {
                state: DebuggerExecutionState::Paused { .. },
                ..
            }
        ));
        assert!(matches!(
            debugger_request(
                &mut separate,
                DebuggerRequest::GetValue {
                    target: root_target
                }
            ),
            DebuggerReply::Error {
                code: DebuggerErrorCode::InvalidTarget,
                ..
            }
        ));
        drop(separate);

        let mut ungranted = UnixStream::connect(&launcher.debugger_socket).unwrap();
        assert!(matches!(
            debugger_request(
                &mut ungranted,
                DebuggerRequest::Hello {
                    protocol_version: DEBUGGER_PROTOCOL_VERSION,
                    requested_bounded_values: false,
                    requested_metadata_capabilities: DebuggerMetadataCapabilityManifest::empty(),
                },
            ),
            DebuggerReply::HelloAck {
                granted_bounded_values: false,
                ..
            }
        ));
        assert!(matches!(
            debugger_request(
                &mut ungranted,
                DebuggerRequest::GetValue {
                    target: root_target
                }
            ),
            DebuggerReply::Error {
                code: DebuggerErrorCode::CapabilityUnavailable,
                ..
            }
        ));
        drop(ungranted);
        launcher.shutdown();
        let _ = std::fs::remove_file(gatekeeper_socket);
    }
}

#[test]
fn launcher_refuses_a_non_plain_bluets_value_without_a_partial_preview() {
    let gatekeeper_socket = clearing_gatekeeper();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let fixture = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0_u8; 1024];
        let _ = stream.read(&mut request);
        let source = "function inner(bad: any, good: number): number { return good; } globalThis.answer = inner(inner, 7);";
        let body = format!(
            "<main>non-plain-value</main><script type=\"application/x-blueice-typescript\">{source}</script>"
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
    let mut launcher = LauncherProcess::spawn_with_static_metadata_policy(
        &gatekeeper_socket,
        StaticMetadataPolicy {
            bounded_values: true,
            ..StaticMetadataPolicy::default()
        },
    );
    let mut browser = launcher.connect_browser();
    blueice_ipc::client_handshake(&mut browser).unwrap();
    navigate(&mut browser, &url);
    fixture.join().unwrap();

    let mut debugger = UnixStream::connect(&launcher.debugger_socket).unwrap();
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::Hello {
                protocol_version: DEBUGGER_PROTOCOL_VERSION,
                requested_bounded_values: true,
                requested_metadata_capabilities: DebuggerMetadataCapabilityManifest::empty(),
            },
        ),
        DebuggerReply::HelloAck {
            granted_bounded_values: true,
            ..
        }
    ));
    let realm = one_realm(debugger_request(
        &mut debugger,
        DebuggerRequest::ListPageRealms,
    ));
    let (program, frame) = arm_first_nested_frame(&mut debugger, realm);
    let mut checked = false;
    for _ in 0..64 {
        let DebuggerReply::Stack(stack) = debugger_request(
            &mut debugger,
            DebuggerRequest::GetStack {
                program,
                frame: Some(frame),
                max_frames: 2,
            },
        ) else {
            panic!("the non-plain fixture must expose its nested frame");
        };
        let safe_point = stack.safe_points[0];
        let DebuggerReply::Scopes(scopes) = debugger_request(
            &mut debugger,
            DebuggerRequest::GetScopes {
                program,
                frame: Some(frame),
                frame_index: 0,
                expected_safe_point: safe_point,
                max_scope_entries: 256,
            },
        ) else {
            panic!("the non-plain fixture must expose exact parameter slots");
        };
        let replies: Vec<_> = scopes
            .entries
            .iter()
            .copied()
            .map(|scope_entry| {
                debugger_request(
                    &mut debugger,
                    DebuggerRequest::GetValue {
                        target: DebuggerValueTarget {
                            program,
                            frame: Some(frame),
                            frame_index: 0,
                            safe_point,
                            scope_entry,
                        },
                    },
                )
            })
            .collect();
        let good_is_ready = replies.iter().any(|reply| {
            matches!(
                reply,
                DebuggerReply::Value(snapshot)
                    if snapshot.preview == DebuggerValuePreview::NumberBits(7.0_f64.to_bits())
            )
        });
        if good_is_ready {
            assert!(
                replies.iter().any(|reply| matches!(
                    reply,
                    DebuggerReply::Error {
                        code: DebuggerErrorCode::InvalidExecutionState,
                        ..
                    }
                )),
                "the initialized function argument must refuse as a whole: {replies:?}"
            );
            checked = true;
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
    assert!(
        checked,
        "the neighboring plain argument must become readable"
    );
    drop(debugger);
    launcher.shutdown();
    let _ = std::fs::remove_file(gatekeeper_socket);
}

#[test]
fn launcher_refuses_every_bounded_value_budget_on_a_real_bluets_socket() {
    let gatekeeper_socket = clearing_gatekeeper();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let fixture = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0_u8; 1024];
        let _ = stream.read(&mut request);
        let edge_array = vec!["1"; 32].join(",");
        let over_nodes = ["row"; 9].join(",");
        let edge_nodes = format!("{},shortRow", ["row"; 7].join(","));
        let short_row = vec!["1"; 23].join(",");
        let source = format!(
            "let row = [{edge_array}]; let shortRow = [{short_row}]; let overNodes = [{over_nodes}]; let edgeNodes = [{edge_nodes}]; function inner(depth: any, length: any, nodes: any, bytes: any, edgeDepth: any, edgeLength: any, edgeNodeCount: any, edgeBytes: any, good: number): number {{ return good; }} globalThis.answer = inner([[[[[1]]]]], new Array(33), overNodes, 'x'.repeat(2049), [[[[1]]]], row, edgeNodes, 'x'.repeat(2048), 7);"
        );
        let body = format!(
            "<main>bounded-value-budgets</main><script type=\"application/x-blueice-typescript\">{source}</script>"
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
    let mut launcher = LauncherProcess::spawn_with_static_metadata_policy(
        &gatekeeper_socket,
        StaticMetadataPolicy {
            bounded_values: true,
            ..StaticMetadataPolicy::default()
        },
    );
    let mut browser = launcher.connect_browser();
    blueice_ipc::client_handshake(&mut browser).unwrap();
    navigate(&mut browser, &url);
    fixture.join().unwrap();

    let mut debugger = UnixStream::connect(&launcher.debugger_socket).unwrap();
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::Hello {
                protocol_version: DEBUGGER_PROTOCOL_VERSION,
                requested_bounded_values: true,
                requested_metadata_capabilities: DebuggerMetadataCapabilityManifest::empty(),
            },
        ),
        DebuggerReply::HelloAck {
            granted_bounded_values: true,
            ..
        }
    ));
    let realm = one_realm(debugger_request(
        &mut debugger,
        DebuggerRequest::ListPageRealms,
    ));
    let (program, frame) = arm_first_nested_frame(&mut debugger, realm);
    let replies = {
        let mut result = None;
        for _ in 0..96 {
            let DebuggerReply::Stack(stack) = debugger_request(
                &mut debugger,
                DebuggerRequest::GetStack {
                    program,
                    frame: Some(frame),
                    max_frames: 2,
                },
            ) else {
                panic!("the budget fixture must expose its paused nested frame");
            };
            let safe_point = stack.safe_points[0];
            let DebuggerReply::Scopes(scopes) = debugger_request(
                &mut debugger,
                DebuggerRequest::GetScopes {
                    program,
                    frame: Some(frame),
                    frame_index: 0,
                    expected_safe_point: safe_point,
                    max_scope_entries: 256,
                },
            ) else {
                panic!("the budget fixture must expose exact parameter slots");
            };
            if scopes.entries.len() >= 9 {
                let replies: Vec<_> = scopes
                    .entries
                    .iter()
                    .copied()
                    .map(|scope_entry| {
                        debugger_request(
                            &mut debugger,
                            DebuggerRequest::GetValue {
                                target: DebuggerValueTarget {
                                    program,
                                    frame: Some(frame),
                                    frame_index: 0,
                                    safe_point,
                                    scope_entry,
                                },
                            },
                        )
                    })
                    .collect();
                if replies.iter().any(|reply| {
                    matches!(reply, DebuggerReply::Value(snapshot)
                        if snapshot.preview == DebuggerValuePreview::NumberBits(7.0_f64.to_bits()))
                }) {
                    result = Some(replies);
                    break;
                }
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
        result.expect("the initialized in-budget nested parameter must become readable")
    };
    assert_eq!(
        replies
            .iter()
            .filter(|reply| matches!(
                reply,
                DebuggerReply::Error {
                    code: DebuggerErrorCode::InvalidExecutionState,
                    ..
                }
            ))
            .count(),
        4,
        "each of the depth, length, node, and byte excesses must refuse: {replies:?}"
    );
    let one = DebuggerValuePreview::NumberBits(1.0_f64.to_bits());
    let mut edge_depth = one.clone();
    for _ in 0..4 {
        edge_depth = DebuggerValuePreview::Array(vec![Some(edge_depth)]);
    }
    let edge_length = DebuggerValuePreview::Array(vec![Some(one.clone()); 32]);
    let mut edge_rows = vec![Some(edge_length.clone()); 7];
    edge_rows.push(Some(DebuggerValuePreview::Array(vec![Some(one); 23])));
    let edge_nodes = DebuggerValuePreview::Array(edge_rows);
    let edge_bytes = DebuggerValuePreview::StringUnits(vec![u16::from(b'x'); 2_048]);
    for (budget, expected) in [
        ("depth", edge_depth),
        ("length", edge_length),
        ("nodes", edge_nodes),
        ("bytes", edge_bytes),
    ] {
        assert!(expected.is_well_formed(), "{budget} boundary must be valid");
        assert!(
            replies
                .iter()
                .any(|reply| matches!(reply, DebuggerReply::Value(snapshot)
                if snapshot.preview == expected && snapshot.is_well_formed())),
            "the exact {budget} boundary must remain readable"
        );
    }
    assert!(
        replies.iter().any(|reply| matches!(
            reply,
            DebuggerReply::Value(snapshot)
                if snapshot.preview == DebuggerValuePreview::NumberBits(7.0_f64.to_bits())
                    && snapshot.is_well_formed()
        )),
        "the neighboring in-budget value must remain readable: {replies:?}"
    );
    drop(debugger);
    launcher.shutdown();
    let _ = std::fs::remove_file(gatekeeper_socket);
}
