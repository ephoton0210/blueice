// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn static_metadata_contract_location_requires_independent_same_stream_receipts() {
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
    let contract = DebuggerStaticMetadataContractId {
        metadata,
        contract_id: 0,
    };
    let source = DebuggerStaticMetadataSourceId {
        metadata,
        source_id: 0,
    };
    let target = DebuggerStaticMetadataContractLocationTarget { contract, source };
    let manifest =
        blueice_ipc::debugger::DebuggerMetadataCapabilityManifest::opaque_contract_location();
    let hello = DebuggerRequest::Hello {
        protocol_version: DEBUGGER_PROTOCOL_VERSION,
        requested_bounded_values: false,
        requested_metadata_capabilities: manifest.clone(),
    };
    let hello_reply = blueice_ipc::debugger::negotiate(&hello, &manifest);
    let session = blueice_ipc::debugger::metadata_session_authorization(&hello, &hello_reply)
        .expect("dependent contract-location grant must create a session");
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
        panic!("live contract-location capability discovery must succeed")
    };
    assert!(capabilities.reports.iter().any(|report| {
        report.capability == DebuggerCapability::StaticMetadataContractLocation
            && report.state == DebuggerCapabilityState::Available
    }));
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&session),
            DebuggerRequest::DescribeStaticMetadataContractLocation { target },
        ),
        unavailable_static_metadata_contract_location(),
    );
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&session),
            DebuggerRequest::ListStaticMetadata { program },
        ),
        DebuggerReply::StaticMetadata(vec![metadata]),
    );
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&session),
            DebuggerRequest::ListStaticMetadataSources { metadata },
        ),
        DebuggerReply::StaticMetadataSources(vec![source]),
    );
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&session),
            DebuggerRequest::ListStaticMetadataContracts { metadata },
        ),
        DebuggerReply::StaticMetadataContracts(vec![
            contract,
            DebuggerStaticMetadataContractId {
                contract_id: 1,
                ..contract
            },
        ]),
    );
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&session),
            DebuggerRequest::DescribeStaticMetadataContractLocation { target },
        ),
        DebuggerReply::StaticMetadataContractLocation(DebuggerStaticMetadataContractLocation {
            contract,
            source,
            start_byte: 6,
            end_byte: 31,
            coordinates: DebuggerSourceCoordinates {
                start_line: 0,
                start_column_utf16: 6,
                end_line: 0,
                end_column_utf16: 31,
            },
        }),
    );
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&session),
            DebuggerRequest::DescribeStaticMetadataContractLocation {
                target: DebuggerStaticMetadataContractLocationTarget {
                    source: DebuggerStaticMetadataSourceId {
                        source_id: 1,
                        ..source
                    },
                    ..target
                },
            },
        ),
        unavailable_static_metadata_contract_location(),
    );
    let separate = blueice_ipc::debugger::metadata_session_authorization(&hello, &hello_reply)
        .expect("a separate stream has its own receipt ledger");
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&separate),
            DebuggerRequest::DescribeStaticMetadataContractLocation { target },
        ),
        unavailable_static_metadata_contract_location(),
    );
}

