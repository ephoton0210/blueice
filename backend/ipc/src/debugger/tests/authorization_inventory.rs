// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn handshake_rejects_wrong_or_missing_versions_before_dispatch() {
    let empty_policy = DebuggerMetadataCapabilityManifest::empty();
    assert_eq!(
        negotiate(
            &hello(DebuggerMetadataCapabilityManifest::empty()),
            &empty_policy
        ),
        DebuggerReply::HelloAck {
            protocol_version: DEBUGGER_PROTOCOL_VERSION,
            granted_metadata_capabilities: DebuggerMetadataCapabilityManifest::empty(),
            granted_bounded_values: false,
        }
    );
    for unsupported_version in [
        1,
        2,
        DEBUGGER_PROTOCOL_VERSION - 1,
        DEBUGGER_PROTOCOL_VERSION + 1,
    ] {
        assert!(matches!(
            negotiate(
                &DebuggerRequest::Hello {
                    protocol_version: unsupported_version,
                    requested_metadata_capabilities: DebuggerMetadataCapabilityManifest::empty(),
                    requested_bounded_values: false,
                },
                &empty_policy,
            ),
            DebuggerReply::Error {
                code: DebuggerErrorCode::ProtocolVersion,
                ..
            }
        ));
    }
    assert!(matches!(
        negotiate(&DebuggerRequest::ListPageRealms, &empty_policy),
        DebuggerReply::Error {
            code: DebuggerErrorCode::ProtocolVersion,
            ..
        }
    ));
    assert!(matches!(
        negotiate(
            &DebuggerRequest::DescribeCapabilities { realm: realm() },
            &empty_policy,
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::ProtocolVersion,
            ..
        }
    ));
}

