// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;
use std::os::unix::net::UnixStream;

fn project() -> CompilerProject {
    CompilerProject { id: 7 }
}

fn generation() -> CompilerGeneration {
    CompilerGeneration {
        project: project(),
        sequence: 3,
    }
}

fn session_attestation() -> CompilerSessionAttestation {
    CompilerSessionAttestation {
        id: "a1".repeat(32),
    }
}

fn session_evidence() -> CompilerSessionHelloEvidence {
    CompilerSessionHelloEvidence {
        session_attestation: session_attestation(),
        capability_manifest: CompilerSessionCapabilityManifest::fixed_query_only(),
    }
}

#[test]
fn requests_and_replies_round_trip_on_a_real_socket() {
    for request in [
        CompilerRequest::Hello {
            protocol_version: COMPILER_PROTOCOL_VERSION,
        },
        CompilerRequest::ListProjects,
        CompilerRequest::DescribeProject { project: project() },
        CompilerRequest::Check { project: project() },
        CompilerRequest::ListDiagnostics {
            generation: generation(),
            cursor: Some(CompilerDiagnosticCursor { id: 6 }),
            limit: Some(2),
        },
        CompilerRequest::ListWorkSet {
            generation: generation(),
            kind: CompilerWorkSetKind::Rechecked,
            cursor: Some(CompilerWorkSetCursor { id: 8 }),
            limit: Some(2),
        },
        CompilerRequest::GetStaticType {
            generation: generation(),
            type_id: 2,
        },
        CompilerRequest::GetStaticSymbol {
            generation: generation(),
            symbol_id: 5,
        },
        CompilerRequest::GetStaticSymbolLocation {
            generation: generation(),
            symbol_id: 5,
            source_id: 7,
        },
        CompilerRequest::ListStaticMetadata {
            generation: generation(),
            kind: CompilerStaticMetadataKind::Symbols,
            cursor: Some(CompilerStaticMetadataCursor { id: 9 }),
            limit: Some(2),
        },
        CompilerRequest::GetStaticProvenance {
            generation: generation(),
            source_id: 7,
        },
        CompilerRequest::GetStaticContract {
            generation: generation(),
            contract_id: 8,
        },
        CompilerRequest::GetStaticContractLocation {
            generation: generation(),
            contract_id: 8,
            source_id: 7,
        },
        CompilerRequest::ValidateStaticContract {
            generation: generation(),
            contract_id: 8,
            value: CompilerContractValue::Object(BTreeMap::from([(
                "enabled".to_string(),
                CompilerContractValue::Boolean(true),
            )])),
        },
        CompilerRequest::Unknown,
    ] {
        let (mut sender, mut receiver) = UnixStream::pair().unwrap();
        write_compiler_request(&mut sender, &request).unwrap();
        assert_eq!(read_compiler_request(&mut receiver).unwrap(), request);
    }

    let reply = CompilerReply::Check(CompilerCheck {
        generation: generation(),
        cache_hit: false,
        parsed_modules: CompilerModuleList {
            entries: vec!["project:///app/main.ts".to_string()],
            truncated: false,
        },
        reused_parsed_modules: CompilerModuleList {
            entries: Vec::new(),
            truncated: false,
        },
        rechecked_modules: CompilerModuleList {
            entries: vec!["project:///app/main.ts".to_string()],
            truncated: false,
        },
        reused_checked_modules: CompilerModuleList {
            entries: Vec::new(),
            truncated: false,
        },
        diagnostics: CompilerDiagnostics {
            entries: Vec::new(),
            truncated: false,
        },
        has_errors: false,
        artifact_fingerprint: Some("bts-1234".to_string()),
        static_metadata: Some(CompilerStaticMetadataSummary {
            language_version: "blue-ts-v1".to_string(),
            compiler_options_hash: "bts-options-1234".to_string(),
            source_count: 1,
            type_count: 1,
            symbol_count: 1,
            contract_count: 1,
        }),
    });
    let (mut sender, mut receiver) = UnixStream::pair().unwrap();
    write_compiler_reply(&mut sender, &reply).unwrap();
    assert_eq!(read_compiler_reply(&mut receiver).unwrap(), reply);

    for exported in [false, true] {
        let symbol = CompilerReply::StaticSymbol(CompilerStaticSymbol {
            generation: generation(),
            id: 4,
            name: "ProjectControlledName".to_string(),
            kind: CompilerSymbolKind::Interface,
            exported,
            module: "project:///app/main.ts".to_string(),
            start: 0,
            end: 31,
            static_type_id: None,
            source_id: 0,
            contract_id: Some(2),
        });
        let (mut sender, mut receiver) = UnixStream::pair().unwrap();
        write_compiler_reply(&mut sender, &symbol).unwrap();
        assert_eq!(read_compiler_reply(&mut receiver).unwrap(), symbol);
    }

    let coordinates = CompilerSourceCoordinates {
        start_line: 1,
        start_column_utf16: 9,
        end_line: 1,
        end_column_utf16: 31,
    };
    for reply in [
        CompilerReply::StaticSymbolLocation(CompilerStaticSymbolLocation {
            generation: generation(),
            symbol_id: 5,
            source_id: 7,
            start_byte: 40,
            end_byte: 62,
            coordinates,
        }),
        CompilerReply::StaticContractLocation(CompilerStaticContractLocation {
            generation: generation(),
            contract_id: 8,
            source_id: 7,
            start_byte: 40,
            end_byte: 62,
            coordinates,
        }),
    ] {
        let (mut sender, mut receiver) = UnixStream::pair().unwrap();
        write_compiler_reply(&mut sender, &reply).unwrap();
        assert_eq!(read_compiler_reply(&mut receiver).unwrap(), reply);
    }

    let diagnostic_page = CompilerReply::DiagnosticPage(CompilerDiagnosticPage {
        generation: generation(),
        entries: vec![CompilerDiagnostic {
            code: "BTS3003".to_string(),
            severity: CompilerDiagnosticSeverity::Error,
            module: "project:///app/main.ts".to_string(),
            start: 3,
            end: 7,
            coordinates: Some(CompilerSourceCoordinates {
                start_line: 0,
                start_column_utf16: 3,
                end_line: 0,
                end_column_utf16: 7,
            }),
            message: "fixture diagnostic".to_string(),
        }],
        next_cursor: Some(CompilerDiagnosticCursor { id: 6 }),
        truncated: false,
    });
    let (mut sender, mut receiver) = UnixStream::pair().unwrap();
    write_compiler_reply(&mut sender, &diagnostic_page).unwrap();
    assert_eq!(read_compiler_reply(&mut receiver).unwrap(), diagnostic_page);
    assert!(CompilerSourceCoordinates {
        start_line: 1,
        start_column_utf16: 3,
        end_line: 1,
        end_column_utf16: 3,
    }
    .is_well_formed_for_diagnostic_range(8, 8));

    let work_set_page = CompilerReply::WorkSetPage(CompilerWorkSetPage {
        generation: generation(),
        kind: CompilerWorkSetKind::Parsed,
        entries: vec!["project:///app/main.ts".to_string()],
        next_cursor: Some(CompilerWorkSetCursor { id: 8 }),
        truncated: false,
    });
    let (mut sender, mut receiver) = UnixStream::pair().unwrap();
    write_compiler_reply(&mut sender, &work_set_page).unwrap();
    assert_eq!(read_compiler_reply(&mut receiver).unwrap(), work_set_page);

    let hello_ack = CompilerReply::HelloAck {
        protocol_version: COMPILER_PROTOCOL_VERSION,
        session_attestation: session_attestation(),
        capability_manifest: CompilerSessionCapabilityManifest::fixed_query_only(),
    };
    let (mut sender, mut receiver) = UnixStream::pair().unwrap();
    write_compiler_reply(&mut sender, &hello_ack).unwrap();
    assert_eq!(read_compiler_reply(&mut receiver).unwrap(), hello_ack);

    let projects = CompilerReply::Projects(CompilerProjectInventory {
        projects: vec![CompilerProject { id: 1 }, CompilerProject { id: 2 }],
    });
    let (mut sender, mut receiver) = UnixStream::pair().unwrap();
    write_compiler_reply(&mut sender, &projects).unwrap();
    assert_eq!(read_compiler_reply(&mut receiver).unwrap(), projects);

    let page = CompilerReply::StaticMetadataPage(CompilerStaticMetadataPage {
        generation: generation(),
        kind: CompilerStaticMetadataKind::Symbols,
        ids: vec![0, 5],
        next_cursor: Some(CompilerStaticMetadataCursor { id: 9 }),
    });
    let (mut sender, mut receiver) = UnixStream::pair().unwrap();
    write_compiler_reply(&mut sender, &page).unwrap();
    assert_eq!(read_compiler_reply(&mut receiver).unwrap(), page);
}

