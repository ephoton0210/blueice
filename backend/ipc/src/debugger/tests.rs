// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;
use std::os::unix::net::UnixStream;

fn realm() -> DebuggerPageRealm {
    DebuggerPageRealm {
        browser_context_id: 1,
        tab_id: 7,
        realm_generation: 3,
    }
}

mod authorization_detail;
mod authorization_inventory;
mod scope_values;
mod shape_contracts;
mod source_location;

fn capabilities(reports: Vec<DebuggerCapabilityReport>) -> DebuggerCapabilities {
    DebuggerCapabilities {
        protocol_version: DEBUGGER_PROTOCOL_VERSION,
        realm: realm(),
        reports,
        max_stack_frames: 64,
        max_scope_bindings: 256,
        max_value_preview_bytes: 4_096,
        max_safe_points_per_program: 4_096,
        max_breakpoints_per_realm: 256,
    }
}

fn capability_report(
    capability: DebuggerCapability,
    state: DebuggerCapabilityState,
) -> DebuggerCapabilityReport {
    DebuggerCapabilityReport {
        capability,
        state,
        detail: "test capability report".to_string(),
    }
}

#[test]
fn denied_source_text_probe_keeps_stream_framing() {
    let program = DebuggerProgram {
        realm: realm(),
        program_handle: 11,
        program_generation: 13,
    };
    let attempted_source_read = serde_json::to_vec(&serde_json::json!({
        "GetSourceText": {"program": program}
    }))
    .unwrap();
    let (mut sender, mut receiver) = UnixStream::pair().unwrap();
    sender
        .write_all(
            &u32::try_from(attempted_source_read.len())
                .unwrap()
                .to_le_bytes(),
        )
        .unwrap();
    sender.write_all(&attempted_source_read).unwrap();
    write_debugger_request(&mut sender, &DebuggerRequest::ListPageRealms).unwrap();
    assert_eq!(
        read_debugger_request(&mut receiver).unwrap(),
        DebuggerRequest::GetSourceText { program }
    );
    assert_eq!(
        read_debugger_request(&mut receiver).unwrap(),
        DebuggerRequest::ListPageRealms
    );
    assert!(matches!(
        negotiate(
            &DebuggerRequest::GetSourceText { program },
            &DebuggerMetadataCapabilityManifest::empty(),
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::ProtocolVersion,
            ..
        }
    ));
}

