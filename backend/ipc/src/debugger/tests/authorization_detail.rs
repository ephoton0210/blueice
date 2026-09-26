// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn symbol_display_requires_symbol_inventory_and_respects_its_fixed_budget() {
    let metadata = DebuggerStaticMetadataHandle {
        program: DebuggerProgram {
            realm: realm(),
            program_handle: 12,
            program_generation: 5,
        },
        metadata_handle: 41,
        metadata_generation: 9,
    };
    let symbol = DebuggerStaticMetadataSymbolId {
        metadata,
        symbol_id: 0,
    };
    let request = hello(DebuggerMetadataCapabilityManifest::opaque_symbol_display());
    let reply = negotiate(
        &request,
        &DebuggerMetadataCapabilityManifest::opaque_symbol_display(),
    );
    let session = metadata_session_authorization(&request, &reply).unwrap();
    assert!(session.permits(DebuggerMetadataCapability::OpaqueInventory));
    assert!(session.permits(DebuggerMetadataCapability::OpaqueSymbolInventory));
    assert!(session.permits(DebuggerMetadataCapability::OpaqueSymbolDisplay));
    assert!(!session.observed_symbol(symbol));
    assert!(session.observe_symbols(&[symbol]));
    assert!(session.observed_symbol(symbol));

    let display = DebuggerStaticMetadataSymbolDisplay {
        symbol,
        display: "ProjectControlledName".to_string(),
        kind: DebuggerStaticMetadataSymbolKind::Interface,
        exported: true,
    };
    assert!(display.is_well_formed());
    for kind in [
        DebuggerStaticMetadataSymbolKind::Import,
        DebuggerStaticMetadataSymbolKind::TypeAlias,
        DebuggerStaticMetadataSymbolKind::Interface,
        DebuggerStaticMetadataSymbolKind::Variable,
        DebuggerStaticMetadataSymbolKind::Function,
    ] {
        let encoded = serde_json::to_string(&kind).unwrap();
        assert_eq!(
            serde_json::from_str::<DebuggerStaticMetadataSymbolKind>(&encoded).unwrap(),
            kind
        );
    }
    assert!(!DebuggerStaticMetadataSymbolDisplay {
        display: String::new(),
        ..display.clone()
    }
    .is_well_formed());
    assert!(!DebuggerStaticMetadataSymbolDisplay {
        display: "x".repeat(DEBUGGER_STATIC_METADATA_SYMBOL_DISPLAY_MAX_BYTES + 1),
        ..display
    }
    .is_well_formed());

    let malformed = DebuggerMetadataCapabilityManifest {
        version: DEBUGGER_METADATA_CAPABILITY_MANIFEST_VERSION,
        capabilities: vec![DebuggerMetadataCapability::OpaqueSymbolDisplay],
    };
    assert!(!malformed.is_well_formed());
}

#[test]
fn contract_display_requires_contract_inventory_and_respects_its_fixed_budget() {
    let metadata = DebuggerStaticMetadataHandle {
        program: DebuggerProgram {
            realm: realm(),
            program_handle: 12,
            program_generation: 5,
        },
        metadata_handle: 41,
        metadata_generation: 9,
    };
    let contract = DebuggerStaticMetadataContractId {
        metadata,
        contract_id: 0,
    };
    let request = hello(DebuggerMetadataCapabilityManifest::opaque_contract_display());
    let reply = negotiate(
        &request,
        &DebuggerMetadataCapabilityManifest::opaque_contract_display(),
    );
    let session = metadata_session_authorization(&request, &reply).unwrap();
    assert!(session.permits(DebuggerMetadataCapability::OpaqueInventory));
    assert!(session.permits(DebuggerMetadataCapability::OpaqueContractInventory));
    assert!(session.permits(DebuggerMetadataCapability::OpaqueContractDisplay));
    assert!(!session.observed_contract(contract));
    assert!(session.observe_contracts(&[contract]));
    assert!(session.observed_contract(contract));

    let display = DebuggerStaticMetadataContractDisplay {
        contract,
        display: "ProjectControlledContract".to_string(),
        root_kind: DebuggerStaticMetadataContractRootKind::Record,
    };
    assert!(display.is_well_formed());
    assert!(!DebuggerStaticMetadataContractDisplay {
        display: String::new(),
        ..display.clone()
    }
    .is_well_formed());
    assert!(!DebuggerStaticMetadataContractDisplay {
        display: "x".repeat(DEBUGGER_STATIC_METADATA_CONTRACT_DISPLAY_MAX_BYTES + 1),
        ..display
    }
    .is_well_formed());

    let malformed = DebuggerMetadataCapabilityManifest {
        version: DEBUGGER_METADATA_CAPABILITY_MANIFEST_VERSION,
        capabilities: vec![DebuggerMetadataCapability::OpaqueContractDisplay],
    };
    assert!(!malformed.is_well_formed());
}