#[test]
fn diagnostic_pages_reject_mismatched_generations_invalid_positions_and_empty_continuations() {
    let valid = CompilerDiagnosticPage {
        generation: generation(),
        entries: vec![CompilerDiagnostic {
            code: "BTS3003".to_string(),
            severity: CompilerDiagnosticSeverity::Error,
            module: "project:///app/main.ts".to_string(),
            start: 8,
            end: 8,
            coordinates: Some(CompilerSourceCoordinates {
                start_line: 1,
                start_column_utf16: 3,
                end_line: 1,
                end_column_utf16: 3,
            }),
            message: "expected token".to_string(),
        }],
        next_cursor: Some(CompilerDiagnosticCursor { id: 2 }),
        truncated: false,
    };
    assert!(valid.is_well_formed_for_generation(generation()));
    let mut malformed = valid.clone();
    malformed.generation.sequence += 1;
    assert!(!malformed.is_well_formed_for_generation(generation()));
    malformed = valid.clone();
    malformed.entries[0]
        .coordinates
        .as_mut()
        .unwrap()
        .start_column_utf16 = 9;
    assert!(!malformed.is_well_formed_for_generation(generation()));
    malformed = valid.clone();
    malformed.entries[0].start = 9;
    assert!(!malformed.is_well_formed_for_generation(generation()));
    malformed = valid.clone();
    malformed.entries[0].code = "X".repeat(COMPILER_DIAGNOSTIC_MAX_CODE_BYTES + 1);
    assert!(!malformed.is_well_formed_for_generation(generation()));
    malformed = valid.clone();
    malformed.entries[0].module.clear();
    assert!(!malformed.is_well_formed_for_generation(generation()));
    malformed = valid.clone();
    malformed.next_cursor = Some(CompilerDiagnosticCursor { id: 0 });
    assert!(!malformed.is_well_formed_for_generation(generation()));
    malformed = valid;
    malformed.entries.clear();
    assert!(!malformed.is_well_formed_for_generation(generation()));
}

