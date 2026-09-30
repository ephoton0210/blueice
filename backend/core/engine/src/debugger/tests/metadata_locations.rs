// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn static_metadata_symbol_location_requires_exact_symbol_and_source_receipts() {
    let (tabs, realm) = loaded_tabs();
    let program = DebuggerProgram {
        realm,
        program_handle: 7,
        program_generation: 3,
    };
    let metadata = DebuggerStaticMetadataHandle {
        program,
        metadata_handle: 41,
        metadata_generation: 9,
    };
    let symbol = DebuggerStaticMetadataSymbolId {
        metadata,
        symbol_id: 0,
    };
    let source = DebuggerStaticMetadataSourceId {
        metadata,
        source_id: 0,
    };
    let target = DebuggerStaticMetadataSymbolLocationTarget { symbol, source };
    let hello = DebuggerRequest::Hello {
        protocol_version: DEBUGGER_PROTOCOL_VERSION,
        requested_bounded_values: false,
        requested_metadata_capabilities:
            blueice_ipc::debugger::DebuggerMetadataCapabilityManifest::opaque_symbol_location(),
    };
    let hello_reply = blueice_ipc::debugger::negotiate(
        &hello,
        &blueice_ipc::debugger::DebuggerMetadataCapabilityManifest::opaque_symbol_location(),
    );
    let session = blueice_ipc::debugger::metadata_session_authorization(&hello, &hello_reply)
        .expect("dependent symbol-location policy must create a core-local session authorization");
    let mut locations = MetadataLocations {
        malformed_summary: false,
        malformed_provenance: false,
        malformed_lowering_summary: false,
        mismatched_symbol_display: false,
        mismatched_contract_display: false,
        mismatched_contract_validation: false,
    };
    let DebuggerReply::Capabilities(capabilities) = handle_debugger_request_with_child_locations(
        &tabs,
        &mut locations,
        Some(&session),
        DebuggerRequest::DescribeCapabilities { realm },
    ) else {
        panic!("live realm symbol-location capability discovery must succeed")
    };
    assert!(capabilities.reports.iter().any(|report| {
        report.capability == DebuggerCapability::StaticMetadataSymbolLocation
            && report.state == DebuggerCapabilityState::Available
    }));
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&session),
            DebuggerRequest::DescribeStaticMetadataSymbolLocation { target },
        ),
        unavailable_static_metadata_symbol_location(),
        "a caller cannot probe a child location before both IDs crossed this stream"
    );
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&session),
            DebuggerRequest::ListStaticMetadata { program },
        ),
        DebuggerReply::StaticMetadata(vec![metadata])
    );
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&session),
            DebuggerRequest::ListStaticMetadataSources { metadata },
        ),
        DebuggerReply::StaticMetadataSources(vec![source])
    );
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&session),
            DebuggerRequest::ListStaticMetadataSymbols { metadata },
        ),
        DebuggerReply::StaticMetadataSymbols(vec![
            symbol,
            DebuggerStaticMetadataSymbolId {
                metadata,
                symbol_id: 1,
            },
            DebuggerStaticMetadataSymbolId {
                metadata,
                symbol_id: 2,
            },
        ])
    );
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&session),
            DebuggerRequest::DescribeStaticMetadataSymbolLocation { target },
        ),
        DebuggerReply::StaticMetadataSymbolLocation(DebuggerStaticMetadataSymbolLocation {
            symbol,
            source,
            start_byte: 6,
            end_byte: 31,
            coordinates: DebuggerSourceCoordinates {
                start_line: 0,
                start_column_utf16: 6,
                end_line: 0,
                end_column_utf16: 31,
            },
        })
    );
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&session),
            DebuggerRequest::DescribeStaticMetadataSymbolLocation {
                target: DebuggerStaticMetadataSymbolLocationTarget {
                    source: DebuggerStaticMetadataSourceId {
                        source_id: 1,
                        ..source
                    },
                    ..target
                },
            },
        ),
        unavailable_static_metadata_symbol_location(),
        "a guessed source ID cannot be paired with an observed symbol"
    );
}