#[test]
fn metadata_handshake_grants_only_the_canonical_policy_intersection() {
    let request = hello(DebuggerMetadataCapabilityManifest::opaque_inventory());
    let deny_reply = negotiate(&request, &DebuggerMetadataCapabilityManifest::empty());
    assert_eq!(
        deny_reply,
        DebuggerReply::HelloAck {
            protocol_version: DEBUGGER_PROTOCOL_VERSION,
            granted_metadata_capabilities: DebuggerMetadataCapabilityManifest::empty(),
            granted_bounded_values: false,
        }
    );
    let denied_session = metadata_session_authorization(&request, &deny_reply)
        .expect("a valid empty grant is still a negotiated session");
    assert!(!denied_session.permits(DebuggerMetadataCapability::OpaqueInventory));

    let allow_reply = negotiate(
        &request,
        &DebuggerMetadataCapabilityManifest::opaque_inventory(),
    );
    let allowed_session = metadata_session_authorization(&request, &allow_reply)
        .expect("the matching canonical requested and allowed sets must negotiate");
    assert!(allowed_session.permits(DebuggerMetadataCapability::OpaqueInventory));
    assert_eq!(
        allow_reply,
        DebuggerReply::HelloAck {
            protocol_version: DEBUGGER_PROTOCOL_VERSION,
            granted_metadata_capabilities: DebuggerMetadataCapabilityManifest::opaque_inventory(),
            granted_bounded_values: false,
        }
    );

    let empty_request = hello(DebuggerMetadataCapabilityManifest::empty());
    assert_eq!(
        negotiate(
            &empty_request,
            &DebuggerMetadataCapabilityManifest::opaque_inventory(),
        ),
        DebuggerReply::HelloAck {
            protocol_version: DEBUGGER_PROTOCOL_VERSION,
            granted_metadata_capabilities: DebuggerMetadataCapabilityManifest::empty(),
            granted_bounded_values: false,
        }
    );

    for malformed in [
        DebuggerMetadataCapabilityManifest {
            version: 0,
            capabilities: Vec::new(),
        },
        DebuggerMetadataCapabilityManifest {
            version: DEBUGGER_METADATA_CAPABILITY_MANIFEST_VERSION,
            capabilities: vec![
                DebuggerMetadataCapability::OpaqueInventory,
                DebuggerMetadataCapability::OpaqueInventory,
            ],
        },
        DebuggerMetadataCapabilityManifest {
            version: DEBUGGER_METADATA_CAPABILITY_MANIFEST_VERSION,
            capabilities: vec![DebuggerMetadataCapability::Unknown],
        },
        DebuggerMetadataCapabilityManifest {
            version: DEBUGGER_METADATA_CAPABILITY_MANIFEST_VERSION,
            capabilities: vec![DebuggerMetadataCapability::OpaqueSummary],
        },
        DebuggerMetadataCapabilityManifest {
            version: DEBUGGER_METADATA_CAPABILITY_MANIFEST_VERSION,
            capabilities: vec![DebuggerMetadataCapability::OpaqueSourceInventory],
        },
    ] {
        assert!(matches!(
            negotiate(
                &hello(malformed),
                &DebuggerMetadataCapabilityManifest::empty()
            ),
            DebuggerReply::Error {
                code: DebuggerErrorCode::InvalidCapabilityManifest,
                ..
            }
        ));
    }
    let unknown_wire_request: DebuggerRequest = serde_json::from_value(serde_json::json!({
        "Hello": {
            "protocol_version": DEBUGGER_PROTOCOL_VERSION,
            "requested_bounded_values": false,
            "requested_metadata_capabilities": {
                "version": DEBUGGER_METADATA_CAPABILITY_MANIFEST_VERSION,
                "capabilities": ["future-metadata-surface"],
            },
        },
    }))
    .expect("an unknown capability must preserve handshake framing");
    assert!(matches!(
        negotiate(
            &unknown_wire_request,
            &DebuggerMetadataCapabilityManifest::empty(),
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidCapabilityManifest,
            ..
        }
    ));
    assert!(matches!(
        negotiate(
            &request,
            &DebuggerMetadataCapabilityManifest {
                version: 0,
                capabilities: Vec::new(),
            },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidCapabilityManifest,
            ..
        }
    ));

    assert!(metadata_session_authorization(
        &empty_request,
        &DebuggerReply::HelloAck {
            protocol_version: DEBUGGER_PROTOCOL_VERSION,
            granted_metadata_capabilities: DebuggerMetadataCapabilityManifest::opaque_inventory(),
            granted_bounded_values: false,
        },
    )
    .is_none());
}

#[test]
fn bounded_values_handshake_requires_independent_owner_and_client_opt_in() {
    let metadata = DebuggerMetadataCapabilityManifest::empty();
    let requested = DebuggerRequest::Hello {
        protocol_version: DEBUGGER_PROTOCOL_VERSION,
        requested_metadata_capabilities: metadata.clone(),
        requested_bounded_values: true,
    };
    let denied = negotiate(&requested, &metadata);
    assert_eq!(
        denied,
        DebuggerReply::HelloAck {
            protocol_version: DEBUGGER_PROTOCOL_VERSION,
            granted_metadata_capabilities: metadata.clone(),
            granted_bounded_values: false,
        }
    );
    assert!(!metadata_session_authorization(&requested, &denied)
        .unwrap()
        .permits_bounded_values());
    let granted = negotiate_with_values(&requested, &metadata, true);
    assert_eq!(
        granted,
        DebuggerReply::HelloAck {
            protocol_version: DEBUGGER_PROTOCOL_VERSION,
            granted_metadata_capabilities: metadata.clone(),
            granted_bounded_values: true,
        }
    );
    assert!(metadata_session_authorization(&requested, &granted)
        .unwrap()
        .permits_bounded_values());
    let unrequested = DebuggerRequest::Hello {
        protocol_version: DEBUGGER_PROTOCOL_VERSION,
        requested_metadata_capabilities: metadata.clone(),
        requested_bounded_values: false,
    };
    assert!(metadata_session_authorization(&unrequested, &granted).is_none());
    assert!(!metadata_session_authorization(
        &unrequested,
        &negotiate_with_values(&unrequested, &metadata, true)
    )
    .unwrap()
    .permits_bounded_values());
}

