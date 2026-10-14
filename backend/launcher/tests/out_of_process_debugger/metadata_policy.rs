// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn launcher_owner_policy_exposes_only_handle_bound_bluets_metadata_after_negotiation() {
    let gatekeeper_socket = clearing_gatekeeper();
    let listener = TcpListener::bind("127.0.0.1:0").expect("local HTTP fixture must bind");
    let url = format!("http://{}", listener.local_addr().unwrap());
    let fixture = serve_two_bluets_documents(listener);
    let mut launcher = LauncherProcess::spawn_with_static_metadata_policy(
        &gatekeeper_socket,
        StaticMetadataPolicy {
            inventory: true,
            summary: true,
            source_inventory: true,
            source_provenance: true,
            type_inventory: true,
            type_display: true,
            symbol_inventory: true,
            contract_inventory: true,
            symbol_display: true,
            symbol_location: true,
            safe_point_span: false,
            source_breakpoint: false,
            source_span_step: false,
            bounded_values: false,
            contract_location: true,
            symbol_type: true,
            symbol_contract: true,
            contract_display: true,
            contract_validation: true,
            lowering_summary: true,
            static_scope_relation: false,
        },
    );

    let mut browser = launcher.connect_browser();
    blueice_ipc::client_handshake(&mut browser).expect("public browser handshake must succeed");
    navigate(&mut browser, &url);

    let mut debugger = UnixStream::connect(&launcher.debugger_socket)
        .expect("launcher public debugger endpoint must accept a peer");
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::Hello {
                protocol_version: DEBUGGER_PROTOCOL_VERSION,
                requested_bounded_values: false,
                requested_metadata_capabilities:
                    DebuggerMetadataCapabilityManifest::opaque_selected(
                        DebuggerMetadataCapabilitySelection {
                            summary: true,
                            source_inventory: true,
                            source_provenance: true,
                            type_inventory: true,
                            type_display: true,
                            symbol_inventory: true,
                            contract_inventory: true,
                            symbol_display: true,
                            symbol_location: true,
                            safe_point_span: false,
                            source_breakpoint: true,
                            source_span_step: false,
                            contract_location: true,
                            symbol_type: true,
                            symbol_contract: true,
                            contract_display: true,
                            contract_validation: true,
                            lowering_summary: true,
                            static_scope_relation: false,
                        },
                    ),
            },
        ),
        DebuggerReply::HelloAck {
            protocol_version: DEBUGGER_PROTOCOL_VERSION,
            granted_bounded_values: false,
            granted_metadata_capabilities: DebuggerMetadataCapabilityManifest::opaque_selected(
                DebuggerMetadataCapabilitySelection {
                    summary: true,
                    source_inventory: true,
                    source_provenance: true,
                    type_inventory: true,
                    type_display: true,
                    symbol_inventory: true,
                    contract_inventory: true,
                    symbol_display: true,
                    symbol_location: true,
                    safe_point_span: false,
                    source_breakpoint: false,
                    source_span_step: false,
                    contract_location: true,
                    symbol_type: true,
                    symbol_contract: true,
                    contract_display: true,
                    contract_validation: true,
                    lowering_summary: true,
                    static_scope_relation: false,
                },
            ),
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
        panic!("typed fixture must expose debugger capabilities")
    };
    assert!(capabilities.reports.iter().any(|report| {
        report.capability == blueice_ipc::debugger::DebuggerCapability::StaticMetadataInventory
            && report.state == blueice_ipc::debugger::DebuggerCapabilityState::Available
    }));
    assert!(capabilities.reports.iter().any(|report| {
        report.capability == blueice_ipc::debugger::DebuggerCapability::StaticMetadataSymbolDisplay
            && report.state == blueice_ipc::debugger::DebuggerCapabilityState::Available
    }));
    assert!(capabilities.reports.iter().any(|report| {
        report.capability == blueice_ipc::debugger::DebuggerCapability::StaticMetadataSymbolLocation
            && report.state == blueice_ipc::debugger::DebuggerCapabilityState::Available
    }));
    assert!(capabilities.reports.iter().any(|report| {
        report.capability
            == blueice_ipc::debugger::DebuggerCapability::StaticMetadataContractLocation
            && report.state == blueice_ipc::debugger::DebuggerCapabilityState::Available
    }));
    assert!(capabilities.reports.iter().any(|report| {
        report.capability == blueice_ipc::debugger::DebuggerCapability::StaticMetadataSymbolType
            && report.state == blueice_ipc::debugger::DebuggerCapabilityState::Available
    }));
    assert!(capabilities.reports.iter().any(|report| {
        report.capability == blueice_ipc::debugger::DebuggerCapability::StaticMetadataSymbolContract
            && report.state == blueice_ipc::debugger::DebuggerCapabilityState::Available
    }));
    assert!(capabilities.reports.iter().any(|report| {
        report.capability
            == blueice_ipc::debugger::DebuggerCapability::StaticMetadataContractDisplay
            && report.state == blueice_ipc::debugger::DebuggerCapabilityState::Available
    }));
    assert!(capabilities.reports.iter().any(|report| {
        report.capability
            == blueice_ipc::debugger::DebuggerCapability::StaticMetadataContractValidation
            && report.state == blueice_ipc::debugger::DebuggerCapabilityState::Available
    }));
    assert!(capabilities.reports.iter().any(|report| {
        report.capability
            == blueice_ipc::debugger::DebuggerCapability::StaticMetadataLoweringSummary
            && report.state == blueice_ipc::debugger::DebuggerCapabilityState::Available
    }));
    assert!(capabilities.reports.iter().any(|report| {
        report.capability
            == blueice_ipc::debugger::DebuggerCapability::StaticMetadataContractInventory
            && report.state == blueice_ipc::debugger::DebuggerCapabilityState::Available
    }));
    assert!(capabilities.reports.iter().any(|report| {
        report.capability == blueice_ipc::debugger::DebuggerCapability::StaticMetadataTypeInventory
            && report.state == blueice_ipc::debugger::DebuggerCapabilityState::Available
    }));
    assert!(capabilities.reports.iter().any(|report| {
        report.capability == blueice_ipc::debugger::DebuggerCapability::StaticMetadataTypeDisplay
            && report.state == blueice_ipc::debugger::DebuggerCapabilityState::Available
    }));
    assert!(capabilities.reports.iter().any(|report| {
        report.capability
            == blueice_ipc::debugger::DebuggerCapability::StaticMetadataSymbolInventory
            && report.state == blueice_ipc::debugger::DebuggerCapabilityState::Available
    }));
    assert!(capabilities.reports.iter().any(|report| {
        report.capability == blueice_ipc::debugger::DebuggerCapability::StaticMetadataSummary
            && report.state == blueice_ipc::debugger::DebuggerCapabilityState::Available
    }));
    assert!(capabilities.reports.iter().any(|report| {
        report.capability
            == blueice_ipc::debugger::DebuggerCapability::StaticMetadataSourceInventory
            && report.state == blueice_ipc::debugger::DebuggerCapabilityState::Available
    }));
    assert!(capabilities.reports.iter().any(|report| {
        report.capability
            == blueice_ipc::debugger::DebuggerCapability::StaticMetadataSourceProvenance
            && report.state == blueice_ipc::debugger::DebuggerCapabilityState::Available
    }));
    assert!(capabilities.reports.iter().any(|report| {
        report.capability
            == blueice_ipc::debugger::DebuggerCapability::StaticMetadataSourceBreakpoint
            && report.state == blueice_ipc::debugger::DebuggerCapabilityState::Planned
    }));
    let DebuggerReply::Programs(programs) =
        debugger_request(&mut debugger, DebuggerRequest::ListPrograms { realm })
    else {
        panic!("typed fixture must expose its opaque program inventory")
    };
    assert!(
        !programs.is_empty(),
        "typed fixture must retain at least one opaque program identity"
    );

    let mut typed_metadata = None;
    for program in programs {
        let reply = debugger_request(
            &mut debugger,
            DebuggerRequest::ListStaticMetadata { program },
        );
        assert!(
            !format!("{reply:?}").contains("privateBlueTsMetadata")
                && !format!("{reply:?}").contains("number"),
            "public reply must contain no BlueTS metadata payload"
        );
        match reply {
            DebuggerReply::StaticMetadata(handles) if handles.is_empty() => {}
            DebuggerReply::StaticMetadata(handles) => {
                assert_eq!(
                    handles.len(),
                    1,
                    "only the direct BlueTS program is eligible"
                );
                let handle = handles[0];
                assert_eq!(handle.program, program);
                assert!(handle.is_well_formed());
                typed_metadata = Some(handle);
            }
            other => panic!("expected a bounded opaque metadata inventory, got {other:?}"),
        }
    }
    let typed_metadata = typed_metadata.expect("fixture must include one direct BlueTS program");
    let guessed_metadata = blueice_ipc::debugger::DebuggerStaticMetadataHandle {
        metadata_handle: typed_metadata.metadata_handle + 1,
        ..typed_metadata
    };
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::DescribeStaticMetadataLoweringSummary {
                metadata: guessed_metadata,
            },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::CapabilityUnavailable,
            ..
        }
    ));
    let lowering_summary_reply = debugger_request(
        &mut debugger,
        DebuggerRequest::DescribeStaticMetadataLoweringSummary {
            metadata: typed_metadata,
        },
    );
    let DebuggerReply::StaticMetadataLoweringSummary(lowering_summary) = lowering_summary_reply
    else {
        panic!("expected a bounded direct BlueTS-to-BlueJS lowering summary")
    };
    assert_eq!(lowering_summary.metadata, typed_metadata);
    assert_eq!(
        lowering_summary.safe_point_map_abi,
        blueice_ipc::debugger::DEBUGGER_STATIC_METADATA_SAFE_POINT_MAP_ABI_V1
    );
    assert_eq!(
        lowering_summary.program_abi,
        blueice_ipc::debugger::DEBUGGER_STATIC_METADATA_PROGRAM_ABI_V1
    );
    assert!(lowering_summary
        .source_set_hash
        .starts_with(blueice_ipc::debugger::DEBUGGER_STATIC_METADATA_SOURCE_SET_HASH_PREFIX));
    assert!(lowering_summary.bound_safe_point_count > 0);
    assert!(
        !format!("{lowering_summary:?}").contains("inline-0.ts")
            && !format!("{lowering_summary:?}").contains("bytecode_offset")
            && !format!("{lowering_summary:?}").contains("privateBlueTsMetadata"),
        "the public lowering summary must not expose source identities, map entries, bytecode offsets, or static records"
    );
    let summary_reply = debugger_request(
        &mut debugger,
        DebuggerRequest::DescribeStaticMetadata {
            metadata: typed_metadata,
        },
    );
    let DebuggerReply::StaticMetadataSummary(summary) = summary_reply else {
        panic!("expected one bounded public static metadata summary")
    };
    assert_eq!(summary.metadata, typed_metadata);
    assert_eq!(summary.language_version, "blue-ts-0.1");
    assert!(summary.source_count > 0);
    assert!(summary.type_count > 0);
    assert!(summary.symbol_count > 0);
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::DescribeStaticMetadataType {
                static_type: blueice_ipc::debugger::DebuggerStaticMetadataTypeId {
                    metadata: typed_metadata,
                    type_id: 0,
                },
            },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::CapabilityUnavailable,
            ..
        }
    ));
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::DescribeStaticMetadataContract {
                contract: blueice_ipc::debugger::DebuggerStaticMetadataContractId {
                    metadata: typed_metadata,
                    contract_id: 0,
                },
            },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::CapabilityUnavailable,
            ..
        }
    ));
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::ValidateStaticMetadataContract {
                contract: blueice_ipc::debugger::DebuggerStaticMetadataContractId {
                    metadata: typed_metadata,
                    contract_id: 0,
                },
                value: CompilerContractValue::Boolean(true),
            },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::CapabilityUnavailable,
            ..
        }
    ));
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::DescribeStaticMetadataSymbol {
                symbol: blueice_ipc::debugger::DebuggerStaticMetadataSymbolId {
                    metadata: typed_metadata,
                    symbol_id: 0,
                },
            },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::CapabilityUnavailable,
            ..
        }
    ));
    let unreceipted_relation = blueice_ipc::debugger::DebuggerStaticMetadataSymbolType {
        symbol: blueice_ipc::debugger::DebuggerStaticMetadataSymbolId {
            metadata: typed_metadata,
            symbol_id: 0,
        },
        static_type: blueice_ipc::debugger::DebuggerStaticMetadataTypeId {
            metadata: typed_metadata,
            type_id: 0,
        },
    };
    let unreceipted_contract_relation =
        blueice_ipc::debugger::DebuggerStaticMetadataSymbolContract {
            symbol: blueice_ipc::debugger::DebuggerStaticMetadataSymbolId {
                metadata: typed_metadata,
                symbol_id: 0,
            },
            contract: blueice_ipc::debugger::DebuggerStaticMetadataContractId {
                metadata: typed_metadata,
                contract_id: 0,
            },
        };
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::DescribeStaticMetadataSymbolType {
                target: unreceipted_relation,
            },
        ),
        DebuggerReply::Unsupported { .. }
    ));
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::DescribeStaticMetadataSymbolContract {
                target: unreceipted_contract_relation,
            },
        ),
        DebuggerReply::Unsupported { .. }
    ));
    let type_inventory_reply = debugger_request(
        &mut debugger,
        DebuggerRequest::ListStaticMetadataTypes {
            metadata: typed_metadata,
        },
    );
    let DebuggerReply::StaticMetadataTypes(types) = type_inventory_reply else {
        panic!("expected bounded public static metadata type-record IDs")
    };
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::DescribeStaticMetadataSymbolType {
                target: unreceipted_relation,
            },
        ),
        DebuggerReply::Unsupported { .. }
    ));
    assert_eq!(types.len(), usize::try_from(summary.type_count).unwrap());
    assert!(types
        .iter()
        .all(|static_type| static_type.metadata == typed_metadata));
    assert!(
        !format!("{types:?}").contains("privateBlueTsMetadata")
            && !format!("{types:?}").contains("number"),
        "type IDs must not contain static type displays or compiler-record payload"
    );
    for static_type in &types {
        let type_display_reply = debugger_request(
            &mut debugger,
            DebuggerRequest::DescribeStaticMetadataType {
                static_type: *static_type,
            },
        );
        let DebuggerReply::StaticMetadataType(type_display) = type_display_reply else {
            panic!("expected bounded public static metadata type display")
        };
        assert_eq!(type_display.static_type, *static_type);
        assert!(!type_display.display.is_empty());
        assert!(
            !format!("{type_display:?}").contains("privateBlueTsMetadata"),
            "type display must not expose source text"
        );
    }
    let symbol_inventory_reply = debugger_request(
        &mut debugger,
        DebuggerRequest::ListStaticMetadataSymbols {
            metadata: typed_metadata,
        },
    );
    let DebuggerReply::StaticMetadataSymbols(symbols) = symbol_inventory_reply else {
        panic!("expected bounded public static metadata symbol-record IDs")
    };
    assert_eq!(
        symbols.len(),
        usize::try_from(summary.symbol_count).unwrap()
    );
    assert!(symbols
        .iter()
        .all(|symbol| symbol.metadata == typed_metadata));
    assert!(
        !format!("{symbols:?}").contains("privateBlueTsMetadata")
            && !format!("{symbols:?}").contains("number"),
        "symbol IDs must not contain names, type displays, or compiler-record payload"
    );
    let mut saw_interface = false;
    let mut saw_variable = false;
    let mut interface_symbol = None;
    let mut variable_symbol = None;
    for symbol in &symbols {
        let symbol_display_reply = debugger_request(
            &mut debugger,
            DebuggerRequest::DescribeStaticMetadataSymbol { symbol: *symbol },
        );
        let DebuggerReply::StaticMetadataSymbol(symbol_display) = symbol_display_reply else {
            panic!("expected bounded public static metadata symbol display")
        };
        assert_eq!(symbol_display.symbol, *symbol);
        assert!(!symbol_display.display.is_empty());
        match symbol_display.display.as_str() {
            "PrivateContract" => {
                assert_eq!(
                    symbol_display.kind,
                    DebuggerStaticMetadataSymbolKind::Interface
                );
                assert!(symbol_display.exported);
                saw_interface = true;
                interface_symbol = Some(*symbol);
            }
            "privateBlueTsMetadata" => {
                assert_eq!(
                    symbol_display.kind,
                    DebuggerStaticMetadataSymbolKind::Variable
                );
                assert!(!symbol_display.exported);
                saw_variable = true;
                variable_symbol = Some(*symbol);
            }
            _ => {}
        }
        assert!(
            !symbol_display.display.contains("const ")
                && !symbol_display.display.contains(": number")
                && !symbol_display.display.contains("= 42")
                && !symbol_display.display.contains("inline-0.ts"),
            "symbol display may expose its authorized name, never declaration source, type, initializer, or module identity"
        );
    }
    assert!(saw_interface && saw_variable);
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::DescribeStaticMetadataSymbolContract {
                target: unreceipted_contract_relation,
            },
        ),
        DebuggerReply::Unsupported { .. }
    ));
    let contract_inventory_reply = debugger_request(
        &mut debugger,
        DebuggerRequest::ListStaticMetadataContracts {
            metadata: typed_metadata,
        },
    );
    let DebuggerReply::StaticMetadataContracts(contracts) = contract_inventory_reply else {
        panic!("expected bounded public static metadata contract IDs")
    };
    assert_eq!(
        contracts.len(),
        usize::try_from(summary.contract_count).unwrap()
    );
    assert!(contracts
        .iter()
        .all(|contract| contract.metadata == typed_metadata));
    assert!(
        !format!("{contracts:?}").contains("privateBlueTsMetadata")
            && !format!("{contracts:?}").contains("number"),
        "contract IDs must not contain names, plans, validation, or compiler-record payload"
    );
    for contract in &contracts {
        let contract_display_reply = debugger_request(
            &mut debugger,
            DebuggerRequest::DescribeStaticMetadataContract {
                contract: *contract,
            },
        );
        let DebuggerReply::StaticMetadataContract(contract_display) = contract_display_reply else {
            panic!("expected bounded public static metadata contract display")
        };
        assert_eq!(contract_display.contract, *contract);
        assert!(!contract_display.display.is_empty());
        assert_eq!(
            contract_display.root_kind,
            blueice_ipc::debugger::DebuggerStaticMetadataContractRootKind::Record
        );
        assert!(
            !contract_display.display.contains("interface ")
                && !contract_display.display.contains("enabled")
                && !contract_display.display.contains("boolean")
                && !contract_display.display.contains("inline-0.ts"),
            "contract display may expose its authorized name, never declaration source, field, type, or module identity"
        );
    }
    let valid_contract_validation = debugger_request(
        &mut debugger,
        DebuggerRequest::ValidateStaticMetadataContract {
            contract: contracts[0],
            value: CompilerContractValue::Object(
                [("enabled".to_string(), CompilerContractValue::Boolean(true))]
                    .into_iter()
                    .collect(),
            ),
        },
    );
    let DebuggerReply::StaticMetadataContractValidation(valid_contract_validation) =
        valid_contract_validation
    else {
        panic!("expected bounded public static contract validation result")
    };
    assert_eq!(valid_contract_validation.contract, contracts[0]);
    assert!(valid_contract_validation.valid);
    let invalid_contract_validation = debugger_request(
        &mut debugger,
        DebuggerRequest::ValidateStaticMetadataContract {
            contract: contracts[0],
            value: CompilerContractValue::Object(
                [(
                    "enabled".to_string(),
                    CompilerContractValue::String("not-a-boolean".to_string()),
                )]
                .into_iter()
                .collect(),
            ),
        },
    );
    let DebuggerReply::StaticMetadataContractValidation(invalid_contract_validation) =
        invalid_contract_validation
    else {
        panic!("expected a redacted invalid static contract validation result")
    };
    assert_eq!(invalid_contract_validation.contract, contracts[0]);
    assert!(!invalid_contract_validation.valid);
    assert!(
        !format!("{invalid_contract_validation:?}").contains("enabled")
            && !format!("{invalid_contract_validation:?}").contains("string"),
        "contract validation must not reflect caller input or structural failure detail"
    );
    assert!(
        !format!("{summary:?}").contains("privateBlueTsMetadata")
            && !format!("{summary:?}").contains("inline-0.ts")
            && !format!("{summary:?}").contains("number"),
        "public summary must not contain a BlueTS metadata record payload"
    );
    let source_inventory_reply = debugger_request(
        &mut debugger,
        DebuggerRequest::ListStaticMetadataSources {
            metadata: typed_metadata,
        },
    );
    let DebuggerReply::StaticMetadataSources(sources) = source_inventory_reply else {
        panic!("expected bounded public static metadata source-record IDs")
    };
    assert_eq!(
        sources.len(),
        usize::try_from(summary.source_count).unwrap()
    );
    assert!(sources
        .iter()
        .all(|source| source.metadata == typed_metadata));
    assert!(
        !format!("{sources:?}").contains("privateBlueTsMetadata")
            && !format!("{sources:?}").contains("inline-0.ts")
            && !format!("{sources:?}").contains("number"),
        "public source IDs must not contain source or compiler-record payload"
    );
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::ResolveStaticMetadataSourceBreakpoint {
                target: blueice_ipc::debugger::DebuggerStaticMetadataSourceBreakpointTarget {
                    source: sources[0],
                    source_byte: 0,
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
                    source: sources[0],
                    source_byte: 0,
                },
            },
        ),
        DebuggerReply::Unsupported { .. }
    ));
    let initial_contract_location_target =
        blueice_ipc::debugger::DebuggerStaticMetadataContractLocationTarget {
            contract: contracts[0],
            source: sources[0],
        };
    let guessed_contract_source = debugger_request(
        &mut debugger,
        DebuggerRequest::DescribeStaticMetadataContractLocation {
            target: blueice_ipc::debugger::DebuggerStaticMetadataContractLocationTarget {
                source: blueice_ipc::debugger::DebuggerStaticMetadataSourceId {
                    source_id: u32::MAX,
                    ..sources[0]
                },
                ..initial_contract_location_target
            },
        },
    );
    assert!(matches!(
        guessed_contract_source,
        DebuggerReply::Unsupported { .. }
    ));
    let mut matching_contract_locations = Vec::new();
    let mut wrong_source_count = 0;
    for source in &sources {
        let target = blueice_ipc::debugger::DebuggerStaticMetadataContractLocationTarget {
            contract: contracts[0],
            source: *source,
        };
        match debugger_request(
            &mut debugger,
            DebuggerRequest::DescribeStaticMetadataContractLocation { target },
        ) {
            DebuggerReply::StaticMetadataContractLocation(location) => {
                matching_contract_locations.push((target, location));
            }
            DebuggerReply::Error {
                code: DebuggerErrorCode::InvalidTarget,
                ..
            } => wrong_source_count += 1,
            reply => panic!("unexpected contract/source location reply: {reply:?}"),
        }
    }
    assert_eq!(matching_contract_locations.len(), 1);
    assert_eq!(wrong_source_count + 1, sources.len());
    let (contract_location_target, contract_location) = matching_contract_locations[0];
    assert_eq!(contract_location.contract, contracts[0]);
    assert_eq!(contract_location.source, contract_location_target.source);
    assert!(contract_location.start_byte < contract_location.end_byte);
    assert!(
        contract_location.end_byte
            <= blueice_ipc::debugger::DEBUGGER_STATIC_METADATA_MAX_SOURCE_SPAN_BYTES
    );
    assert_eq!(contract_location.coordinates.start_line, 0);
    assert_eq!(contract_location.coordinates.end_line, 0);
    assert!(!format!("{contract_location:?}").contains("PrivateContract"));
    assert!(!format!("{contract_location:?}").contains("enabled"));
    for source in &sources {
        let provenance_reply = debugger_request(
            &mut debugger,
            DebuggerRequest::DescribeStaticMetadataSource { source: *source },
        );
        let DebuggerReply::StaticMetadataSourceProvenance(provenance) = provenance_reply else {
            panic!("expected source-free debugger provenance for an inventoried source")
        };
        assert_eq!(provenance.source, *source);
        assert!(!provenance.module.is_empty());
        assert!(
            !provenance.module.starts_with('/'),
            "compiler provenance must expose a canonical module identity, not a filesystem path"
        );
        assert!(provenance.content_hash.starts_with("bts-sha256:"));
        assert_eq!(provenance.content_hash.len(), "bts-sha256:".len() + 64);
        assert!(
            !format!("{provenance:?}").contains("privateBlueTsMetadata")
                && !format!("{provenance:?}").contains("number"),
            "provenance must contain no source text or static-record payload"
        );
    }
    let location_target = blueice_ipc::debugger::DebuggerStaticMetadataSymbolLocationTarget {
        symbol: interface_symbol.expect("fixture must retain its interface symbol"),
        source: contract_location_target.source,
    };
    let guessed_location_reply = debugger_request(
        &mut debugger,
        DebuggerRequest::DescribeStaticMetadataSymbolLocation {
            target: blueice_ipc::debugger::DebuggerStaticMetadataSymbolLocationTarget {
                source: blueice_ipc::debugger::DebuggerStaticMetadataSourceId {
                    source_id: u32::MAX,
                    ..contract_location_target.source
                },
                ..location_target
            },
        },
    );
    assert!(
        matches!(guessed_location_reply, DebuggerReply::Unsupported { .. },),
        "a guessed source ID must fail before the child: {guessed_location_reply:?}"
    );
    let location_reply = debugger_request(
        &mut debugger,
        DebuggerRequest::DescribeStaticMetadataSymbolLocation {
            target: location_target,
        },
    );
    let DebuggerReply::StaticMetadataSymbolLocation(location) = location_reply else {
        panic!("expected a bounded public static symbol location")
    };
    assert_eq!(location.symbol, location_target.symbol);
    assert_eq!(location.source, location_target.source);
    assert!(location.start_byte < location.end_byte);
    assert!(
        location.end_byte <= blueice_ipc::debugger::DEBUGGER_STATIC_METADATA_MAX_SOURCE_SPAN_BYTES
    );
    assert_eq!(location.coordinates.start_line, 0);
    assert_eq!(location.coordinates.end_line, 0);
    assert!(
        !format!("{location:?}").contains("privateBlueTsMetadata")
            && !format!("{location:?}").contains("inline-0.ts")
            && !format!("{location:?}").contains("number"),
        "symbol location must expose only opaque IDs and a bounded byte range"
    );
    let variable_symbol = variable_symbol.expect("fixture must retain its local variable");
    let mut variable_locations = Vec::new();
    for source in &sources {
        match debugger_request(
            &mut debugger,
            DebuggerRequest::DescribeStaticMetadataSymbolLocation {
                target: blueice_ipc::debugger::DebuggerStaticMetadataSymbolLocationTarget {
                    symbol: variable_symbol,
                    source: *source,
                },
            },
        ) {
            DebuggerReply::StaticMetadataSymbolLocation(location) => {
                variable_locations.push(location);
            }
            DebuggerReply::Error {
                code: DebuggerErrorCode::InvalidTarget | DebuggerErrorCode::CapabilityUnavailable,
                ..
            } => {}
            reply => panic!("unexpected variable/source location reply: {reply:?}"),
        }
    }
    assert_eq!(variable_locations.len(), 1);
    let variable_location = variable_locations[0];
    assert_eq!(variable_location.coordinates.start_line, 1);
    assert_eq!(
        variable_location.coordinates.start_column_utf16,
        "/* 🚀 */ ".encode_utf16().count() as u32
    );
    assert_eq!(variable_location.coordinates.end_line, 1);
    let guessed_type_relation = blueice_ipc::debugger::DebuggerStaticMetadataSymbolType {
        symbol: symbols[0],
        static_type: blueice_ipc::debugger::DebuggerStaticMetadataTypeId {
            type_id: u32::MAX,
            ..types[0]
        },
    };
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::DescribeStaticMetadataSymbolType {
                target: guessed_type_relation,
            },
        ),
        DebuggerReply::Unsupported { .. }
    ));
    let mut verified_relation = None;
    for symbol in &symbols {
        for static_type in &types {
            let target = blueice_ipc::debugger::DebuggerStaticMetadataSymbolType {
                symbol: *symbol,
                static_type: *static_type,
            };
            match debugger_request(
                &mut debugger,
                DebuggerRequest::DescribeStaticMetadataSymbolType { target },
            ) {
                DebuggerReply::StaticMetadataSymbolType(relation) => {
                    assert_eq!(relation, target);
                    verified_relation = Some(relation);
                    break;
                }
                DebuggerReply::Error {
                    code: DebuggerErrorCode::InvalidTarget,
                    ..
                } => {}
                reply => panic!("unexpected symbol/type relation reply: {reply:?}"),
            }
        }
        if verified_relation.is_some() {
            break;
        }
    }
    let verified_relation =
        verified_relation.expect("the typed fixture has a symbol/type relation");
    assert!(verified_relation.is_well_formed());
    assert!(
        !format!("{verified_relation:?}").contains("privateBlueTsMetadata")
            && !format!("{verified_relation:?}").contains("number")
            && !format!("{verified_relation:?}").contains("inline-0.ts"),
        "the relation must repeat only opaque IDs"
    );
    let guessed_contract_relation = blueice_ipc::debugger::DebuggerStaticMetadataSymbolContract {
        symbol: symbols[0],
        contract: blueice_ipc::debugger::DebuggerStaticMetadataContractId {
            contract_id: u32::MAX,
            ..contracts[0]
        },
    };
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::DescribeStaticMetadataSymbolContract {
                target: guessed_contract_relation,
            },
        ),
        DebuggerReply::Unsupported { .. }
    ));
    let mut verified_contract_relation = None;
    for symbol in &symbols {
        for contract in &contracts {
            let target = blueice_ipc::debugger::DebuggerStaticMetadataSymbolContract {
                symbol: *symbol,
                contract: *contract,
            };
            match debugger_request(
                &mut debugger,
                DebuggerRequest::DescribeStaticMetadataSymbolContract { target },
            ) {
                DebuggerReply::StaticMetadataSymbolContract(relation) => {
                    assert_eq!(relation, target);
                    verified_contract_relation = Some(relation);
                    break;
                }
                DebuggerReply::Error {
                    code: DebuggerErrorCode::InvalidTarget,
                    ..
                } => {}
                reply => panic!("unexpected symbol/contract relation reply: {reply:?}"),
            }
        }
        if verified_contract_relation.is_some() {
            break;
        }
    }
    let verified_contract_relation = verified_contract_relation
        .expect("the interface fixture has a reifiable symbol/contract relation");
    assert!(verified_contract_relation.is_well_formed());
    let unrelated_symbol = symbols
        .iter()
        .copied()
        .find(|symbol| *symbol != verified_contract_relation.symbol)
        .expect("the fixture also retains a non-contract value symbol");
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::DescribeStaticMetadataSymbolContract {
                target: blueice_ipc::debugger::DebuggerStaticMetadataSymbolContract {
                    symbol: unrelated_symbol,
                    contract: verified_contract_relation.contract,
                },
            },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            ..
        }
    ));
    assert!(
        !format!("{verified_contract_relation:?}").contains("PrivateContract")
            && !format!("{verified_contract_relation:?}").contains("enabled")
            && !format!("{verified_contract_relation:?}").contains("inline-0.ts"),
        "the relation must repeat only opaque IDs, not a contract plan or name"
    );

    navigate(&mut browser, &url);
    fixture
        .join()
        .expect("local HTTP fixture must serve both documents");
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::ListStaticMetadata {
                program: typed_metadata.program,
            },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::StaleRealm,
            ..
        }
    ));
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::DescribeStaticMetadataContract {
                contract: contracts[0],
            },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::StaleRealm,
            ..
        }
    ));
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::ValidateStaticMetadataContract {
                contract: contracts[0],
                value: CompilerContractValue::Boolean(true),
            },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::StaleRealm,
            ..
        }
    ));
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::DescribeStaticMetadataSymbol { symbol: symbols[0] },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::StaleRealm,
            ..
        }
    ));
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::DescribeStaticMetadataContractLocation {
                target: contract_location_target,
            },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::StaleRealm,
            ..
        }
    ));
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::DescribeStaticMetadataSymbolLocation {
                target: location_target,
            },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::StaleRealm,
            ..
        }
    ));
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::DescribeStaticMetadataSymbolType {
                target: verified_relation,
            },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::StaleRealm,
            ..
        }
    ));
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::DescribeStaticMetadataSymbolContract {
                target: verified_contract_relation,
            },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::StaleRealm,
            ..
        }
    ));
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::ListStaticMetadataSources {
                metadata: typed_metadata,
            },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::StaleRealm,
            ..
        }
    ));
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::ListStaticMetadataTypes {
                metadata: typed_metadata,
            },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::StaleRealm,
            ..
        }
    ));
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::DescribeStaticMetadataType {
                static_type: types[0],
            },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::StaleRealm,
            ..
        }
    ));
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::ListStaticMetadataSymbols {
                metadata: typed_metadata,
            },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::StaleRealm,
            ..
        }
    ));
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::ListStaticMetadataContracts {
                metadata: typed_metadata,
            },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::StaleRealm,
            ..
        }
    ));
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::DescribeStaticMetadataSource { source: sources[0] },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::StaleRealm,
            ..
        }
    ));
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::DescribeStaticMetadata {
                metadata: typed_metadata,
            },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::StaleRealm,
            ..
        }
    ));
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::DescribeStaticMetadataLoweringSummary {
                metadata: typed_metadata,
            },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::StaleRealm,
            ..
        }
    ));

    launcher.shutdown();
    let _ = std::fs::remove_file(gatekeeper_socket);
}
