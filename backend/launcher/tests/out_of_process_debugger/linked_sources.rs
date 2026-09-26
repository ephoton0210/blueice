// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn launcher_links_two_receipted_bluets_sources_and_expires_the_graph_on_reload() {
    const DOCUMENT: &str = "<script type=\"application/x-blueice-typescript-module\" src=\"/linked-entry.ts\"></script>";
    let gatekeeper_socket = clearing_gatekeeper();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let policy = OwnerHttpPolicyBootstrap {
        origin_rule: OwnerHttpOriginRule::SameDocumentOrigin,
        resources: vec![
            OwnerHttpResource {
                canonical_url: format!("{origin}/linked-entry.ts"),
                integrity:
                    "sha256:0e2a801c62c1a42cfef5afc39c6ad266dba586c6a834679a43adf674cf89f942".into(),
            },
            OwnerHttpResource {
                canonical_url: format!("{origin}/linked-dependency.ts"),
                integrity:
                    "sha256:cb66277fc65ebe92e842330a18b6dc5dfba68a2a09a5e2c546eafdb835830d9d".into(),
            },
        ],
    };
    let policy_file = unique_path("linked-owner-policy");
    std::fs::write(&policy_file, serde_json::to_vec(&policy).unwrap()).unwrap();
    let (stop_tx, stop_rx) = mpsc::channel();
    let fixture = thread::spawn(move || {
        let mut requested = Vec::new();
        while matches!(stop_rx.try_recv(), Err(mpsc::TryRecvError::Empty)) {
            let Ok((mut stream, _)) = listener.accept() else {
                thread::sleep(Duration::from_millis(10));
                continue;
            };
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = [0_u8; 2048];
            let count = stream.read(&mut request).unwrap();
            let request = std::str::from_utf8(&request[..count]).unwrap();
            let path = request.split_whitespace().nth(1).unwrap();
            let (mime, body) = match path {
                "/" => ("text/html", DOCUMENT),
                "/linked-entry.ts" => ("text/typescript", LINKED_ENTRY_SOURCE),
                "/linked-dependency.ts" => ("text/typescript", LINKED_DEPENDENCY_SOURCE),
                other => panic!("unexpected linked graph resource {other}"),
            };
            requested.push(path.to_string());
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                )
                .unwrap();
        }
        requested
    });
    let mut launcher = LauncherProcess::spawn_with_owner_http_policy(
        &gatekeeper_socket,
        StaticMetadataPolicy {
            inventory: true,
            source_inventory: true,
            type_inventory: true,
            symbol_inventory: true,
            symbol_display: true,
            symbol_location: true,
            safe_point_span: true,
            source_breakpoint: true,
            static_scope_relation: true,
            ..StaticMetadataPolicy::default()
        },
        Some(&policy_file),
    );
    let mut browser = launcher.connect_browser();
    blueice_ipc::client_handshake(&mut browser).unwrap();
    let url = format!("{origin}/");
    navigate(&mut browser, &url);

    let manifest =
        DebuggerMetadataCapabilityManifest::opaque_selected(DebuggerMetadataCapabilitySelection {
            safe_point_span: true,
            source_breakpoint: true,
            symbol_display: true,
            symbol_location: true,
            static_scope_relation: true,
            ..DebuggerMetadataCapabilitySelection::default()
        });
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
    let DebuggerReply::Capabilities(capabilities) = debugger_request(
        &mut debugger,
        DebuggerRequest::DescribeCapabilities { realm },
    ) else {
        panic!("the live launcher must report linked debugger capability")
    };
    assert!(capabilities.reports.iter().any(|report| {
        report.capability == DebuggerCapability::LinkedModules
            && report.state == DebuggerCapabilityState::Available
    }));
    let DebuggerReply::Programs(programs) =
        debugger_request(&mut debugger, DebuggerRequest::ListPrograms { realm })
    else {
        panic!("linked BlueTS graph must expose separate programs")
    };
    assert_eq!(programs.len(), 2);
    let (dependency, dependency_safe_point) = programs
        .iter()
        .find_map(|program| {
            safe_points(
                debugger_request(
                    &mut debugger,
                    DebuggerRequest::ListSafePoints { program: *program },
                ),
                *program,
            )
            .into_iter()
            .find(|point| point.code_unit_ordinal == 1 && point.bytecode_offset == 0)
            .map(|point| (*program, point))
        })
        .expect("dependency function must expose its verified entry boundary");
    let entry = *programs
        .iter()
        .find(|program| **program != dependency)
        .unwrap();
    let arm = DebuggerLinkedArmTarget {
        entry,
        dependency_safe_point,
    };
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::ArmLinkedNestedSafePointBreakpoint {
                target: DebuggerLinkedArmTarget {
                    entry: dependency,
                    ..arm
                },
            },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            ..
        }
    ));
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::ArmLinkedNestedSafePointBreakpoint {
                target: DebuggerLinkedArmTarget {
                    dependency_safe_point: DebuggerSafePoint {
                        program: DebuggerProgram {
                            program_generation: dependency.program_generation + 1,
                            ..dependency
                        },
                        ..dependency_safe_point
                    },
                    ..arm
                },
            },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::StaleProgram | DebuggerErrorCode::InvalidTarget,
            ..
        }
    ));
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::ArmLinkedNestedSafePointBreakpoint { target: arm },
        ),
        DebuggerReply::LinkedNestedSafePointBreakpointArmed { target: arm }
    );
    let DebuggerReply::StaticMetadata(dependency_metadata) = debugger_request(
        &mut debugger,
        DebuggerRequest::ListStaticMetadata {
            program: dependency,
        },
    ) else {
        panic!("pending dependency must retain its own metadata")
    };
    assert_eq!(dependency_metadata.len(), 1);
    let DebuggerReply::StaticMetadataSources(dependency_sources) = debugger_request(
        &mut debugger,
        DebuggerRequest::ListStaticMetadataSources {
            metadata: dependency_metadata[0],
        },
    ) else {
        panic!("pending dependency must receipt its own original source")
    };
    assert_eq!(dependency_sources.len(), 1);
    let DebuggerReply::StaticMetadataSymbols(dependency_symbols) = debugger_request(
        &mut debugger,
        DebuggerRequest::ListStaticMetadataSymbols {
            metadata: dependency_metadata[0],
        },
    ) else {
        panic!("pending dependency must receipt its own function symbol")
    };
    let (function_symbol, function_display) = dependency_symbols
        .into_iter()
        .find_map(|symbol| {
            let DebuggerReply::StaticMetadataSymbol(display) = debugger_request(
                &mut debugger,
                DebuggerRequest::DescribeStaticMetadataSymbol { symbol },
            ) else {
                panic!("dependency symbol kind requires its independent display grant")
            };
            (display.display == "inner").then_some((symbol, display))
        })
        .expect("dependency must retain its executable function symbol");
    assert_eq!(
        function_display.kind,
        DebuggerStaticMetadataSymbolKind::Function
    );
    let DebuggerReply::StaticMetadataSymbolLocation(function_location) = debugger_request(
        &mut debugger,
        DebuggerRequest::DescribeStaticMetadataSymbolLocation {
            target: DebuggerStaticMetadataSymbolLocationTarget {
                symbol: function_symbol,
                source: dependency_sources[0],
            },
        },
    ) else {
        panic!("dependency function must own an original declaration range")
    };
    assert_eq!(
        &LINKED_DEPENDENCY_SOURCE
            [function_location.start_byte as usize..function_location.end_byte as usize],
        LINKED_DEPENDENCY_SOURCE
    );
    let source_target = DebuggerStaticMetadataSourceBreakpointTarget {
        source: dependency_sources[0],
        source_byte: function_location.start_byte,
    };
    let DebuggerReply::StaticMetadataSourceBreakpoint(binding) = debugger_request(
        &mut debugger,
        DebuggerRequest::ResolveStaticMetadataSourceBreakpoint {
            target: source_target,
        },
    ) else {
        panic!("receipted dependency position must resolve before linked arm")
    };
    assert_eq!(binding.target, source_target);
    assert_eq!(binding.safe_point, Some(dependency_safe_point));
    let DebuggerReply::StaticMetadataSafePointSpan(function_span) = debugger_request(
        &mut debugger,
        DebuggerRequest::DescribeStaticMetadataSafePointSpan {
            target: DebuggerStaticMetadataSafePointSpanTarget {
                safe_point: dependency_safe_point,
                source: dependency_sources[0],
            },
        },
    ) else {
        panic!("dependency function candidate requires its exact original span")
    };
    assert_eq!(
        function_location.executable_breakpoint_candidate(
            &function_display,
            binding,
            function_span,
        ),
        Some(dependency_safe_point)
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    let stack = loop {
        match debugger_request(
            &mut debugger,
            DebuggerRequest::GetLinkedExecutionState { entry },
        ) {
            DebuggerReply::LinkedExecutionState {
                entry: actual,
                state,
            } if actual == entry => match *state {
                DebuggerLinkedExecutionState::Paused { stack } => break stack,
                DebuggerLinkedExecutionState::Pending if Instant::now() < deadline => {
                    thread::sleep(Duration::from_millis(10));
                }
                other => panic!("linked graph did not pause in its dependency: {other:?}"),
            },
            other => panic!("linked graph did not return an exact state: {other:?}"),
        }
    };
    assert_eq!(stack.frames[0].frame.program, dependency);
    assert_eq!(stack.frames[1].frame.program, entry);
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::ArmLinkedNestedSafePointBreakpoint { target: arm },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidExecutionState,
            ..
        }
    ));
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::GetLinkedStack {
                top_frame: stack.frames[0].frame,
            },
        ),
        DebuggerReply::LinkedStack(Box::new(stack))
    );
    let metadata = [dependency, entry].map(|program| {
        let DebuggerReply::StaticMetadata(handles) = debugger_request(
            &mut debugger,
            DebuggerRequest::ListStaticMetadata { program },
        ) else {
            panic!("each linked program must own its metadata receipt")
        };
        assert_eq!(handles.len(), 1);
        handles[0]
    });
    assert_ne!(metadata[0], metadata[1]);
    let DebuggerReply::LinkedScopes(linked_scopes) = debugger_request(
        &mut debugger,
        DebuggerRequest::GetLinkedScopes {
            expected_stack: stack,
            max_scope_entries: 256,
        },
    ) else {
        panic!("complete linked pause must expose entry-root slots only");
    };
    assert!(linked_scopes.is_well_formed());
    assert_eq!(linked_scopes.stack, stack);
    assert_eq!(linked_scopes.frame_index, 1);
    assert!(!linked_scopes.scope_truncated);
    assert!(!linked_scopes.entries.is_empty());
    let DebuggerReply::StaticMetadataTypes(entry_types) = debugger_request(
        &mut debugger,
        DebuggerRequest::ListStaticMetadataTypes {
            metadata: metadata[1],
        },
    ) else {
        panic!("linked entry must inventory its own type IDs");
    };
    let DebuggerReply::StaticMetadataSymbols(entry_symbols) = debugger_request(
        &mut debugger,
        DebuggerRequest::ListStaticMetadataSymbols {
            metadata: metadata[1],
        },
    ) else {
        panic!("linked entry must inventory its own symbol IDs");
    };
    let mut bound = None;
    for scope_entry in linked_scopes.entries.iter().copied() {
        let target = blueice_ipc::debugger::DebuggerStaticScopeTarget::Linked {
            metadata: metadata[1],
            target: blueice_ipc::debugger::DebuggerLinkedScopeTarget {
                stack,
                frame_index: 1,
                scope_entry,
            },
        };
        match debugger_request(
            &mut debugger,
            DebuggerRequest::GetStaticScopeRelation { target },
        ) {
            DebuggerReply::StaticScopeRelation(relation) => {
                assert_eq!(relation.target, target);
                assert!(relation.is_well_formed());
                assert!(entry_types.contains(&relation.static_type));
                assert!(entry_symbols.contains(&relation.symbol));
                bound = Some(scope_entry);
                break;
            }
            DebuggerReply::Error {
                code: DebuggerErrorCode::InvalidTarget | DebuggerErrorCode::InvalidExecutionState,
                ..
            } => {}
            other => panic!("linked entry relation must be typed: {other:?}"),
        }
    }
    let bound = bound.expect("linked entry root must retain one compiler-bound slot");
    let receipted_target = blueice_ipc::debugger::DebuggerLinkedScopeTarget {
        stack,
        frame_index: 1,
        scope_entry: bound,
    };
    let linked_target = blueice_ipc::debugger::DebuggerStaticScopeTarget::Linked {
        metadata: metadata[1],
        target: receipted_target,
    };
    for forged in [
        blueice_ipc::debugger::DebuggerStaticScopeTarget::Linked {
            metadata: metadata[1],
            target: blueice_ipc::debugger::DebuggerLinkedScopeTarget {
                scope_entry: blueice_ipc::debugger::DebuggerScopeEntry {
                    slot_ordinal: u32::MAX,
                    ..bound
                },
                ..receipted_target
            },
        },
        blueice_ipc::debugger::DebuggerStaticScopeTarget::Linked {
            metadata: blueice_ipc::debugger::DebuggerStaticMetadataHandle {
                metadata_generation: metadata[1].metadata_generation + 1,
                ..metadata[1]
            },
            target: blueice_ipc::debugger::DebuggerLinkedScopeTarget {
                stack,
                frame_index: 1,
                scope_entry: bound,
            },
        },
    ] {
        assert!(forged.is_well_formed());
        let reply = debugger_request(
            &mut debugger,
            DebuggerRequest::GetStaticScopeRelation { target: forged },
        );
        assert!(
            matches!(
                reply,
                DebuggerReply::Error {
                    code: DebuggerErrorCode::InvalidTarget
                        | DebuggerErrorCode::CapabilityUnavailable,
                    ..
                }
            ),
            "forged linked selector {forged:?} returned {reply:?}"
        );
    }
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::GetStaticScopeRelation {
                target: linked_target,
            },
        ),
        DebuggerReply::StaticScopeRelation(relation) if relation.target == linked_target
    ));
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::GetStaticScopeRelation {
                target: blueice_ipc::debugger::DebuggerStaticScopeTarget::Linked {
                    metadata: metadata[0],
                    target: blueice_ipc::debugger::DebuggerLinkedScopeTarget {
                        stack,
                        frame_index: 1,
                        scope_entry: bound,
                    },
                },
            },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            ..
        }
    ));
    let guessed_dependency_source = DebuggerStaticMetadataSourceId {
        metadata: metadata[0],
        source_id: u32::MAX,
    };
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::ResolveStaticMetadataSourceBreakpoint {
                target: DebuggerStaticMetadataSourceBreakpointTarget {
                    source: guessed_dependency_source,
                    source_byte: 0,
                },
            },
        ),
        DebuggerReply::Unsupported { .. }
    ));
    let source_for = |debugger: &mut UnixStream, metadata| {
        let DebuggerReply::StaticMetadataSources(sources) = debugger_request(
            debugger,
            DebuggerRequest::ListStaticMetadataSources { metadata },
        ) else {
            panic!("each linked program must own its source receipt")
        };
        assert_eq!(sources.len(), 1);
        sources[0]
    };
    let dependency_source = source_for(&mut debugger, metadata[0]);
    let unreceipted = DebuggerLinkedStackCoordinatesTarget {
        expected_stack: stack,
        sources: [
            dependency_source,
            blueice_ipc::debugger::DebuggerStaticMetadataSourceId {
                metadata: metadata[1],
                source_id: dependency_source.source_id,
            },
        ],
    };
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::GetLinkedStackCoordinates {
                target: unreceipted
            },
        ),
        DebuggerReply::Unsupported { .. }
    ));
    let entry_source = source_for(&mut debugger, metadata[1]);
    assert_ne!(dependency_source, entry_source);
    let mut matched_symbols = Vec::new();
    for (program, source, original, position, declaration, ordinal) in [
        (
            dependency,
            dependency_source,
            LINKED_DEPENDENCY_SOURCE,
            "function inner",
            LINKED_DEPENDENCY_SOURCE,
            1,
        ),
        (
            entry,
            entry_source,
            LINKED_ENTRY_SOURCE,
            "export const answer",
            "export const answer: number = inner() + 1;",
            0,
        ),
    ] {
        let target = DebuggerStaticMetadataSourceBreakpointTarget {
            source,
            source_byte: original.find(position).unwrap() as u32,
        };
        let DebuggerReply::StaticMetadataSourceBreakpoint(binding) = debugger_request(
            &mut debugger,
            DebuggerRequest::ResolveStaticMetadataSourceBreakpoint { target },
        ) else {
            panic!("module source position must resolve under its own program")
        };
        assert_eq!(binding.target, target);
        let point = binding
            .safe_point
            .expect("module declaration must be bound");
        assert_eq!(point.program, program);
        assert_eq!(point.code_unit_ordinal, ordinal);
        if program == dependency {
            assert_eq!(point, dependency_safe_point);
        }
        let unbound = DebuggerStaticMetadataSourceBreakpointTarget {
            source_byte: original.len() as u32,
            ..target
        };
        let DebuggerReply::StaticMetadataSourceBreakpoint(binding) = debugger_request(
            &mut debugger,
            DebuggerRequest::ResolveStaticMetadataSourceBreakpoint { target: unbound },
        ) else {
            panic!("module source end must return an explicit unbound result")
        };
        assert_eq!(binding.target, unbound);
        assert_eq!(binding.safe_point, None);

        let guessed_symbol = blueice_ipc::debugger::DebuggerStaticMetadataSymbolId {
            metadata: source.metadata,
            symbol_id: u32::MAX,
        };
        assert!(matches!(
            debugger_request(
                &mut debugger,
                DebuggerRequest::DescribeStaticMetadataSymbolLocation {
                    target: DebuggerStaticMetadataSymbolLocationTarget {
                        symbol: guessed_symbol,
                        source,
                    },
                },
            ),
            DebuggerReply::Unsupported { .. }
        ));
        let DebuggerReply::StaticMetadataSymbols(symbols) = debugger_request(
            &mut debugger,
            DebuggerRequest::ListStaticMetadataSymbols {
                metadata: source.metadata,
            },
        ) else {
            panic!("each module program must inventory its own symbol IDs")
        };
        let mut matching = None;
        let mut seen = Vec::new();
        for symbol in symbols {
            let reply = debugger_request(
                &mut debugger,
                DebuggerRequest::DescribeStaticMetadataSymbolLocation {
                    target: DebuggerStaticMetadataSymbolLocationTarget { symbol, source },
                },
            );
            let DebuggerReply::StaticMetadataSymbolLocation(location) = reply else {
                panic!("receipted module symbol must have an original location: {reply:?}")
            };
            assert_eq!(location.symbol, symbol);
            assert_eq!(location.source, source);
            let slice = &original[location.start_byte as usize..location.end_byte as usize];
            seen.push(slice);
            if slice == declaration {
                assert_eq!(location.coordinates.start_line, 0);
                assert_eq!(
                    location.coordinates.start_column_utf16,
                    original[..location.start_byte as usize]
                        .encode_utf16()
                        .count() as u32
                );
                matching = Some(symbol);
            }
        }
        matched_symbols.push(matching.unwrap_or_else(|| {
            panic!("module declaration {declaration} not found in original locations {seen:?}")
        }));
    }
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::DescribeStaticMetadataSymbolLocation {
                target: DebuggerStaticMetadataSymbolLocationTarget {
                    symbol: matched_symbols[0],
                    source: entry_source,
                },
            },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            ..
        }
    ));
    let target = DebuggerLinkedStackCoordinatesTarget {
        expected_stack: stack,
        sources: [dependency_source, entry_source],
    };
    let swapped = DebuggerLinkedStackCoordinatesTarget {
        sources: [entry_source, dependency_source],
        ..target
    };
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::GetLinkedStackCoordinates { target: swapped },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            ..
        }
    ));
    let DebuggerReply::LinkedStackCoordinates(coordinates) = debugger_request(
        &mut debugger,
        DebuggerRequest::GetLinkedStackCoordinates { target },
    ) else {
        panic!("both receipted linked programs must return exact original spans")
    };
    assert!(coordinates.is_well_formed());
    assert_eq!(coordinates.stack, stack);
    assert_eq!(coordinates.spans[0].source, dependency_source);
    assert_eq!(coordinates.spans[1].source, entry_source);
    for (span, (original, expected)) in coordinates.spans.iter().zip([
        (
            LINKED_DEPENDENCY_SOURCE,
            "export function inner(): number { return 41; }",
        ),
        (
            LINKED_ENTRY_SOURCE,
            "export const answer: number = inner() + 1;",
        ),
    ]) {
        let selected = &original[span.start_byte as usize..span.end_byte as usize];
        assert_eq!(selected, expected);
        assert_eq!(span.coordinates.start_line, 0);
        assert_eq!(
            span.coordinates.start_column_utf16,
            original[..span.start_byte as usize].encode_utf16().count() as u32
        );
        assert_eq!(
            span.coordinates.end_column_utf16,
            original[..span.end_byte as usize].encode_utf16().count() as u32
        );
    }
    assert!(!format!("{coordinates:?}").contains("linked-dependency.ts"));

    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::ResumeLinkedNestedExecution {
                top_frame: stack.frames[0].frame,
            },
        ),
        DebuggerReply::LinkedNestedResumeRequested {
            top_frame: stack.frames[0].frame,
        }
    );
    for request in [
        DebuggerRequest::GetLinkedScopes {
            expected_stack: stack,
            max_scope_entries: 256,
        },
        DebuggerRequest::GetStaticScopeRelation {
            target: linked_target,
        },
    ] {
        assert!(matches!(
            debugger_request(&mut debugger, request),
            DebuggerReply::Error {
                code: DebuggerErrorCode::InvalidTarget | DebuggerErrorCode::InvalidExecutionState,
                ..
            }
        ));
    }

    navigate(&mut browser, &url);
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::GetStaticScopeRelation {
                target: linked_target,
            },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::StaleRealm | DebuggerErrorCode::CapabilityUnavailable,
            ..
        }
    ));
    for request in [
        DebuggerRequest::GetLinkedExecutionState { entry },
        DebuggerRequest::GetLinkedScopes {
            expected_stack: stack,
            max_scope_entries: 256,
        },
        DebuggerRequest::GetLinkedStackCoordinates { target },
        DebuggerRequest::ArmLinkedNestedSafePointBreakpoint { target: arm },
        DebuggerRequest::ResolveStaticMetadataSourceBreakpoint {
            target: DebuggerStaticMetadataSourceBreakpointTarget {
                source: dependency_source,
                source_byte: LINKED_DEPENDENCY_SOURCE.find("function inner").unwrap() as u32,
            },
        },
        DebuggerRequest::ResolveStaticMetadataSourceBreakpoint {
            target: DebuggerStaticMetadataSourceBreakpointTarget {
                source: entry_source,
                source_byte: LINKED_ENTRY_SOURCE.find("export const answer").unwrap() as u32,
            },
        },
        DebuggerRequest::DescribeStaticMetadataSymbolLocation {
            target: DebuggerStaticMetadataSymbolLocationTarget {
                symbol: matched_symbols[0],
                source: dependency_source,
            },
        },
        DebuggerRequest::DescribeStaticMetadataSymbolLocation {
            target: DebuggerStaticMetadataSymbolLocationTarget {
                symbol: matched_symbols[1],
                source: entry_source,
            },
        },
    ] {
        let reply = debugger_request(&mut debugger, request.clone());
        assert!(
            matches!(
                reply,
                DebuggerReply::Error {
                    code: DebuggerErrorCode::StaleRealm,
                    ..
                }
            ),
            "stale linked request {request:?} returned {reply:?}"
        );
    }
    stop_tx.send(()).unwrap();
    let requested = fixture.join().unwrap();
    assert!(requested.iter().filter(|path| path.as_str() == "/").count() >= 2);
    assert!(requested.contains(&"/linked-entry.ts".to_string()));
    assert!(requested.contains(&"/linked-dependency.ts".to_string()));
    drop(debugger);
    launcher.shutdown();
    let _ = std::fs::remove_file(policy_file);
    let _ = std::fs::remove_file(gatekeeper_socket);
}

