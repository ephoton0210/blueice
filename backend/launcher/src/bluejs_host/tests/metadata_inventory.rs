// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn child_bluets_metadata_inventory_mints_only_opaque_live_attachment_handles() {
    let mut host = BlueJsChildHost::default();
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: document(
                1,
                vec![
                    classic(0, "globalThis.javaScriptOnly = true;"),
                    blue_ts_classic(1, "const typedAnswer: number = 42;"),
                ],
            ),
        }),
        PageHostReply::Synchronized { reports, .. }
            if reports.iter().all(|report| report.outcome == PageHostScriptOutcome::Executed)
    ));
    let programs = match host.handle_request(PageHostRequest::ListDebuggerPrograms {
        tab_id: 7,
        document_generation: 1,
    }) {
        PageHostReply::DebuggerPrograms { programs, .. } => programs,
        reply => panic!("expected private program inventory, got {reply:?}"),
    };
    assert_eq!(programs.len(), 2);

    let mut blue_ts = None;
    for program in programs {
        let reply = host.handle_request(PageHostRequest::ListDebuggerBlueTsMetadata {
            tab_id: 7,
            document_generation: 1,
            program,
        });
        let PageHostReply::DebuggerBlueTsMetadata { metadata, .. } = &reply else {
            panic!("expected private BlueTS metadata inventory, got {reply:?}");
        };
        // The child exposes no source/module/name/type/span/contract data:
        // only the bounded list's length and its opaque values are visible.
        assert!(!format!("{reply:?}").contains("typedAnswer"));
        assert!(!format!("{reply:?}").contains("inline-1.ts"));
        assert!(!format!("{reply:?}").contains("number"));
        if let [metadata] = metadata.as_slice() {
            blue_ts = Some((program, *metadata));
        } else {
            assert!(
                metadata.is_empty(),
                "only the JavaScript program is ineligible"
            );
        }
    }
    let (typed_program, first_metadata) = blue_ts.expect("the live BlueTS attachment exists");
    assert!(first_metadata.is_well_formed());
    assert_ne!(
        first_metadata.metadata_handle, typed_program.program_handle,
        "metadata IDs must not reuse the child program namespace"
    );
    assert!(
        first_metadata.metadata_handle >= CHILD_DEBUGGER_METADATA_ID_NAMESPACE_START,
        "metadata IDs have a child-private namespace separate from program IDs"
    );
    let summary_reply = host.handle_request(PageHostRequest::DescribeDebuggerBlueTsMetadata {
        tab_id: 7,
        document_generation: 1,
        program: typed_program,
        metadata: first_metadata,
    });
    let PageHostReply::DebuggerBlueTsMetadataSummary {
        metadata, summary, ..
    } = summary_reply
    else {
        panic!("expected bounded private BlueTS metadata summary")
    };
    assert_eq!(metadata, first_metadata);
    assert_eq!(summary.language_version, "blue-ts-0.1");
    assert!(summary.source_count > 0);
    assert!(summary.type_count > 0);
    assert!(summary.symbol_count > 0);
    // The summary intentionally reveals no source/module/name/type/span/
    // contract record. A separate future capability would be needed for
    // every individual record family.
    assert!(!format!("{summary:?}").contains("typedAnswer"));
    assert!(!format!("{summary:?}").contains("inline-1.ts"));
    assert!(!format!("{summary:?}").contains("number"));
    let source_inventory_reply =
        host.handle_request(PageHostRequest::ListDebuggerBlueTsMetadataSources {
            tab_id: 7,
            document_generation: 1,
            program: typed_program,
            metadata: first_metadata,
        });
    let PageHostReply::DebuggerBlueTsMetadataSources {
        program,
        metadata,
        sources,
        ..
    } = source_inventory_reply
    else {
        panic!("expected bounded private BlueTS source-record identity inventory")
    };
    assert_eq!(program, typed_program);
    assert_eq!(metadata, first_metadata);
    assert_eq!(
        sources.len(),
        usize::try_from(summary.source_count).unwrap()
    );
    assert_eq!(
        sources
            .iter()
            .map(|source| source.source_id)
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        sources.len(),
        "one metadata attachment cannot repeat a compiler source-record ID"
    );
    // The opaque inventory conveys source-record cardinality and IDs only;
    // no module, source, hash, span, or compiler-record field crosses it.
    assert!(!format!("{sources:?}").contains("typedAnswer"));
    assert!(!format!("{sources:?}").contains("inline-1.ts"));
    assert!(!format!("{sources:?}").contains("number"));
    let type_inventory_reply =
        host.handle_request(PageHostRequest::ListDebuggerBlueTsMetadataTypes {
            tab_id: 7,
            document_generation: 1,
            program: typed_program,
            metadata: first_metadata,
        });
    let PageHostReply::DebuggerBlueTsMetadataTypes {
        program,
        metadata,
        types,
        ..
    } = type_inventory_reply
    else {
        panic!("expected bounded private BlueTS type-record identity inventory")
    };
    assert_eq!(program, typed_program);
    assert_eq!(metadata, first_metadata);
    assert_eq!(types.len(), usize::try_from(summary.type_count).unwrap());
    assert_eq!(
        types
            .iter()
            .map(|static_type| static_type.type_id)
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        types.len(),
        "one metadata attachment cannot repeat a compiler type-record ID"
    );
    assert!(!format!("{types:?}").contains("typedAnswer"));
    assert!(!format!("{types:?}").contains("inline-1.ts"));
    assert!(!format!("{types:?}").contains("number"));
    assert!(matches!(
        host.handle_request(PageHostRequest::DescribeDebuggerBlueTsMetadata {
            tab_id: 7,
            document_generation: 1,
            program: typed_program,
            metadata: PageHostDebuggerMetadataHandle {
                metadata_handle: first_metadata.metadata_handle,
                metadata_generation: first_metadata.metadata_generation + 1,
            },
        }),
        PageHostReply::Error {
            code: PageHostErrorCode::InvalidRequest,
            ..
        }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::ListDebuggerBlueTsMetadataSources {
            tab_id: 7,
            document_generation: 1,
            program: typed_program,
            metadata: PageHostDebuggerMetadataHandle {
                metadata_handle: first_metadata.metadata_handle,
                metadata_generation: first_metadata.metadata_generation + 1,
            },
        }),
        PageHostReply::Error {
            code: PageHostErrorCode::InvalidRequest,
            ..
        }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::ListDebuggerBlueTsMetadataTypes {
            tab_id: 7,
            document_generation: 1,
            program: typed_program,
            metadata: PageHostDebuggerMetadataHandle {
                metadata_handle: first_metadata.metadata_handle,
                metadata_generation: first_metadata.metadata_generation + 1,
            },
        }),
        PageHostReply::Error {
            code: PageHostErrorCode::InvalidRequest,
            ..
        }
    ));

    assert!(matches!(
        host.handle_request(PageHostRequest::ListDebuggerBlueTsMetadata {
            tab_id: 7,
            document_generation: 1,
            program: typed_program,
        }),
        PageHostReply::DebuggerBlueTsMetadata { metadata, .. }
            if metadata == vec![first_metadata]
    ));

    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: document(
                2,
                vec![blue_ts_classic(0, "const replacement: string = 'next';")]
            ),
        }),
        PageHostReply::Synchronized { .. }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::ListDebuggerBlueTsMetadata {
            tab_id: 7,
            document_generation: 1,
            program: typed_program,
        }),
        PageHostReply::Error {
            code: PageHostErrorCode::StaleDocument,
            ..
        }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::DescribeDebuggerBlueTsMetadata {
            tab_id: 7,
            document_generation: 1,
            program: typed_program,
            metadata: first_metadata,
        }),
        PageHostReply::Error {
            code: PageHostErrorCode::StaleDocument,
            ..
        }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::ListDebuggerBlueTsMetadataSources {
            tab_id: 7,
            document_generation: 1,
            program: typed_program,
            metadata: first_metadata,
        }),
        PageHostReply::Error {
            code: PageHostErrorCode::StaleDocument,
            ..
        }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::ListDebuggerBlueTsMetadataTypes {
            tab_id: 7,
            document_generation: 1,
            program: typed_program,
            metadata: first_metadata,
        }),
        PageHostReply::Error {
            code: PageHostErrorCode::StaleDocument,
            ..
        }
    ));
    let replacement_program = match host.handle_request(PageHostRequest::ListDebuggerPrograms {
        tab_id: 7,
        document_generation: 2,
    }) {
        PageHostReply::DebuggerPrograms { programs, .. } => *programs
            .first()
            .expect("the replacement BlueTS program remains live"),
        reply => panic!("expected replacement private program inventory, got {reply:?}"),
    };
    let replacement_metadata =
        match host.handle_request(PageHostRequest::ListDebuggerBlueTsMetadata {
            tab_id: 7,
            document_generation: 2,
            program: replacement_program,
        }) {
            PageHostReply::DebuggerBlueTsMetadata { metadata, .. } => *metadata
                .first()
                .expect("the replacement live BlueTS attachment remains eligible"),
            reply => panic!("expected replacement metadata inventory, got {reply:?}"),
        };
    assert_ne!(replacement_metadata, first_metadata);

    assert!(matches!(
        host.handle_request(PageHostRequest::CloseRealm {
            tab_id: 7,
            document_generation: 2,
        }),
        PageHostReply::RealmClosed { .. }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::ListDebuggerBlueTsMetadata {
            tab_id: 7,
            document_generation: 2,
            program: replacement_program,
        }),
        PageHostReply::Error {
            code: PageHostErrorCode::UnknownRealm,
            ..
        }
    ));
}