#[test]
fn static_metadata_symbol_type_requires_both_exact_receipts() {
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
    let target = DebuggerStaticMetadataSymbolType {
        symbol: DebuggerStaticMetadataSymbolId {
            metadata,
            symbol_id: 0,
        },
        static_type: DebuggerStaticMetadataTypeId {
            metadata,
            type_id: 1,
        },
    };
    let manifest = blueice_ipc::debugger::DebuggerMetadataCapabilityManifest::opaque_symbol_type();
    let hello = DebuggerRequest::Hello {
        protocol_version: DEBUGGER_PROTOCOL_VERSION,
        requested_bounded_values: false,
        requested_metadata_capabilities: manifest.clone(),
    };
    let hello_reply = blueice_ipc::debugger::negotiate(&hello, &manifest);
    let session = blueice_ipc::debugger::metadata_session_authorization(&hello, &hello_reply)
        .expect("the exact dependent symbol/type grant creates a local session");
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
        panic!("the live child must report its symbol/type capability")
    };
    assert!(capabilities.reports.iter().any(|report| {
        report.capability == DebuggerCapability::StaticMetadataSymbolType
            && report.state == DebuggerCapabilityState::Available
    }));
    let request = DebuggerRequest::DescribeStaticMetadataSymbolType { target };
    assert_eq!(
        handle_debugger_request_with_child_locations(&tabs, &mut locations, None, request.clone(),),
        unavailable_static_metadata_symbol_type(),
        "the public relation is default-denied without an owner/Hello session"
    );
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&session),
            request.clone(),
        ),
        unavailable_static_metadata_symbol_type(),
    );
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&session),
            DebuggerRequest::ListStaticMetadata { program },
        ),
        DebuggerReply::StaticMetadata(vec![metadata]),
    );
    assert!(matches!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&session),
            DebuggerRequest::ListStaticMetadataSymbols { metadata },
        ),
        DebuggerReply::StaticMetadataSymbols(_)
    ));
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&session),
            request.clone(),
        ),
        unavailable_static_metadata_symbol_type(),
        "symbol inventory cannot stand in for the separate type receipt"
    );
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&session),
            DebuggerRequest::ListStaticMetadataTypes { metadata },
        ),
        DebuggerReply::StaticMetadataTypes(vec![
            DebuggerStaticMetadataTypeId {
                metadata,
                type_id: 0
            },
            target.static_type,
        ]),
    );
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&session),
            request,
        ),
        DebuggerReply::StaticMetadataSymbolType(target),
    );
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&session),
            DebuggerRequest::DescribeStaticMetadataSymbolType {
                target: DebuggerStaticMetadataSymbolType {
                    static_type: DebuggerStaticMetadataTypeId {
                        type_id: 999,
                        ..target.static_type
                    },
                    ..target
                },
            },
        ),
        unavailable_static_metadata_symbol_type(),
        "a guessed type ID cannot reach the child"
    );
    assert!(matches!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&session),
            DebuggerRequest::DescribeStaticMetadataSymbolType {
                target: DebuggerStaticMetadataSymbolType {
                    static_type: DebuggerStaticMetadataTypeId {
                        type_id: 0,
                        ..target.static_type
                    },
                    ..target
                },
            },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            ..
        }
    ));
}

#[test]
fn static_metadata_symbol_contract_requires_both_exact_receipts() {
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
    let target = DebuggerStaticMetadataSymbolContract {
        symbol: DebuggerStaticMetadataSymbolId {
            metadata,
            symbol_id: 0,
        },
        contract: DebuggerStaticMetadataContractId {
            metadata,
            contract_id: 1,
        },
    };
    let manifest =
        blueice_ipc::debugger::DebuggerMetadataCapabilityManifest::opaque_symbol_contract();
    let hello = DebuggerRequest::Hello {
        protocol_version: DEBUGGER_PROTOCOL_VERSION,
        requested_bounded_values: false,
        requested_metadata_capabilities: manifest.clone(),
    };
    let hello_reply = blueice_ipc::debugger::negotiate(&hello, &manifest);
    let session = blueice_ipc::debugger::metadata_session_authorization(&hello, &hello_reply)
        .expect("the exact dependent symbol/contract grant creates a local session");
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
        panic!("the live child must report its symbol/contract capability")
    };
    assert!(capabilities.reports.iter().any(|report| {
        report.capability == DebuggerCapability::StaticMetadataSymbolContract
            && report.state == DebuggerCapabilityState::Available
    }));
    let request = DebuggerRequest::DescribeStaticMetadataSymbolContract { target };
    assert_eq!(
        handle_debugger_request_with_child_locations(&tabs, &mut locations, None, request.clone(),),
        unavailable_static_metadata_symbol_contract(),
        "the public relation is default-denied without an owner/Hello session"
    );
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&session),
            request.clone(),
        ),
        unavailable_static_metadata_symbol_contract(),
    );
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&session),
            DebuggerRequest::ListStaticMetadata { program },
        ),
        DebuggerReply::StaticMetadata(vec![metadata]),
    );
    assert!(matches!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&session),
            DebuggerRequest::ListStaticMetadataSymbols { metadata },
        ),
        DebuggerReply::StaticMetadataSymbols(_)
    ));
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&session),
            request.clone(),
        ),
        unavailable_static_metadata_symbol_contract(),
        "symbol inventory cannot stand in for the separate contract receipt"
    );
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&session),
            DebuggerRequest::ListStaticMetadataContracts { metadata },
        ),
        DebuggerReply::StaticMetadataContracts(vec![
            DebuggerStaticMetadataContractId {
                metadata,
                contract_id: 0
            },
            target.contract,
        ]),
    );
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&session),
            request,
        ),
        DebuggerReply::StaticMetadataSymbolContract(target),
    );
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&session),
            DebuggerRequest::DescribeStaticMetadataSymbolContract {
                target: DebuggerStaticMetadataSymbolContract {
                    contract: DebuggerStaticMetadataContractId {
                        contract_id: 999,
                        ..target.contract
                    },
                    ..target
                },
            },
        ),
        unavailable_static_metadata_symbol_contract(),
        "a guessed contract ID cannot reach the child"
    );
    assert!(matches!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&session),
            DebuggerRequest::DescribeStaticMetadataSymbolContract {
                target: DebuggerStaticMetadataSymbolContract {
                    contract: DebuggerStaticMetadataContractId {
                        contract_id: 0,
                        ..target.contract
                    },
                    ..target
                },
            },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            ..
        }
    ));
}