#[test]
fn exception_location_requires_its_own_grant_and_exact_stream_source_receipt() {
    let (tabs, realm) = loaded_tabs();
    let program = DebuggerProgram {
        realm,
        program_handle: 7,
        program_generation: 3,
    };
    let metadata = DebuggerStaticMetadataHandle {
        program,
        metadata_handle: 41,
        metadata_generation: 9,
    };
    let source = DebuggerStaticMetadataSourceId {
        metadata,
        source_id: 0,
    };
    let manifest =
        blueice_ipc::debugger::DebuggerMetadataCapabilityManifest::opaque_safe_point_span();
    let hello = DebuggerRequest::Hello {
        protocol_version: DEBUGGER_PROTOCOL_VERSION,
        requested_bounded_values: false,
        requested_metadata_capabilities: manifest.clone(),
    };
    let ack = blueice_ipc::debugger::negotiate(&hello, &manifest);
    let session = blueice_ipc::debugger::metadata_session_authorization(&hello, &ack).unwrap();
    let mut locations = MetadataLocations {
        malformed_summary: false,
        malformed_provenance: false,
        malformed_lowering_summary: false,
        mismatched_symbol_display: false,
        mismatched_contract_display: false,
        mismatched_contract_validation: false,
    };
    let request = DebuggerRequest::DescribeExceptionLocation { source };
    assert_eq!(
        handle_debugger_request_with_child_locations(&tabs, &mut locations, None, request.clone(),),
        unavailable_exception_location()
    );
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&session),
            request.clone(),
        ),
        invalid_exception_location()
    );
    let inventory_only =
        blueice_ipc::debugger::DebuggerMetadataCapabilityManifest::opaque_source_inventory();
    let denied_ack = blueice_ipc::debugger::negotiate(&hello, &inventory_only);
    let denied_session =
        blueice_ipc::debugger::metadata_session_authorization(&hello, &denied_ack).unwrap();
    assert!(denied_session.observe_metadata(&[metadata]));
    assert!(denied_session.observe_sources(&[source]));
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&denied_session),
            request.clone(),
        ),
        unavailable_exception_location()
    );
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&session),
            DebuggerRequest::ListStaticMetadata { program },
        ),
        DebuggerReply::StaticMetadata(vec![metadata])
    );
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&session),
            request.clone(),
        ),
        invalid_exception_location()
    );
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&session),
            DebuggerRequest::ListStaticMetadataSources { metadata },
        ),
        DebuggerReply::StaticMetadataSources(vec![source])
    );
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&session),
            request.clone(),
        ),
        DebuggerReply::ExceptionLocation(DebuggerExceptionLocation {
            source,
            safe_point: DebuggerSafePoint {
                program,
                code_unit_ordinal: 1,
                bytecode_offset: 4,
            },
            start_byte: 6,
            end_byte: 31,
            coordinates: DebuggerSourceCoordinates {
                start_line: 0,
                start_column_utf16: 6,
                end_line: 0,
                end_column_utf16: 31,
            },
        })
    );
    let separate_session =
        blueice_ipc::debugger::metadata_session_authorization(&hello, &ack).unwrap();
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&separate_session),
            request,
        ),
        invalid_exception_location()
    );
    let other_source = DebuggerStaticMetadataSourceId {
        source_id: 1,
        ..source
    };
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&session),
            DebuggerRequest::DescribeExceptionLocation {
                source: other_source,
            },
        ),
        invalid_exception_location()
    );
    assert!(session.observe_sources(&[other_source]));
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&session),
            DebuggerRequest::DescribeExceptionLocation {
                source: other_source,
            },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidExecutionState,
            message: "BlueTS program has no terminal uncaught exception location".to_string(),
        }
    );
}

