// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn discovery_requires_the_live_tab_and_exact_document_generation() {
    let (mut tabs, realm) = loaded_tabs();
    assert_eq!(
        handle_debugger_request(&tabs, DebuggerRequest::ListPageRealms),
        DebuggerReply::PageRealms(vec![realm])
    );
    let reply = handle_debugger_request(&tabs, DebuggerRequest::DescribeCapabilities { realm });
    let DebuggerReply::Capabilities(capabilities) = reply else {
        panic!("the live realm must have a discovery reply")
    };
    assert_eq!(capabilities.realm, realm);
    assert_eq!(capabilities.protocol_version, DEBUGGER_PROTOCOL_VERSION);
    assert!(capabilities
        .reports
        .iter()
        .all(|report| report.state == DebuggerCapabilityState::Planned));

    tabs.get_mut(TabId::from_u64(realm.tab_id))
        .unwrap()
        .load_html_str(
            "<main>replacement</main>",
            Some("https://example.test/replacement".to_string()),
        );
    assert!(matches!(
        handle_debugger_request(&tabs, DebuggerRequest::DescribeCapabilities { realm }),
        DebuggerReply::Error {
            code: DebuggerErrorCode::StaleRealm,
            ..
        }
    ));
    assert_eq!(
        handle_debugger_request(&tabs, DebuggerRequest::ListPageRealms),
        DebuggerReply::PageRealms(vec![DebuggerPageRealm {
            realm_generation: 2,
            ..realm
        }])
    );
}