#[test]
fn static_metadata_authorization_requires_session_and_one_exact_live_realm_grant() {
    let inventory = DebuggerMetadataCapability::OpaqueInventory;
    let request = hello(DebuggerMetadataCapabilityManifest::opaque_inventory());
    let denied_reply = negotiate(&request, &DebuggerMetadataCapabilityManifest::empty());
    let denied_session = metadata_session_authorization(&request, &denied_reply).unwrap();
    let allowed_reply = negotiate(
        &request,
        &DebuggerMetadataCapabilityManifest::opaque_inventory(),
    );
    let allowed_session = metadata_session_authorization(&request, &allowed_reply).unwrap();

    let available = capabilities(vec![capability_report(
        DebuggerCapability::StaticMetadataInventory,
        DebuggerCapabilityState::Available,
    )]);
    assert!(available
        .authorize_metadata(&denied_session, inventory)
        .is_none());
    assert!(capabilities(Vec::new())
        .authorize_metadata(&allowed_session, inventory)
        .is_none());
    assert!(capabilities(vec![capability_report(
        DebuggerCapability::StaticMetadataInventory,
        DebuggerCapabilityState::Planned,
    )])
    .authorize_metadata(&allowed_session, inventory)
    .is_none());
    assert!(capabilities(vec![capability_report(
        DebuggerCapability::StaticMetadataInventory,
        DebuggerCapabilityState::Unsupported,
    )])
    .authorize_metadata(&allowed_session, inventory)
    .is_none());
    assert!(capabilities(vec![
        capability_report(
            DebuggerCapability::StaticMetadataInventory,
            DebuggerCapabilityState::Available,
        ),
        capability_report(
            DebuggerCapability::StaticMetadataInventory,
            DebuggerCapabilityState::Available,
        ),
    ])
    .authorize_metadata(&allowed_session, inventory)
    .is_none());

    let mut wrong_version = available.clone();
    wrong_version.protocol_version -= 1;
    assert!(wrong_version
        .authorize_metadata(&allowed_session, inventory)
        .is_none());

    let mut malformed_realm = available.clone();
    malformed_realm.realm.realm_generation = 0;
    assert!(malformed_realm
        .authorize_metadata(&allowed_session, inventory)
        .is_none());

    let authorization = available
        .authorize_metadata(&allowed_session, inventory)
        .expect("one exact session and realm grant must authorize only that realm");
    assert!(authorization.permits(realm(), inventory));
    assert!(!authorization.permits(
        DebuggerPageRealm {
            realm_generation: realm().realm_generation + 1,
            ..realm()
        },
        inventory,
    ));
}