#[test]
fn static_metadata_lowering_summary_requires_a_receipt_and_rejects_noncanonical_child_data() {
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
    let hello = DebuggerRequest::Hello {
        protocol_version: DEBUGGER_PROTOCOL_VERSION,
        requested_bounded_values: false,
        requested_metadata_capabilities:
            blueice_ipc::debugger::DebuggerMetadataCapabilityManifest::opaque_lowering_summary(),
    };
    let hello_reply = blueice_ipc::debugger::negotiate(
        &hello,
        &blueice_ipc::debugger::DebuggerMetadataCapabilityManifest::opaque_lowering_summary(),
    );
    let session = blueice_ipc::debugger::metadata_session_authorization(&hello, &hello_reply)
        .expect("dependent lowering-summary policy must create a core-local session authorization");
    let mut locations = MetadataLocations {
        malformed_summary: false,
        malformed_provenance: false,
        malformed_lowering_summary: false,
        mismatched_symbol_display: false,
        mismatched_contract_display: false,
        mismatched_contract_validation: false,
    };
    let capabilities = handle_debugger_request_with_child_locations(
        &tabs,
        &mut locations,
        Some(&session),
        DebuggerRequest::DescribeCapabilities { realm },
    );
    let DebuggerReply::Capabilities(capabilities) = capabilities else {
        panic!("live realm lowering-summary capability discovery must succeed")
    };
    assert!(capabilities.reports.iter().any(|report| {
        report.capability == DebuggerCapability::StaticMetadataLoweringSummary
            && report.state == DebuggerCapabilityState::Available
    }));
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&session),
            DebuggerRequest::DescribeStaticMetadataLoweringSummary { metadata },
        ),
        unavailable_static_metadata_lowering_summary(),
        "a guessed metadata handle must fail before core reaches the child"
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
    let reply = handle_debugger_request_with_child_locations(
        &tabs,
        &mut locations,
        Some(&session),
        DebuggerRequest::DescribeStaticMetadataLoweringSummary { metadata },
    );
    let DebuggerReply::StaticMetadataLoweringSummary(summary) = reply else {
        panic!("an inventoried metadata handle must expose its bounded lowering summary")
    };
    assert_eq!(summary.metadata, metadata);
    assert_eq!(
        summary.safe_point_map_abi,
        blueice_ipc::debugger::DEBUGGER_STATIC_METADATA_SAFE_POINT_MAP_ABI_V1
    );
    assert_eq!(
        summary.program_abi,
        blueice_ipc::debugger::DEBUGGER_STATIC_METADATA_PROGRAM_ABI_V1
    );
    assert_eq!(summary.source_set_hash, "bts-source-set-0123456789abcdef");
    assert_eq!(summary.bound_safe_point_count, 1);
    let disclosure = format!("{summary:?}");
    assert!(
        !disclosure.contains("page://")
            && !disclosure.contains("main.ts")
            && !disclosure.contains("bytecode")
            && !disclosure.contains("privateBlueTsMetadata"),
        "lowering summary must exclude source identities, map entries, offsets, and static-record payloads"
    );

    locations.malformed_lowering_summary = true;
    assert!(matches!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&session),
            DebuggerRequest::DescribeStaticMetadataLoweringSummary { metadata },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            ..
        }
    ));
}