#[test]
fn contract_validation_requires_contract_inventory_and_returns_only_a_boolean() {
    let metadata = DebuggerStaticMetadataHandle {
        program: DebuggerProgram {
            realm: realm(),
            program_handle: 12,
            program_generation: 5,
        },
        metadata_handle: 41,
        metadata_generation: 9,
    };
    let contract = DebuggerStaticMetadataContractId {
        metadata,
        contract_id: 0,
    };
    let request = hello(DebuggerMetadataCapabilityManifest::opaque_contract_validation());
    let reply = negotiate(
        &request,
        &DebuggerMetadataCapabilityManifest::opaque_contract_validation(),
    );
    let session = metadata_session_authorization(&request, &reply).unwrap();
    assert!(session.permits(DebuggerMetadataCapability::OpaqueInventory));
    assert!(session.permits(DebuggerMetadataCapability::OpaqueContractInventory));
    assert!(session.permits(DebuggerMetadataCapability::OpaqueContractValidation));
    assert!(!session.observed_contract(contract));
    assert!(session.observe_contracts(&[contract]));
    assert!(session.observed_contract(contract));

    assert!(DebuggerStaticMetadataContractValidation {
        contract,
        valid: false,
    }
    .is_well_formed());

    let malformed = DebuggerMetadataCapabilityManifest {
        version: DEBUGGER_METADATA_CAPABILITY_MANIFEST_VERSION,
        capabilities: vec![DebuggerMetadataCapability::OpaqueContractValidation],
    };
    assert!(!malformed.is_well_formed());
}

#[test]
fn lowering_summary_requires_inventory_and_contains_no_map_entries() {
    let metadata = DebuggerStaticMetadataHandle {
        program: DebuggerProgram {
            realm: realm(),
            program_handle: 12,
            program_generation: 5,
        },
        metadata_handle: 41,
        metadata_generation: 9,
    };
    let request = hello(DebuggerMetadataCapabilityManifest::opaque_lowering_summary());
    let reply = negotiate(
        &request,
        &DebuggerMetadataCapabilityManifest::opaque_lowering_summary(),
    );
    let session = metadata_session_authorization(&request, &reply).unwrap();
    assert!(session.permits(DebuggerMetadataCapability::OpaqueInventory));
    assert!(session.permits(DebuggerMetadataCapability::OpaqueLoweringSummary));
    assert!(DebuggerStaticMetadataLoweringSummary {
        metadata,
        safe_point_map_abi: DEBUGGER_STATIC_METADATA_SAFE_POINT_MAP_ABI_V1.to_string(),
        program_abi: DEBUGGER_STATIC_METADATA_PROGRAM_ABI_V1.to_string(),
        source_set_hash: "bts-source-set-0123456789abcdef".to_string(),
        bound_safe_point_count: 1,
    }
    .is_well_formed());
    assert!(
        !DebuggerStaticMetadataLoweringSummary {
            metadata,
            safe_point_map_abi: "child-controlled-label".to_string(),
            program_abi: DEBUGGER_STATIC_METADATA_PROGRAM_ABI_V1.to_string(),
            source_set_hash: "bts-source-set-0123456789abcdef".to_string(),
            bound_safe_point_count: 1,
        }
        .is_well_formed(),
        "ABI labels are a fixed protocol vocabulary, never child-controlled text"
    );
    assert!(
        !DebuggerStaticMetadataLoweringSummary {
            metadata,
            safe_point_map_abi: DEBUGGER_STATIC_METADATA_SAFE_POINT_MAP_ABI_V1.to_string(),
            program_abi: DEBUGGER_STATIC_METADATA_PROGRAM_ABI_V1.to_string(),
            source_set_hash: "bts-source-set-0123456789ABCDEf".to_string(),
            bound_safe_point_count: 1,
        }
        .is_well_formed(),
        "source-set receipts must remain canonical lowercase opaque digests"
    );

    let malformed = DebuggerMetadataCapabilityManifest {
        version: DEBUGGER_METADATA_CAPABILITY_MANIFEST_VERSION,
        capabilities: vec![DebuggerMetadataCapability::OpaqueLoweringSummary],
    };
    assert!(!malformed.is_well_formed());
}

