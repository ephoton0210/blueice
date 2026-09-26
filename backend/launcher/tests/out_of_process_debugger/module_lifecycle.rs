// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn launcher_inventories_a_pending_bluets_module_before_execution() {
    let gatekeeper_socket = clearing_gatekeeper();
    let listener = TcpListener::bind("127.0.0.1:0").expect("local HTTP fixture must bind");
    let url = format!("http://{}", listener.local_addr().unwrap());
    let fixture = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("fixture must receive navigation");
        let mut request = [0_u8; 1024];
        let _ = stream.read(&mut request);
        let body = format!(
            "<main>pending-module</main><script type=\"application/x-blueice-typescript-module\">{MODULE_BLUETS_SOURCE}</script>"
        );
        stream
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            )
            .expect("fixture must reply with a module document");
    });
    let mut launcher = LauncherProcess::spawn(&gatekeeper_socket);
    let mut browser = launcher.connect_browser();
    blueice_ipc::client_handshake(&mut browser).expect("public browser handshake must succeed");
    navigate(&mut browser, &url);

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
    let realm = one_realm(debugger_request(
        &mut debugger,
        DebuggerRequest::ListPageRealms,
    ));
    let program = one_program(
        debugger_request(&mut debugger, DebuggerRequest::ListPrograms { realm }),
        realm,
    );
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::GetExecutionState { program }
        ),
        DebuggerReply::ExecutionState {
            program,
            state: blueice_ipc::debugger::DebuggerExecutionState::Pending,
        }
    );
    let points = safe_points(
        debugger_request(&mut debugger, DebuggerRequest::ListSafePoints { program }),
        program,
    );
    assert!(points.iter().any(|point| point.code_unit_ordinal == 0));

    blueice_ipc::write_client_message(&mut browser, &blueice_ipc::ClientMessage::GetDom).unwrap();
    assert!(matches!(
        read_server_message(&mut browser).unwrap(),
        ServerMessage::Dom(_)
    ));
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::GetExecutionState { program }
        ),
        DebuggerReply::ExecutionState {
            program,
            state: blueice_ipc::debugger::DebuggerExecutionState::Completed,
        }
    );

    fixture.join().unwrap();
    launcher.shutdown();
    let _ = std::fs::remove_file(gatekeeper_socket);
}

#[test]
fn launcher_pauses_and_resumes_a_real_bluets_module_entry() {
    let gatekeeper_socket = clearing_gatekeeper();
    let listener = TcpListener::bind("127.0.0.1:0").expect("local HTTP fixture must bind");
    let url = format!("http://{}", listener.local_addr().unwrap());
    let fixture = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("fixture must receive navigation");
        let mut request = [0_u8; 1024];
        let _ = stream.read(&mut request);
        let body = format!(
            "<main>module-entry-debugger</main><script type=\"application/x-blueice-typescript-module\">{MODULE_BLUETS_SOURCE}</script>"
        );
        stream
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            )
            .expect("fixture must reply with the module document");
    });
    let mut launcher = LauncherProcess::spawn(&gatekeeper_socket);
    let mut browser = launcher.connect_browser();
    blueice_ipc::client_handshake(&mut browser).expect("public browser handshake must succeed");
    navigate(&mut browser, &url);

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
    let realm = one_realm(debugger_request(
        &mut debugger,
        DebuggerRequest::ListPageRealms,
    ));
    let DebuggerReply::Programs(programs) =
        debugger_request(&mut debugger, DebuggerRequest::ListPrograms { realm })
    else {
        panic!("expected the module and following classic programs");
    };
    if programs.len() != 1 {
        write_client_message(&mut browser, &ClientMessage::GetBlueTsScriptReports).unwrap();
        let bluets = read_server_message(&mut browser).unwrap();
        write_client_message(&mut browser, &ClientMessage::GetBlueJsScriptReports).unwrap();
        let bluejs = read_server_message(&mut browser).unwrap();
        panic!("expected one module program, got {programs:?}; {bluets:?}; {bluejs:?}");
    }
    let program = programs[0];
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::GetExecutionState { program }
        ),
        DebuggerReply::ExecutionState {
            program,
            state: blueice_ipc::debugger::DebuggerExecutionState::Pending,
        }
    );
    let points = safe_points(
        debugger_request(&mut debugger, DebuggerRequest::ListSafePoints { program }),
        program,
    );
    let mut armed = None;
    for point in points
        .into_iter()
        .filter(|point| point.code_unit_ordinal == 0)
    {
        match debugger_request(
            &mut debugger,
            DebuggerRequest::ArmRootSafePointBreakpoint { safe_point: point },
        ) {
            DebuggerReply::RootSafePointBreakpointArmed { safe_point } => {
                armed = Some(safe_point);
                break;
            }
            DebuggerReply::Error {
                code: DebuggerErrorCode::InvalidSafePoint | DebuggerErrorCode::InvalidExecutionState,
                ..
            } => {}
            reply => panic!("unexpected module arm reply: {reply:?}"),
        }
    }
    let target = armed.expect("the first entry evaluate-body root point must arm");
    await_paused_execution(&mut debugger, program, target);
    assert_eq!(
        debugger_request(&mut debugger, DebuggerRequest::ResumeExecution { program }),
        DebuggerReply::ExecutionResumed { program }
    );
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::GetExecutionState { program }
        ),
        DebuggerReply::ExecutionState {
            program,
            state: blueice_ipc::debugger::DebuggerExecutionState::Resuming,
        }
    );
    await_completed_execution(&mut debugger, program);
    write_client_message(&mut browser, &ClientMessage::GetBlueTsScriptReports).unwrap();
    assert!(matches!(
        read_server_message(&mut browser).unwrap(),
        ServerMessage::BlueTsScriptReports(reports)
            if reports.len() == 1
                && reports[0].kind == blueice_ipc::BlueTsScriptKind::Module
                && reports[0].outcome == blueice_ipc::BlueTsScriptExecutionOutcome::Executed
    ));
    write_client_message(&mut browser, &ClientMessage::GetDom).unwrap();
    assert!(matches!(
        read_server_message(&mut browser).unwrap(),
        ServerMessage::Dom(dom) if format!("{dom:?}").contains("module-entry-debugger")
    ));

    fixture.join().unwrap();
    launcher.shutdown();
    let _ = std::fs::remove_file(gatekeeper_socket);
}