#[test]
fn request_and_reply_round_trip_on_a_real_socket() {
    for request in [
        DebuggerRequest::Hello {
            protocol_version: DEBUGGER_PROTOCOL_VERSION,
            requested_metadata_capabilities: DebuggerMetadataCapabilityManifest::empty(),
            requested_bounded_values: false,
        },
        DebuggerRequest::ListPageRealms,
        DebuggerRequest::DescribeCapabilities { realm: realm() },
        DebuggerRequest::ListPrograms { realm: realm() },
        DebuggerRequest::ListStaticMetadata {
            program: DebuggerProgram {
                realm: realm(),
                program_handle: 12,
                program_generation: 5,
            },
        },
        DebuggerRequest::DescribeStaticMetadata {
            metadata: DebuggerStaticMetadataHandle {
                program: DebuggerProgram {
                    realm: realm(),
                    program_handle: 12,
                    program_generation: 5,
                },
                metadata_handle: 24,
                metadata_generation: 7,
            },
        },
        DebuggerRequest::DescribeStaticMetadataLoweringSummary {
            metadata: DebuggerStaticMetadataHandle {
                program: DebuggerProgram {
                    realm: realm(),
                    program_handle: 12,
                    program_generation: 5,
                },
                metadata_handle: 24,
                metadata_generation: 7,
            },
        },
        DebuggerRequest::ListStaticMetadataSources {
            metadata: DebuggerStaticMetadataHandle {
                program: DebuggerProgram {
                    realm: realm(),
                    program_handle: 12,
                    program_generation: 5,
                },
                metadata_handle: 24,
                metadata_generation: 7,
            },
        },
        DebuggerRequest::ListStaticMetadataTypes {
            metadata: DebuggerStaticMetadataHandle {
                program: DebuggerProgram {
                    realm: realm(),
                    program_handle: 12,
                    program_generation: 5,
                },
                metadata_handle: 24,
                metadata_generation: 7,
            },
        },
        DebuggerRequest::DescribeStaticMetadataType {
            static_type: DebuggerStaticMetadataTypeId {
                metadata: DebuggerStaticMetadataHandle {
                    program: DebuggerProgram {
                        realm: realm(),
                        program_handle: 12,
                        program_generation: 5,
                    },
                    metadata_handle: 24,
                    metadata_generation: 7,
                },
                type_id: 0,
            },
        },
        DebuggerRequest::ListStaticMetadataSymbols {
            metadata: DebuggerStaticMetadataHandle {
                program: DebuggerProgram {
                    realm: realm(),
                    program_handle: 12,
                    program_generation: 5,
                },
                metadata_handle: 24,
                metadata_generation: 7,
            },
        },
        DebuggerRequest::DescribeStaticMetadataSymbol {
            symbol: DebuggerStaticMetadataSymbolId {
                metadata: DebuggerStaticMetadataHandle {
                    program: DebuggerProgram {
                        realm: realm(),
                        program_handle: 12,
                        program_generation: 5,
                    },
                    metadata_handle: 24,
                    metadata_generation: 7,
                },
                symbol_id: 0,
            },
        },
        DebuggerRequest::DescribeStaticMetadataSymbolLocation {
            target: DebuggerStaticMetadataSymbolLocationTarget {
                symbol: DebuggerStaticMetadataSymbolId {
                    metadata: DebuggerStaticMetadataHandle {
                        program: DebuggerProgram {
                            realm: realm(),
                            program_handle: 12,
                            program_generation: 5,
                        },
                        metadata_handle: 24,
                        metadata_generation: 7,
                    },
                    symbol_id: 0,
                },
                source: DebuggerStaticMetadataSourceId {
                    metadata: DebuggerStaticMetadataHandle {
                        program: DebuggerProgram {
                            realm: realm(),
                            program_handle: 12,
                            program_generation: 5,
                        },
                        metadata_handle: 24,
                        metadata_generation: 7,
                    },
                    source_id: 0,
                },
            },
        },
        DebuggerRequest::ListStaticMetadataContracts {
            metadata: DebuggerStaticMetadataHandle {
                program: DebuggerProgram {
                    realm: realm(),
                    program_handle: 12,
                    program_generation: 5,
                },
                metadata_handle: 24,
                metadata_generation: 7,
            },
        },
        DebuggerRequest::DescribeStaticMetadataContract {
            contract: DebuggerStaticMetadataContractId {
                metadata: DebuggerStaticMetadataHandle {
                    program: DebuggerProgram {
                        realm: realm(),
                        program_handle: 12,
                        program_generation: 5,
                    },
                    metadata_handle: 24,
                    metadata_generation: 7,
                },
                contract_id: 0,
            },
        },
        DebuggerRequest::ValidateStaticMetadataContract {
            contract: DebuggerStaticMetadataContractId {
                metadata: DebuggerStaticMetadataHandle {
                    program: DebuggerProgram {
                        realm: realm(),
                        program_handle: 12,
                        program_generation: 5,
                    },
                    metadata_handle: 24,
                    metadata_generation: 7,
                },
                contract_id: 0,
            },
            value: CompilerContractValue::Object(
                [("enabled".to_string(), CompilerContractValue::Boolean(true))]
                    .into_iter()
                    .collect(),
            ),
        },
        DebuggerRequest::ListSafePoints {
            program: DebuggerProgram {
                realm: realm(),
                program_handle: 12,
                program_generation: 5,
            },
        },
        DebuggerRequest::ValidateSafePoint {
            safe_point: DebuggerSafePoint {
                program: DebuggerProgram {
                    realm: realm(),
                    program_handle: 12,
                    program_generation: 5,
                },
                code_unit_ordinal: 0,
                bytecode_offset: 0,
            },
        },
        DebuggerRequest::SetBreakpoint {
            safe_point: DebuggerSafePoint {
                program: DebuggerProgram {
                    realm: realm(),
                    program_handle: 12,
                    program_generation: 5,
                },
                code_unit_ordinal: 0,
                bytecode_offset: 0,
            },
        },
        DebuggerRequest::ArmEntryBreakpoint {
            safe_point: DebuggerSafePoint {
                program: DebuggerProgram {
                    realm: realm(),
                    program_handle: 12,
                    program_generation: 5,
                },
                code_unit_ordinal: 0,
                bytecode_offset: 0,
            },
        },
        DebuggerRequest::ArmRootSafePointBreakpoint {
            safe_point: DebuggerSafePoint {
                program: DebuggerProgram {
                    realm: realm(),
                    program_handle: 12,
                    program_generation: 5,
                },
                code_unit_ordinal: 0,
                bytecode_offset: 5,
            },
        },
        DebuggerRequest::ListBreakpoints { realm: realm() },
        DebuggerRequest::ClearBreakpoint {
            safe_point: DebuggerSafePoint {
                program: DebuggerProgram {
                    realm: realm(),
                    program_handle: 12,
                    program_generation: 5,
                },
                code_unit_ordinal: 0,
                bytecode_offset: 0,
            },
        },
        DebuggerRequest::GetExecutionState {
            program: DebuggerProgram {
                realm: realm(),
                program_handle: 12,
                program_generation: 5,
            },
        },
        DebuggerRequest::ResumeExecution {
            program: DebuggerProgram {
                realm: realm(),
                program_handle: 12,
                program_generation: 5,
            },
        },
        DebuggerRequest::StepRootInstruction {
            program: DebuggerProgram {
                realm: realm(),
                program_handle: 12,
                program_generation: 5,
            },
        },
        DebuggerRequest::GetSourceText {
            program: DebuggerProgram {
                realm: realm(),
                program_handle: 12,
                program_generation: 5,
            },
        },
        DebuggerRequest::Unknown,
    ] {
        let (mut sender, mut receiver) = UnixStream::pair().unwrap();
        write_debugger_request(&mut sender, &request).unwrap();
        assert_eq!(read_debugger_request(&mut receiver).unwrap(), request);
    }

    let reply = DebuggerReply::Capabilities(capabilities(vec![
        DebuggerCapabilityReport {
            capability: DebuggerCapability::BreakpointConfiguration,
            state: DebuggerCapabilityState::Available,
            detail: "exact breakpoint configuration is installed".to_string(),
        },
        capability_report(
            DebuggerCapability::StaticMetadataInventory,
            DebuggerCapabilityState::Planned,
        ),
        capability_report(
            DebuggerCapability::StaticMetadataSummary,
            DebuggerCapabilityState::Planned,
        ),
    ]));
    let (mut sender, mut receiver) = UnixStream::pair().unwrap();
    write_debugger_reply(&mut sender, &reply).unwrap();
    assert_eq!(read_debugger_reply(&mut receiver).unwrap(), reply);

    let reply = DebuggerReply::PageRealms(vec![realm()]);
    let (mut sender, mut receiver) = UnixStream::pair().unwrap();
    write_debugger_reply(&mut sender, &reply).unwrap();
    assert_eq!(read_debugger_reply(&mut receiver).unwrap(), reply);

    let program = DebuggerProgram {
        realm: realm(),
        program_handle: 12,
        program_generation: 5,
    };
    let safe_point = DebuggerSafePoint {
        program,
        code_unit_ordinal: 0,
        bytecode_offset: 0,
    };
    for reply in [
        DebuggerReply::Programs(vec![program]),
        DebuggerReply::StaticMetadata(vec![DebuggerStaticMetadataHandle {
            program,
            metadata_handle: 24,
            metadata_generation: 7,
        }]),
        DebuggerReply::StaticMetadataSummary(DebuggerStaticMetadataSummary {
            metadata: DebuggerStaticMetadataHandle {
                program,
                metadata_handle: 24,
                metadata_generation: 7,
            },
            language_version: "blue-ts-0.1".to_string(),
            compiler_options_hash: "0123456789abcdef".to_string(),
            source_count: 1,
            type_count: 2,
            symbol_count: 3,
            contract_count: 4,
        }),
        DebuggerReply::StaticMetadataLoweringSummary(Box::new(
            DebuggerStaticMetadataLoweringSummary {
                metadata: DebuggerStaticMetadataHandle {
                    program,
                    metadata_handle: 24,
                    metadata_generation: 7,
                },
                safe_point_map_abi: "bluejs-safe-point-map-v1".to_string(),
                program_abi: "bluejs-program-v1".to_string(),
                source_set_hash: "bts-source-set-0123456789abcdef".to_string(),
                bound_safe_point_count: 1,
            },
        )),
        DebuggerReply::StaticMetadataSources(vec![DebuggerStaticMetadataSourceId {
            metadata: DebuggerStaticMetadataHandle {
                program,
                metadata_handle: 24,
                metadata_generation: 7,
            },
            source_id: 0,
        }]),
        DebuggerReply::StaticMetadataTypes(vec![DebuggerStaticMetadataTypeId {
            metadata: DebuggerStaticMetadataHandle {
                program,
                metadata_handle: 24,
                metadata_generation: 7,
            },
            type_id: 0,
        }]),
        DebuggerReply::StaticMetadataType(DebuggerStaticMetadataTypeDisplay {
            static_type: DebuggerStaticMetadataTypeId {
                metadata: DebuggerStaticMetadataHandle {
                    program,
                    metadata_handle: 24,
                    metadata_generation: 7,
                },
                type_id: 0,
            },
            display: "number".to_string(),
        }),
        DebuggerReply::StaticMetadataSymbols(vec![DebuggerStaticMetadataSymbolId {
            metadata: DebuggerStaticMetadataHandle {
                program,
                metadata_handle: 24,
                metadata_generation: 7,
            },
            symbol_id: 0,
        }]),
        DebuggerReply::StaticMetadataSymbol(DebuggerStaticMetadataSymbolDisplay {
            symbol: DebuggerStaticMetadataSymbolId {
                metadata: DebuggerStaticMetadataHandle {
                    program,
                    metadata_handle: 24,
                    metadata_generation: 7,
                },
                symbol_id: 0,
            },
            display: "ProjectControlledName".to_string(),
            kind: DebuggerStaticMetadataSymbolKind::Variable,
            exported: true,
        }),
        DebuggerReply::StaticMetadataSymbolLocation(DebuggerStaticMetadataSymbolLocation {
            symbol: DebuggerStaticMetadataSymbolId {
                metadata: DebuggerStaticMetadataHandle {
                    program,
                    metadata_handle: 24,
                    metadata_generation: 7,
                },
                symbol_id: 0,
            },
            source: DebuggerStaticMetadataSourceId {
                metadata: DebuggerStaticMetadataHandle {
                    program,
                    metadata_handle: 24,
                    metadata_generation: 7,
                },
                source_id: 0,
            },
            start_byte: 6,
            end_byte: 31,
            coordinates: DebuggerSourceCoordinates {
                start_line: 0,
                start_column_utf16: 6,
                end_line: 0,
                end_column_utf16: 31,
            },
        }),
        DebuggerReply::StaticMetadataContracts(vec![DebuggerStaticMetadataContractId {
            metadata: DebuggerStaticMetadataHandle {
                program,
                metadata_handle: 24,
                metadata_generation: 7,
            },
            contract_id: 0,
        }]),
        DebuggerReply::StaticMetadataContract(DebuggerStaticMetadataContractDisplay {
            contract: DebuggerStaticMetadataContractId {
                metadata: DebuggerStaticMetadataHandle {
                    program,
                    metadata_handle: 24,
                    metadata_generation: 7,
                },
                contract_id: 0,
            },
            display: "ProjectControlledContract".to_string(),
            root_kind: DebuggerStaticMetadataContractRootKind::Record,
        }),
        DebuggerReply::StaticMetadataContractValidation(DebuggerStaticMetadataContractValidation {
            contract: DebuggerStaticMetadataContractId {
                metadata: DebuggerStaticMetadataHandle {
                    program,
                    metadata_handle: 24,
                    metadata_generation: 7,
                },
                contract_id: 0,
            },
            valid: true,
        }),
        DebuggerReply::SafePoints(vec![safe_point]),
        DebuggerReply::SafePointValidated { safe_point },
        DebuggerReply::BreakpointSet { safe_point },
        DebuggerReply::BreakpointArmed { safe_point },
        DebuggerReply::RootSafePointBreakpointArmed { safe_point },
        DebuggerReply::Breakpoints(vec![safe_point]),
        DebuggerReply::BreakpointCleared {
            safe_point,
            was_present: true,
        },
        DebuggerReply::ExecutionState {
            program,
            state: DebuggerExecutionState::Paused { safe_point },
        },
        DebuggerReply::ExecutionResumed { program },
        DebuggerReply::ExecutionStepRequested { program },
        DebuggerReply::ExecutionState {
            program,
            state: DebuggerExecutionState::Stepping,
        },
    ] {
        let (mut sender, mut receiver) = UnixStream::pair().unwrap();
        write_debugger_reply(&mut sender, &reply).unwrap();
        assert_eq!(read_debugger_reply(&mut receiver).unwrap(), reply);
    }
}

fn hello(requested_metadata_capabilities: DebuggerMetadataCapabilityManifest) -> DebuggerRequest {
    DebuggerRequest::Hello {
        protocol_version: DEBUGGER_PROTOCOL_VERSION,
        requested_metadata_capabilities,
        requested_bounded_values: false,
    }
}