#[test]
fn metadata_inventory_receipts_are_exact_and_stream_local() {
    let request = hello(DebuggerMetadataCapabilityManifest::opaque_summary());
    let reply = negotiate(
        &request,
        &DebuggerMetadataCapabilityManifest::opaque_summary(),
    );
    let session = metadata_session_authorization(&request, &reply)
        .expect("the canonical summary grant must create a session receipt ledger");
    let metadata = DebuggerStaticMetadataHandle {
        program: DebuggerProgram {
            realm: realm(),
            program_handle: 12,
            program_generation: 4,
        },
        metadata_handle: 24,
        metadata_generation: 7,
    };
    assert!(
        !session.observed_metadata(metadata),
        "a numerically well-formed handle is not a receipt before inventory"
    );
    assert!(session.observe_metadata(&[metadata]));
    assert!(session.observed_metadata(metadata));
    assert!(
        !session.observed_metadata(DebuggerStaticMetadataHandle {
            metadata_generation: metadata.metadata_generation + 1,
            ..metadata
        }),
        "a changed generation cannot borrow a prior receipt"
    );

    let other_session = metadata_session_authorization(&request, &reply)
        .expect("a second handshake has its own receipt ledger");
    assert!(
        !other_session.observed_metadata(metadata),
        "a metadata receipt must not cross debugger streams"
    );

    let budget_session = metadata_session_authorization(&request, &reply)
        .expect("a new stream must start with an empty receipt ledger");
    let full_budget = (1..=DEBUGGER_METADATA_SESSION_MAX_OBSERVED_METADATA_IDENTITIES)
        .map(|metadata_handle| DebuggerStaticMetadataHandle {
            metadata_handle: u64::try_from(metadata_handle).unwrap(),
            ..metadata
        })
        .collect::<Vec<_>>();
    assert!(budget_session.observe_metadata(&full_budget));
    let overflow = DebuggerStaticMetadataHandle {
        metadata_handle: u64::try_from(
            DEBUGGER_METADATA_SESSION_MAX_OBSERVED_METADATA_IDENTITIES + 1,
        )
        .unwrap(),
        ..metadata
    };
    assert!(
        !budget_session.observe_metadata(&[overflow]),
        "the bounded insertion must reject rather than partially grow the receipt ledger"
    );
    assert!(
        !budget_session.observed_metadata(overflow),
        "a rejected batch must not mint its overflow handle"
    );
}

#[test]
fn static_metadata_summary_requires_its_own_dependent_capability_grant() {
    let request = hello(DebuggerMetadataCapabilityManifest::opaque_summary());
    let reply = negotiate(
        &request,
        &DebuggerMetadataCapabilityManifest::opaque_summary(),
    );
    let session = metadata_session_authorization(&request, &reply)
        .expect("the canonical dependent metadata grant must negotiate");
    assert!(session.permits(DebuggerMetadataCapability::OpaqueInventory));
    assert!(session.permits(DebuggerMetadataCapability::OpaqueSummary));

    let inventory_only_request = hello(DebuggerMetadataCapabilityManifest::opaque_inventory());
    let inventory_only_reply = negotiate(
        &inventory_only_request,
        &DebuggerMetadataCapabilityManifest::opaque_summary(),
    );
    let inventory_only =
        metadata_session_authorization(&inventory_only_request, &inventory_only_reply).unwrap();
    assert!(!inventory_only.permits(DebuggerMetadataCapability::OpaqueSummary));

    let summary_available = capabilities(vec![capability_report(
        DebuggerCapability::StaticMetadataSummary,
        DebuggerCapabilityState::Available,
    )]);
    let authorization = summary_available
        .authorize_metadata(&session, DebuggerMetadataCapability::OpaqueSummary)
        .expect("summary requires its exact available report and session grant");
    assert!(authorization.permits(realm(), DebuggerMetadataCapability::OpaqueSummary));
    assert!(summary_available
        .authorize_metadata(&inventory_only, DebuggerMetadataCapability::OpaqueSummary)
        .is_none());
}

