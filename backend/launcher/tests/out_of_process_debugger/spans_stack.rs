// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn launcher_exposes_exact_bluets_safe_point_spans_only_after_same_stream_source_receipts() {
    let gatekeeper_socket = clearing_gatekeeper();
    let listener = TcpListener::bind("127.0.0.1:0").expect("local HTTP fixture must bind");
    let url = format!("http://{}", listener.local_addr().unwrap());
    let fixture = serve_two_bluets_documents(listener);
    let mut launcher = LauncherProcess::spawn_with_static_metadata_policy(
        &gatekeeper_socket,
        StaticMetadataPolicy {
            inventory: true,
            source_inventory: true,
            safe_point_span: true,
            ..StaticMetadataPolicy::default()
        },
    );
    let mut browser = launcher.connect_browser();
    blueice_ipc::client_handshake(&mut browser).expect("public browser handshake must succeed");
    navigate(&mut browser, &url);

    let manifest = DebuggerMetadataCapabilityManifest::opaque_safe_point_span();
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
        panic!("the live child must report debugger capabilities");
    };
    assert!(capabilities.reports.iter().any(|report| {
        report.capability == blueice_ipc::debugger::DebuggerCapability::StaticMetadataSafePointSpan
            && report.state == blueice_ipc::debugger::DebuggerCapabilityState::Available
    }));
    let DebuggerReply::StaticMetadata(metadata) = debugger_request(
        &mut debugger,
        DebuggerRequest::ListStaticMetadata { program },
    ) else {
        panic!("the direct BlueTS program must have one opaque metadata attachment");
    };
    let metadata = metadata[0];
    let safe_points = safe_points(
        debugger_request(&mut debugger, DebuggerRequest::ListSafePoints { program }),
        program,
    );
    let guessed = blueice_ipc::debugger::DebuggerStaticMetadataSafePointSpanTarget {
        safe_point: safe_points[0],
        source: blueice_ipc::debugger::DebuggerStaticMetadataSourceId {
            metadata,
            source_id: 0,
        },
    };
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::DescribeStaticMetadataSafePointSpan { target: guessed },
        ),
        DebuggerReply::Unsupported { .. }
    ));
    let DebuggerReply::StaticMetadataSources(sources) = debugger_request(
        &mut debugger,
        DebuggerRequest::ListStaticMetadataSources { metadata },
    ) else {
        panic!("the exact stream must receive compiler source IDs");
    };
    assert!(!sources.is_empty());
    let mut mapped = None;
    for safe_point in safe_points {
        if safe_point.code_unit_ordinal != 0 || safe_point.bytecode_offset == 0 {
            continue;
        }
        for source in &sources {
            let target = blueice_ipc::debugger::DebuggerStaticMetadataSafePointSpanTarget {
                safe_point,
                source: *source,
            };
            match debugger_request(
                &mut debugger,
                DebuggerRequest::DescribeStaticMetadataSafePointSpan { target },
            ) {
                DebuggerReply::StaticMetadataSafePointSpan(span) => {
                    mapped = Some((target, span));
                    break;
                }
                DebuggerReply::Error {
                    code:
                        DebuggerErrorCode::CapabilityUnavailable | DebuggerErrorCode::InvalidTarget,
                    ..
                } => {}
                reply => panic!("unexpected safe-point span lookup result: {reply:?}"),
            }
        }
        if mapped.is_some() {
            break;
        }
    }
    let (target, span) = mapped.expect("one verified typed instruction has an exact source span");
    assert_eq!(span.safe_point, target.safe_point);
    assert_eq!(span.source, target.source);
    assert!(span.is_well_formed());
    assert!(usize::try_from(span.end_byte).unwrap() <= FIRST_BLUETS_SOURCE.len());
    let source_span = FIRST_BLUETS_SOURCE
        .get(span.start_byte as usize..span.end_byte as usize)
        .expect("the verified span must have original UTF-8 boundaries");
    assert!(source_span.contains("privateBlueTsMetadata"));
    assert_eq!(span.coordinates.start_line, 1);
    assert_eq!(span.coordinates.start_column_utf16, 9);
    assert_eq!(span.coordinates.end_line, 1);
    assert!(span.coordinates.end_column_utf16 > 9);
    assert!(!format!("{span:?}").contains("privateBlueTsMetadata"));
    assert!(!format!("{span:?}").contains("inline-0.ts"));

    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::DescribeStaticMetadataSafePointSpan {
                target: blueice_ipc::debugger::DebuggerStaticMetadataSafePointSpanTarget {
                    source: blueice_ipc::debugger::DebuggerStaticMetadataSourceId {
                        source_id: target.source.source_id + 1,
                        ..target.source
                    },
                    ..target
                },
            },
        ),
        DebuggerReply::Unsupported { .. }
    ));

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
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::ArmRootSafePointBreakpoint {
                safe_point: target.safe_point,
            },
        ),
        DebuggerReply::RootSafePointBreakpointArmed {
            safe_point: target.safe_point,
        }
    );
    await_paused_execution(&mut debugger, program, target.safe_point);
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::DescribeStaticMetadataSafePointSpan { target },
        ),
        DebuggerReply::StaticMetadataSafePointSpan(span),
        "the paused BlueJS frame must retain the same original BlueTS position"
    );
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::StepStaticMetadataSourceSpan { target },
        ),
        DebuggerReply::Unsupported { .. }
    ));
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
            DebuggerRequest::DescribeStaticMetadataSafePointSpan { target },
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
            DebuggerRequest::DescribeStaticMetadataSafePointSpan { target },
        ),
        DebuggerReply::Unsupported { .. }
    ));
    drop(separate);
    launcher.shutdown();
    let _ = std::fs::remove_file(gatekeeper_socket);
}