#[test]
fn child_bluets_symbol_contract_verifies_only_one_live_reifiable_pair() {
    let mut host = BlueJsChildHost::default();
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: document(
                1,
                vec![blue_ts_classic(
                    0,
                    "interface PrivateContract { enabled: boolean; } const typedAnswer: number = 42;",
                )],
            ),
        }),
        PageHostReply::Synchronized { reports, .. }
            if reports.iter().all(|report| report.outcome == PageHostScriptOutcome::Executed)
    ));
    let program = match host.handle_request(PageHostRequest::ListDebuggerPrograms {
        tab_id: 7,
        document_generation: 1,
    }) {
        PageHostReply::DebuggerPrograms { programs, .. } => programs[0],
        reply => panic!("expected live child program, got {reply:?}"),
    };
    let metadata = match host.handle_request(PageHostRequest::ListDebuggerBlueTsMetadata {
        tab_id: 7,
        document_generation: 1,
        program,
    }) {
        PageHostReply::DebuggerBlueTsMetadata { metadata, .. } => metadata[0],
        reply => panic!("expected live child metadata, got {reply:?}"),
    };
    let symbols = match host.handle_request(PageHostRequest::ListDebuggerBlueTsMetadataSymbols {
        tab_id: 7,
        document_generation: 1,
        program,
        metadata,
    }) {
        PageHostReply::DebuggerBlueTsMetadataSymbols { symbols, .. } => symbols,
        reply => panic!("expected child symbol IDs, got {reply:?}"),
    };
    let contracts =
        match host.handle_request(PageHostRequest::ListDebuggerBlueTsMetadataContracts {
            tab_id: 7,
            document_generation: 1,
            program,
            metadata,
        }) {
            PageHostReply::DebuggerBlueTsMetadataContracts { contracts, .. } => contracts,
            reply => panic!("expected child contract IDs, got {reply:?}"),
        };
    assert!(!symbols.is_empty() && !contracts.is_empty());
    let mut verified = None;
    for symbol in &symbols {
        for contract in &contracts {
            let reply = host.handle_request(
                PageHostRequest::DescribeDebuggerBlueTsMetadataSymbolContract {
                    tab_id: 7,
                    document_generation: 1,
                    program,
                    metadata,
                    symbol_id: symbol.symbol_id,
                    contract_id: contract.contract_id,
                },
            );
            match reply {
                PageHostReply::DebuggerBlueTsMetadataSymbolContract {
                    symbol_contract, ..
                } => {
                    assert_eq!(symbol_contract.symbol_id, symbol.symbol_id);
                    assert_eq!(symbol_contract.contract_id, contract.contract_id);
                    verified = Some(symbol_contract);
                    break;
                }
                PageHostReply::Error {
                    code: PageHostErrorCode::InvalidRequest,
                    ..
                } => {}
                reply => panic!("unexpected private relation reply: {reply:?}"),
            }
        }
        if verified.is_some() {
            break;
        }
    }
    let verified = verified.expect("the interface has one reifiable contract relation");
    assert!(!format!("{verified:?}").contains("PrivateContract"));
    assert!(!format!("{verified:?}").contains("enabled"));
    let sources = match host.handle_request(PageHostRequest::ListDebuggerBlueTsMetadataSources {
        tab_id: 7,
        document_generation: 1,
        program,
        metadata,
    }) {
        PageHostReply::DebuggerBlueTsMetadataSources { sources, .. } => sources,
        reply => panic!("expected child source IDs, got {reply:?}"),
    };
    let location = match host.handle_request(
        PageHostRequest::DescribeDebuggerBlueTsMetadataContractLocation {
            tab_id: 7,
            document_generation: 1,
            program,
            metadata,
            contract_id: verified.contract_id,
        },
    ) {
        PageHostReply::DebuggerBlueTsMetadataContractLocation { location, .. } => location,
        reply => panic!("expected child contract location, got {reply:?}"),
    };
    assert_eq!(location.contract_id, verified.contract_id);
    assert!(sources
        .iter()
        .any(|source| source.source_id == location.source_id));
    assert!(location.start_byte < location.end_byte);
    assert!(location.end_byte <= DEBUGGER_STATIC_METADATA_MAX_SOURCE_SPAN_BYTES);
    assert_eq!(location.coordinates.start_line, 0);
    assert_eq!(location.coordinates.end_line, 0);
    assert!(!format!("{location:?}").contains("PrivateContract"));
    assert!(!format!("{location:?}").contains("enabled"));
    assert!(matches!(
        host.handle_request(
            PageHostRequest::DescribeDebuggerBlueTsMetadataContractLocation {
                tab_id: 7,
                document_generation: 1,
                program,
                metadata,
                contract_id: u32::MAX,
            }
        ),
        PageHostReply::Error {
            code: PageHostErrorCode::InvalidRequest,
            ..
        }
    ));
    assert!(matches!(
        host.handle_request(
            PageHostRequest::DescribeDebuggerBlueTsMetadataSymbolContract {
                tab_id: 7,
                document_generation: 1,
                program,
                metadata,
                symbol_id: verified.symbol_id,
                contract_id: u32::MAX,
            }
        ),
        PageHostReply::Error {
            code: PageHostErrorCode::InvalidRequest,
            ..
        }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: document(2, vec![blue_ts_classic(0, "const successor: number = 1;")]),
        }),
        PageHostReply::Synchronized { .. }
    ));
    assert!(matches!(
        host.handle_request(
            PageHostRequest::DescribeDebuggerBlueTsMetadataSymbolContract {
                tab_id: 7,
                document_generation: 1,
                program,
                metadata,
                symbol_id: verified.symbol_id,
                contract_id: verified.contract_id,
            }
        ),
        PageHostReply::Error {
            code: PageHostErrorCode::StaleDocument,
            ..
        }
    ));
    assert!(matches!(
        host.handle_request(
            PageHostRequest::DescribeDebuggerBlueTsMetadataContractLocation {
                tab_id: 7,
                document_generation: 1,
                program,
                metadata,
                contract_id: verified.contract_id,
            }
        ),
        PageHostReply::Error {
            code: PageHostErrorCode::StaleDocument,
            ..
        }
    ));
}