#[test]
fn static_metadata_symbol_display_rejects_a_mismatched_child_identity() {
    let (tabs, realm) = loaded_tabs();
    let metadata = DebuggerStaticMetadataHandle {
        program: DebuggerProgram {
            realm,
            program_handle: 7,
            program_generation: 3,
        },
        metadata_handle: 41,
        metadata_generation: 9,
    };
    let symbol = DebuggerStaticMetadataSymbolId {
        metadata,
        symbol_id: 0,
    };
    let hello = DebuggerRequest::Hello {
        protocol_version: DEBUGGER_PROTOCOL_VERSION,
        requested_bounded_values: false,
        requested_metadata_capabilities:
            blueice_ipc::debugger::DebuggerMetadataCapabilityManifest::opaque_symbol_display(),
    };
    let hello_reply = blueice_ipc::debugger::negotiate(
        &hello,
        &blueice_ipc::debugger::DebuggerMetadataCapabilityManifest::opaque_symbol_display(),
    );
    let session = blueice_ipc::debugger::metadata_session_authorization(&hello, &hello_reply)
        .expect("dependent symbol display policy must create a core-local session authorization");
    let mut locations = MetadataLocations {
        malformed_summary: false,
        malformed_provenance: false,
        malformed_lowering_summary: false,
        mismatched_symbol_display: true,
        mismatched_contract_display: false,
        mismatched_contract_validation: false,
    };
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&session),
            DebuggerRequest::ListStaticMetadata {
                program: metadata.program,
            },
        ),
        DebuggerReply::StaticMetadata(vec![metadata])
    );
    assert!(matches!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&session),
            DebuggerRequest::ListStaticMetadataSymbols { metadata },
        ),
        DebuggerReply::StaticMetadataSymbols(_)
    ));
    assert!(matches!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&session),
            DebuggerRequest::DescribeStaticMetadataSymbol { symbol },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            ..
        }
    ));
}

#[test]
fn static_metadata_contract_display_rejects_a_mismatched_child_identity() {
    let (tabs, realm) = loaded_tabs();
    let metadata = DebuggerStaticMetadataHandle {
        program: DebuggerProgram {
            realm,
            program_handle: 7,
            program_generation: 3,
        },
        metadata_handle: 41,
        metadata_generation: 9,
    };
    let contract = DebuggerStaticMetadataContractId {
        metadata,
        contract_id: 0,
    };
    let hello = DebuggerRequest::Hello {
        protocol_version: DEBUGGER_PROTOCOL_VERSION,
        requested_bounded_values: false,
        requested_metadata_capabilities:
            blueice_ipc::debugger::DebuggerMetadataCapabilityManifest::opaque_contract_display(),
    };
    let hello_reply = blueice_ipc::debugger::negotiate(
        &hello,
        &blueice_ipc::debugger::DebuggerMetadataCapabilityManifest::opaque_contract_display(),
    );
    let session = blueice_ipc::debugger::metadata_session_authorization(&hello, &hello_reply)
        .expect("dependent contract display policy must create a core-local session authorization");
    let mut locations = MetadataLocations {
        malformed_summary: false,
        malformed_provenance: false,
        malformed_lowering_summary: false,
        mismatched_symbol_display: false,
        mismatched_contract_display: true,
        mismatched_contract_validation: false,
    };
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&session),
            DebuggerRequest::ListStaticMetadata {
                program: metadata.program,
            },
        ),
        DebuggerReply::StaticMetadata(vec![metadata])
    );
    assert!(matches!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&session),
            DebuggerRequest::ListStaticMetadataContracts { metadata },
        ),
        DebuggerReply::StaticMetadataContracts(_)
    ));
    assert!(matches!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&session),
            DebuggerRequest::DescribeStaticMetadataContract { contract },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            ..
        }
    ));
}