#[test]
fn launcher_batches_exact_classic_and_module_stack_coordinates_only_with_receipts() {
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
                "<main>{slug}-stack-coordinates</main><script type=\"{mime}\">{STACK_COORDINATE_BLUETS_SOURCE}</script>"
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
                inventory: true,
                source_inventory: true,
                safe_point_span: true,
                ..StaticMetadataPolicy::default()
            },
        );
        let mut browser = launcher.connect_browser();
        blueice_ipc::client_handshake(&mut browser).unwrap();
        navigate(&mut browser, &url);
        fixture.join().unwrap();

        let manifest = DebuggerMetadataCapabilityManifest::opaque_safe_point_span();
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
        let realm = one_realm(debugger_request(
            &mut debugger,
            DebuggerRequest::ListPageRealms,
        ));
        let program = one_program(
            debugger_request(&mut debugger, DebuggerRequest::ListPrograms { realm }),
            realm,
        );
        let child_entry = safe_points(
            debugger_request(&mut debugger, DebuggerRequest::ListSafePoints { program }),
            program,
        )
        .into_iter()
        .find(|point| point.code_unit_ordinal == 1 && point.bytecode_offset == 0)
        .expect("the direct BlueTS function has an entry safe point");
        assert_eq!(
            debugger_request(
                &mut debugger,
                DebuggerRequest::ArmNestedSafePointBreakpoint {
                    safe_point: child_entry,
                },
            ),
            DebuggerReply::NestedSafePointBreakpointArmed {
                safe_point: child_entry,
            }
        );
        let (frame, paused) = await_nested_paused_execution(&mut debugger, program);
        assert_eq!(paused, child_entry);
        let DebuggerReply::Stack(stack) = debugger_request(
            &mut debugger,
            DebuggerRequest::GetStack {
                program,
                frame: Some(frame),
                max_frames: 2,
            },
        ) else {
            panic!("{slug} child must expose its caller and callee");
        };
        assert_eq!(stack.safe_points.len(), 2);
        assert_eq!(stack.safe_points[0], child_entry);
        assert_eq!(stack.safe_points[1].code_unit_ordinal, 0);
        assert!(!stack.stack_truncated);
        let DebuggerReply::StaticMetadata(metadata) = debugger_request(
            &mut debugger,
            DebuggerRequest::ListStaticMetadata { program },
        ) else {
            panic!("{slug} BlueTS program must have metadata");
        };
        let metadata = metadata[0];
        let guessed = DebuggerStackCoordinatesTarget {
            expected_stack: stack.clone(),
            sources: vec![
                DebuggerStaticMetadataSourceId {
                    metadata,
                    source_id: 0,
                };
                2
            ],
        };
        assert!(matches!(
            debugger_request(
                &mut debugger,
                DebuggerRequest::GetStackCoordinates { target: guessed },
            ),
            DebuggerReply::Unsupported { .. }
        ));
        let DebuggerReply::StaticMetadataSources(sources) = debugger_request(
            &mut debugger,
            DebuggerRequest::ListStaticMetadataSources { metadata },
        ) else {
            panic!("{slug} source IDs must be inventoried on this stream");
        };
        if slug == "classic" {
            assert!(
                sources.len() >= 2,
                "the classic source set must include a second inventoried source"
            );
        }
        let page_source = sources
            .iter()
            .copied()
            .find(|source| {
                stack.safe_points.iter().all(|safe_point| {
                    matches!(
                        debugger_request(
                            &mut debugger,
                            DebuggerRequest::DescribeStaticMetadataSafePointSpan {
                                target: DebuggerStaticMetadataSafePointSpanTarget {
                                    safe_point: *safe_point,
                                    source: *source,
                                },
                            },
                        ),
                        DebuggerReply::StaticMetadataSafePointSpan(_)
                    )
                })
            })
            .expect("both frames must map to one receipted BlueTS source");
        let target = DebuggerStackCoordinatesTarget {
            expected_stack: stack.clone(),
            sources: vec![page_source; 2],
        };
        let request = DebuggerRequest::GetStackCoordinates {
            target: target.clone(),
        };
        let DebuggerReply::StackCoordinates(coordinates) =
            debugger_request(&mut debugger, request.clone())
        else {
            panic!("{slug} exact paused stack must have original coordinates");
        };
        assert_eq!(coordinates.stack, stack);
        assert!(coordinates.is_well_formed());
        let source = STACK_COORDINATE_BLUETS_SOURCE;
        let expected_ranges = [
            (
                source.find("function inner").unwrap(),
                source.find("} globalThis").unwrap() + 1,
            ),
            (source.find("globalThis.answer").unwrap(), source.len()),
        ];
        for (span, (start, end)) in coordinates.spans.iter().zip(expected_ranges) {
            assert_eq!(
                (span.start_byte as usize, span.end_byte as usize),
                (start, end)
            );
            assert_eq!(span.source, page_source);
            assert_eq!(span.coordinates.start_line, 0);
            assert_eq!(
                span.coordinates.start_column_utf16,
                source[..start].encode_utf16().count() as u32
            );
            assert_eq!(
                span.coordinates.end_column_utf16,
                source[..end].encode_utf16().count() as u32
            );
        }
        assert!(!format!("{coordinates:?}").contains("function inner"));
        let mut unreceipted = target.clone();
        unreceipted.sources[1].source_id =
            sources.iter().map(|source| source.source_id).max().unwrap() + 1;
        assert!(matches!(
            debugger_request(
                &mut debugger,
                DebuggerRequest::GetStackCoordinates {
                    target: unreceipted
                },
            ),
            DebuggerReply::Unsupported { .. }
        ));
        let wrong = sources
            .iter()
            .copied()
            .find(|source| *source != page_source);
        if slug == "classic" {
            assert!(
                wrong.is_some(),
                "the classic source set must expose another receipted source"
            );
        }
        if let Some(wrong) = wrong {
            let mut wrong_source = target.clone();
            wrong_source.sources[1] = wrong;
            assert!(matches!(
                debugger_request(
                    &mut debugger,
                    DebuggerRequest::GetStackCoordinates {
                        target: wrong_source
                    },
                ),
                DebuggerReply::Error {
                    code: DebuggerErrorCode::InvalidTarget,
                    ..
                }
            ));
        }
        assert_eq!(
            debugger_request(
                &mut debugger,
                DebuggerRequest::StepNestedInstruction { frame },
            ),
            DebuggerReply::NestedStepRequested { frame }
        );
        let (same_frame, moved) = await_nested_paused_execution(&mut debugger, program);
        assert_eq!(same_frame, frame);
        assert_ne!(moved, child_entry);
        assert!(matches!(
            debugger_request(&mut debugger, request.clone()),
            DebuggerReply::Error {
                code: DebuggerErrorCode::InvalidExecutionState,
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
            debugger_request(&mut separate, request),
            DebuggerReply::Unsupported { .. }
        ));
        drop(separate);
        launcher.shutdown();
        let _ = std::fs::remove_file(gatekeeper_socket);
    }
}