#[test]
fn negotiation_requires_the_exact_version_and_first_request() {
    assert_eq!(
        negotiate(
            &CompilerRequest::Hello {
                protocol_version: COMPILER_PROTOCOL_VERSION,
            },
            Some(session_evidence()),
        ),
        CompilerReply::HelloAck {
            protocol_version: COMPILER_PROTOCOL_VERSION,
            session_attestation: session_attestation(),
            capability_manifest: CompilerSessionCapabilityManifest::fixed_query_only(),
        }
    );
    assert!(matches!(
        negotiate(
            &CompilerRequest::Hello {
                protocol_version: COMPILER_PROTOCOL_VERSION + 1,
            },
            Some(session_evidence()),
        ),
        CompilerReply::Error {
            code: CompilerErrorCode::ProtocolVersion,
            ..
        }
    ));
    assert!(matches!(
        negotiate(
            &CompilerRequest::Hello {
                protocol_version: COMPILER_PROTOCOL_VERSION,
            },
            Some(CompilerSessionHelloEvidence {
                session_attestation: session_attestation(),
                capability_manifest: CompilerSessionCapabilityManifest {
                    version: 0,
                    operation_ids: Vec::new(),
                },
            }),
        ),
        CompilerReply::Error {
            code: CompilerErrorCode::Unavailable,
            ..
        }
    ));
    assert!(matches!(
        negotiate(
            &CompilerRequest::Hello {
                protocol_version: 1,
            },
            Some(session_evidence()),
        ),
        CompilerReply::Error {
            code: CompilerErrorCode::ProtocolVersion,
            ..
        }
    ));
    assert!(matches!(
        negotiate(&CompilerRequest::Check { project: project() }, None),
        CompilerReply::Error {
            code: CompilerErrorCode::ProtocolVersion,
            ..
        }
    ));
    assert!(matches!(
        negotiate(
            &CompilerRequest::Hello {
                protocol_version: COMPILER_PROTOCOL_VERSION,
            },
            Some(CompilerSessionHelloEvidence {
                session_attestation: CompilerSessionAttestation {
                    id: "not-a-core-attestation".to_string(),
                },
                capability_manifest: CompilerSessionCapabilityManifest::fixed_query_only(),
            }),
        ),
        CompilerReply::Error {
            code: CompilerErrorCode::Unavailable,
            ..
        }
    ));
}