#[test]
fn symbol_type_requires_two_receipted_ids_and_round_trips_on_a_socket() {
    let metadata = DebuggerStaticMetadataHandle {
        program: DebuggerProgram {
            realm: realm(),
            program_handle: 12,
            program_generation: 5,
        },
        metadata_handle: 41,
        metadata_generation: 9,
    };
    let relation = DebuggerStaticMetadataSymbolType {
        symbol: DebuggerStaticMetadataSymbolId {
            metadata,
            symbol_id: 1,
        },
        static_type: DebuggerStaticMetadataTypeId {
            metadata,
            type_id: 2,
        },
    };
    assert!(relation.is_well_formed());
    assert!(!DebuggerStaticMetadataSymbolType {
        static_type: DebuggerStaticMetadataTypeId {
            metadata: DebuggerStaticMetadataHandle {
                metadata_generation: 10,
                ..metadata
            },
            ..relation.static_type
        },
        ..relation
    }
    .is_well_formed());
    let manifest = DebuggerMetadataCapabilityManifest::opaque_symbol_type();
    assert!(manifest.is_well_formed());
    let request = hello(manifest.clone());
    let reply = negotiate(&request, &manifest);
    let session = metadata_session_authorization(&request, &reply).unwrap();
    assert!(session.permits(DebuggerMetadataCapability::OpaqueSymbolType));
    assert!(!session.observed_symbol(relation.symbol));
    assert!(!session.observed_type(relation.static_type));
    assert!(session.observe_symbols(&[relation.symbol]));
    assert!(session.observe_types(&[relation.static_type]));
    assert!(session.observed_symbol(relation.symbol));
    assert!(session.observed_type(relation.static_type));
    let separate_stream = metadata_session_authorization(&request, &reply).unwrap();
    assert!(!separate_stream.observed_symbol(relation.symbol));
    assert!(!separate_stream.observed_type(relation.static_type));
    let inventory_only =
        DebuggerMetadataCapabilityManifest::opaque_selected(DebuggerMetadataCapabilitySelection {
            type_inventory: true,
            symbol_inventory: true,
            ..DebuggerMetadataCapabilitySelection::default()
        });
    let hello_without_relation = hello(manifest);
    let granted_without_relation = negotiate(&hello_without_relation, &inventory_only);
    let denied_session =
        metadata_session_authorization(&hello_without_relation, &granted_without_relation).unwrap();
    assert!(!denied_session.permits(DebuggerMetadataCapability::OpaqueSymbolType));
    for missing in [
        vec![DebuggerMetadataCapability::OpaqueSymbolType],
        vec![
            DebuggerMetadataCapability::OpaqueInventory,
            DebuggerMetadataCapability::OpaqueSymbolInventory,
            DebuggerMetadataCapability::OpaqueSymbolType,
        ],
        vec![
            DebuggerMetadataCapability::OpaqueInventory,
            DebuggerMetadataCapability::OpaqueTypeInventory,
            DebuggerMetadataCapability::OpaqueSymbolType,
        ],
    ] {
        assert!(!DebuggerMetadataCapabilityManifest {
            version: DEBUGGER_METADATA_CAPABILITY_MANIFEST_VERSION,
            capabilities: missing,
        }
        .is_well_formed());
    }
    let request = DebuggerRequest::DescribeStaticMetadataSymbolType { target: relation };
    let (mut sender, mut receiver) = UnixStream::pair().unwrap();
    write_debugger_request(&mut sender, &request).unwrap();
    assert_eq!(read_debugger_request(&mut receiver).unwrap(), request);
    let reply = DebuggerReply::StaticMetadataSymbolType(relation);
    let (mut sender, mut receiver) = UnixStream::pair().unwrap();
    write_debugger_reply(&mut sender, &reply).unwrap();
    assert_eq!(read_debugger_reply(&mut receiver).unwrap(), reply);
}