#[test]
fn static_metadata_contract_validation_requires_a_receipt_and_hides_failure_detail() {
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
    let contract = DebuggerStaticMetadataContractId {
        metadata,
        contract_id: 0,
    };
    let hello = DebuggerRequest::Hello {
        protocol_version: DEBUGGER_PROTOCOL_VERSION,
        requested_bounded_values: false,
        requested_metadata_capabilities:
            blueice_ipc::debugger::DebuggerMetadataCapabilityManifest::opaque_contract_validation(),
    };
    let hello_reply = blueice_ipc::debugger::negotiate(
        &hello,
        &blueice_ipc::debugger::DebuggerMetadataCapabilityManifest::opaque_contract_validation(),
    );
    let session = blueice_ipc::debugger::metadata_session_authorization(&hello, &hello_reply)
        .expect(
            "dependent contract-validation policy must create a core-local session authorization",
        );
    let mut locations = MetadataLocations {
        malformed_summary: false,
        malformed_provenance: false,
        malformed_lowering_summary: false,
        mismatched_symbol_display: false,
        mismatched_contract_display: false,
        mismatched_contract_validation: false,
    };
    let capabilities = handle_debugger_request_with_child_locations(
        &tabs,
        &mut locations,
        Some(&session),
        DebuggerRequest::DescribeCapabilities { realm },
    );
    let DebuggerReply::Capabilities(capabilities) = capabilities else {
        panic!("live realm contract-validation capability discovery must succeed")
    };
    assert!(capabilities.reports.iter().any(|report| {
        report.capability == DebuggerCapability::StaticMetadataContractValidation
            && report.state == DebuggerCapabilityState::Available
    }));
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&session),
            DebuggerRequest::ValidateStaticMetadataContract {
                contract,
                value: CompilerContractValue::Boolean(true),
            },
        ),
        unavailable_static_metadata_contract_validation(),
        "a guessed contract ID must fail before core reaches the child"
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
            DebuggerRequest::ListStaticMetadataContracts { metadata },
        ),
        DebuggerReply::StaticMetadataContracts(vec![
            contract,
            DebuggerStaticMetadataContractId {
                metadata,
                contract_id: 1,
            },
        ])
    );
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&session),
            DebuggerRequest::ValidateStaticMetadataContract {
                contract,
                value: CompilerContractValue::Boolean(true),
            },
        ),
        DebuggerReply::StaticMetadataContractValidation(DebuggerStaticMetadataContractValidation {
            contract,
            valid: true,
        },)
    );
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&session),
            DebuggerRequest::ValidateStaticMetadataContract {
                contract,
                value: CompilerContractValue::Boolean(false),
            },
        ),
        DebuggerReply::StaticMetadataContractValidation(DebuggerStaticMetadataContractValidation {
            contract,
            valid: false,
        },)
    );
    assert!(matches!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&session),
            DebuggerRequest::ValidateStaticMetadataContract {
                contract,
                value: CompilerContractValue::String(
                    "x".repeat(
                        blueice_ipc::debugger::DEBUGGER_STATIC_METADATA_CONTRACT_VALIDATION_MAX_STRING_BYTES
                            + 1,
                    ),
                ),
            },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            ..
        }
    ));
    locations.mismatched_contract_validation = true;
    assert!(matches!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&session),
            DebuggerRequest::ValidateStaticMetadataContract {
                contract,
                value: CompilerContractValue::Boolean(true),
            },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            ..
        }
    ));
}