#[test]
fn static_metadata_source_inventory_requires_its_own_dependent_grant() {
    let request = hello(DebuggerMetadataCapabilityManifest::opaque_source_inventory());
    let reply = negotiate(
        &request,
        &DebuggerMetadataCapabilityManifest::opaque_source_inventory(),
    );
    let session = metadata_session_authorization(&request, &reply)
        .expect("the canonical dependent source-inventory grant must negotiate");
    assert!(session.permits(DebuggerMetadataCapability::OpaqueInventory));
    assert!(session.permits(DebuggerMetadataCapability::OpaqueSourceInventory));

    let inventory_only_request = hello(DebuggerMetadataCapabilityManifest::opaque_inventory());
    let inventory_only_reply = negotiate(
        &inventory_only_request,
        &DebuggerMetadataCapabilityManifest::opaque_source_inventory(),
    );
    let inventory_only =
        metadata_session_authorization(&inventory_only_request, &inventory_only_reply).unwrap();
    assert!(!inventory_only.permits(DebuggerMetadataCapability::OpaqueSourceInventory));

    let source_inventory_available = capabilities(vec![capability_report(
        DebuggerCapability::StaticMetadataSourceInventory,
        DebuggerCapabilityState::Available,
    )]);
    let authorization = source_inventory_available
        .authorize_metadata(&session, DebuggerMetadataCapability::OpaqueSourceInventory)
        .expect("source inventory requires its exact available report and session grant");
    assert!(authorization.permits(realm(), DebuggerMetadataCapability::OpaqueSourceInventory));
    assert!(source_inventory_available
        .authorize_metadata(
            &inventory_only,
            DebuggerMetadataCapability::OpaqueSourceInventory
        )
        .is_none());

    assert_eq!(
        DebuggerMetadataCapabilityManifest::opaque_summary_and_source_inventory().capabilities,
        vec![
            DebuggerMetadataCapability::OpaqueInventory,
            DebuggerMetadataCapability::OpaqueSummary,
            DebuggerMetadataCapability::OpaqueSourceInventory,
        ]
    );
    assert!(
        DebuggerMetadataCapabilityManifest::opaque_summary_and_source_inventory().is_well_formed()
    );
}

#[test]
fn static_metadata_source_provenance_requires_source_inventory_and_sha256_format() {
    let request = hello(DebuggerMetadataCapabilityManifest::opaque_source_provenance());
    let reply = negotiate(
        &request,
        &DebuggerMetadataCapabilityManifest::opaque_source_provenance(),
    );
    let session = metadata_session_authorization(&request, &reply)
        .expect("the canonical provenance grant must negotiate");
    assert!(session.permits(DebuggerMetadataCapability::OpaqueInventory));
    assert!(session.permits(DebuggerMetadataCapability::OpaqueSourceInventory));
    assert!(session.permits(DebuggerMetadataCapability::OpaqueSourceProvenance));

    let source_inventory_request =
        hello(DebuggerMetadataCapabilityManifest::opaque_source_inventory());
    let source_inventory_reply = negotiate(
        &source_inventory_request,
        &DebuggerMetadataCapabilityManifest::opaque_source_provenance(),
    );
    let source_inventory =
        metadata_session_authorization(&source_inventory_request, &source_inventory_reply).unwrap();
    assert!(!source_inventory.permits(DebuggerMetadataCapability::OpaqueSourceProvenance));

    let provenance_available = capabilities(vec![capability_report(
        DebuggerCapability::StaticMetadataSourceProvenance,
        DebuggerCapabilityState::Available,
    )]);
    assert!(provenance_available
        .authorize_metadata(&session, DebuggerMetadataCapability::OpaqueSourceProvenance)
        .is_some());
    assert!(provenance_available
        .authorize_metadata(
            &source_inventory,
            DebuggerMetadataCapability::OpaqueSourceProvenance
        )
        .is_none());

    let source = DebuggerStaticMetadataSourceId {
        metadata: DebuggerStaticMetadataHandle {
            program: DebuggerProgram {
                realm: realm(),
                program_handle: 12,
                program_generation: 4,
            },
            metadata_handle: 24,
            metadata_generation: 7,
        },
        source_id: 0,
    };
    let provenance = DebuggerStaticMetadataSourceProvenance {
        source,
        module: "page-inline:///0.ts".to_string(),
        content_hash: "bts-sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
            .to_string(),
    };
    assert!(provenance.is_well_formed());
    assert!(!DebuggerStaticMetadataSourceProvenance {
        content_hash: "bts-fnv:0000000000000000".to_string(),
        ..provenance.clone()
    }
    .is_well_formed());
    assert!(!DebuggerStaticMetadataSourceProvenance {
        module: "/private/source.ts".to_string(),
        ..provenance.clone()
    }
    .is_well_formed());
    assert!(!DebuggerStaticMetadataSourceProvenance {
        module: "file:///private/source.ts".to_string(),
        ..provenance
    }
    .is_well_formed());
}