#[test]
fn symbol_contract_requires_two_receipted_ids_and_round_trips_on_a_socket() {
    let metadata = DebuggerStaticMetadataHandle {
        program: DebuggerProgram {
            realm: realm(),
            program_handle: 12,
            program_generation: 5,
        },
        metadata_handle: 41,
        metadata_generation: 9,
    };
    let relation = DebuggerStaticMetadataSymbolContract {
        symbol: DebuggerStaticMetadataSymbolId {
            metadata,
            symbol_id: 1,
        },
        contract: DebuggerStaticMetadataContractId {
            metadata,
            contract_id: 2,
        },
    };
    assert!(relation.is_well_formed());
    assert!(!DebuggerStaticMetadataSymbolContract {
        contract: DebuggerStaticMetadataContractId {
            metadata: DebuggerStaticMetadataHandle {
                metadata_generation: 10,
                ..metadata
            },
            ..relation.contract
        },
        ..relation
    }
    .is_well_formed());
    let manifest = DebuggerMetadataCapabilityManifest::opaque_symbol_contract();
    assert!(manifest.is_well_formed());
    let request = hello(manifest.clone());
    let reply = negotiate(&request, &manifest);
    let session = metadata_session_authorization(&request, &reply).unwrap();
    assert!(session.permits(DebuggerMetadataCapability::OpaqueSymbolContract));
    assert!(!session.observed_symbol(relation.symbol));
    assert!(!session.observed_contract(relation.contract));
    assert!(session.observe_symbols(&[relation.symbol]));
    assert!(session.observe_contracts(&[relation.contract]));
    assert!(session.observed_symbol(relation.symbol));
    assert!(session.observed_contract(relation.contract));
    let separate_stream = metadata_session_authorization(&request, &reply).unwrap();
    assert!(!separate_stream.observed_symbol(relation.symbol));
    assert!(!separate_stream.observed_contract(relation.contract));
    let inventory_only =
        DebuggerMetadataCapabilityManifest::opaque_selected(DebuggerMetadataCapabilitySelection {
            symbol_inventory: true,
            contract_inventory: true,
            ..DebuggerMetadataCapabilitySelection::default()
        });
    let denied_reply = negotiate(&request, &inventory_only);
    let denied_session = metadata_session_authorization(&request, &denied_reply).unwrap();
    assert!(!denied_session.permits(DebuggerMetadataCapability::OpaqueSymbolContract));
    for missing in [
        vec![DebuggerMetadataCapability::OpaqueSymbolContract],
        vec![
            DebuggerMetadataCapability::OpaqueInventory,
            DebuggerMetadataCapability::OpaqueSymbolInventory,
            DebuggerMetadataCapability::OpaqueSymbolContract,
        ],
        vec![
            DebuggerMetadataCapability::OpaqueInventory,
            DebuggerMetadataCapability::OpaqueContractInventory,
            DebuggerMetadataCapability::OpaqueSymbolContract,
        ],
    ] {
        assert!(!DebuggerMetadataCapabilityManifest {
            version: DEBUGGER_METADATA_CAPABILITY_MANIFEST_VERSION,
            capabilities: missing,
        }
        .is_well_formed());
    }
    let request = DebuggerRequest::DescribeStaticMetadataSymbolContract { target: relation };
    let (mut sender, mut receiver) = UnixStream::pair().unwrap();
    write_debugger_request(&mut sender, &request).unwrap();
    assert_eq!(read_debugger_request(&mut receiver).unwrap(), request);
    let reply = DebuggerReply::StaticMetadataSymbolContract(relation);
    let (mut sender, mut receiver) = UnixStream::pair().unwrap();
    write_debugger_reply(&mut sender, &reply).unwrap();
    assert_eq!(read_debugger_reply(&mut receiver).unwrap(), reply);
}

