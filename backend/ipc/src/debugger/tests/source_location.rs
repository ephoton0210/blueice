// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn generation_bound_target_and_safe_point_ids_reject_zero_placeholders() {
    let valid_program = DebuggerProgram {
        realm: realm(),
        program_handle: 12,
        program_generation: 5,
    };
    assert!(valid_program.is_well_formed());
    assert!(DebuggerSafePoint {
        program: valid_program,
        code_unit_ordinal: 0,
        bytecode_offset: 0,
    }
    .is_well_formed());
    assert!(!DebuggerPageRealm {
        browser_context_id: 1,
        tab_id: 7,
        realm_generation: 0,
    }
    .is_well_formed());
    assert!(!DebuggerProgram {
        realm: realm(),
        program_handle: 0,
        program_generation: 5,
    }
    .is_well_formed());
}

#[test]
fn safe_point_span_manifest_and_wire_require_exact_parent_and_source() {
    let manifest = DebuggerMetadataCapabilityManifest::opaque_safe_point_span();
    assert!(manifest.is_well_formed());
    assert_eq!(
        manifest,
        DebuggerMetadataCapabilityManifest::opaque_selected(DebuggerMetadataCapabilitySelection {
            safe_point_span: true,
            ..DebuggerMetadataCapabilitySelection::default()
        })
    );
    assert!(!DebuggerMetadataCapabilityManifest {
        version: DEBUGGER_METADATA_CAPABILITY_MANIFEST_VERSION - 1,
        ..manifest.clone()
    }
    .is_well_formed());
    for capabilities in [
        vec![DebuggerMetadataCapability::OpaqueSafePointSpan],
        vec![
            DebuggerMetadataCapability::OpaqueInventory,
            DebuggerMetadataCapability::OpaqueSafePointSpan,
        ],
    ] {
        assert!(!DebuggerMetadataCapabilityManifest {
            version: DEBUGGER_METADATA_CAPABILITY_MANIFEST_VERSION,
            capabilities,
        }
        .is_well_formed());
    }
    let hello = hello(manifest.clone());
    let ack = negotiate(&hello, &manifest);
    let session = metadata_session_authorization(&hello, &ack).unwrap();
    assert!(session.permits(DebuggerMetadataCapability::OpaqueSafePointSpan));

    let program = DebuggerProgram {
        realm: realm(),
        program_handle: 12,
        program_generation: 5,
    };
    let source = DebuggerStaticMetadataSourceId {
        metadata: DebuggerStaticMetadataHandle {
            program,
            metadata_handle: 24,
            metadata_generation: 7,
        },
        source_id: 0,
    };
    let safe_point = DebuggerSafePoint {
        program,
        code_unit_ordinal: 0,
        bytecode_offset: 4,
    };
    let target = DebuggerStaticMetadataSafePointSpanTarget { safe_point, source };
    assert!(target.is_well_formed());
    assert!(!DebuggerStaticMetadataSafePointSpanTarget {
        source: DebuggerStaticMetadataSourceId {
            metadata: DebuggerStaticMetadataHandle {
                program: DebuggerProgram {
                    program_generation: 6,
                    ..program
                },
                ..source.metadata
            },
            ..source
        },
        ..target
    }
    .is_well_formed());
    let span = DebuggerStaticMetadataSafePointSpan {
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
    };
    assert!(span.is_well_formed());
    assert!(!DebuggerStaticMetadataSafePointSpan {
        end_byte: span.start_byte,
        ..span
    }
    .is_well_formed());
    assert!(!DebuggerStaticMetadataSafePointSpan {
        end_byte: DEBUGGER_STATIC_METADATA_MAX_SOURCE_SPAN_BYTES + 1,
        ..span
    }
    .is_well_formed());
    assert!(!DebuggerStaticMetadataSafePointSpan {
        coordinates: DebuggerSourceCoordinates {
            start_column_utf16: 32,
            ..span.coordinates
        },
        ..span
    }
    .is_well_formed());
    let request = DebuggerRequest::DescribeStaticMetadataSafePointSpan { target };
    let (mut sender, mut receiver) = UnixStream::pair().unwrap();
    write_debugger_request(&mut sender, &request).unwrap();
    assert_eq!(read_debugger_request(&mut receiver).unwrap(), request);
    let reply = DebuggerReply::StaticMetadataSafePointSpan(span);
    let (mut sender, mut receiver) = UnixStream::pair().unwrap();
    write_debugger_reply(&mut sender, &reply).unwrap();
    assert_eq!(read_debugger_reply(&mut receiver).unwrap(), reply);
}