#[test]
fn launcher_arms_only_a_receipted_module_root_source_position() {
    let gatekeeper_socket = clearing_gatekeeper();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let fixture = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0_u8; 1024];
        let _ = stream.read(&mut request);
        let body = format!(
            "<script type=\"application/x-blueice-typescript-module\">{MODULE_SYMBOL_BREAKPOINT_BLUETS_SOURCE}</script>"
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
            symbol_inventory: true,
            symbol_display: true,
            symbol_location: true,
            safe_point_span: true,
            source_breakpoint: true,
            ..StaticMetadataPolicy::default()
        },
    );
    let mut browser = launcher.connect_browser();
    blueice_ipc::client_handshake(&mut browser).unwrap();
    navigate(&mut browser, &url);
    fixture.join().unwrap();

    let manifest =
        DebuggerMetadataCapabilityManifest::opaque_selected(DebuggerMetadataCapabilitySelection {
            symbol_display: true,
            symbol_location: true,
            safe_point_span: true,
            source_breakpoint: true,
            ..DebuggerMetadataCapabilitySelection::default()
        });
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
    let DebuggerReply::StaticMetadata(metadata) = debugger_request(
        &mut debugger,
        DebuggerRequest::ListStaticMetadata { program },
    ) else {
        panic!("pending typed module must expose one metadata receipt")
    };
    assert_eq!(metadata.len(), 1);
    let guessed = DebuggerStaticMetadataSourceBreakpointTarget {
        source: DebuggerStaticMetadataSourceId {
            metadata: metadata[0],
            source_id: 0,
        },
        source_byte: MODULE_SYMBOL_BREAKPOINT_BLUETS_SOURCE
            .find("const rootValue")
            .unwrap() as u32,
    };
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::ArmStaticMetadataSourceBreakpoint { target: guessed },
        ),
        DebuggerReply::Unsupported { .. }
    ));
    let DebuggerReply::StaticMetadataSources(sources) = debugger_request(
        &mut debugger,
        DebuggerRequest::ListStaticMetadataSources {
            metadata: metadata[0],
        },
    ) else {
        panic!("pending typed module must expose its original source receipt")
    };
    assert_eq!(sources.len(), 1);
    let child = DebuggerStaticMetadataSourceBreakpointTarget {
        source: sources[0],
        source_byte: MODULE_SYMBOL_BREAKPOINT_BLUETS_SOURCE
            .find("function inner")
            .unwrap() as u32,
    };
    let DebuggerReply::StaticMetadataSourceBreakpoint(child_binding) = debugger_request(
        &mut debugger,
        DebuggerRequest::ResolveStaticMetadataSourceBreakpoint { target: child },
    ) else {
        panic!("module function declaration must resolve to its child entry")
    };
    assert_eq!(child_binding.safe_point.unwrap().code_unit_ordinal, 1);
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::ArmStaticMetadataSourceBreakpoint { target: child },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            ..
        }
    ));
    let unbound = DebuggerStaticMetadataSourceBreakpointTarget {
        source_byte: MODULE_SYMBOL_BREAKPOINT_BLUETS_SOURCE.len() as u32,
        ..child
    };
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::ArmStaticMetadataSourceBreakpoint { target: unbound },
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
            state: DebuggerExecutionState::Pending,
        }
    );
    let root = DebuggerStaticMetadataSourceBreakpointTarget {
        source: sources[0],
        ..guessed
    };
    let DebuggerReply::StaticMetadataSymbols(symbols) = debugger_request(
        &mut debugger,
        DebuggerRequest::ListStaticMetadataSymbols {
            metadata: metadata[0],
        },
    ) else {
        panic!("module symbol composition requires its own inventory receipt")
    };
    let mut runtime_symbol = None;
    let mut type_only_symbol = None;
    for symbol in symbols {
        let DebuggerReply::StaticMetadataSymbol(display) = debugger_request(
            &mut debugger,
            DebuggerRequest::DescribeStaticMetadataSymbol { symbol },
        ) else {
            panic!("module symbol composition requires a separately granted kind")
        };
        let DebuggerReply::StaticMetadataSymbolLocation(location) = debugger_request(
            &mut debugger,
            DebuggerRequest::DescribeStaticMetadataSymbolLocation {
                target: DebuggerStaticMetadataSymbolLocationTarget {
                    symbol,
                    source: sources[0],
                },
            },
        ) else {
            panic!("module symbol composition requires its original declaration")
        };
        match display.display.as_str() {
            "rootValue" => runtime_symbol = Some((display, location)),
            "Shape" => type_only_symbol = Some((display, location)),
            _ => {}
        }
    }
    let (root_display, root_location) = runtime_symbol.expect("root variable symbol must exist");
    assert_eq!(
        root_display.kind,
        DebuggerStaticMetadataSymbolKind::Variable
    );
    assert_eq!(root_location.source, root.source);
    assert_eq!(root_location.start_byte, root.source_byte);
    assert_eq!(
        &MODULE_SYMBOL_BREAKPOINT_BLUETS_SOURCE
            [root_location.start_byte as usize..root_location.end_byte as usize],
        "const rootValue: number = 1;"
    );
    let DebuggerReply::StaticMetadataSourceBreakpoint(root_binding) = debugger_request(
        &mut debugger,
        DebuggerRequest::ResolveStaticMetadataSourceBreakpoint { target: root },
    ) else {
        panic!("root symbol declaration must resolve under its own source receipt")
    };
    let root_point = root_binding.safe_point.unwrap();
    let DebuggerReply::StaticMetadataSafePointSpan(root_span) = debugger_request(
        &mut debugger,
        DebuggerRequest::DescribeStaticMetadataSafePointSpan {
            target: DebuggerStaticMetadataSafePointSpanTarget {
                safe_point: root_point,
                source: root.source,
            },
        },
    ) else {
        panic!("root symbol candidate requires a separately granted exact span")
    };
    assert_eq!(
        root_location.executable_breakpoint_candidate(&root_display, root_binding, root_span),
        Some(root_point)
    );
    let (type_display, type_location) = type_only_symbol.expect("type-only symbol must exist");
    assert_eq!(
        type_display.kind,
        DebuggerStaticMetadataSymbolKind::Interface
    );
    let type_target = DebuggerStaticMetadataSourceBreakpointTarget {
        source: sources[0],
        source_byte: type_location.start_byte,
    };
    let DebuggerReply::StaticMetadataSourceBreakpoint(type_binding) = debugger_request(
        &mut debugger,
        DebuggerRequest::ResolveStaticMetadataSourceBreakpoint {
            target: type_target,
        },
    ) else {
        panic!("type-only source position must return an explicit binding result")
    };
    assert_eq!(
        type_location.executable_breakpoint_candidate(&type_display, type_binding, root_span),
        None,
        "an interface must not be retargeted to a later executable declaration"
    );
    assert_eq!(
        root_location.executable_breakpoint_candidate(
            &root_display,
            blueice_ipc::debugger::DebuggerStaticMetadataSourceBreakpoint {
                safe_point: None,
                ..root_binding
            },
            root_span,
        ),
        None,
        "an unbound executable declaration must not arm a later instruction"
    );
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::GetExecutionState { program }
        ),
        DebuggerReply::ExecutionState {
            program,
            state: DebuggerExecutionState::Pending,
        }
    );
    let DebuggerReply::RootSafePointBreakpointArmed { safe_point } = debugger_request(
        &mut debugger,
        DebuggerRequest::ArmStaticMetadataSourceBreakpoint {
            target: root_binding.target,
        },
    ) else {
        panic!("receipted module root position must atomically arm its own safe point")
    };
    assert_eq!(safe_point, root_point);
    assert_eq!(safe_point.program, program);
    assert_eq!(safe_point.code_unit_ordinal, 0);
    await_paused_execution(&mut debugger, program, safe_point);
    assert_eq!(
        debugger_request(&mut debugger, DebuggerRequest::ResumeExecution { program }),
        DebuggerReply::ExecutionResumed { program }
    );
    await_completed_execution(&mut debugger, program);
    drop(debugger);
    for requested in [DebuggerMetadataCapabilityManifest::empty(), manifest] {
        let mut separate = UnixStream::connect(&launcher.debugger_socket).unwrap();
        assert_eq!(
            debugger_request(
                &mut separate,
                DebuggerRequest::Hello {
                    protocol_version: DEBUGGER_PROTOCOL_VERSION,
                    requested_bounded_values: false,
                    requested_metadata_capabilities: requested.clone(),
                },
            ),
            DebuggerReply::HelloAck {
                protocol_version: DEBUGGER_PROTOCOL_VERSION,
                granted_bounded_values: false,
                granted_metadata_capabilities: requested,
            }
        );
        for request in [
            DebuggerRequest::ResolveStaticMetadataSourceBreakpoint { target: root },
            DebuggerRequest::DescribeStaticMetadataSymbol {
                symbol: root_display.symbol,
            },
            DebuggerRequest::DescribeStaticMetadataSymbolLocation {
                target: DebuggerStaticMetadataSymbolLocationTarget {
                    symbol: root_display.symbol,
                    source: root.source,
                },
            },
            DebuggerRequest::DescribeStaticMetadataSafePointSpan {
                target: DebuggerStaticMetadataSafePointSpanTarget {
                    safe_point: root_point,
                    source: root.source,
                },
            },
        ] {
            let reply = debugger_request(&mut separate, request.clone());
            assert!(
                matches!(
                    reply,
                    DebuggerReply::Unsupported { .. }
                        | DebuggerReply::Error {
                            code: DebuggerErrorCode::CapabilityUnavailable,
                            ..
                        }
                ),
                "{request:?} returned {reply:?}"
            );
        }
    }
    launcher.shutdown();
    let _ = std::fs::remove_file(gatekeeper_socket);
}