#[test]
fn contract_location_requires_both_receipts_and_round_trips() {
    let metadata = DebuggerStaticMetadataHandle {
        program: DebuggerProgram {
            realm: realm(),
            program_handle: 12,
            program_generation: 5,
        },
        metadata_handle: 41,
        metadata_generation: 9,
    };
    let target = DebuggerStaticMetadataContractLocationTarget {
        contract: DebuggerStaticMetadataContractId {
            metadata,
            contract_id: 0,
        },
        source: DebuggerStaticMetadataSourceId {
            metadata,
            source_id: 0,
        },
    };
    assert!(target.is_well_formed());
    assert!(!DebuggerStaticMetadataContractLocationTarget {
        source: DebuggerStaticMetadataSourceId {
            metadata: DebuggerStaticMetadataHandle {
                metadata_generation: 10,
                ..metadata
            },
            ..target.source
        },
        ..target
    }
    .is_well_formed());
    let location = DebuggerStaticMetadataContractLocation {
        contract: target.contract,
        source: target.source,
        start_byte: 6,
        end_byte: 31,
        coordinates: DebuggerSourceCoordinates {
            start_line: 0,
            start_column_utf16: 6,
            end_line: 0,
            end_column_utf16: 31,
        },
    };
    assert!(location.is_well_formed());
    assert!(!DebuggerStaticMetadataContractLocation {
        coordinates: DebuggerSourceCoordinates {
            end_line: 0,
            end_column_utf16: 6,
            ..location.coordinates
        },
        ..location
    }
    .is_well_formed());
    assert!(!DebuggerStaticMetadataContractLocation {
        coordinates: DebuggerSourceCoordinates {
            end_line: 32,
            end_column_utf16: 0,
            ..location.coordinates
        },
        ..location
    }
    .is_well_formed());
    assert!(!DebuggerStaticMetadataContractLocation {
        end_byte: 6,
        ..location
    }
    .is_well_formed());
    assert!(!DebuggerStaticMetadataContractLocation {
        end_byte: DEBUGGER_STATIC_METADATA_MAX_SOURCE_SPAN_BYTES + 1,
        ..location
    }
    .is_well_formed());
    let manifest = DebuggerMetadataCapabilityManifest::opaque_contract_location();
    assert!(manifest.is_well_formed());
    let hello = hello(manifest.clone());
    let ack = negotiate(&hello, &manifest);
    let session = metadata_session_authorization(&hello, &ack).unwrap();
    assert!(session.permits(DebuggerMetadataCapability::OpaqueContractLocation));
    assert!(!session.observed_source(target.source));
    assert!(!session.observed_contract(target.contract));
    assert!(session.observe_sources(&[target.source]));
    assert!(session.observe_contracts(&[target.contract]));
    assert!(session.observed_source(target.source));
    assert!(session.observed_contract(target.contract));
    let separate = metadata_session_authorization(&hello, &ack).unwrap();
    assert!(!separate.observed_source(target.source));
    assert!(!separate.observed_contract(target.contract));
    for capabilities in [
        vec![DebuggerMetadataCapability::OpaqueContractLocation],
        vec![
            DebuggerMetadataCapability::OpaqueInventory,
            DebuggerMetadataCapability::OpaqueSourceInventory,
            DebuggerMetadataCapability::OpaqueContractLocation,
        ],
        vec![
            DebuggerMetadataCapability::OpaqueInventory,
            DebuggerMetadataCapability::OpaqueContractInventory,
            DebuggerMetadataCapability::OpaqueContractLocation,
        ],
    ] {
        assert!(!DebuggerMetadataCapabilityManifest {
            version: DEBUGGER_METADATA_CAPABILITY_MANIFEST_VERSION,
            capabilities,
        }
        .is_well_formed());
    }
    let request = DebuggerRequest::DescribeStaticMetadataContractLocation { target };
    let (mut sender, mut receiver) = UnixStream::pair().unwrap();
    write_debugger_request(&mut sender, &request).unwrap();
    assert_eq!(read_debugger_request(&mut receiver).unwrap(), request);
    let reply = DebuggerReply::StaticMetadataContractLocation(location);
    let (mut sender, mut receiver) = UnixStream::pair().unwrap();
    write_debugger_reply(&mut sender, &reply).unwrap();
    assert_eq!(read_debugger_reply(&mut receiver).unwrap(), reply);
}