#[test]
fn terminal_exception_location_round_trips_only_receipted_source_and_span() {
    let program = DebuggerProgram {
        realm: realm(),
        program_handle: 12,
        program_generation: 5,
    };
    let source = DebuggerStaticMetadataSourceId {
        metadata: DebuggerStaticMetadataHandle {
            program,
            metadata_handle: 24,
            metadata_generation: 7,
        },
        source_id: 0,
    };
    let location = DebuggerExceptionLocation {
        source,
        safe_point: DebuggerSafePoint {
            program,
            code_unit_ordinal: 1,
            bytecode_offset: 4,
        },
        start_byte: 2,
        end_byte: 31,
        coordinates: DebuggerSourceCoordinates {
            start_line: 0,
            start_column_utf16: 2,
            end_line: 0,
            end_column_utf16: 31,
        },
    };
    assert!(location.is_well_formed());
    assert!(!DebuggerExceptionLocation {
        safe_point: DebuggerSafePoint {
            program: DebuggerProgram {
                program_generation: 6,
                ..program
            },
            ..location.safe_point
        },
        ..location
    }
    .is_well_formed());
    assert!(!DebuggerExceptionLocation {
        end_byte: location.start_byte,
        ..location
    }
    .is_well_formed());
    let request = DebuggerRequest::DescribeExceptionLocation { source };
    let (mut sender, mut receiver) = UnixStream::pair().unwrap();
    write_debugger_request(&mut sender, &request).unwrap();
    assert_eq!(read_debugger_request(&mut receiver).unwrap(), request);
    let reply = DebuggerReply::ExceptionLocation(location);
    let (mut sender, mut receiver) = UnixStream::pair().unwrap();
    write_debugger_reply(&mut sender, &reply).unwrap();
    assert_eq!(read_debugger_reply(&mut receiver).unwrap(), reply);
    assert!(!format!("{reply:?}").contains("throw"));
}

#[test]
fn source_span_step_requires_its_own_grant_and_round_trips_limit_state() {
    let manifest = DebuggerMetadataCapabilityManifest::opaque_source_span_step();
    assert!(manifest.is_well_formed());
    assert_eq!(
        manifest,
        DebuggerMetadataCapabilityManifest::opaque_selected(DebuggerMetadataCapabilitySelection {
            source_span_step: true,
            ..DebuggerMetadataCapabilitySelection::default()
        })
    );
    assert!(!DebuggerMetadataCapabilityManifest {
        version: DEBUGGER_METADATA_CAPABILITY_MANIFEST_VERSION,
        capabilities: vec![
            DebuggerMetadataCapability::OpaqueInventory,
            DebuggerMetadataCapability::OpaqueSourceInventory,
            DebuggerMetadataCapability::OpaqueSourceSpanStep,
        ],
    }
    .is_well_formed());
    let program = DebuggerProgram {
        realm: realm(),
        program_handle: 12,
        program_generation: 5,
    };
    let safe_point = DebuggerSafePoint {
        program,
        code_unit_ordinal: 0,
        bytecode_offset: 4,
    };
    let target = DebuggerStaticMetadataSafePointSpanTarget {
        safe_point,
        source: DebuggerStaticMetadataSourceId {
            metadata: DebuggerStaticMetadataHandle {
                program,
                metadata_handle: 24,
                metadata_generation: 7,
            },
            source_id: 0,
        },
    };
    let request = DebuggerRequest::StepStaticMetadataSourceSpan { target };
    let (mut sender, mut receiver) = UnixStream::pair().unwrap();
    write_debugger_request(&mut sender, &request).unwrap();
    assert_eq!(read_debugger_request(&mut receiver).unwrap(), request);
    for reply in [
        DebuggerReply::ExecutionSourceSpanStepRequested { safe_point },
        DebuggerReply::ExecutionState {
            program,
            state: DebuggerExecutionState::SourceStepLimitReached { safe_point },
        },
    ] {
        let (mut sender, mut receiver) = UnixStream::pair().unwrap();
        write_debugger_reply(&mut sender, &reply).unwrap();
        assert_eq!(read_debugger_reply(&mut receiver).unwrap(), reply);
    }
}