#[test]
fn static_metadata_summary_requires_a_dependent_session_grant_and_exact_handle() {
    let (tabs, realm) = loaded_tabs();
    let program = DebuggerProgram {
        realm,
        program_handle: 7,
        program_generation: 3,
    };
    let mut locations = MetadataLocations {
        malformed_summary: false,
        malformed_provenance: false,
        malformed_lowering_summary: false,
        mismatched_symbol_display: false,
        mismatched_contract_display: false,
        mismatched_contract_validation: false,
    };

    let denied_capabilities = handle_debugger_request_with_child_locations(
        &tabs,
        &mut locations,
        None,
        DebuggerRequest::DescribeCapabilities { realm },
    );
    let DebuggerReply::Capabilities(denied_capabilities) = denied_capabilities else {
        panic!("live realm capability discovery must succeed")
    };
    assert!(denied_capabilities.reports.iter().any(|report| {
        report.capability == DebuggerCapability::StaticMetadataInventory
            && report.state == DebuggerCapabilityState::Planned
    }));
    assert!(denied_capabilities.reports.iter().any(|report| {
        report.capability == DebuggerCapability::StaticMetadataSummary
            && report.state == DebuggerCapabilityState::Planned
    }));
    assert!(denied_capabilities.reports.iter().any(|report| {
        report.capability == DebuggerCapability::StaticMetadataSymbolInventory
            && report.state == DebuggerCapabilityState::Planned
    }));
    assert!(denied_capabilities.reports.iter().any(|report| {
        report.capability == DebuggerCapability::StaticMetadataContractInventory
            && report.state == DebuggerCapabilityState::Planned
    }));
    assert!(denied_capabilities.reports.iter().any(|report| {
        report.capability == DebuggerCapability::StaticMetadataContractDisplay
            && report.state == DebuggerCapabilityState::Planned
    }));
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            None,
            DebuggerRequest::ListStaticMetadata { program },
        ),
        unavailable_static_metadata_inventory()
    );
    let metadata = DebuggerStaticMetadataHandle {
        program,
        metadata_handle: 41,
        metadata_generation: 9,
    };
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            None,
            DebuggerRequest::DescribeStaticMetadata { metadata },
        ),
        unavailable_static_metadata_summary()
    );

    let hello = DebuggerRequest::Hello {
        protocol_version: DEBUGGER_PROTOCOL_VERSION,
        requested_bounded_values: false,
        requested_metadata_capabilities:
            blueice_ipc::debugger::DebuggerMetadataCapabilityManifest::opaque_inventory(),
    };
    let hello_reply = blueice_ipc::debugger::negotiate(
        &hello,
        &blueice_ipc::debugger::DebuggerMetadataCapabilityManifest::opaque_inventory(),
    );
    let metadata_session =
        blueice_ipc::debugger::metadata_session_authorization(&hello, &hello_reply)
            .expect("matching core policy must create a core-local session authorization");

    let allowed_capabilities = handle_debugger_request_with_child_locations(
        &tabs,
        &mut locations,
        Some(&metadata_session),
        DebuggerRequest::DescribeCapabilities { realm },
    );
    let DebuggerReply::Capabilities(allowed_capabilities) = allowed_capabilities else {
        panic!("live realm capability discovery must succeed")
    };
    assert!(allowed_capabilities.reports.iter().any(|report| {
        report.capability == DebuggerCapability::StaticMetadataInventory
            && report.state == DebuggerCapabilityState::Available
    }));
    assert!(allowed_capabilities.reports.iter().any(|report| {
        report.capability == DebuggerCapability::StaticMetadataSummary
            && report.state == DebuggerCapabilityState::Planned
    }));
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&metadata_session),
            DebuggerRequest::ListStaticMetadata { program },
        ),
        DebuggerReply::StaticMetadata(vec![DebuggerStaticMetadataHandle {
            program,
            metadata_handle: 41,
            metadata_generation: 9,
        }])
    );
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&metadata_session),
            DebuggerRequest::DescribeStaticMetadata { metadata },
        ),
        unavailable_static_metadata_summary()
    );

    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&metadata_session),
            DebuggerRequest::ListStaticMetadataSources { metadata },
        ),
        unavailable_static_metadata_source_inventory()
    );
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&metadata_session),
            DebuggerRequest::ListStaticMetadataSymbols { metadata },
        ),
        unavailable_static_metadata_symbol_inventory(),
        "symbol inventory remains default-denied under a parent-only grant"
    );
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&metadata_session),
            DebuggerRequest::DescribeStaticMetadataSymbol {
                symbol: DebuggerStaticMetadataSymbolId {
                    metadata,
                    symbol_id: 0,
                },
            },
        ),
        unavailable_static_metadata_symbol_display(),
        "symbol display remains default-denied under a parent-only grant"
    );
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&metadata_session),
            DebuggerRequest::ListStaticMetadataContracts { metadata },
        ),
        unavailable_static_metadata_contract_inventory(),
        "contract inventory remains default-denied under a parent-only grant"
    );
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&metadata_session),
            DebuggerRequest::DescribeStaticMetadataContract {
                contract: DebuggerStaticMetadataContractId {
                    metadata,
                    contract_id: 0,
                },
            },
        ),
        unavailable_static_metadata_contract_display(),
        "contract display remains default-denied under a parent-only grant"
    );

    let symbol_inventory_hello = DebuggerRequest::Hello {
        protocol_version: DEBUGGER_PROTOCOL_VERSION,
        requested_bounded_values: false,
        requested_metadata_capabilities:
            blueice_ipc::debugger::DebuggerMetadataCapabilityManifest::opaque_symbol_inventory(),
    };
    let symbol_inventory_hello_reply = blueice_ipc::debugger::negotiate(
        &symbol_inventory_hello,
        &blueice_ipc::debugger::DebuggerMetadataCapabilityManifest::opaque_symbol_inventory(),
    );
    let symbol_inventory_session = blueice_ipc::debugger::metadata_session_authorization(
        &symbol_inventory_hello,
        &symbol_inventory_hello_reply,
    )
    .expect("dependent symbol inventory policy must create a core-local session authorization");
    let symbol_inventory_capabilities = handle_debugger_request_with_child_locations(
        &tabs,
        &mut locations,
        Some(&symbol_inventory_session),
        DebuggerRequest::DescribeCapabilities { realm },
    );
    let DebuggerReply::Capabilities(symbol_inventory_capabilities) = symbol_inventory_capabilities
    else {
        panic!("live realm symbol inventory capability discovery must succeed")
    };
    assert!(symbol_inventory_capabilities.reports.iter().any(|report| {
        report.capability == DebuggerCapability::StaticMetadataSymbolInventory
            && report.state == DebuggerCapabilityState::Available
    }));
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&symbol_inventory_session),
            DebuggerRequest::ListStaticMetadataSymbols { metadata },
        ),
        unavailable_static_metadata_symbol_inventory(),
        "symbol inventory cannot dereference a parent handle guessed before inventory"
    );
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&symbol_inventory_session),
            DebuggerRequest::ListStaticMetadata { program },
        ),
        DebuggerReply::StaticMetadata(vec![metadata])
    );
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&symbol_inventory_session),
            DebuggerRequest::ListStaticMetadataSymbols { metadata },
        ),
        DebuggerReply::StaticMetadataSymbols(vec![
            DebuggerStaticMetadataSymbolId {
                metadata,
                symbol_id: 0,
            },
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

    let symbol_display_hello = DebuggerRequest::Hello {
        protocol_version: DEBUGGER_PROTOCOL_VERSION,
        requested_bounded_values: false,
        requested_metadata_capabilities:
            blueice_ipc::debugger::DebuggerMetadataCapabilityManifest::opaque_symbol_display(),
    };
    let symbol_display_hello_reply = blueice_ipc::debugger::negotiate(
        &symbol_display_hello,
        &blueice_ipc::debugger::DebuggerMetadataCapabilityManifest::opaque_symbol_display(),
    );
    let symbol_display_session = blueice_ipc::debugger::metadata_session_authorization(
        &symbol_display_hello,
        &symbol_display_hello_reply,
    )
    .expect("dependent symbol display policy must create a core-local session authorization");
    let symbol_display_capabilities = handle_debugger_request_with_child_locations(
        &tabs,
        &mut locations,
        Some(&symbol_display_session),
        DebuggerRequest::DescribeCapabilities { realm },
    );
    let DebuggerReply::Capabilities(symbol_display_capabilities) = symbol_display_capabilities
    else {
        panic!("live realm symbol display capability discovery must succeed")
    };
    assert!(symbol_display_capabilities.reports.iter().any(|report| {
        report.capability == DebuggerCapability::StaticMetadataSymbolDisplay
            && report.state == DebuggerCapabilityState::Available
    }));
    let displayed_symbol = DebuggerStaticMetadataSymbolId {
        metadata,
        symbol_id: 0,
    };
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&symbol_display_session),
            DebuggerRequest::DescribeStaticMetadataSymbol {
                symbol: displayed_symbol,
            },
        ),
        unavailable_static_metadata_symbol_display(),
        "symbol display cannot dereference an ID guessed before its inventory receipt"
    );
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&symbol_display_session),
            DebuggerRequest::ListStaticMetadata { program },
        ),
        DebuggerReply::StaticMetadata(vec![metadata])
    );
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&symbol_display_session),
            DebuggerRequest::ListStaticMetadataSymbols { metadata },
        ),
        DebuggerReply::StaticMetadataSymbols(vec![
            displayed_symbol,
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
            Some(&symbol_display_session),
            DebuggerRequest::DescribeStaticMetadataSymbol {
                symbol: displayed_symbol,
            },
        ),
        DebuggerReply::StaticMetadataSymbol(DebuggerStaticMetadataSymbolDisplay {
            symbol: displayed_symbol,
            display: "ProjectControlledName".to_string(),
            kind: blueice_ipc::debugger::DebuggerStaticMetadataSymbolKind::Interface,
            exported: true,
        })
    );

    let contract_inventory_hello = DebuggerRequest::Hello {
        protocol_version: DEBUGGER_PROTOCOL_VERSION,
        requested_bounded_values: false,
        requested_metadata_capabilities:
            blueice_ipc::debugger::DebuggerMetadataCapabilityManifest::opaque_contract_inventory(),
    };
    let contract_inventory_hello_reply = blueice_ipc::debugger::negotiate(
        &contract_inventory_hello,
        &blueice_ipc::debugger::DebuggerMetadataCapabilityManifest::opaque_contract_inventory(),
    );
    let contract_inventory_session = blueice_ipc::debugger::metadata_session_authorization(
        &contract_inventory_hello,
        &contract_inventory_hello_reply,
    )
    .expect("dependent contract inventory policy must create a core-local session authorization");
    let contract_inventory_capabilities = handle_debugger_request_with_child_locations(
        &tabs,
        &mut locations,
        Some(&contract_inventory_session),
        DebuggerRequest::DescribeCapabilities { realm },
    );
    let DebuggerReply::Capabilities(contract_inventory_capabilities) =
        contract_inventory_capabilities
    else {
        panic!("live realm contract inventory capability discovery must succeed")
    };
    assert!(contract_inventory_capabilities
        .reports
        .iter()
        .any(|report| {
            report.capability == DebuggerCapability::StaticMetadataContractInventory
                && report.state == DebuggerCapabilityState::Available
        }));
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&contract_inventory_session),
            DebuggerRequest::ListStaticMetadataContracts { metadata },
        ),
        unavailable_static_metadata_contract_inventory(),
        "contract inventory cannot dereference a parent handle guessed before inventory"
    );
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&contract_inventory_session),
            DebuggerRequest::ListStaticMetadata { program },
        ),
        DebuggerReply::StaticMetadata(vec![metadata])
    );
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&contract_inventory_session),
            DebuggerRequest::ListStaticMetadataContracts { metadata },
        ),
        DebuggerReply::StaticMetadataContracts(vec![
            DebuggerStaticMetadataContractId {
                metadata,
                contract_id: 0,
            },
            DebuggerStaticMetadataContractId {
                metadata,
                contract_id: 1,
            },
        ])
    );

    let contract_display_hello = DebuggerRequest::Hello {
        protocol_version: DEBUGGER_PROTOCOL_VERSION,
        requested_bounded_values: false,
        requested_metadata_capabilities:
            blueice_ipc::debugger::DebuggerMetadataCapabilityManifest::opaque_contract_display(),
    };
    let contract_display_hello_reply = blueice_ipc::debugger::negotiate(
        &contract_display_hello,
        &blueice_ipc::debugger::DebuggerMetadataCapabilityManifest::opaque_contract_display(),
    );
    let contract_display_session = blueice_ipc::debugger::metadata_session_authorization(
        &contract_display_hello,
        &contract_display_hello_reply,
    )
    .expect("dependent contract display policy must create a core-local session authorization");
    let contract_display_capabilities = handle_debugger_request_with_child_locations(
        &tabs,
        &mut locations,
        Some(&contract_display_session),
        DebuggerRequest::DescribeCapabilities { realm },
    );
    let DebuggerReply::Capabilities(contract_display_capabilities) = contract_display_capabilities
    else {
        panic!("live realm contract display capability discovery must succeed")
    };
    assert!(contract_display_capabilities.reports.iter().any(|report| {
        report.capability == DebuggerCapability::StaticMetadataContractDisplay
            && report.state == DebuggerCapabilityState::Available
    }));
    let displayed_contract = DebuggerStaticMetadataContractId {
        metadata,
        contract_id: 0,
    };
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&contract_display_session),
            DebuggerRequest::DescribeStaticMetadataContract {
                contract: displayed_contract,
            },
        ),
        unavailable_static_metadata_contract_display(),
        "contract display cannot dereference an ID guessed before its inventory receipt"
    );
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&contract_display_session),
            DebuggerRequest::ListStaticMetadata { program },
        ),
        DebuggerReply::StaticMetadata(vec![metadata])
    );
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&contract_display_session),
            DebuggerRequest::ListStaticMetadataContracts { metadata },
        ),
        DebuggerReply::StaticMetadataContracts(vec![
            displayed_contract,
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
            Some(&contract_display_session),
            DebuggerRequest::DescribeStaticMetadataContract {
                contract: displayed_contract,
            },
        ),
        DebuggerReply::StaticMetadataContract(DebuggerStaticMetadataContractDisplay {
            contract: displayed_contract,
            display: "ProjectControlledContract".to_string(),
            root_kind: blueice_ipc::debugger::DebuggerStaticMetadataContractRootKind::Record,
        })
    );

    let source_inventory_hello = DebuggerRequest::Hello {
        protocol_version: DEBUGGER_PROTOCOL_VERSION,
        requested_bounded_values: false,
        requested_metadata_capabilities:
            blueice_ipc::debugger::DebuggerMetadataCapabilityManifest::opaque_source_inventory(),
    };
    let source_inventory_hello_reply = blueice_ipc::debugger::negotiate(
        &source_inventory_hello,
        &blueice_ipc::debugger::DebuggerMetadataCapabilityManifest::opaque_source_inventory(),
    );
    let source_inventory_session = blueice_ipc::debugger::metadata_session_authorization(
        &source_inventory_hello,
        &source_inventory_hello_reply,
    )
    .expect("dependent source inventory policy must create a core-local session authorization");
    let source_inventory_capabilities = handle_debugger_request_with_child_locations(
        &tabs,
        &mut locations,
        Some(&source_inventory_session),
        DebuggerRequest::DescribeCapabilities { realm },
    );
    let DebuggerReply::Capabilities(source_inventory_capabilities) = source_inventory_capabilities
    else {
        panic!("live realm source inventory capability discovery must succeed")
    };
    assert!(source_inventory_capabilities.reports.iter().any(|report| {
        report.capability == DebuggerCapability::StaticMetadataSourceInventory
            && report.state == DebuggerCapabilityState::Available
    }));
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&source_inventory_session),
            DebuggerRequest::ListStaticMetadataSources { metadata },
        ),
        unavailable_static_metadata_source_inventory(),
        "source inventory requires a parent handle emitted to this stream"
    );
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&source_inventory_session),
            DebuggerRequest::ListStaticMetadata { program },
        ),
        DebuggerReply::StaticMetadata(vec![metadata])
    );
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&source_inventory_session),
            DebuggerRequest::ListStaticMetadataSources { metadata },
        ),
        DebuggerReply::StaticMetadataSources(vec![DebuggerStaticMetadataSourceId {
            metadata,
            source_id: 0,
        }])
    );

    let summary_hello = DebuggerRequest::Hello {
        protocol_version: DEBUGGER_PROTOCOL_VERSION,
        requested_bounded_values: false,
        requested_metadata_capabilities:
            blueice_ipc::debugger::DebuggerMetadataCapabilityManifest::opaque_summary(),
    };
    let summary_hello_reply = blueice_ipc::debugger::negotiate(
        &summary_hello,
        &blueice_ipc::debugger::DebuggerMetadataCapabilityManifest::opaque_summary(),
    );
    let summary_session =
        blueice_ipc::debugger::metadata_session_authorization(&summary_hello, &summary_hello_reply)
            .expect("dependent summary policy must create a core-local session authorization");
    let summary_capabilities = handle_debugger_request_with_child_locations(
        &tabs,
        &mut locations,
        Some(&summary_session),
        DebuggerRequest::DescribeCapabilities { realm },
    );
    let DebuggerReply::Capabilities(summary_capabilities) = summary_capabilities else {
        panic!("live realm summary capability discovery must succeed")
    };
    assert!(summary_capabilities.reports.iter().any(|report| {
        report.capability == DebuggerCapability::StaticMetadataSummary
            && report.state == DebuggerCapabilityState::Available
    }));
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&summary_session),
            DebuggerRequest::DescribeStaticMetadata { metadata },
        ),
        unavailable_static_metadata_summary(),
        "summary requires a parent handle emitted to this stream"
    );
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&summary_session),
            DebuggerRequest::ListStaticMetadata { program },
        ),
        DebuggerReply::StaticMetadata(vec![metadata])
    );
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&summary_session),
            DebuggerRequest::DescribeStaticMetadata { metadata },
        ),
        DebuggerReply::StaticMetadataSummary(DebuggerStaticMetadataSummary {
            metadata,
            language_version: "blue-ts-0.1".to_string(),
            compiler_options_hash: "0123456789abcdef".to_string(),
            source_count: 1,
            type_count: 2,
            symbol_count: 3,
            contract_count: 4,
        })
    );
}

#[test]
fn static_metadata_summary_rejects_an_over_budget_child_reply() {
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
    let hello = DebuggerRequest::Hello {
        protocol_version: DEBUGGER_PROTOCOL_VERSION,
        requested_bounded_values: false,
        requested_metadata_capabilities:
            blueice_ipc::debugger::DebuggerMetadataCapabilityManifest::opaque_summary(),
    };
    let hello_reply = blueice_ipc::debugger::negotiate(
        &hello,
        &blueice_ipc::debugger::DebuggerMetadataCapabilityManifest::opaque_summary(),
    );
    let session = blueice_ipc::debugger::metadata_session_authorization(&hello, &hello_reply)
        .expect("dependent summary policy must create a core-local session authorization");
    let mut locations = MetadataLocations {
        malformed_summary: true,
        malformed_provenance: false,
        malformed_lowering_summary: false,
        mismatched_symbol_display: false,
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
            DebuggerRequest::DescribeStaticMetadata { metadata },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            ..
        }
    ));
}