#[test]
fn child_bluets_symbol_location_is_live_bound_and_source_text_free() {
    let mut host = BlueJsChildHost::default();
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: document(1, vec![blue_ts_classic(0, "const typedAnswer: number = 42;")]),
        }),
        PageHostReply::Synchronized { reports, .. }
            if reports.iter().all(|report| report.outcome == PageHostScriptOutcome::Executed)
    ));
    let program = match host.handle_request(PageHostRequest::ListDebuggerPrograms {
        tab_id: 7,
        document_generation: 1,
    }) {
        PageHostReply::DebuggerPrograms { programs, .. } => *programs
            .first()
            .expect("the one BlueTS program remains live"),
        reply => panic!("expected private debugger program inventory, got {reply:?}"),
    };
    let metadata = match host.handle_request(PageHostRequest::ListDebuggerBlueTsMetadata {
        tab_id: 7,
        document_generation: 1,
        program,
    }) {
        PageHostReply::DebuggerBlueTsMetadata { metadata, .. } => *metadata
            .first()
            .expect("the BlueTS program retains one static attachment"),
        reply => panic!("expected private BlueTS metadata inventory, got {reply:?}"),
    };
    let sources = match host.handle_request(PageHostRequest::ListDebuggerBlueTsMetadataSources {
        tab_id: 7,
        document_generation: 1,
        program,
        metadata,
    }) {
        PageHostReply::DebuggerBlueTsMetadataSources { sources, .. } => sources,
        reply => panic!("expected private source-ID inventory, got {reply:?}"),
    };
    let symbols = match host.handle_request(PageHostRequest::ListDebuggerBlueTsMetadataSymbols {
        tab_id: 7,
        document_generation: 1,
        program,
        metadata,
    }) {
        PageHostReply::DebuggerBlueTsMetadataSymbols { symbols, .. } => symbols,
        reply => panic!("expected private symbol-ID inventory, got {reply:?}"),
    };
    let symbol = symbols
        .into_iter()
        .find(|candidate| {
            matches!(
                host.handle_request(PageHostRequest::DescribeDebuggerBlueTsMetadataSymbol {
                    tab_id: 7,
                    document_generation: 1,
                    program,
                    metadata,
                    symbol_id: candidate.symbol_id,
                }),
                PageHostReply::DebuggerBlueTsMetadataSymbol { symbol, .. }
                    if symbol.display == "typedAnswer"
            )
        })
        .expect("the page declaration must retain its own symbol ID");
    let reply = host.handle_request(
        PageHostRequest::DescribeDebuggerBlueTsMetadataSymbolLocation {
            tab_id: 7,
            document_generation: 1,
            program,
            metadata,
            symbol_id: symbol.symbol_id,
        },
    );
    let PageHostReply::DebuggerBlueTsMetadataSymbolLocation { location, .. } = reply else {
        panic!("expected bounded private symbol location")
    };
    assert_eq!(location.symbol_id, symbol.symbol_id);
    assert!(sources
        .iter()
        .any(|source| source.source_id == location.source_id));
    assert!(location.start_byte < location.end_byte);
    assert!(location.end_byte <= DEBUGGER_STATIC_METADATA_MAX_SOURCE_SPAN_BYTES);
    assert_eq!(location.coordinates.start_line, 0);
    assert_eq!(location.coordinates.end_line, 0);
    // The result is deliberately only ID/range/coordinate structure, even inside the
    // private bridge: no source text, module identity, name, or type leaks.
    assert!(!format!("{location:?}").contains("typedAnswer"));
    assert!(!format!("{location:?}").contains("inline-0.ts"));
    assert!(!format!("{location:?}").contains("number"));
    let types = match host.handle_request(PageHostRequest::ListDebuggerBlueTsMetadataTypes {
        tab_id: 7,
        document_generation: 1,
        program,
        metadata,
    }) {
        PageHostReply::DebuggerBlueTsMetadataTypes { types, .. } => types,
        reply => panic!("expected private type-ID inventory, got {reply:?}"),
    };
    assert!(!types.is_empty());
    let matching = types
        .iter()
        .filter_map(|static_type| {
            let reply =
                host.handle_request(PageHostRequest::DescribeDebuggerBlueTsMetadataSymbolType {
                    tab_id: 7,
                    document_generation: 1,
                    program,
                    metadata,
                    symbol_id: symbol.symbol_id,
                    type_id: static_type.type_id,
                });
            match reply {
                PageHostReply::DebuggerBlueTsMetadataSymbolType { symbol_type, .. } => {
                    Some(symbol_type)
                }
                _ => None,
            }
        })
        .collect::<Vec<_>>();
    assert_eq!(matching.len(), 1);
    assert_eq!(matching[0].symbol_id, symbol.symbol_id);
    assert!(types
        .iter()
        .any(|static_type| static_type.type_id == matching[0].type_id));
    assert!(!format!("{:?}", matching[0]).contains("typedAnswer"));
    assert!(!format!("{:?}", matching[0]).contains("number"));
    assert!(matches!(
        host.handle_request(PageHostRequest::DescribeDebuggerBlueTsMetadataSymbolType {
            tab_id: 7,
            document_generation: 1,
            program,
            metadata,
            symbol_id: symbol.symbol_id,
            type_id: u32::MAX,
        }),
        PageHostReply::Error {
            code: PageHostErrorCode::InvalidRequest,
            ..
        }
    ));
    assert!(matches!(
        host.handle_request(
            PageHostRequest::DescribeDebuggerBlueTsMetadataSymbolLocation {
                tab_id: 7,
                document_generation: 1,
                program,
                metadata,
                symbol_id: u32::MAX,
            }
        ),
        PageHostReply::Error {
            code: PageHostErrorCode::InvalidRequest,
            ..
        }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: document(
                2,
                vec![blue_ts_classic(0, "const replacement: number = 1;")]
            ),
        }),
        PageHostReply::Synchronized { .. }
    ));
    assert!(matches!(
        host.handle_request(
            PageHostRequest::DescribeDebuggerBlueTsMetadataSymbolLocation {
                tab_id: 7,
                document_generation: 1,
                program,
                metadata,
                symbol_id: symbol.symbol_id,
            }
        ),
        PageHostReply::Error {
            code: PageHostErrorCode::StaleDocument,
            ..
        }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::DescribeDebuggerBlueTsMetadataSymbolType {
            tab_id: 7,
            document_generation: 1,
            program,
            metadata,
            symbol_id: symbol.symbol_id,
            type_id: matching[0].type_id,
        }),
        PageHostReply::Error {
            code: PageHostErrorCode::StaleDocument,
            ..
        }
    ));
}