#[test]
fn launcher_rejects_a_real_paused_unbound_bluets_stack_without_partial_coordinates() {
    let gatekeeper_socket = clearing_gatekeeper();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let fixture = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0_u8; 1024];
        let _ = stream.read(&mut request);
        let source = "const answer: number = 42; globalThis.answer = answer;";
        let body = format!(
            "<main>unbound-stack-coordinates</main><script type=\"application/x-blueice-typescript\">{source}</script>"
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
            inventory: true,
            source_inventory: true,
            safe_point_span: true,
            ..StaticMetadataPolicy::default()
        },
    );
    let mut browser = launcher.connect_browser();
    blueice_ipc::client_handshake(&mut browser).unwrap();
    navigate(&mut browser, &url);
    fixture.join().unwrap();
    let manifest = DebuggerMetadataCapabilityManifest::opaque_safe_point_span();
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
    let halt = points
        .iter()
        .copied()
        .filter(|point| point.code_unit_ordinal == 0)
        .max_by_key(|point| point.bytecode_offset)
        .expect("the classic root must end in an unowned Halt instruction");
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::ArmRootSafePointBreakpoint { safe_point: halt },
        ),
        DebuggerReply::RootSafePointBreakpointArmed { safe_point: halt }
    );
    await_paused_execution(&mut debugger, program, halt);
    let DebuggerReply::Stack(stack) = debugger_request(
        &mut debugger,
        DebuggerRequest::GetStack {
            program,
            frame: None,
            max_frames: 1,
        },
    ) else {
        panic!("the root must really pause at the unbound instruction");
    };
    assert_eq!(stack.safe_points, vec![halt]);
    let DebuggerReply::StaticMetadata(metadata) = debugger_request(
        &mut debugger,
        DebuggerRequest::ListStaticMetadata { program },
    ) else {
        panic!("the BlueTS program must retain metadata");
    };
    let metadata = metadata[0];
    let DebuggerReply::StaticMetadataSources(sources) = debugger_request(
        &mut debugger,
        DebuggerRequest::ListStaticMetadataSources { metadata },
    ) else {
        panic!("the same stream must inventory source IDs");
    };
    let page_source = points
        .iter()
        .copied()
        .filter(|point| point.code_unit_ordinal == 0 && *point != halt)
        .find_map(|bound_root| {
            sources.iter().copied().find(|source| {
                matches!(
                    debugger_request(
                        &mut debugger,
                        DebuggerRequest::DescribeStaticMetadataSafePointSpan {
                            target: DebuggerStaticMetadataSafePointSpanTarget {
                                safe_point: bound_root,
                                source: *source,
                            },
                        },
                    ),
                    DebuggerReply::StaticMetadataSafePointSpan(_)
                )
            })
        })
        .expect("an earlier root statement must identify the correct receipted source");
    let unbound_span_reply = debugger_request(
        &mut debugger,
        DebuggerRequest::DescribeStaticMetadataSafePointSpan {
            target: DebuggerStaticMetadataSafePointSpanTarget {
                safe_point: halt,
                source: page_source,
            },
        },
    );
    assert!(
        matches!(
            unbound_span_reply,
            DebuggerReply::Error {
                code: DebuggerErrorCode::InvalidTarget,
                ..
            }
        ),
        "unexpected unbound span reply: {unbound_span_reply:?}"
    );
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::GetStackCoordinates {
                target: DebuggerStackCoordinatesTarget {
                    expected_stack: stack,
                    sources: vec![page_source],
                },
            },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            ..
        }
    ));
    launcher.shutdown();
    let _ = std::fs::remove_file(gatekeeper_socket);
}