#[test]
fn static_metadata_safe_point_span_requires_its_own_grant_and_source_receipt() {
    let (tabs, realm) = loaded_tabs();
    let program = DebuggerProgram {
        realm,
        program_handle: 7,
        program_generation: 3,
    };
    let metadata = DebuggerStaticMetadataHandle {
        program,
        metadata_handle: 41,
        metadata_generation: 9,
    };
    let source = DebuggerStaticMetadataSourceId {
        metadata,
        source_id: 0,
    };
    let safe_point = DebuggerSafePoint {
        program,
        code_unit_ordinal: 0,
        bytecode_offset: 4,
    };
    let target = DebuggerStaticMetadataSafePointSpanTarget { safe_point, source };
    let manifest =
        blueice_ipc::debugger::DebuggerMetadataCapabilityManifest::opaque_safe_point_span();
    let hello = DebuggerRequest::Hello {
        protocol_version: DEBUGGER_PROTOCOL_VERSION,
        requested_bounded_values: false,
        requested_metadata_capabilities: manifest.clone(),
    };
    let reply = blueice_ipc::debugger::negotiate(&hello, &manifest);
    let session = blueice_ipc::debugger::metadata_session_authorization(&hello, &reply)
        .expect("explicit safe-point span grant must create a session");
    let mut locations = MetadataLocations {
        malformed_summary: false,
        malformed_provenance: false,
        malformed_lowering_summary: false,
        mismatched_symbol_display: false,
        mismatched_contract_display: false,
        mismatched_contract_validation: false,
    };
    let DebuggerReply::Capabilities(capabilities) = handle_debugger_request_with_child_locations(
        &tabs,
        &mut locations,
        Some(&session),
        DebuggerRequest::DescribeCapabilities { realm },
    ) else {
        panic!("live child must report metadata capabilities");
    };
    assert!(capabilities.reports.iter().any(|report| {
        report.capability == DebuggerCapability::StaticMetadataSafePointSpan
            && report.state == DebuggerCapabilityState::Available
    }));
    let request = DebuggerRequest::DescribeStaticMetadataSafePointSpan { target };
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&session),
            request.clone(),
        ),
        unavailable_static_metadata_safe_point_span()
    );
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&session),
            DebuggerRequest::ListStaticMetadata { program },
        ),
        DebuggerReply::StaticMetadata(vec![metadata])
    );
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&session),
            request.clone(),
        ),
        unavailable_static_metadata_safe_point_span()
    );
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&session),
            DebuggerRequest::ListStaticMetadataSources { metadata },
        ),
        DebuggerReply::StaticMetadataSources(vec![source])
    );
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&session),
            request.clone(),
        ),
        DebuggerReply::StaticMetadataSafePointSpan(DebuggerStaticMetadataSafePointSpan {
            safe_point,
            source,
            start_byte: 6,
            end_byte: 31,
            coordinates: DebuggerSourceCoordinates {
                start_line: 0,
                start_column_utf16: 6,
                end_line: 0,
                end_column_utf16: 31,
            },
        })
    );
    let separate_session = blueice_ipc::debugger::metadata_session_authorization(&hello, &reply)
        .expect("a second stream must negotiate independently");
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&separate_session),
            request.clone(),
        ),
        unavailable_static_metadata_safe_point_span()
    );
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&session),
            DebuggerRequest::DescribeStaticMetadataSafePointSpan {
                target: DebuggerStaticMetadataSafePointSpanTarget {
                    source: DebuggerStaticMetadataSourceId {
                        source_id: 1,
                        ..source
                    },
                    ..target
                },
            },
        ),
        unavailable_static_metadata_safe_point_span()
    );
    assert!(matches!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&session),
            DebuggerRequest::DescribeStaticMetadataSafePointSpan {
                target: DebuggerStaticMetadataSafePointSpanTarget {
                    safe_point: DebuggerSafePoint {
                        bytecode_offset: 5,
                        ..safe_point
                    },
                    ..target
                },
            },
        ),
        DebuggerReply::Error { .. }
    ));
    let inventory_only =
        blueice_ipc::debugger::DebuggerMetadataCapabilityManifest::opaque_source_inventory();
    let denied_reply = blueice_ipc::debugger::negotiate(&hello, &inventory_only);
    let denied_session =
        blueice_ipc::debugger::metadata_session_authorization(&hello, &denied_reply).unwrap();
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&denied_session),
            request,
        ),
        unavailable_static_metadata_safe_point_span()
    );
}

