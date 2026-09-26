// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn launcher_resolves_receipted_bluets_source_positions_to_live_breakpoints() {
    let gatekeeper_socket = clearing_gatekeeper();
    let listener = TcpListener::bind("127.0.0.1:0").expect("local HTTP fixture must bind");
    let url = format!("http://{}", listener.local_addr().unwrap());
    let fixture = serve_two_bluets_documents(listener);
    let mut launcher = LauncherProcess::spawn_with_static_metadata_policy(
        &gatekeeper_socket,
        StaticMetadataPolicy {
            inventory: true,
            source_inventory: true,
            source_breakpoint: true,
            ..StaticMetadataPolicy::default()
        },
    );
    let mut browser = launcher.connect_browser();
    blueice_ipc::client_handshake(&mut browser).expect("public browser handshake must succeed");

    let manifest = DebuggerMetadataCapabilityManifest::opaque_source_breakpoint();
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
            granted_metadata_capabilities: manifest.clone(),
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
    let DebuggerReply::Capabilities(capabilities) = debugger_request(
        &mut debugger,
        DebuggerRequest::DescribeCapabilities { realm },
    ) else {
        panic!("the live child must report source-breakpoint capabilities");
    };
    assert!(capabilities.reports.iter().any(|report| {
        report.capability
            == blueice_ipc::debugger::DebuggerCapability::StaticMetadataSourceBreakpoint
            && report.state == blueice_ipc::debugger::DebuggerCapabilityState::Available
    }));
    assert!(capabilities.reports.iter().any(|report| {
        report.capability == blueice_ipc::debugger::DebuggerCapability::StaticMetadataSafePointSpan
            && report.state == blueice_ipc::debugger::DebuggerCapabilityState::Planned
    }));
    let DebuggerReply::StaticMetadata(metadata) = debugger_request(
        &mut debugger,
        DebuggerRequest::ListStaticMetadata { program },
    ) else {
        panic!("the BlueTS program must have one opaque metadata attachment");
    };
    let metadata = metadata[0];
    let source_byte = u32::try_from(
        FIRST_BLUETS_SOURCE
            .find("const privateBlueTsMetadata")
            .expect("fixture must contain its executable TypeScript declaration"),
    )
    .unwrap();
    let guessed = blueice_ipc::debugger::DebuggerStaticMetadataSourceBreakpointTarget {
        source: blueice_ipc::debugger::DebuggerStaticMetadataSourceId {
            metadata,
            source_id: 0,
        },
        source_byte,
    };
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::ResolveStaticMetadataSourceBreakpoint { target: guessed },
        ),
        DebuggerReply::Unsupported { .. }
    ));
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::ArmStaticMetadataSourceBreakpoint { target: guessed },
        ),
        DebuggerReply::Unsupported { .. }
    ));
    let DebuggerReply::StaticMetadataSources(sources) = debugger_request(
        &mut debugger,
        DebuggerRequest::ListStaticMetadataSources { metadata },
    ) else {
        panic!("the same stream must receive compiler source IDs");
    };
    let mut armed = None;
    for source in sources {
        let candidate = blueice_ipc::debugger::DebuggerStaticMetadataSourceBreakpointTarget {
            source,
            source_byte,
        };
        match debugger_request(
            &mut debugger,
            DebuggerRequest::ArmStaticMetadataSourceBreakpoint { target: candidate },
        ) {
            DebuggerReply::RootSafePointBreakpointArmed { safe_point } => {
                armed = Some((candidate, safe_point));
                break;
            }
            DebuggerReply::Error {
                code: DebuggerErrorCode::InvalidSafePoint | DebuggerErrorCode::InvalidTarget,
                ..
            } => assert_eq!(
                debugger_request(
                    &mut debugger,
                    DebuggerRequest::GetExecutionState { program }
                ),
                DebuggerReply::ExecutionState {
                    program,
                    state: blueice_ipc::debugger::DebuggerExecutionState::Pending,
                },
                "an unbound source must leave the classic declaration pending"
            ),
            reply => panic!("unexpected atomic source-breakpoint arm reply: {reply:?}"),
        }
    }
    let (target, safe_point) = armed.expect("one original BlueTS source must arm its root point");
    assert_eq!(safe_point.program, program);
    assert_eq!(safe_point.code_unit_ordinal, 0);
    assert_ne!(safe_point.bytecode_offset, 0);
    await_paused_execution(&mut debugger, program, safe_point);
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::ResolveStaticMetadataSourceBreakpoint {
                target: blueice_ipc::debugger::DebuggerStaticMetadataSourceBreakpointTarget {
                    source_byte: FIRST_BLUETS_SOURCE.len() as u32,
                    ..target
                },
            },
        ),
        DebuggerReply::StaticMetadataSourceBreakpoint(
            blueice_ipc::debugger::DebuggerStaticMetadataSourceBreakpoint {
                target: blueice_ipc::debugger::DebuggerStaticMetadataSourceBreakpointTarget {
                    source_byte: FIRST_BLUETS_SOURCE.len() as u32,
                    ..target
                },
                safe_point: None,
            }
        )
    );
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::ResolveStaticMetadataSourceBreakpoint {
                target: blueice_ipc::debugger::DebuggerStaticMetadataSourceBreakpointTarget {
                    source: blueice_ipc::debugger::DebuggerStaticMetadataSourceId {
                        source_id: u32::MAX,
                        ..target.source
                    },
                    ..target
                },
            },
        ),
        DebuggerReply::Unsupported { .. }
    ));
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::ArmStaticMetadataSourceBreakpoint {
                target: blueice_ipc::debugger::DebuggerStaticMetadataSourceBreakpointTarget {
                    source_byte: FIRST_BLUETS_SOURCE.len() as u32,
                    ..target
                },
            },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidSafePoint,
            ..
        }
    ));
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::GetExecutionState { program }
        ),
        DebuggerReply::ExecutionState {
            program,
            state: blueice_ipc::debugger::DebuggerExecutionState::Paused { safe_point },
        },
        "an unbound arm must not release the paused root frame"
    );
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::ArmStaticMetadataSourceBreakpoint { target },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidExecutionState,
            ..
        }
    ));
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::ResolveStaticMetadataSourceBreakpoint { target },
        ),
        DebuggerReply::StaticMetadataSourceBreakpoint(
            blueice_ipc::debugger::DebuggerStaticMetadataSourceBreakpoint {
                target,
                safe_point: Some(safe_point),
            }
        )
    );
    assert_eq!(
        debugger_request(&mut debugger, DebuggerRequest::ResumeExecution { program }),
        DebuggerReply::ExecutionResumed { program }
    );
    await_completed_execution(&mut debugger, program);

    navigate(&mut browser, &url);
    fixture.join().expect("fixture must serve both documents");
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::ResolveStaticMetadataSourceBreakpoint { target },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::StaleRealm,
            ..
        }
    ));
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::ArmStaticMetadataSourceBreakpoint { target },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::StaleRealm,
            ..
        }
    ));
    drop(debugger);
    let mut separate = UnixStream::connect(&launcher.debugger_socket).unwrap();
    assert!(matches!(
        debugger_request(
            &mut separate,
            DebuggerRequest::Hello {
                protocol_version: DEBUGGER_PROTOCOL_VERSION,
                requested_bounded_values: false,
                requested_metadata_capabilities: manifest,
            },
        ),
        DebuggerReply::HelloAck { .. }
    ));
    assert!(matches!(
        debugger_request(
            &mut separate,
            DebuggerRequest::ResolveStaticMetadataSourceBreakpoint { target },
        ),
        DebuggerReply::Unsupported { .. }
    ));
    assert!(matches!(
        debugger_request(
            &mut separate,
            DebuggerRequest::ArmStaticMetadataSourceBreakpoint { target },
        ),
        DebuggerReply::Unsupported { .. }
    ));
    drop(separate);
    launcher.shutdown();
    let _ = std::fs::remove_file(gatekeeper_socket);
}