#[test]
fn launcher_maps_a_real_bluets_module_safe_point_to_original_coordinates() {
    let gatekeeper_socket = clearing_gatekeeper();
    let listener = TcpListener::bind("127.0.0.1:0").expect("local HTTP fixture must bind");
    let url = format!("http://{}", listener.local_addr().unwrap());
    let fixture = serve_bluets_module_document(listener);
    let mut launcher = LauncherProcess::spawn_with_static_metadata_policy(
        &gatekeeper_socket,
        StaticMetadataPolicy {
            inventory: true,
            source_inventory: true,
            safe_point_span: true,
            ..StaticMetadataPolicy::default()
        },
    );
    let mut browser = launcher.connect_browser();
    blueice_ipc::client_handshake(&mut browser).expect("public browser handshake must succeed");
    navigate(&mut browser, &url);
    fixture.join().expect("fixture must serve the typed module");

    let manifest = DebuggerMetadataCapabilityManifest::opaque_safe_point_span();
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
        panic!("the live BlueTS module must retain static metadata")
    };
    let DebuggerReply::StaticMetadataSources(sources) = debugger_request(
        &mut debugger,
        DebuggerRequest::ListStaticMetadataSources {
            metadata: metadata[0],
        },
    ) else {
        panic!("the module's original source must be receipted")
    };
    let points = safe_points(
        debugger_request(&mut debugger, DebuggerRequest::ListSafePoints { program }),
        program,
    );
    let mut mapped = None;
    for safe_point in points {
        for source in &sources {
            let target = blueice_ipc::debugger::DebuggerStaticMetadataSafePointSpanTarget {
                safe_point,
                source: *source,
            };
            if let DebuggerReply::StaticMetadataSafePointSpan(span) = debugger_request(
                &mut debugger,
                DebuggerRequest::DescribeStaticMetadataSafePointSpan { target },
            ) {
                mapped = Some(span);
                break;
            }
        }
        if mapped.is_some() {
            break;
        }
    }
    let span = mapped.expect("module root must map to an original BlueTS source span");
    // HTML normalizes CRLF before handing inline script text to BlueTS.
    let normalized_source = MODULE_BLUETS_SOURCE.replace("\r\n", "\n");
    let source_span = normalized_source
        .get(span.start_byte as usize..span.end_byte as usize)
        .expect("module span must have original UTF-8 boundaries");
    assert!(source_span.contains("moduleAnswer"));
    assert!(span.is_well_formed());
    let line_start = normalized_source.find("export").unwrap();
    assert_eq!(span.coordinates.start_line, 1);
    assert_eq!(span.coordinates.end_line, 1);
    assert_eq!(span.coordinates.start_column_utf16, 0);
    assert_eq!(span.start_byte as usize, line_start);
    assert_eq!(
        span.coordinates.end_column_utf16,
        normalized_source[line_start..span.end_byte as usize]
            .encode_utf16()
            .count() as u32
    );
    assert!(!format!("{span:?}").contains("moduleAnswer"));

    drop(debugger);
    launcher.shutdown();
    let _ = std::fs::remove_file(gatekeeper_socket);
}
