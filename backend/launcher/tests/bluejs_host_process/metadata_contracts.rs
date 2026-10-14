// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Real-subprocess evidence for the BlueTS private contract/symbol-relation
//! debugger endpoints (`bluets_metadata_catalog.rs`/`bluets_metadata_details.rs`):
//! contract listing/display/location/validation and symbol/type, symbol/contract
//! relation verification, and source provenance, all over the actual wire
//! protocol rather than by constructing `BlueJsChildHost` state directly.

use super::*;
use blueice_ipc::compiler::CompilerContractValue;
use blueice_ipc::debugger::DebuggerStaticMetadataContractRootKind;
use std::collections::BTreeMap;

#[test]
fn isolated_child_answers_every_private_bluets_contract_and_relation_endpoint() {
    assert!(std::path::Path::new(CHILD_BINARY).exists());
    let mut host = SpawnedBlueJsHost::spawn().unwrap();
    let entry = "blueice://page/contracts.ts";
    let source = "interface Settings { enabled: boolean; name?: string; } \
         const count: number = 1;";
    let script = PageHostScript {
        ordinal: 0,
        language: PageHostScriptLanguage::BlueTs,
        kind: PageHostScriptKind::Classic,
        graph: graph(entry, vec![PageHostSource::new(entry, source)]),
    };
    let admitted = host
        .synchronize_document(document(1, vec![script]))
        .unwrap();
    let debug_admitted = format!("{admitted:?}");
    assert!(
        matches!(
            &admitted,
            PageHostReply::Synchronized { reports, .. }
                if reports.iter().all(|report| report.outcome == PageHostScriptOutcome::Executed)
        ),
        "admission returned {debug_admitted}"
    );

    let program = match host
        .request(PageHostRequest::ListDebuggerPrograms {
            tab_id: 41,
            document_generation: 1,
        })
        .unwrap()
    {
        PageHostReply::DebuggerPrograms { programs, .. } => {
            assert_eq!(programs.len(), 1);
            programs[0]
        }
        reply => panic!("expected one private program, got {reply:?}"),
    };
    let metadata = match host
        .request(PageHostRequest::ListDebuggerBlueTsMetadata {
            tab_id: 41,
            document_generation: 1,
            program,
        })
        .unwrap()
    {
        PageHostReply::DebuggerBlueTsMetadata { metadata, .. } => metadata[0],
        reply => panic!("expected private BlueTS metadata, got {reply:?}"),
    };

    // -- symbol inventory: locate the `Settings` interface and `count` variable.
    let symbol_ids = match host
        .request(PageHostRequest::ListDebuggerBlueTsMetadataSymbols {
            tab_id: 41,
            document_generation: 1,
            program,
            metadata,
        })
        .unwrap()
    {
        PageHostReply::DebuggerBlueTsMetadataSymbols { symbols, .. } => symbols,
        reply => panic!("expected private symbol IDs, got {reply:?}"),
    };
    assert!(symbol_ids.len() >= 2);
    let mut settings_symbol = None;
    let mut count_symbol = None;
    for id in &symbol_ids {
        let display = match host
            .request(PageHostRequest::DescribeDebuggerBlueTsMetadataSymbol {
                tab_id: 41,
                document_generation: 1,
                program,
                metadata,
                symbol_id: id.symbol_id,
            })
            .unwrap()
        {
            PageHostReply::DebuggerBlueTsMetadataSymbol { symbol, .. } => symbol,
            reply => panic!("expected a private symbol display, got {reply:?}"),
        };
        match display.display.as_str() {
            "Settings" => settings_symbol = Some(display.symbol_id),
            "count" => count_symbol = Some(display.symbol_id),
            _ => {}
        }
    }
    let settings_symbol = settings_symbol.expect("the interface must mint a symbol");
    let count_symbol = count_symbol.expect("the typed variable must mint a symbol");

    // -- contract inventory: exactly the `Settings` interface's own contract.
    let contract_ids = match host
        .request(PageHostRequest::ListDebuggerBlueTsMetadataContracts {
            tab_id: 41,
            document_generation: 1,
            program,
            metadata,
        })
        .unwrap()
    {
        PageHostReply::DebuggerBlueTsMetadataContracts { contracts, .. } => contracts,
        reply => panic!("expected private contract IDs, got {reply:?}"),
    };
    assert_eq!(contract_ids.len(), 1);
    let contract_id = contract_ids[0].contract_id;

    // -- contract display.
    let contract_display = match host
        .request(PageHostRequest::DescribeDebuggerBlueTsMetadataContract {
            tab_id: 41,
            document_generation: 1,
            program,
            metadata,
            contract_id,
        })
        .unwrap()
    {
        PageHostReply::DebuggerBlueTsMetadataContract { contract, .. } => contract,
        reply => panic!("expected a private contract display, got {reply:?}"),
    };
    assert_eq!(contract_display.contract_id, contract_id);
    assert_eq!(contract_display.display, "Settings");
    assert_eq!(
        contract_display.root_kind,
        DebuggerStaticMetadataContractRootKind::Record
    );
    assert!(!format!("{contract_display:?}").contains("enabled"));

    // -- contract location.
    let source_ids = match host
        .request(PageHostRequest::ListDebuggerBlueTsMetadataSources {
            tab_id: 41,
            document_generation: 1,
            program,
            metadata,
        })
        .unwrap()
    {
        PageHostReply::DebuggerBlueTsMetadataSources { sources, .. } => sources,
        reply => panic!("expected private source IDs, got {reply:?}"),
    };
    assert!(!source_ids.is_empty());
    let location = match host
        .request(
            PageHostRequest::DescribeDebuggerBlueTsMetadataContractLocation {
                tab_id: 41,
                document_generation: 1,
                program,
                metadata,
                contract_id,
            },
        )
        .unwrap()
    {
        PageHostReply::DebuggerBlueTsMetadataContractLocation { location, .. } => location,
        reply => panic!("expected a private contract location, got {reply:?}"),
    };
    assert_eq!(location.contract_id, contract_id);
    assert!(source_ids
        .iter()
        .any(|source| source.source_id == location.source_id));
    assert!(location.start_byte < location.end_byte);
    assert!(!format!("{location:?}").contains("interface"));

    // -- source provenance for the contract's own source (the interface's
    // declaring module, guaranteed present regardless of how many other
    // synthetic sources the compiler also retains).
    let provenance = match host
        .request(PageHostRequest::DescribeDebuggerBlueTsMetadataSource {
            tab_id: 41,
            document_generation: 1,
            program,
            metadata,
            source_id: location.source_id,
        })
        .unwrap()
    {
        PageHostReply::DebuggerBlueTsMetadataSourceProvenance { provenance, .. } => provenance,
        reply => panic!("expected private source provenance, got {reply:?}"),
    };
    assert_eq!(provenance.source_id, location.source_id);
    assert_eq!(provenance.module, entry);
    assert_ne!(provenance.content_hash, source);
    assert!(!provenance.content_hash.is_empty());

    // -- contract validation: a matching snapshot is valid, a type-mismatched
    // one is not, both over the real child without ever echoing the input.
    let valid_value = CompilerContractValue::Object(BTreeMap::from([(
        "enabled".to_string(),
        CompilerContractValue::Boolean(true),
    )]));
    let valid = match host
        .request(PageHostRequest::ValidateDebuggerBlueTsMetadataContract {
            tab_id: 41,
            document_generation: 1,
            program,
            metadata,
            contract_id,
            value: valid_value,
        })
        .unwrap()
    {
        PageHostReply::DebuggerBlueTsMetadataContractValidation { validation, .. } => validation,
        reply => panic!("expected a private contract validation, got {reply:?}"),
    };
    assert_eq!(valid.contract_id, contract_id);
    assert!(valid.valid);

    let invalid_value = CompilerContractValue::Object(BTreeMap::from([(
        "enabled".to_string(),
        CompilerContractValue::String("no".to_string()),
    )]));
    let invalid = match host
        .request(PageHostRequest::ValidateDebuggerBlueTsMetadataContract {
            tab_id: 41,
            document_generation: 1,
            program,
            metadata,
            contract_id,
            value: invalid_value,
        })
        .unwrap()
    {
        PageHostReply::DebuggerBlueTsMetadataContractValidation { validation, .. } => validation,
        reply => panic!("expected a private contract validation, got {reply:?}"),
    };
    assert!(!invalid.valid);
    assert!(!format!("{invalid:?}").contains("enabled"));

    // -- symbol/type and symbol/contract relation verification: brute-force
    // over the small opaque ID space rather than assuming numbering, since the
    // wire protocol never exposes which type or contract a symbol carries.
    let type_ids = match host
        .request(PageHostRequest::ListDebuggerBlueTsMetadataTypes {
            tab_id: 41,
            document_generation: 1,
            program,
            metadata,
        })
        .unwrap()
    {
        PageHostReply::DebuggerBlueTsMetadataTypes { types, .. } => types,
        reply => panic!("expected private type IDs, got {reply:?}"),
    };
    let mut confirmed_symbol_types = 0;
    for symbol in &symbol_ids {
        for r#type in &type_ids {
            let reply = host
                .request(PageHostRequest::DescribeDebuggerBlueTsMetadataSymbolType {
                    tab_id: 41,
                    document_generation: 1,
                    program,
                    metadata,
                    symbol_id: symbol.symbol_id,
                    type_id: r#type.type_id,
                })
                .unwrap();
            match reply {
                PageHostReply::DebuggerBlueTsMetadataSymbolType { symbol_type, .. } => {
                    assert_eq!(symbol_type.symbol_id, symbol.symbol_id);
                    assert_eq!(symbol_type.type_id, r#type.type_id);
                    confirmed_symbol_types += 1;
                }
                PageHostReply::Error {
                    code: PageHostErrorCode::InvalidRequest,
                    ..
                } => {}
                reply => panic!("unexpected private symbol/type reply: {reply:?}"),
            }
        }
    }
    assert!(
        confirmed_symbol_types >= 1,
        "`count: number` must verify against exactly its own static type"
    );

    let mut confirmed_symbol_contracts = 0;
    for symbol in &symbol_ids {
        let reply = host
            .request(
                PageHostRequest::DescribeDebuggerBlueTsMetadataSymbolContract {
                    tab_id: 41,
                    document_generation: 1,
                    program,
                    metadata,
                    symbol_id: symbol.symbol_id,
                    contract_id,
                },
            )
            .unwrap();
        match reply {
            PageHostReply::DebuggerBlueTsMetadataSymbolContract {
                symbol_contract, ..
            } => {
                assert_eq!(symbol_contract.symbol_id, symbol.symbol_id);
                assert_eq!(symbol_contract.contract_id, contract_id);
                assert_eq!(symbol.symbol_id, settings_symbol);
                confirmed_symbol_contracts += 1;
            }
            PageHostReply::Error {
                code: PageHostErrorCode::InvalidRequest,
                ..
            } => {}
            reply => panic!("unexpected private symbol/contract reply: {reply:?}"),
        }
    }
    assert_eq!(
        confirmed_symbol_contracts, 1,
        "only the `Settings` interface's own symbol carries this contract"
    );

    // -- unknown IDs are rejected, not guessed at, for every relation endpoint.
    let unknown_id = contract_ids
        .iter()
        .map(|id| id.contract_id)
        .chain(symbol_ids.iter().map(|id| id.symbol_id))
        .chain(type_ids.iter().map(|id| id.type_id))
        .chain(source_ids.iter().map(|id| id.source_id))
        .max()
        .unwrap_or(0)
        + 1;
    assert!(matches!(
        host.request(PageHostRequest::DescribeDebuggerBlueTsMetadataContract {
            tab_id: 41,
            document_generation: 1,
            program,
            metadata,
            contract_id: unknown_id,
        })
        .unwrap(),
        PageHostReply::Error {
            code: PageHostErrorCode::InvalidRequest,
            ..
        }
    ));
    assert!(matches!(
        host.request(
            PageHostRequest::DescribeDebuggerBlueTsMetadataContractLocation {
                tab_id: 41,
                document_generation: 1,
                program,
                metadata,
                contract_id: unknown_id,
            }
        )
        .unwrap(),
        PageHostReply::Error {
            code: PageHostErrorCode::InvalidRequest,
            ..
        }
    ));
    assert!(matches!(
        host.request(
            PageHostRequest::DescribeDebuggerBlueTsMetadataSymbolContract {
                tab_id: 41,
                document_generation: 1,
                program,
                metadata,
                symbol_id: count_symbol,
                contract_id,
            }
        )
        .unwrap(),
        PageHostReply::Error {
            code: PageHostErrorCode::InvalidRequest,
            ..
        }
    ));
    assert!(matches!(
        host.request(PageHostRequest::DescribeDebuggerBlueTsMetadataSource {
            tab_id: 41,
            document_generation: 1,
            program,
            metadata,
            source_id: unknown_id,
        })
        .unwrap(),
        PageHostReply::Error {
            code: PageHostErrorCode::InvalidRequest,
            ..
        }
    ));

    // -- lowering summary and per-symbol/type declaration locations: two more
    // metadata-family endpoints this fixture can answer for real without a
    // second subprocess.
    let summary = match host
        .request(
            PageHostRequest::DescribeDebuggerBlueTsMetadataLoweringSummary {
                tab_id: 41,
                document_generation: 1,
                program,
                metadata,
            },
        )
        .unwrap()
    {
        PageHostReply::DebuggerBlueTsMetadataLoweringSummary { summary, .. } => summary,
        reply => panic!("expected a private lowering summary, got {reply:?}"),
    };
    assert!(!summary.safe_point_map_abi.is_empty());
    assert!(!summary.source_set_hash.is_empty());

    let count_type_display = {
        let mut display = None;
        for r#type in &type_ids {
            let reply = host
                .request(PageHostRequest::DescribeDebuggerBlueTsMetadataType {
                    tab_id: 41,
                    document_generation: 1,
                    program,
                    metadata,
                    type_id: r#type.type_id,
                })
                .unwrap();
            if let PageHostReply::DebuggerBlueTsMetadataType { static_type, .. } = reply {
                if static_type.display == "number" {
                    display = Some(static_type);
                }
            }
        }
        display.expect("`count`'s static type must display as `number`")
    };
    assert_eq!(count_type_display.display, "number");

    let count_location = match host
        .request(
            PageHostRequest::DescribeDebuggerBlueTsMetadataSymbolLocation {
                tab_id: 41,
                document_generation: 1,
                program,
                metadata,
                symbol_id: count_symbol,
            },
        )
        .unwrap()
    {
        PageHostReply::DebuggerBlueTsMetadataSymbolLocation { location, .. } => location,
        reply => panic!("expected a private symbol location, got {reply:?}"),
    };
    assert_eq!(count_location.symbol_id, count_symbol);
    assert!(count_location.start_byte < count_location.end_byte);
    assert!(!format!("{count_location:?}").contains("count"));

    // -- a malformed program or metadata handle is rejected up front by every
    // one of these endpoints, before any registry lookup is attempted.
    let malformed_program = blueice_ipc::page_host::PageHostDebuggerProgram {
        program_handle: 0,
        program_generation: program.program_generation,
    };
    let malformed_metadata = blueice_ipc::page_host::PageHostDebuggerMetadataHandle {
        metadata_handle: 0,
        metadata_generation: metadata.metadata_generation,
    };
    macro_rules! assert_rejects_malformed {
        ($request:expr) => {
            assert!(
                matches!(
                    host.request($request).unwrap(),
                    PageHostReply::Error {
                        code: PageHostErrorCode::InvalidRequest,
                        ..
                    }
                ),
                "expected a malformed-handle rejection for {}",
                stringify!($request)
            );
        };
    }
    assert_rejects_malformed!(PageHostRequest::ListDebuggerBlueTsMetadata {
        tab_id: 41,
        document_generation: 1,
        program: malformed_program,
    });
    assert_rejects_malformed!(
        PageHostRequest::DescribeDebuggerBlueTsMetadataLoweringSummary {
            tab_id: 41,
            document_generation: 1,
            program,
            metadata: malformed_metadata,
        }
    );
    assert_rejects_malformed!(PageHostRequest::ListDebuggerBlueTsMetadataSources {
        tab_id: 41,
        document_generation: 1,
        program: malformed_program,
        metadata,
    });
    assert_rejects_malformed!(PageHostRequest::ListDebuggerBlueTsMetadataTypes {
        tab_id: 41,
        document_generation: 1,
        program,
        metadata: malformed_metadata,
    });
    assert_rejects_malformed!(PageHostRequest::DescribeDebuggerBlueTsMetadataType {
        tab_id: 41,
        document_generation: 1,
        program: malformed_program,
        metadata,
        type_id: count_type_display.type_id,
    });
    assert_rejects_malformed!(PageHostRequest::ListDebuggerBlueTsMetadataSymbols {
        tab_id: 41,
        document_generation: 1,
        program,
        metadata: malformed_metadata,
    });
    assert_rejects_malformed!(PageHostRequest::DescribeDebuggerBlueTsMetadataSymbol {
        tab_id: 41,
        document_generation: 1,
        program: malformed_program,
        metadata,
        symbol_id: count_symbol,
    });
    assert_rejects_malformed!(
        PageHostRequest::DescribeDebuggerBlueTsMetadataSymbolLocation {
            tab_id: 41,
            document_generation: 1,
            program,
            metadata: malformed_metadata,
            symbol_id: count_symbol,
        }
    );
    assert_rejects_malformed!(PageHostRequest::ListDebuggerBlueTsMetadataContracts {
        tab_id: 41,
        document_generation: 1,
        program: malformed_program,
        metadata,
    });
    assert_rejects_malformed!(PageHostRequest::DescribeDebuggerBlueTsMetadataContract {
        tab_id: 41,
        document_generation: 1,
        program,
        metadata: malformed_metadata,
        contract_id,
    });
    assert_rejects_malformed!(
        PageHostRequest::DescribeDebuggerBlueTsMetadataContractLocation {
            tab_id: 41,
            document_generation: 1,
            program: malformed_program,
            metadata,
            contract_id,
        }
    );
    assert_rejects_malformed!(PageHostRequest::ValidateDebuggerBlueTsMetadataContract {
        tab_id: 41,
        document_generation: 1,
        program,
        metadata: malformed_metadata,
        contract_id,
        value: CompilerContractValue::Null,
    });
    assert_rejects_malformed!(PageHostRequest::DescribeDebuggerBlueTsMetadataSymbolType {
        tab_id: 41,
        document_generation: 1,
        program: malformed_program,
        metadata,
        symbol_id: count_symbol,
        type_id: count_type_display.type_id,
    });
    assert_rejects_malformed!(
        PageHostRequest::DescribeDebuggerBlueTsMetadataSymbolContract {
            tab_id: 41,
            document_generation: 1,
            program,
            metadata: malformed_metadata,
            symbol_id: settings_symbol,
            contract_id,
        }
    );
    assert_rejects_malformed!(PageHostRequest::DescribeDebuggerBlueTsMetadataSource {
        tab_id: 41,
        document_generation: 1,
        program: malformed_program,
        metadata,
        source_id: location.source_id,
    });

    host.shutdown().unwrap();
}