#[test]
fn static_metadata_handles_are_opaque_and_generation_bound() {
    let handle = DebuggerStaticMetadataHandle {
        program: DebuggerProgram {
            realm: realm(),
            program_handle: 12,
            program_generation: 5,
        },
        metadata_handle: 41,
        metadata_generation: 9,
    };
    assert!(handle.is_well_formed());
    assert_eq!(
        serde_json::to_value(handle).unwrap(),
        serde_json::json!({
            "program": {
                "realm": {
                    "browser_context_id": 1,
                    "tab_id": 7,
                    "realm_generation": 3,
                },
                "program_handle": 12,
                "program_generation": 5,
            },
            "metadata_handle": 41,
            "metadata_generation": 9,
        })
    );
    assert!(!DebuggerStaticMetadataHandle {
        metadata_handle: 0,
        ..handle
    }
    .is_well_formed());
    assert!(!DebuggerStaticMetadataHandle {
        metadata_generation: 0,
        ..handle
    }
    .is_well_formed());
}

#[test]
fn static_metadata_summary_is_bounded_and_source_free() {
    let metadata = DebuggerStaticMetadataHandle {
        program: DebuggerProgram {
            realm: realm(),
            program_handle: 12,
            program_generation: 5,
        },
        metadata_handle: 41,
        metadata_generation: 9,
    };
    let summary = DebuggerStaticMetadataSummary {
        metadata,
        language_version: "blue-ts-0.1".to_string(),
        compiler_options_hash: "0123456789abcdef".to_string(),
        source_count: 1,
        type_count: 2,
        symbol_count: 3,
        contract_count: 4,
    };
    assert!(summary.is_well_formed());
    assert!(!DebuggerStaticMetadataSummary {
        compiler_options_hash: String::new(),
        ..summary.clone()
    }
    .is_well_formed());
    assert!(!DebuggerStaticMetadataSummary {
        symbol_count: DEBUGGER_STATIC_METADATA_MAX_SYMBOLS + 1,
        ..summary
    }
    .is_well_formed());
}

#[test]
fn type_inventory_is_parent_bound_and_receipted_without_a_type_display() {
    let metadata = DebuggerStaticMetadataHandle {
        program: DebuggerProgram {
            realm: realm(),
            program_handle: 12,
            program_generation: 5,
        },
        metadata_handle: 41,
        metadata_generation: 9,
    };
    let static_type = DebuggerStaticMetadataTypeId {
        metadata,
        type_id: 0,
    };
    assert!(static_type.is_well_formed());
    let request = hello(DebuggerMetadataCapabilityManifest::opaque_type_inventory());
    let reply = negotiate(
        &request,
        &DebuggerMetadataCapabilityManifest::opaque_type_inventory(),
    );
    let session = metadata_session_authorization(&request, &reply).unwrap();
    assert!(session.permits(DebuggerMetadataCapability::OpaqueTypeInventory));
    assert!(!session.permits(DebuggerMetadataCapability::OpaqueTypeDisplay));
    assert!(!session.observed_type(static_type));
    assert!(session.observe_types(&[static_type]));
    assert!(session.observed_type(static_type));
    assert!(!session.observed_type(DebuggerStaticMetadataTypeId {
        type_id: 1,
        ..static_type
    }));
    assert_eq!(
        serde_json::to_value(static_type).unwrap(),
        serde_json::json!({
            "metadata": {
                "program": {
                    "realm": {
                        "browser_context_id": 1,
                        "tab_id": 7,
                        "realm_generation": 3,
                    },
                    "program_handle": 12,
                    "program_generation": 5,
                },
                "metadata_handle": 41,
                "metadata_generation": 9,
            },
            "type_id": 0,
        })
    );
}