#[test]
fn source_provenance_requires_its_own_dependent_grant_and_exact_source_id() {
    let (tabs, realm) = loaded_tabs();
    let metadata = DebuggerStaticMetadataHandle {
        program: DebuggerProgram {
            realm,
            program_handle: 7,
            program_generation: 3,
        },
        metadata_handle: 41,
        metadata_generation: 9,
    };
    let source = DebuggerStaticMetadataSourceId {
        metadata,
        source_id: 0,
    };
    let mut locations = MetadataLocations {
        malformed_summary: false,
        malformed_provenance: false,
        malformed_lowering_summary: false,
        mismatched_symbol_display: false,
        mismatched_contract_display: false,
        mismatched_contract_validation: false,
    };

    let source_inventory_hello = DebuggerRequest::Hello {
        protocol_version: DEBUGGER_PROTOCOL_VERSION,
        requested_bounded_values: false,
        requested_metadata_capabilities:
            blueice_ipc::debugger::DebuggerMetadataCapabilityManifest::opaque_source_inventory(),
    };
    let source_inventory_reply = blueice_ipc::debugger::negotiate(
        &source_inventory_hello,
        &blueice_ipc::debugger::DebuggerMetadataCapabilityManifest::opaque_source_inventory(),
    );
    let source_inventory_session = blueice_ipc::debugger::metadata_session_authorization(
        &source_inventory_hello,
        &source_inventory_reply,
    )
    .expect("source inventory policy must create a core-local session authorization");
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&source_inventory_session),
            DebuggerRequest::DescribeStaticMetadataSource { source },
        ),
        unavailable_static_metadata_source_provenance()
    );

    let provenance_hello = DebuggerRequest::Hello {
        protocol_version: DEBUGGER_PROTOCOL_VERSION,
        requested_bounded_values: false,
        requested_metadata_capabilities:
            blueice_ipc::debugger::DebuggerMetadataCapabilityManifest::opaque_source_provenance(),
    };
    let provenance_hello_reply = blueice_ipc::debugger::negotiate(
        &provenance_hello,
        &blueice_ipc::debugger::DebuggerMetadataCapabilityManifest::opaque_source_provenance(),
    );
    let provenance_session = blueice_ipc::debugger::metadata_session_authorization(
        &provenance_hello,
        &provenance_hello_reply,
    )
    .expect("source provenance policy must create a core-local session authorization");
    let capabilities = handle_debugger_request_with_child_locations(
        &tabs,
        &mut locations,
        Some(&provenance_session),
        DebuggerRequest::DescribeCapabilities { realm },
    );
    let DebuggerReply::Capabilities(capabilities) = capabilities else {
        panic!("live realm provenance capability discovery must succeed")
    };
    assert!(capabilities.reports.iter().any(|report| {
        report.capability == DebuggerCapability::StaticMetadataSourceProvenance
            && report.state == DebuggerCapabilityState::Available
    }));
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&provenance_session),
            DebuggerRequest::DescribeStaticMetadataSource { source },
        ),
        unavailable_static_metadata_source_provenance(),
        "a provenance target must have been emitted by this stream's source inventory"
    );
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&provenance_session),
            DebuggerRequest::ListStaticMetadataSources { metadata },
        ),
        unavailable_static_metadata_source_inventory(),
        "source inventory cannot dereference a parent handle guessed before inventory"
    );
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&provenance_session),
            DebuggerRequest::ListStaticMetadata {
                program: metadata.program,
            },
        ),
        DebuggerReply::StaticMetadata(vec![metadata])
    );
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&provenance_session),
            DebuggerRequest::ListStaticMetadataSources { metadata },
        ),
        DebuggerReply::StaticMetadataSources(vec![DebuggerStaticMetadataSourceId {
            metadata,
            source_id: 0,
        }])
    );
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&provenance_session),
            DebuggerRequest::DescribeStaticMetadataSource { source },
        ),
        DebuggerReply::StaticMetadataSourceProvenance(DebuggerStaticMetadataSourceProvenance {
            source,
            module: "page:///main.ts".to_string(),
            content_hash:
                "bts-sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
                    .to_string(),
        })
    );
    locations.malformed_provenance = true;
    assert!(matches!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&provenance_session),
            DebuggerRequest::DescribeStaticMetadataSource { source },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            ..
        }
    ));
}