#[test]
fn launcher_steps_only_receipted_paused_bluets_source_spans() {
    use blueice_ipc::debugger::{
        DebuggerCapability, DebuggerCapabilityState, DebuggerExecutionState,
        DebuggerStaticMetadataSafePointSpanTarget, DebuggerStaticMetadataSourceId,
    };

    let gatekeeper_socket = clearing_gatekeeper();
    let listener = TcpListener::bind("127.0.0.1:0").expect("local HTTP fixture must bind");
    let url = format!("http://{}", listener.local_addr().unwrap());
    let fixture = serve_two_bluets_source_step_documents(listener);
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
            granted_metadata_capabilities: manifest.clone(),
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
    let points = safe_points(
        debugger_request(&mut debugger, DebuggerRequest::ListSafePoints { program }),
        program,
    );
    let arm_point = points
        .iter()
        .copied()
        .find(|point| point.code_unit_ordinal == 0 && point.bytecode_offset != 0)
        .expect("source-step fixture must expose an executable root point");
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::ArmRootSafePointBreakpoint {
                safe_point: arm_point,
            },
        ),
        DebuggerReply::RootSafePointBreakpointArmed {
            safe_point: arm_point,
        }
    );
    await_paused_execution(&mut debugger, program, arm_point);
    let DebuggerReply::Capabilities(capabilities) = debugger_request(
        &mut debugger,
        DebuggerRequest::DescribeCapabilities { realm },
    ) else {
        panic!("the live child must report debugger capabilities");
    };
    assert!(capabilities.reports.iter().any(|report| {
        report.capability == DebuggerCapability::StaticMetadataSourceSpanStep
            && report.state == DebuggerCapabilityState::Available
    }));
    let DebuggerReply::StaticMetadata(metadata) = debugger_request(
        &mut debugger,
        DebuggerRequest::ListStaticMetadata { program },
    ) else {
        panic!("BlueTS program must have metadata");
    };
    let metadata = metadata[0];
    let guessed = DebuggerStaticMetadataSafePointSpanTarget {
        safe_point: points[0],
        source: DebuggerStaticMetadataSourceId {
            metadata,
            source_id: 0,
        },
    };
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::StepStaticMetadataSourceSpan { target: guessed },
        ),
        DebuggerReply::Unsupported { .. }
    ));
    let DebuggerReply::StaticMetadataSources(sources) = debugger_request(
        &mut debugger,
        DebuggerRequest::ListStaticMetadataSources { metadata },
    ) else {
        panic!("the stream must receive source receipts");
    };
    let mut mapped = Vec::new();
    for safe_point in points {
        if safe_point.code_unit_ordinal != 0 || safe_point.bytecode_offset == 0 {
            continue;
        }
        for source in &sources {
            let target = DebuggerStaticMetadataSafePointSpanTarget {
                safe_point,
                source: *source,
            };
            if let DebuggerReply::StaticMetadataSafePointSpan(span) = debugger_request(
                &mut debugger,
                DebuggerRequest::DescribeStaticMetadataSafePointSpan { target },
            ) {
                mapped.push((target, span));
                break;
            }
        }
    }
    let (target, original_span) = mapped
        .iter()
        .find(|(candidate, span)| {
            candidate.safe_point == arm_point
                && mapped.iter().any(|(_, later)| {
                    later.safe_point.bytecode_offset > span.safe_point.bytecode_offset
                        && (later.start_byte, later.end_byte) != (span.start_byte, span.end_byte)
                })
        })
        .copied()
        .expect("armed root point must have a later distinct bound span");
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::StepStaticMetadataSourceSpan { target },
        ),
        DebuggerReply::ExecutionSourceSpanStepRequested {
            safe_point: target.safe_point,
        }
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    let successor = loop {
        match debugger_request(
            &mut debugger,
            DebuggerRequest::GetExecutionState { program },
        ) {
            DebuggerReply::ExecutionState {
                program: reply_program,
                state: DebuggerExecutionState::Paused { safe_point },
            } if reply_program == program => break safe_point,
            DebuggerReply::ExecutionState {
                program: reply_program,
                state: DebuggerExecutionState::Stepping,
            } if reply_program == program && Instant::now() < deadline => {
                thread::sleep(Duration::from_millis(10));
            }
            other => panic!("source step must pause at a new bound span: {other:?}"),
        }
    };
    let successor_target = DebuggerStaticMetadataSafePointSpanTarget {
        safe_point: successor,
        source: target.source,
    };
    let DebuggerReply::StaticMetadataSafePointSpan(successor_span) = debugger_request(
        &mut debugger,
        DebuggerRequest::DescribeStaticMetadataSafePointSpan {
            target: successor_target,
        },
    ) else {
        panic!("source-step stop must remain compiler-bound");
    };
    assert_ne!(
        (original_span.start_byte, original_span.end_byte),
        (successor_span.start_byte, successor_span.end_byte)
    );
    assert_eq!(
        debugger_request(&mut debugger, DebuggerRequest::ResumeExecution { program }),
        DebuggerReply::ExecutionResumed { program }
    );
    await_completed_execution(&mut debugger, program);
    drop(debugger);
    let mut separate = UnixStream::connect(&launcher.debugger_socket).unwrap();
    assert!(matches!(
        debugger_request(
            &mut separate,
            DebuggerRequest::Hello {
                protocol_version: DEBUGGER_PROTOCOL_VERSION,
                requested_bounded_values: false,
                requested_metadata_capabilities: manifest,
            },
        ),
        DebuggerReply::HelloAck { .. }
    ));
    assert!(matches!(
        debugger_request(
            &mut separate,
            DebuggerRequest::StepStaticMetadataSourceSpan { target },
        ),
        DebuggerReply::Unsupported { .. }
    ));
    assert_eq!(
        one_program(
            debugger_request(&mut separate, DebuggerRequest::ListPrograms { realm }),
            realm,
        ),
        program
    );
    let DebuggerReply::StaticMetadata(second_metadata) = debugger_request(
        &mut separate,
        DebuggerRequest::ListStaticMetadata { program },
    ) else {
        panic!("second stream must inventory live metadata independently");
    };
    assert_eq!(second_metadata.len(), 1);
    assert_ne!(
        second_metadata[0], metadata,
        "metadata handles are stream-local"
    );
    let DebuggerReply::StaticMetadataSources(second_sources) = debugger_request(
        &mut separate,
        DebuggerRequest::ListStaticMetadataSources {
            metadata: second_metadata[0],
        },
    ) else {
        panic!("second stream must inventory source IDs independently");
    };
    let second_source = *second_sources
        .iter()
        .find(|source| source.source_id == target.source.source_id)
        .expect("same compiler source remains attached under a new stream-local handle");
    let second_target = DebuggerStaticMetadataSafePointSpanTarget {
        source: second_source,
        ..target
    };
    navigate(&mut browser, &url);
    fixture.join().expect("fixture must serve both documents");
    assert!(matches!(
        debugger_request(
            &mut separate,
            DebuggerRequest::StepStaticMetadataSourceSpan {
                target: second_target,
            },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::StaleRealm,
            ..
        }
    ));
    drop(separate);
    launcher.shutdown();
    let _ = std::fs::remove_file(gatekeeper_socket);
}