#[test]
fn capability_manifest_requires_the_complete_canonical_query_inventory() {
    let manifest = CompilerSessionCapabilityManifest::fixed_query_only();
    assert!(manifest.is_well_formed());
    assert_eq!(manifest.version, COMPILER_QUERY_CAPABILITY_MANIFEST_VERSION);
    assert_eq!(
        manifest.operation_ids,
        vec![
            CompilerQueryOperationId::ListProjects,
            CompilerQueryOperationId::DescribeProject,
            CompilerQueryOperationId::Check,
            CompilerQueryOperationId::ListDiagnostics,
            CompilerQueryOperationId::ListWorkSet,
            CompilerQueryOperationId::GetStaticType,
            CompilerQueryOperationId::GetStaticSymbol,
            CompilerQueryOperationId::GetStaticSymbolLocation,
            CompilerQueryOperationId::ListStaticMetadata,
            CompilerQueryOperationId::GetStaticProvenance,
            CompilerQueryOperationId::GetStaticContract,
            CompilerQueryOperationId::GetStaticContractLocation,
            CompilerQueryOperationId::ValidateStaticContract,
        ]
    );

    let mut reordered = manifest.clone();
    reordered.operation_ids.swap(0, 1);
    assert!(!reordered.is_well_formed());

    let mut subset = manifest.clone();
    subset.operation_ids.pop();
    assert!(!subset.is_well_formed());

    let mut duplicate = manifest.clone();
    duplicate
        .operation_ids
        .push(CompilerQueryOperationId::Check);
    assert!(!duplicate.is_well_formed());

    let mut unknown_version = manifest;
    unknown_version.version += 1;
    assert!(!unknown_version.is_well_formed());
}

#[test]
fn project_inventory_rejects_guessed_zero_duplicate_and_unsorted_ids() {
    let valid = CompilerProjectInventory {
        projects: vec![CompilerProject { id: 1 }, CompilerProject { id: 2 }],
    };
    assert!(valid.is_well_formed());
    for projects in [
        vec![CompilerProject { id: 0 }],
        vec![CompilerProject { id: 2 }, CompilerProject { id: 1 }],
        vec![CompilerProject { id: 1 }, CompilerProject { id: 1 }],
        vec![CompilerProject { id: 1 }; COMPILER_MAX_PROJECT_INVENTORY + 1],
    ] {
        assert!(!CompilerProjectInventory { projects }.is_well_formed());
    }
}

#[test]
fn oversized_length_prefix_is_rejected_before_payload_allocation() {
    let mut frame = Vec::new();
    frame.extend_from_slice(&((MAX_COMPILER_MESSAGE_BYTES as u32) + 1).to_le_bytes());
    let error = read_compiler_request(&mut std::io::Cursor::new(frame)).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
}

#[test]
fn oversized_reply_is_rejected_before_it_is_written() {
    let reply = CompilerReply::Unsupported {
        operation: "x".repeat(MAX_COMPILER_MESSAGE_BYTES),
        reason: "too large".to_string(),
    };
    let error = write_compiler_reply(&mut Vec::new(), &reply).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
}

#[test]
fn zero_identifiers_are_not_well_formed() {
    assert!(!CompilerProject { id: 0 }.is_well_formed());
    assert!(!CompilerGeneration {
        project: project(),
        sequence: 0,
    }
    .is_well_formed());
    assert!(!CompilerStaticMetadataCursor { id: 0 }.is_well_formed());
    assert!(CompilerStaticMetadataCursor { id: 1 }.is_well_formed());
    assert!(!CompilerDiagnosticCursor { id: 0 }.is_well_formed());
    assert!(CompilerDiagnosticCursor { id: 1 }.is_well_formed());
    assert!(generation().is_well_formed());
}

#[test]
fn declaration_coordinates_reject_empty_reversed_and_over_limit_ranges() {
    let coordinates = CompilerSourceCoordinates {
        start_line: 1,
        start_column_utf16: 9,
        end_line: 1,
        end_column_utf16: 20,
    };
    assert!(coordinates.is_well_formed_for_range(30, 41));
    assert!(!coordinates.is_well_formed_for_range(30, 30));
    assert!(!coordinates.is_well_formed_for_range(30, 1_048_577));
    assert!(!CompilerSourceCoordinates {
        end_line: 0,
        ..coordinates
    }
    .is_well_formed_for_range(30, 41));
    assert!(!CompilerSourceCoordinates {
        start_line: 32,
        ..coordinates
    }
    .is_well_formed_for_range(30, 41));
}