#[test]
fn launcher_steps_a_real_bluets_module_then_rejects_stale_generation() {
    use blueice_ipc::debugger::DebuggerStaticMetadataSafePointSpanTarget;

    let gatekeeper_socket = clearing_gatekeeper();
    let listener = TcpListener::bind("127.0.0.1:0").expect("local HTTP fixture must bind");
    let url = format!("http://{}", listener.local_addr().unwrap());
    let fixture = thread::spawn(move || {
        for _ in 0..2 {
            let (mut stream, _) = listener.accept().expect("fixture must receive navigation");
            let mut request = [0_u8; 1024];
            let _ = stream.read(&mut request);
            let body = format!(
                "<main>module-step-debugger</main><script type=\"application/x-blueice-typescript-module\">{MODULE_STEP_BLUETS_SOURCE}</script>"
            );
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                )
                .expect("fixture must reply with the module document");
        }
    });
    let mut launcher = LauncherProcess::spawn_with_static_metadata_policy(
        &gatekeeper_socket,
        StaticMetadataPolicy {
            inventory: true,
            source_inventory: true,
            safe_point_span: true,
            source_span_step: true,
            ..StaticMetadataPolicy::default()
        },
    );
    let mut browser = launcher.connect_browser();
    blueice_ipc::client_handshake(&mut browser).expect("public browser handshake must succeed");

    let manifest = DebuggerMetadataCapabilityManifest::opaque_source_span_step();
    let mut debugger = UnixStream::connect(&launcher.debugger_socket).unwrap();
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::Hello {
                protocol_version: DEBUGGER_PROTOCOL_VERSION,
                requested_bounded_values: false,
                requested_metadata_capabilities: manifest.clone(),
            },
        ),
        DebuggerReply::HelloAck {
            protocol_version: DEBUGGER_PROTOCOL_VERSION,
            granted_bounded_values: false,
            granted_metadata_capabilities: manifest,
        }
    );
    navigate(&mut browser, &url);
    let realm = one_realm(debugger_request(
        &mut debugger,
        DebuggerRequest::ListPageRealms,
    ));
    let program = one_program(
        debugger_request(&mut debugger, DebuggerRequest::ListPrograms { realm }),
        realm,
    );
    let DebuggerReply::StaticMetadata(metadata) = debugger_request(
        &mut debugger,
        DebuggerRequest::ListStaticMetadata { program },
    ) else {
        panic!("the BlueTS module needs one opaque metadata attachment");
    };
    let metadata = metadata[0];
    let DebuggerReply::StaticMetadataSources(sources) = debugger_request(
        &mut debugger,
        DebuggerRequest::ListStaticMetadataSources { metadata },
    ) else {
        panic!("the same debugger stream must receive source IDs");
    };
    let points = safe_points(
        debugger_request(&mut debugger, DebuggerRequest::ListSafePoints { program }),
        program,
    );
    let mut armed = None;
    for point in points
        .iter()
        .copied()
        .filter(|point| point.code_unit_ordinal == 0)
    {
        match debugger_request(
            &mut debugger,
            DebuggerRequest::ArmRootSafePointBreakpoint { safe_point: point },
        ) {
            DebuggerReply::RootSafePointBreakpointArmed { safe_point } => {
                armed = Some(safe_point);
                break;
            }
            DebuggerReply::Error {
                code: DebuggerErrorCode::InvalidSafePoint | DebuggerErrorCode::InvalidExecutionState,
                ..
            } => {}
            reply => panic!("unexpected module arm reply: {reply:?}"),
        }
    }
    let target = armed.expect("the module entry evaluate-body point must arm");
    await_paused_execution(&mut debugger, program, target);
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::StepRootInstruction { program }
        ),
        DebuggerReply::ExecutionStepRequested { program }
    );
    let mut current = await_step_paused_execution(&mut debugger, program, target);
    assert!(points.contains(&current));
    let bound_span = |debugger: &mut UnixStream, point: DebuggerSafePoint| {
        sources.iter().find_map(|source| {
            let target = DebuggerStaticMetadataSafePointSpanTarget {
                safe_point: point,
                source: *source,
            };
            match debugger_request(
                debugger,
                DebuggerRequest::DescribeStaticMetadataSafePointSpan { target },
            ) {
                DebuggerReply::StaticMetadataSafePointSpan(span) => Some((target, span)),
                _ => None,
            }
        })
    };
    let mut origin = None;
    for _ in 0..128 {
        if let Some(span) = bound_span(&mut debugger, current) {
            origin = Some(span);
            break;
        }
        assert_eq!(
            debugger_request(
                &mut debugger,
                DebuggerRequest::StepRootInstruction { program }
            ),
            DebuggerReply::ExecutionStepRequested { program }
        );
        current = await_step_paused_execution(&mut debugger, program, current);
        assert!(points.contains(&current));
    }
    let (source_target, original_span) =
        origin.expect("module instruction step must reach a bound span");
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::StepStaticMetadataSourceSpan {
                target: source_target,
            },
        ),
        DebuggerReply::ExecutionSourceSpanStepRequested {
            safe_point: current,
        }
    );
    let successor = await_step_paused_execution(&mut debugger, program, current);
    assert!(points.contains(&successor));
    let (_, successor_span) = bound_span(&mut debugger, successor)
        .expect("source step stops at another bound module span");
    assert_ne!(
        (original_span.start_byte, original_span.end_byte),
        (successor_span.start_byte, successor_span.end_byte)
    );
    assert_eq!(
        debugger_request(&mut debugger, DebuggerRequest::ResumeExecution { program }),
        DebuggerReply::ExecutionResumed { program }
    );
    await_completed_execution(&mut debugger, program);
    write_client_message(&mut browser, &ClientMessage::GetBlueTsScriptReports).unwrap();
    assert!(matches!(
        read_server_message(&mut browser).unwrap(),
        ServerMessage::BlueTsScriptReports(reports)
            if reports.len() == 1
                && reports[0].kind == blueice_ipc::BlueTsScriptKind::Module
                && reports[0].outcome == blueice_ipc::BlueTsScriptExecutionOutcome::Executed
    ));

    navigate(&mut browser, &url);
    fixture.join().unwrap();
    for request in [
        DebuggerRequest::ArmRootSafePointBreakpoint { safe_point: target },
        DebuggerRequest::StepRootInstruction { program },
        DebuggerRequest::StepStaticMetadataSourceSpan {
            target: source_target,
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
    assert_eq!(successor_realm.tab_id, realm.tab_id);
    assert_ne!(successor_realm.realm_generation, realm.realm_generation);
    let _ = one_program(
        debugger_request(
            &mut debugger,
            DebuggerRequest::ListPrograms {
                realm: successor_realm,
            },
        ),
        successor_realm,
    );
    launcher.shutdown();
    let _ = std::fs::remove_file(gatekeeper_socket);
}

#[test]
fn launcher_exposes_bluets_metadata_while_its_root_frame_is_pending_and_paused() {
    let gatekeeper_socket = clearing_gatekeeper();
    let listener = TcpListener::bind("127.0.0.1:0").expect("local HTTP fixture must bind");
    let url = format!("http://{}", listener.local_addr().unwrap());
    let fixture = serve_two_bluets_documents(listener);
    let mut launcher = LauncherProcess::spawn_with_static_metadata_policy(
        &gatekeeper_socket,
        StaticMetadataPolicy {
            inventory: true,
            summary: true,
            ..StaticMetadataPolicy::default()
        },
    );

    let mut browser = launcher.connect_browser();
    blueice_ipc::client_handshake(&mut browser).expect("public browser handshake must succeed");

    let mut debugger = UnixStream::connect(&launcher.debugger_socket)
        .expect("launcher public debugger endpoint must accept a peer");
    let metadata_capabilities =
        DebuggerMetadataCapabilityManifest::opaque_selected(DebuggerMetadataCapabilitySelection {
            summary: true,
            ..DebuggerMetadataCapabilitySelection::default()
        });
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::Hello {
                protocol_version: DEBUGGER_PROTOCOL_VERSION,
                requested_bounded_values: false,
                requested_metadata_capabilities: metadata_capabilities.clone(),
            },
        ),
        DebuggerReply::HelloAck {
            protocol_version: DEBUGGER_PROTOCOL_VERSION,
            granted_bounded_values: false,
            granted_metadata_capabilities: metadata_capabilities,
        }
    );
    navigate(&mut browser, &url);
    let realm = one_realm(debugger_request(
        &mut debugger,
        DebuggerRequest::ListPageRealms,
    ));
    let program = one_program(
        debugger_request(&mut debugger, DebuggerRequest::ListPrograms { realm }),
        realm,
    );
    let DebuggerReply::StaticMetadata(metadata) = debugger_request(
        &mut debugger,
        DebuggerRequest::ListStaticMetadata { program },
    ) else {
        panic!("pending BlueTS program must expose an authorized opaque metadata handle")
    };
    assert_eq!(metadata.len(), 1);
    let metadata = metadata[0];
    let DebuggerReply::StaticMetadataSummary(summary) = debugger_request(
        &mut debugger,
        DebuggerRequest::DescribeStaticMetadata { metadata },
    ) else {
        panic!("pending BlueTS program must expose an authorized bounded summary")
    };
    assert_eq!(summary.metadata, metadata);
    assert!(summary.symbol_count > 0);
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::GetExecutionState { program },
        ),
        DebuggerReply::ExecutionState {
            program,
            state: blueice_ipc::debugger::DebuggerExecutionState::Pending,
        }
    );
    let target = *safe_points(
        debugger_request(&mut debugger, DebuggerRequest::ListSafePoints { program }),
        program,
    )
    .iter()
    .find(|point| point.code_unit_ordinal == 0 && point.bytecode_offset != 0)
    .expect("typed classic program must have a non-entry root safe point");
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::ArmRootSafePointBreakpoint { safe_point: target },
        ),
        DebuggerReply::RootSafePointBreakpointArmed { safe_point: target }
    );
    await_paused_execution(&mut debugger, program, target);
    let paused_summary = debugger_request(
        &mut debugger,
        DebuggerRequest::DescribeStaticMetadata { metadata },
    );
    assert_eq!(
        paused_summary,
        DebuggerReply::StaticMetadataSummary(summary)
    );
    assert!(!format!("{paused_summary:?}").contains("PrivateContract"));
    assert!(!format!("{paused_summary:?}").contains("privateBlueTsMetadata"));
    assert_eq!(
        debugger_request(&mut debugger, DebuggerRequest::ResumeExecution { program }),
        DebuggerReply::ExecutionResumed { program }
    );
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::GetExecutionState { program },
        ),
        DebuggerReply::ExecutionState {
            program,
            state: blueice_ipc::debugger::DebuggerExecutionState::Resuming,
        }
    );
    await_completed_execution(&mut debugger, program);

    navigate(&mut browser, &url);
    fixture.join().expect("fixture must serve both documents");
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::DescribeStaticMetadata { metadata },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::StaleRealm,
            ..
        }
    ));
    launcher.shutdown();
    let _ = std::fs::remove_file(gatekeeper_socket);
}