#[test]
fn source_breakpoint_is_separately_granted_and_bound_to_one_source_position() {
    let manifest = DebuggerMetadataCapabilityManifest::opaque_source_breakpoint();
    assert!(manifest.is_well_formed());
    assert_eq!(
        manifest,
        DebuggerMetadataCapabilityManifest::opaque_selected(DebuggerMetadataCapabilitySelection {
            source_breakpoint: true,
            ..DebuggerMetadataCapabilitySelection::default()
        })
    );
    assert!(!manifest.contains(DebuggerMetadataCapability::OpaqueSafePointSpan));
    assert!(
        !DebuggerMetadataCapabilityManifest::opaque_safe_point_span()
            .contains(DebuggerMetadataCapability::OpaqueSourceBreakpoint)
    );
    for capabilities in [
        vec![DebuggerMetadataCapability::OpaqueSourceBreakpoint],
        vec![
            DebuggerMetadataCapability::OpaqueInventory,
            DebuggerMetadataCapability::OpaqueSourceBreakpoint,
        ],
    ] {
        assert!(!DebuggerMetadataCapabilityManifest {
            version: DEBUGGER_METADATA_CAPABILITY_MANIFEST_VERSION,
            capabilities,
        }
        .is_well_formed());
    }
    let hello = hello(manifest.clone());
    let ack = negotiate(&hello, &manifest);
    let session = metadata_session_authorization(&hello, &ack).unwrap();
    assert!(session.permits(DebuggerMetadataCapability::OpaqueSourceBreakpoint));

    let program = DebuggerProgram {
        realm: realm(),
        program_handle: 12,
        program_generation: 5,
    };
    let target = DebuggerStaticMetadataSourceBreakpointTarget {
        source: DebuggerStaticMetadataSourceId {
            metadata: DebuggerStaticMetadataHandle {
                program,
                metadata_handle: 24,
                metadata_generation: 7,
            },
            source_id: 0,
        },
        source_byte: 31,
    };
    assert!(target.is_well_formed());
    assert!(!DebuggerStaticMetadataSourceBreakpointTarget {
        source_byte: DEBUGGER_STATIC_METADATA_MAX_SOURCE_SPAN_BYTES + 1,
        ..target
    }
    .is_well_formed());
    let result = DebuggerStaticMetadataSourceBreakpoint {
        target,
        safe_point: Some(DebuggerSafePoint {
            program,
            code_unit_ordinal: 0,
            bytecode_offset: 4,
        }),
    };
    assert!(result.is_well_formed());
    assert!(DebuggerStaticMetadataSourceBreakpoint {
        safe_point: None,
        ..result
    }
    .is_well_formed());
    assert!(!DebuggerStaticMetadataSourceBreakpoint {
        safe_point: Some(DebuggerSafePoint {
            program: DebuggerProgram {
                program_generation: 6,
                ..program
            },
            ..result.safe_point.unwrap()
        }),
        ..result
    }
    .is_well_formed());
    let request = DebuggerRequest::ResolveStaticMetadataSourceBreakpoint { target };
    let (mut sender, mut receiver) = UnixStream::pair().unwrap();
    write_debugger_request(&mut sender, &request).unwrap();
    assert_eq!(read_debugger_request(&mut receiver).unwrap(), request);
    let arm = DebuggerRequest::ArmStaticMetadataSourceBreakpoint { target };
    let (mut sender, mut receiver) = UnixStream::pair().unwrap();
    write_debugger_request(&mut sender, &arm).unwrap();
    assert_eq!(read_debugger_request(&mut receiver).unwrap(), arm);
    assert!(matches!(
        negotiate(&arm, &manifest),
        DebuggerReply::Error {
            code: DebuggerErrorCode::ProtocolVersion,
            ..
        }
    ));
    let reply = DebuggerReply::StaticMetadataSourceBreakpoint(result);
    let (mut sender, mut receiver) = UnixStream::pair().unwrap();
    write_debugger_reply(&mut sender, &reply).unwrap();
    assert_eq!(read_debugger_reply(&mut receiver).unwrap(), reply);
}