#[test]
fn type_display_requires_type_inventory_and_respects_its_fixed_budget() {
    let metadata = DebuggerStaticMetadataHandle {
        program: DebuggerProgram {
            realm: realm(),
            program_handle: 12,
            program_generation: 5,
        },
        metadata_handle: 41,
        metadata_generation: 9,
    };
    let static_type = DebuggerStaticMetadataTypeId {
        metadata,
        type_id: 0,
    };
    let request = hello(DebuggerMetadataCapabilityManifest::opaque_type_display());
    let reply = negotiate(
        &request,
        &DebuggerMetadataCapabilityManifest::opaque_type_display(),
    );
    let session = metadata_session_authorization(&request, &reply).unwrap();
    assert!(session.permits(DebuggerMetadataCapability::OpaqueInventory));
    assert!(session.permits(DebuggerMetadataCapability::OpaqueTypeInventory));
    assert!(session.permits(DebuggerMetadataCapability::OpaqueTypeDisplay));
    assert!(!session.observed_type(static_type));
    assert!(session.observe_types(&[static_type]));
    assert!(session.observed_type(static_type));

    let display = DebuggerStaticMetadataTypeDisplay {
        static_type,
        display: "ProjectControlledName".to_string(),
    };
    assert!(display.is_well_formed());
    assert!(!DebuggerStaticMetadataTypeDisplay {
        display: String::new(),
        ..display.clone()
    }
    .is_well_formed());
    assert!(!DebuggerStaticMetadataTypeDisplay {
        display: "x".repeat(DEBUGGER_STATIC_METADATA_TYPE_DISPLAY_MAX_BYTES + 1),
        ..display
    }
    .is_well_formed());

    let malformed = DebuggerMetadataCapabilityManifest {
        version: DEBUGGER_METADATA_CAPABILITY_MANIFEST_VERSION,
        capabilities: vec![DebuggerMetadataCapability::OpaqueTypeDisplay],
    };
    assert!(!malformed.is_well_formed());
}

#[test]
fn symbol_inventory_is_parent_bound_and_receipted_without_symbol_detail() {
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
    assert!(symbol.is_well_formed());
    let request = hello(DebuggerMetadataCapabilityManifest::opaque_symbol_inventory());
    let reply = negotiate(
        &request,
        &DebuggerMetadataCapabilityManifest::opaque_symbol_inventory(),
    );
    let session = metadata_session_authorization(&request, &reply).unwrap();
    assert!(session.permits(DebuggerMetadataCapability::OpaqueSymbolInventory));
    assert!(!session.observed_symbol(symbol));
    assert!(session.observe_symbols(&[symbol]));
    assert!(session.observed_symbol(symbol));
    assert!(!session.observed_symbol(DebuggerStaticMetadataSymbolId {
        symbol_id: 1,
        ..symbol
    }));

    let malformed = DebuggerMetadataCapabilityManifest {
        version: DEBUGGER_METADATA_CAPABILITY_MANIFEST_VERSION,
        capabilities: vec![DebuggerMetadataCapability::OpaqueSymbolInventory],
    };
    assert!(!malformed.is_well_formed());
}

#[test]
fn contract_inventory_is_parent_bound_and_receipted_without_contract_detail() {
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
    assert!(contract.is_well_formed());
    let request = hello(DebuggerMetadataCapabilityManifest::opaque_contract_inventory());
    let reply = negotiate(
        &request,
        &DebuggerMetadataCapabilityManifest::opaque_contract_inventory(),
    );
    let session = metadata_session_authorization(&request, &reply).unwrap();
    assert!(session.permits(DebuggerMetadataCapability::OpaqueContractInventory));
    assert!(!session.observed_contract(contract));
    assert!(session.observe_contracts(&[contract]));
    assert!(session.observed_contract(contract));
    assert!(
        !session.observed_contract(DebuggerStaticMetadataContractId {
            contract_id: 1,
            ..contract
        })
    );

    let malformed = DebuggerMetadataCapabilityManifest {
        version: DEBUGGER_METADATA_CAPABILITY_MANIFEST_VERSION,
        capabilities: vec![DebuggerMetadataCapability::OpaqueContractInventory],
    };
    assert!(!malformed.is_well_formed());
}