#[test]
fn source_breakpoint_requires_its_own_receipt_and_core_revalidates_child_point() {
    let (tabs, realm) = loaded_tabs();
    let program = DebuggerProgram {
        realm,
        program_handle: 7,
        program_generation: 3,
    };
    let metadata = DebuggerStaticMetadataHandle {
        program,
        metadata_handle: 41,
        metadata_generation: 9,
    };
    let source = DebuggerStaticMetadataSourceId {
        metadata,
        source_id: 0,
    };
    let target = DebuggerStaticMetadataSourceBreakpointTarget {
        source,
        source_byte: 6,
    };
    let manifest =
        blueice_ipc::debugger::DebuggerMetadataCapabilityManifest::opaque_source_breakpoint();
    let hello = DebuggerRequest::Hello {
        protocol_version: DEBUGGER_PROTOCOL_VERSION,
        requested_bounded_values: false,
        requested_metadata_capabilities: manifest.clone(),
    };
    let reply = blueice_ipc::debugger::negotiate(&hello, &manifest);
    let session = blueice_ipc::debugger::metadata_session_authorization(&hello, &reply)
        .expect("source-breakpoint grant must create a session");
    let mut locations = MetadataLocations {
        malformed_summary: false,
        malformed_provenance: false,
        malformed_lowering_summary: false,
        mismatched_symbol_display: false,
        mismatched_contract_display: false,
        mismatched_contract_validation: false,
    };
    let request = DebuggerRequest::ResolveStaticMetadataSourceBreakpoint { target };
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&session),
            request.clone(),
        ),
        unavailable_static_metadata_source_breakpoint()
    );
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&session),
            DebuggerRequest::ArmStaticMetadataSourceBreakpoint { target },
        ),
        unavailable_static_metadata_source_breakpoint_arm(),
        "an unreceipted arm must fail before checking execution control"
    );
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&session),
            DebuggerRequest::ListStaticMetadata { program },
        ),
        DebuggerReply::StaticMetadata(vec![metadata])
    );
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&session),
            request.clone(),
        ),
        unavailable_static_metadata_source_breakpoint()
    );
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&session),
            DebuggerRequest::ListStaticMetadataSources { metadata },
        ),
        DebuggerReply::StaticMetadataSources(vec![source])
    );
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&session),
            request.clone(),
        ),
        DebuggerReply::StaticMetadataSourceBreakpoint(DebuggerStaticMetadataSourceBreakpoint {
            target,
            safe_point: Some(DebuggerSafePoint {
                program,
                code_unit_ordinal: 0,
                bytecode_offset: 4,
            }),
        })
    );
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&session),
            DebuggerRequest::ArmStaticMetadataSourceBreakpoint { target },
        ),
        unavailable_execution_control(),
        "metadata authority alone must not arm a page host without execution control"
    );
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&session),
            DebuggerRequest::ResolveStaticMetadataSourceBreakpoint {
                target: DebuggerStaticMetadataSourceBreakpointTarget {
                    source_byte: 31,
                    ..target
                },
            },
        ),
        DebuggerReply::StaticMetadataSourceBreakpoint(DebuggerStaticMetadataSourceBreakpoint {
            target: DebuggerStaticMetadataSourceBreakpointTarget {
                source_byte: 31,
                ..target
            },
            safe_point: None,
        })
    );
    assert!(matches!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&session),
            DebuggerRequest::ResolveStaticMetadataSourceBreakpoint {
                target: DebuggerStaticMetadataSourceBreakpointTarget {
                    source_byte: 7,
                    ..target
                },
            },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::CapabilityUnavailable,
            ..
        }
    ));
    let other_session =
        blueice_ipc::debugger::metadata_session_authorization(&hello, &reply).unwrap();
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&other_session),
            request.clone(),
        ),
        unavailable_static_metadata_source_breakpoint()
    );
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&session),
            DebuggerRequest::ResolveStaticMetadataSourceBreakpoint {
                target: DebuggerStaticMetadataSourceBreakpointTarget {
                    source: DebuggerStaticMetadataSourceId {
                        source_id: 1,
                        ..source
                    },
                    ..target
                },
            },
        ),
        unavailable_static_metadata_source_breakpoint()
    );
    let span_only =
        blueice_ipc::debugger::DebuggerMetadataCapabilityManifest::opaque_safe_point_span();
    let denied_reply = blueice_ipc::debugger::negotiate(&hello, &span_only);
    let denied_session =
        blueice_ipc::debugger::metadata_session_authorization(&hello, &denied_reply).unwrap();
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&denied_session),
            request,
        ),
        unavailable_static_metadata_source_breakpoint()
    );
}
