// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn core_proxies_real_child_debugger_locations_with_core_ids_and_rejects_stale_cross_tab_targets() {
    use crate::debugger::handle_debugger_request_with_page_javascript_executor;
    use blueice_ipc::debugger::{
        DebuggerCapability, DebuggerCapabilityState, DebuggerErrorCode, DebuggerPageRealm,
        DebuggerReply, DebuggerRequest,
    };

    let (path, token, child) = spawn_child();
    let (mut tabs, first_tab) = loaded_tabs(
        "<script>let first = 1; first += 1;</script>",
        "https://example.test/first.html",
    );
    let second_tab = tabs.open_tab();
    tabs.get_mut(second_tab).unwrap().load_html_str(
        "<script>let second = 2;</script>",
        Some("https://example.test/second.html".to_string()),
    );
    let mut executor = OutOfProcessJavaScriptPageExecutor::connect(&path, &token).unwrap();
    executor.synchronize_and_execute(&tabs).unwrap();

    let first_realm = DebuggerPageRealm {
        browser_context_id: crate::debugger::DEFAULT_BROWSER_CONTEXT_ID,
        tab_id: first_tab.as_u64(),
        realm_generation: 1,
    };
    let second_realm = DebuggerPageRealm {
        browser_context_id: crate::debugger::DEFAULT_BROWSER_CONTEXT_ID,
        tab_id: second_tab.as_u64(),
        realm_generation: 1,
    };
    let capabilities = handle_debugger_request_with_page_javascript_executor(
        &tabs,
        Some(&mut executor),
        DebuggerRequest::DescribeCapabilities { realm: first_realm },
    );
    let DebuggerReply::Capabilities(capabilities) = capabilities else {
        panic!("expected child debugger capabilities");
    };
    assert_eq!(
        capabilities
            .reports
            .iter()
            .find(|report| report.capability == DebuggerCapability::ProgramLocations)
            .map(|report| report.state),
        Some(DebuggerCapabilityState::Available)
    );
    assert_eq!(
        capabilities
            .reports
            .iter()
            .find(|report| report.capability == DebuggerCapability::BreakpointConfiguration)
            .map(|report| report.state),
        Some(DebuggerCapabilityState::Available)
    );
    assert!(
        capabilities
            .reports
            .iter()
            .all(|report| !report.detail.contains("first") && !report.detail.contains("bytecode")),
        "capability replies must remain source/bytecode-free"
    );

    let programs = match handle_debugger_request_with_page_javascript_executor(
        &tabs,
        Some(&mut executor),
        DebuggerRequest::ListPrograms { realm: first_realm },
    ) {
        DebuggerReply::Programs(programs) => programs,
        reply => panic!("expected public program inventory, got {reply:?}"),
    };
    let program = *programs.first().expect("first child page has one program");
    assert!(
        program.program_handle >= CORE_CHILD_DEBUGGER_ID_NAMESPACE_START
            && program.program_generation >= CORE_CHILD_DEBUGGER_ID_NAMESPACE_START,
        "core must mint a public namespace instead of forwarding child IDs"
    );
    let safe_points = match handle_debugger_request_with_page_javascript_executor(
        &tabs,
        Some(&mut executor),
        DebuggerRequest::ListSafePoints { program },
    ) {
        DebuggerReply::SafePoints(safe_points) => safe_points,
        reply => panic!("expected source-free public safe points, got {reply:?}"),
    };
    let safe_point = *safe_points.first().expect("program exposes one safe point");
    assert_eq!(
        handle_debugger_request_with_page_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::ValidateSafePoint { safe_point },
        ),
        DebuggerReply::SafePointValidated { safe_point }
    );
    assert_eq!(
        handle_debugger_request_with_page_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::SetBreakpoint { safe_point },
        ),
        DebuggerReply::BreakpointSet { safe_point }
    );
    // Configuration is idempotent and does not create another public or
    // child-private record on a debugger socket retry.
    assert_eq!(
        handle_debugger_request_with_page_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::SetBreakpoint { safe_point },
        ),
        DebuggerReply::BreakpointSet { safe_point }
    );
    assert_eq!(
        handle_debugger_request_with_page_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::ListBreakpoints { realm: first_realm },
        ),
        DebuggerReply::Breakpoints(vec![safe_point])
    );
    assert_eq!(
        handle_debugger_request_with_page_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::ClearBreakpoint { safe_point },
        ),
        DebuggerReply::BreakpointCleared {
            safe_point,
            was_present: true,
        }
    );
    assert_eq!(
        handle_debugger_request_with_page_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::ClearBreakpoint { safe_point },
        ),
        DebuggerReply::BreakpointCleared {
            safe_point,
            was_present: false,
        }
    );
    assert_eq!(
        handle_debugger_request_with_page_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::SetBreakpoint { safe_point },
        ),
        DebuggerReply::BreakpointSet { safe_point }
    );

    let cross_tab = blueice_ipc::debugger::DebuggerProgram {
        realm: second_realm,
        ..program
    };
    assert!(matches!(
        handle_debugger_request_with_page_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::ListSafePoints { program: cross_tab },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            ..
        }
    ));
    let cross_tab_safe_point = blueice_ipc::debugger::DebuggerSafePoint {
        program: cross_tab,
        ..safe_point
    };
    assert!(matches!(
        handle_debugger_request_with_page_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::SetBreakpoint {
                safe_point: cross_tab_safe_point,
            },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            ..
        }
    ));

    // Reconfiguration before navigation proves that the successor's
    // empty child table cannot retain prior private IDs or public tuples.
    tabs.get_mut(first_tab).unwrap().load_html_str(
        "<script>let successor = 4;</script>",
        Some("https://example.test/successor-again.html".to_string()),
    );
    executor.synchronize_and_execute(&tabs).unwrap();
    let successor_realm = DebuggerPageRealm {
        realm_generation: 2,
        ..first_realm
    };
    assert_eq!(
        handle_debugger_request_with_page_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::ListBreakpoints {
                realm: successor_realm,
            },
        ),
        DebuggerReply::Breakpoints(Vec::new())
    );

    tabs.get_mut(first_tab).unwrap().load_html_str(
        "<script>let successor = 3;</script>",
        Some("https://example.test/successor.html".to_string()),
    );
    assert!(matches!(
        handle_debugger_request_with_page_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::ListSafePoints { program },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::StaleRealm,
            ..
        }
    ));

    drop(executor);
    shutdown_child(&path, &token);
    child.join().unwrap();
    let _ = std::fs::remove_file(path);
}

#[test]
fn core_remints_real_child_bluets_metadata_handles_and_discards_them_on_navigation() {
    let (path, token, child) = spawn_child();
    let (mut tabs, tab_id) = loaded_tabs(
        concat!(
            "<script>globalThis.javaScriptOnly = true;</script>",
            "<script type=\"application/x-blueice-typescript\">",
            "const opaqueCompilerMetadata: number = 42;",
            "</script>"
        ),
        "https://example.test/opaque-metadata.html",
    );
    let mut executor = OutOfProcessJavaScriptPageExecutor::connect(&path, &token).unwrap();
    executor.synchronize_and_execute(&tabs).unwrap();

    let programs = executor.debugger_programs(tab_id, 1).unwrap();
    assert_eq!(programs.len(), 2);
    let mut metadata = None;
    for program in programs {
        let handles = executor
            .debugger_static_metadata(
                tab_id,
                1,
                program.program_handle,
                program.program_generation,
            )
            .unwrap();
        if let [handle] = handles.as_slice() {
            assert!(
                handle.metadata_handle >= CORE_CHILD_DEBUGGER_METADATA_ID_NAMESPACE_START
                    && handle.metadata_handle < CORE_CHILD_DEBUGGER_ID_NAMESPACE_START
                    && handle.metadata_generation
                        >= CORE_CHILD_DEBUGGER_METADATA_ID_NAMESPACE_START
                    && handle.metadata_generation < CORE_CHILD_DEBUGGER_ID_NAMESPACE_START,
                "core must remint metadata IDs outside both child and public program namespaces"
            );
            metadata = Some((program, *handle));
        } else {
            assert!(handles.is_empty(), "the JavaScript program is ineligible");
        }
    }
    let (typed_program, metadata) = metadata.expect("direct BlueTS has one private attachment");
    assert!(
        !format!("{metadata:?}").contains("opaqueCompilerMetadata"),
        "the core-facing handle must contain no compiler metadata payload"
    );
    let summary = executor
        .debugger_static_metadata_summary(
            tab_id,
            1,
            typed_program.program_handle,
            typed_program.program_generation,
            metadata.metadata_handle,
            metadata.metadata_generation,
        )
        .expect("the exact core-reminted metadata identity resolves a bounded summary");
    assert_eq!(summary.language_version, "blue-ts-0.1");
    assert!(summary.source_count > 0);
    assert!(summary.type_count > 0);
    assert!(summary.symbol_count > 0);
    assert!(
        !format!("{summary:?}").contains("opaqueCompilerMetadata"),
        "the core-facing summary must contain no compiler record payload"
    );
    let sources = executor
        .debugger_static_metadata_sources(
            tab_id,
            1,
            typed_program.program_handle,
            typed_program.program_generation,
            metadata.metadata_handle,
            metadata.metadata_generation,
        )
        .expect("the exact core-reminted metadata identity resolves source-record IDs");
    assert_eq!(
        sources.len(),
        usize::try_from(summary.source_count).unwrap()
    );
    assert_eq!(
        sources
            .iter()
            .map(|source| source.source_id)
            .collect::<BTreeSet<_>>()
            .len(),
        sources.len(),
        "the child must not repeat compiler source-record IDs"
    );
    assert!(
        !format!("{sources:?}").contains("opaqueCompilerMetadata"),
        "source-record identities must not carry compiler record payloads"
    );
    assert!(matches!(
        executor.debugger_static_metadata_sources(
            tab_id,
            1,
            typed_program.program_handle,
            typed_program.program_generation,
            metadata.metadata_handle,
            metadata.metadata_generation + 1,
        ),
        Err(JavaScriptPageDebuggerError::UnknownProgram)
    ));
    assert!(matches!(
        executor.debugger_static_metadata_summary(
            tab_id,
            1,
            typed_program.program_handle,
            typed_program.program_generation,
            metadata.metadata_handle,
            metadata.metadata_generation + 1,
        ),
        Err(JavaScriptPageDebuggerError::UnknownProgram)
    ));

    tabs.get_mut(tab_id).unwrap().load_html_str(
        "<script type=\"application/x-blueice-typescript\">const successor: number = 1;</script>",
        Some("https://example.test/opaque-metadata-successor.html".to_string()),
    );
    executor.synchronize_and_execute(&tabs).unwrap();
    assert!(matches!(
        executor.debugger_static_metadata(
            tab_id,
            1,
            typed_program.program_handle,
            typed_program.program_generation,
        ),
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    ));
    assert!(matches!(
        executor.debugger_static_metadata_summary(
            tab_id,
            1,
            typed_program.program_handle,
            typed_program.program_generation,
            metadata.metadata_handle,
            metadata.metadata_generation,
        ),
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    ));
    assert!(matches!(
        executor.debugger_static_metadata_sources(
            tab_id,
            1,
            typed_program.program_handle,
            typed_program.program_generation,
            metadata.metadata_handle,
            metadata.metadata_generation,
        ),
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    ));

    let successor = executor
        .debugger_programs(tab_id, 2)
        .unwrap()
        .into_iter()
        .find_map(|program| {
            executor
                .debugger_static_metadata(
                    tab_id,
                    2,
                    program.program_handle,
                    program.program_generation,
                )
                .ok()
                .and_then(|handles| handles.into_iter().next().map(|handle| (program, handle)))
        })
        .expect("successor must retain its own compiler metadata before close");
    assert!(tabs.close_tab(tab_id));
    executor.synchronize_and_execute(&tabs).unwrap();
    assert!(!executor.live_documents.contains_key(&tab_id));
    assert!(!executor.debugger_static_metadata.contains_key(&tab_id));
    assert!(matches!(
        executor.debugger_static_metadata_summary(
            tab_id,
            2,
            successor.0.program_handle,
            successor.0.program_generation,
            successor.1.metadata_handle,
            successor.1.metadata_generation,
        ),
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    ));

    drop(executor);
    shutdown_child(&path, &token);
    child.join().unwrap();
    let _ = std::fs::remove_file(path);
}

/// Which `core_debugger_static_metadata*` method a [`MetadataProbeChild`]
/// deliberately misbehaves on. Every other private reply stays well-formed,
/// so a probe isolates exactly one validation branch in
/// `debugger/metadata_catalog.rs` without needing a real spawned child.
#[derive(Clone, Copy, PartialEq, Eq)]
enum MetadataProbeTarget {
    Inventory,
    Summary,
    LoweringSummary,
    Sources,
    SourceProvenance,
    Types,
    TypeDisplay,
    Symbols,
    Contracts,
    ContractDisplay,
    ContractValidation,
    SymbolDisplay,
    SymbolLocation,
    ContractLocation,
    SymbolType,
    SymbolContract,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum MetadataProbeFault {
    WrongVariant,
    MismatchedTuple,
    NotWellFormed,
    Duplicate,
    ExceedsMax,
    EmptyDisplay,
    DisplayTooLong,
    /// The reply is structurally well-formed (valid coordinates/echoed
    /// tab/generation/program/metadata) but names a different relation than
    /// the one requested (e.g. a contract/source ID that doesn't match).
    MismatchedRelation,
    /// No fault: the probed method should return a fully well-formed reply.
    None,
}

/// A hostile-or-buggy private child that behaves correctly for every step of
/// program/metadata minting except the one `target` method, where it applies
/// `fault`. Core must reject the faulty reply rather than trust it.
struct MetadataProbeChild {
    target: MetadataProbeTarget,
    fault: MetadataProbeFault,
}

impl MetadataProbeChild {
    fn new(target: MetadataProbeTarget, fault: MetadataProbeFault) -> Self {
        Self { target, fault }
    }

    fn wrong_variant_reply() -> PageHostReply {
        PageHostReply::Error {
            code: PageHostErrorCode::InvalidRequest,
            message: "probe: unexpected request".to_string(),
        }
    }

    fn not_under_test() -> io::Result<PageHostReply> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "probe: this method is not the one under test",
        ))
    }
}

impl PageHostClient for MetadataProbeChild {
    fn synchronize_document(&mut self, document: PageHostDocument) -> io::Result<PageHostReply> {
        Ok(PageHostReply::Synchronized {
            tab_id: document.tab_id,
            document_generation: document.document_generation,
            already_current: false,
            reports: Vec::new(),
        })
    }

    fn close_realm(&mut self, tab_id: u64, document_generation: u64) -> io::Result<PageHostReply> {
        Ok(PageHostReply::RealmClosed {
            tab_id,
            document_generation,
        })
    }

    fn debugger_programs(
        &mut self,
        tab_id: u64,
        document_generation: u64,
    ) -> io::Result<PageHostReply> {
        Ok(PageHostReply::DebuggerPrograms {
            tab_id,
            document_generation,
            programs: vec![PageHostDebuggerProgram {
                program_handle: 1,
                program_generation: 1,
            }],
        })
    }

    fn debugger_bluets_metadata_available(&self) -> bool {
        true
    }

    fn debugger_bluets_metadata_summary_available(&self) -> bool {
        true
    }

    fn debugger_bluets_metadata_sources_available(&self) -> bool {
        true
    }

    fn debugger_bluets_metadata_types_available(&self) -> bool {
        true
    }

    fn debugger_bluets_metadata_type_display_available(&self) -> bool {
        true
    }

    fn debugger_bluets_metadata_lowering_summary_available(&self) -> bool {
        true
    }

    fn debugger_bluets_metadata_source_provenance_available(&self) -> bool {
        true
    }

    fn debugger_bluets_metadata_symbols_available(&self) -> bool {
        true
    }

    fn debugger_bluets_metadata_contracts_available(&self) -> bool {
        true
    }

    fn debugger_bluets_metadata_contract_display_available(&self) -> bool {
        true
    }

    fn debugger_bluets_metadata_contract_validation_available(&self) -> bool {
        true
    }

    fn debugger_bluets_metadata_symbol_display_available(&self) -> bool {
        true
    }

    fn debugger_bluets_metadata_symbol_location_available(&self) -> bool {
        true
    }

    fn debugger_bluets_metadata_contract_location_available(&self) -> bool {
        true
    }

    fn debugger_bluets_metadata_symbol_type_available(&self) -> bool {
        true
    }

    fn debugger_bluets_metadata_symbol_contract_available(&self) -> bool {
        true
    }

    fn debugger_bluets_metadata(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
    ) -> io::Result<PageHostReply> {
        let well_formed = || PageHostReply::DebuggerBlueTsMetadata {
            tab_id,
            document_generation,
            program,
            metadata: vec![PageHostDebuggerMetadataHandle {
                metadata_handle: 1,
                metadata_generation: 1,
            }],
        };
        if self.target != MetadataProbeTarget::Inventory {
            return Ok(well_formed());
        }
        Ok(match self.fault {
            MetadataProbeFault::WrongVariant => Self::wrong_variant_reply(),
            MetadataProbeFault::MismatchedTuple => PageHostReply::DebuggerBlueTsMetadata {
                tab_id: tab_id.wrapping_add(1),
                document_generation,
                program,
                metadata: vec![PageHostDebuggerMetadataHandle {
                    metadata_handle: 1,
                    metadata_generation: 1,
                }],
            },
            MetadataProbeFault::NotWellFormed => PageHostReply::DebuggerBlueTsMetadata {
                tab_id,
                document_generation,
                program,
                metadata: vec![PageHostDebuggerMetadataHandle {
                    metadata_handle: 0,
                    metadata_generation: 0,
                }],
            },
            MetadataProbeFault::Duplicate => PageHostReply::DebuggerBlueTsMetadata {
                tab_id,
                document_generation,
                program,
                metadata: vec![
                    PageHostDebuggerMetadataHandle {
                        metadata_handle: 1,
                        metadata_generation: 1,
                    },
                    PageHostDebuggerMetadataHandle {
                        metadata_handle: 1,
                        metadata_generation: 1,
                    },
                ],
            },
            MetadataProbeFault::ExceedsMax => PageHostReply::DebuggerBlueTsMetadata {
                tab_id,
                document_generation,
                program,
                metadata: vec![
                    PageHostDebuggerMetadataHandle {
                        metadata_handle: 1,
                        metadata_generation: 1,
                    },
                    PageHostDebuggerMetadataHandle {
                        metadata_handle: 2,
                        metadata_generation: 2,
                    },
                ],
            },
            MetadataProbeFault::EmptyDisplay
            | MetadataProbeFault::DisplayTooLong
            | MetadataProbeFault::MismatchedRelation
            | MetadataProbeFault::None => {
                unreachable!("fault not applicable to the inventory probe")
            }
        })
    }

    fn debugger_bluets_metadata_summary(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
    ) -> io::Result<PageHostReply> {
        if self.target != MetadataProbeTarget::Summary {
            return Self::not_under_test();
        }
        let summary = page_host::PageHostDebuggerBlueTsMetadataSummary {
            language_version: "blue-ts-0.1".to_string(),
            compiler_options_hash: "probe-hash".to_string(),
            source_count: 0,
            type_count: 0,
            symbol_count: 0,
            contract_count: 0,
        };
        Ok(match self.fault {
            MetadataProbeFault::WrongVariant => Self::wrong_variant_reply(),
            MetadataProbeFault::MismatchedTuple => PageHostReply::DebuggerBlueTsMetadataSummary {
                tab_id,
                document_generation,
                program,
                metadata: PageHostDebuggerMetadataHandle {
                    metadata_handle: metadata.metadata_handle.wrapping_add(1),
                    ..metadata
                },
                summary,
            },
            _ => unreachable!("fault not applicable to the summary probe"),
        })
    }

    fn debugger_bluets_metadata_sources(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
    ) -> io::Result<PageHostReply> {
        if self.target != MetadataProbeTarget::Sources {
            return Self::not_under_test();
        }
        Ok(match self.fault {
            MetadataProbeFault::WrongVariant => Self::wrong_variant_reply(),
            MetadataProbeFault::Duplicate => PageHostReply::DebuggerBlueTsMetadataSources {
                tab_id,
                document_generation,
                program,
                metadata,
                sources: vec![
                    page_host::PageHostDebuggerBlueTsMetadataSourceId { source_id: 1 },
                    page_host::PageHostDebuggerBlueTsMetadataSourceId { source_id: 1 },
                ],
            },
            MetadataProbeFault::ExceedsMax => PageHostReply::DebuggerBlueTsMetadataSources {
                tab_id,
                document_generation,
                program,
                metadata,
                sources: (0..=DEBUGGER_STATIC_METADATA_MAX_SOURCES)
                    .map(|source_id| page_host::PageHostDebuggerBlueTsMetadataSourceId {
                        source_id,
                    })
                    .collect(),
            },
            _ => unreachable!("fault not applicable to the sources probe"),
        })
    }

    fn debugger_bluets_metadata_type_display(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        type_id: u32,
    ) -> io::Result<PageHostReply> {
        if self.target != MetadataProbeTarget::TypeDisplay {
            return Self::not_under_test();
        }
        Ok(match self.fault {
            MetadataProbeFault::WrongVariant => Self::wrong_variant_reply(),
            MetadataProbeFault::MismatchedTuple => PageHostReply::DebuggerBlueTsMetadataType {
                tab_id,
                document_generation,
                program,
                metadata,
                static_type: page_host::PageHostDebuggerBlueTsMetadataTypeDisplay {
                    type_id: type_id.wrapping_add(1),
                    display: "T".to_string(),
                },
            },
            MetadataProbeFault::EmptyDisplay => PageHostReply::DebuggerBlueTsMetadataType {
                tab_id,
                document_generation,
                program,
                metadata,
                static_type: page_host::PageHostDebuggerBlueTsMetadataTypeDisplay {
                    type_id,
                    display: String::new(),
                },
            },
            MetadataProbeFault::DisplayTooLong => PageHostReply::DebuggerBlueTsMetadataType {
                tab_id,
                document_generation,
                program,
                metadata,
                static_type: page_host::PageHostDebuggerBlueTsMetadataTypeDisplay {
                    type_id,
                    display: "x".repeat(DEBUGGER_STATIC_METADATA_TYPE_DISPLAY_MAX_BYTES + 1),
                },
            },
            _ => unreachable!("fault not applicable to the type-display probe"),
        })
    }

    fn debugger_bluets_metadata_lowering_summary(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
    ) -> io::Result<PageHostReply> {
        if self.target != MetadataProbeTarget::LoweringSummary {
            return Self::not_under_test();
        }
        let well_formed_summary = || page_host::PageHostDebuggerBlueTsMetadataLoweringSummary {
            safe_point_map_abi: "abi".to_string(),
            program_abi: "abi".to_string(),
            source_set_hash: "hash".to_string(),
            bound_safe_point_count: 0,
        };
        Ok(match self.fault {
            MetadataProbeFault::WrongVariant => Self::wrong_variant_reply(),
            MetadataProbeFault::MismatchedTuple => {
                PageHostReply::DebuggerBlueTsMetadataLoweringSummary {
                    tab_id,
                    document_generation,
                    program,
                    metadata: PageHostDebuggerMetadataHandle {
                        metadata_handle: metadata.metadata_handle.wrapping_add(1),
                        ..metadata
                    },
                    summary: Box::new(well_formed_summary()),
                }
            }
            MetadataProbeFault::EmptyDisplay => PageHostReply::DebuggerBlueTsMetadataLoweringSummary {
                tab_id,
                document_generation,
                program,
                metadata,
                summary: Box::new(page_host::PageHostDebuggerBlueTsMetadataLoweringSummary {
                    safe_point_map_abi: String::new(),
                    ..well_formed_summary()
                }),
            },
            MetadataProbeFault::ExceedsMax => PageHostReply::DebuggerBlueTsMetadataLoweringSummary {
                tab_id,
                document_generation,
                program,
                metadata,
                summary: Box::new(page_host::PageHostDebuggerBlueTsMetadataLoweringSummary {
                    bound_safe_point_count: blueice_ipc::debugger::DEBUGGER_STATIC_METADATA_MAX_BOUND_SAFE_POINTS + 1,
                    ..well_formed_summary()
                }),
            },
            _ => unreachable!("fault not applicable to the lowering-summary probe"),
        })
    }

    fn debugger_bluets_metadata_source_provenance(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        source_id: u32,
    ) -> io::Result<PageHostReply> {
        if self.target != MetadataProbeTarget::SourceProvenance {
            return Self::not_under_test();
        }
        Ok(match self.fault {
            MetadataProbeFault::WrongVariant => Self::wrong_variant_reply(),
            MetadataProbeFault::MismatchedTuple => {
                PageHostReply::DebuggerBlueTsMetadataSourceProvenance {
                    tab_id,
                    document_generation,
                    program,
                    metadata,
                    provenance: page_host::PageHostDebuggerBlueTsMetadataSourceProvenance {
                        source_id: source_id.wrapping_add(1),
                        module: "probe".to_string(),
                        content_hash: "hash".to_string(),
                    },
                }
            }
            _ => unreachable!("fault not applicable to the source-provenance probe"),
        })
    }

    fn debugger_bluets_metadata_types(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
    ) -> io::Result<PageHostReply> {
        if self.target != MetadataProbeTarget::Types {
            return Self::not_under_test();
        }
        Ok(match self.fault {
            MetadataProbeFault::WrongVariant => Self::wrong_variant_reply(),
            MetadataProbeFault::Duplicate => PageHostReply::DebuggerBlueTsMetadataTypes {
                tab_id,
                document_generation,
                program,
                metadata,
                types: vec![
                    page_host::PageHostDebuggerBlueTsMetadataTypeId { type_id: 1 },
                    page_host::PageHostDebuggerBlueTsMetadataTypeId { type_id: 1 },
                ],
            },
            MetadataProbeFault::ExceedsMax => PageHostReply::DebuggerBlueTsMetadataTypes {
                tab_id,
                document_generation,
                program,
                metadata,
                types: (0..=DEBUGGER_STATIC_METADATA_MAX_TYPES)
                    .map(|type_id| page_host::PageHostDebuggerBlueTsMetadataTypeId { type_id })
                    .collect(),
            },
            _ => unreachable!("fault not applicable to the types probe"),
        })
    }

    fn debugger_bluets_metadata_symbols(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
    ) -> io::Result<PageHostReply> {
        if self.target != MetadataProbeTarget::Symbols {
            return Self::not_under_test();
        }
        Ok(match self.fault {
            MetadataProbeFault::WrongVariant => Self::wrong_variant_reply(),
            MetadataProbeFault::Duplicate => PageHostReply::DebuggerBlueTsMetadataSymbols {
                tab_id,
                document_generation,
                program,
                metadata,
                symbols: vec![
                    page_host::PageHostDebuggerBlueTsMetadataSymbolId { symbol_id: 1 },
                    page_host::PageHostDebuggerBlueTsMetadataSymbolId { symbol_id: 1 },
                ],
            },
            MetadataProbeFault::ExceedsMax => PageHostReply::DebuggerBlueTsMetadataSymbols {
                tab_id,
                document_generation,
                program,
                metadata,
                symbols: (0..=DEBUGGER_STATIC_METADATA_MAX_SYMBOLS)
                    .map(|symbol_id| page_host::PageHostDebuggerBlueTsMetadataSymbolId {
                        symbol_id,
                    })
                    .collect(),
            },
            _ => unreachable!("fault not applicable to the symbols probe"),
        })
    }

    fn debugger_bluets_metadata_contracts(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
    ) -> io::Result<PageHostReply> {
        if self.target != MetadataProbeTarget::Contracts {
            return Self::not_under_test();
        }
        Ok(match self.fault {
            MetadataProbeFault::WrongVariant => Self::wrong_variant_reply(),
            MetadataProbeFault::Duplicate => PageHostReply::DebuggerBlueTsMetadataContracts {
                tab_id,
                document_generation,
                program,
                metadata,
                contracts: vec![
                    page_host::PageHostDebuggerBlueTsMetadataContractId { contract_id: 1 },
                    page_host::PageHostDebuggerBlueTsMetadataContractId { contract_id: 1 },
                ],
            },
            MetadataProbeFault::ExceedsMax => PageHostReply::DebuggerBlueTsMetadataContracts {
                tab_id,
                document_generation,
                program,
                metadata,
                contracts: (0..=DEBUGGER_STATIC_METADATA_MAX_CONTRACTS)
                    .map(|contract_id| page_host::PageHostDebuggerBlueTsMetadataContractId {
                        contract_id,
                    })
                    .collect(),
            },
            _ => unreachable!("fault not applicable to the contracts probe"),
        })
    }

    fn debugger_bluets_metadata_contract_display(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        contract_id: u32,
    ) -> io::Result<PageHostReply> {
        if self.target != MetadataProbeTarget::ContractDisplay {
            return Self::not_under_test();
        }
        let root_kind = blueice_ipc::debugger::DebuggerStaticMetadataContractRootKind::Null;
        Ok(match self.fault {
            MetadataProbeFault::WrongVariant => Self::wrong_variant_reply(),
            MetadataProbeFault::MismatchedTuple => PageHostReply::DebuggerBlueTsMetadataContract {
                tab_id,
                document_generation,
                program,
                metadata,
                contract: page_host::PageHostDebuggerBlueTsMetadataContractDisplay {
                    contract_id: contract_id.wrapping_add(1),
                    display: "C".to_string(),
                    root_kind,
                },
            },
            MetadataProbeFault::EmptyDisplay => PageHostReply::DebuggerBlueTsMetadataContract {
                tab_id,
                document_generation,
                program,
                metadata,
                contract: page_host::PageHostDebuggerBlueTsMetadataContractDisplay {
                    contract_id,
                    display: String::new(),
                    root_kind,
                },
            },
            MetadataProbeFault::DisplayTooLong => PageHostReply::DebuggerBlueTsMetadataContract {
                tab_id,
                document_generation,
                program,
                metadata,
                contract: page_host::PageHostDebuggerBlueTsMetadataContractDisplay {
                    contract_id,
                    display: "x".repeat(DEBUGGER_STATIC_METADATA_CONTRACT_DISPLAY_MAX_BYTES + 1),
                    root_kind,
                },
            },
            _ => unreachable!("fault not applicable to the contract-display probe"),
        })
    }

    fn debugger_bluets_metadata_contract_validation(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        contract_id: u32,
        _value: CompilerContractValue,
    ) -> io::Result<PageHostReply> {
        if self.target != MetadataProbeTarget::ContractValidation {
            return Self::not_under_test();
        }
        Ok(match self.fault {
            MetadataProbeFault::WrongVariant => Self::wrong_variant_reply(),
            MetadataProbeFault::MismatchedTuple => {
                PageHostReply::DebuggerBlueTsMetadataContractValidation {
                    tab_id,
                    document_generation,
                    program,
                    metadata,
                    validation: page_host::PageHostDebuggerBlueTsMetadataContractValidation {
                        contract_id: contract_id.wrapping_add(1),
                        valid: true,
                    },
                }
            }
            _ => unreachable!("fault not applicable to the contract-validation probe"),
        })
    }

    fn debugger_bluets_metadata_symbol_display(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        symbol_id: u32,
    ) -> io::Result<PageHostReply> {
        if self.target != MetadataProbeTarget::SymbolDisplay {
            return Self::not_under_test();
        }
        let kind = blueice_ipc::debugger::DebuggerStaticMetadataSymbolKind::Variable;
        Ok(match self.fault {
            MetadataProbeFault::WrongVariant => Self::wrong_variant_reply(),
            MetadataProbeFault::MismatchedTuple => PageHostReply::DebuggerBlueTsMetadataSymbol {
                tab_id,
                document_generation,
                program,
                metadata,
                symbol: page_host::PageHostDebuggerBlueTsMetadataSymbolDisplay {
                    symbol_id: symbol_id.wrapping_add(1),
                    display: "S".to_string(),
                    kind,
                    exported: false,
                },
            },
            MetadataProbeFault::EmptyDisplay => PageHostReply::DebuggerBlueTsMetadataSymbol {
                tab_id,
                document_generation,
                program,
                metadata,
                symbol: page_host::PageHostDebuggerBlueTsMetadataSymbolDisplay {
                    symbol_id,
                    display: String::new(),
                    kind,
                    exported: false,
                },
            },
            MetadataProbeFault::DisplayTooLong => PageHostReply::DebuggerBlueTsMetadataSymbol {
                tab_id,
                document_generation,
                program,
                metadata,
                symbol: page_host::PageHostDebuggerBlueTsMetadataSymbolDisplay {
                    symbol_id,
                    display: "x".repeat(DEBUGGER_STATIC_METADATA_SYMBOL_DISPLAY_MAX_BYTES + 1),
                    kind,
                    exported: false,
                },
            },
            _ => unreachable!("fault not applicable to the symbol-display probe"),
        })
    }

    fn debugger_bluets_metadata_symbol_location(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        symbol_id: u32,
    ) -> io::Result<PageHostReply> {
        if self.target != MetadataProbeTarget::SymbolLocation {
            return Self::not_under_test();
        }
        let well_formed_location = || page_host::PageHostDebuggerBlueTsMetadataSymbolLocation {
            symbol_id,
            source_id: 3,
            start_byte: 2,
            end_byte: 8,
            coordinates: blueice_ipc::debugger::DebuggerSourceCoordinates {
                start_line: 0,
                start_column_utf16: 2,
                end_line: 0,
                end_column_utf16: 8,
            },
        };
        Ok(match self.fault {
            MetadataProbeFault::WrongVariant => Self::wrong_variant_reply(),
            MetadataProbeFault::MismatchedTuple => PageHostReply::DebuggerBlueTsMetadataSymbolLocation {
                tab_id: tab_id.wrapping_add(1),
                document_generation,
                program,
                metadata,
                location: well_formed_location(),
            },
            MetadataProbeFault::NotWellFormed => PageHostReply::DebuggerBlueTsMetadataSymbolLocation {
                tab_id,
                document_generation,
                program,
                metadata,
                location: page_host::PageHostDebuggerBlueTsMetadataSymbolLocation {
                    symbol_id: symbol_id.wrapping_add(1),
                    ..well_formed_location()
                },
            },
            _ => PageHostReply::DebuggerBlueTsMetadataSymbolLocation {
                tab_id,
                document_generation,
                program,
                metadata,
                location: well_formed_location(),
            },
        })
    }

    fn debugger_bluets_metadata_contract_location(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        contract_id: u32,
    ) -> io::Result<PageHostReply> {
        if self.target != MetadataProbeTarget::ContractLocation {
            return Self::not_under_test();
        }
        let well_formed_location = || page_host::PageHostDebuggerBlueTsMetadataContractLocation {
            contract_id,
            source_id: 3,
            start_byte: 2,
            end_byte: 8,
            coordinates: blueice_ipc::debugger::DebuggerSourceCoordinates {
                start_line: 0,
                start_column_utf16: 2,
                end_line: 0,
                end_column_utf16: 8,
            },
        };
        Ok(match self.fault {
            MetadataProbeFault::WrongVariant => Self::wrong_variant_reply(),
            MetadataProbeFault::MismatchedTuple => PageHostReply::DebuggerBlueTsMetadataContractLocation {
                tab_id: tab_id.wrapping_add(1),
                document_generation,
                program,
                metadata,
                location: well_formed_location(),
            },
            MetadataProbeFault::NotWellFormed => PageHostReply::DebuggerBlueTsMetadataContractLocation {
                tab_id,
                document_generation,
                program,
                metadata,
                location: page_host::PageHostDebuggerBlueTsMetadataContractLocation {
                    start_byte: 8,
                    end_byte: 2,
                    ..well_formed_location()
                },
            },
            MetadataProbeFault::MismatchedRelation => {
                PageHostReply::DebuggerBlueTsMetadataContractLocation {
                    tab_id,
                    document_generation,
                    program,
                    metadata,
                    location: page_host::PageHostDebuggerBlueTsMetadataContractLocation {
                        contract_id: contract_id.wrapping_add(1),
                        ..well_formed_location()
                    },
                }
            }
            _ => PageHostReply::DebuggerBlueTsMetadataContractLocation {
                tab_id,
                document_generation,
                program,
                metadata,
                location: well_formed_location(),
            },
        })
    }

    fn debugger_bluets_metadata_symbol_type(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        symbol_id: u32,
        type_id: u32,
    ) -> io::Result<PageHostReply> {
        if self.target != MetadataProbeTarget::SymbolType {
            return Self::not_under_test();
        }
        let well_formed = || page_host::PageHostDebuggerBlueTsMetadataSymbolType { symbol_id, type_id };
        Ok(match self.fault {
            MetadataProbeFault::WrongVariant => Self::wrong_variant_reply(),
            MetadataProbeFault::MismatchedTuple => PageHostReply::DebuggerBlueTsMetadataSymbolType {
                tab_id: tab_id.wrapping_add(1),
                document_generation,
                program,
                metadata,
                symbol_type: well_formed(),
            },
            MetadataProbeFault::NotWellFormed => PageHostReply::DebuggerBlueTsMetadataSymbolType {
                tab_id,
                document_generation,
                program,
                metadata,
                symbol_type: page_host::PageHostDebuggerBlueTsMetadataSymbolType {
                    symbol_id: symbol_id.wrapping_add(1),
                    type_id,
                },
            },
            _ => PageHostReply::DebuggerBlueTsMetadataSymbolType {
                tab_id,
                document_generation,
                program,
                metadata,
                symbol_type: well_formed(),
            },
        })
    }

    fn debugger_bluets_metadata_symbol_contract(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        symbol_id: u32,
        contract_id: u32,
    ) -> io::Result<PageHostReply> {
        if self.target != MetadataProbeTarget::SymbolContract {
            return Self::not_under_test();
        }
        let well_formed =
            || page_host::PageHostDebuggerBlueTsMetadataSymbolContract { symbol_id, contract_id };
        Ok(match self.fault {
            MetadataProbeFault::WrongVariant => Self::wrong_variant_reply(),
            MetadataProbeFault::MismatchedTuple => PageHostReply::DebuggerBlueTsMetadataSymbolContract {
                tab_id: tab_id.wrapping_add(1),
                document_generation,
                program,
                metadata,
                symbol_contract: well_formed(),
            },
            MetadataProbeFault::NotWellFormed => PageHostReply::DebuggerBlueTsMetadataSymbolContract {
                tab_id,
                document_generation,
                program,
                metadata,
                symbol_contract: page_host::PageHostDebuggerBlueTsMetadataSymbolContract {
                    symbol_id: symbol_id.wrapping_add(1),
                    contract_id,
                },
            },
            _ => PageHostReply::DebuggerBlueTsMetadataSymbolContract {
                tab_id,
                document_generation,
                program,
                metadata,
                symbol_contract: well_formed(),
            },
        })
    }
}

/// The minted public program/metadata identity pair a probe test drives its
/// method-under-test with.
#[derive(Clone, Copy)]
struct ProbeTarget {
    program_handle: u64,
    program_generation: u64,
    metadata_handle: u64,
    metadata_generation: u64,
}

/// Builds a probe executor with one live document, a minted public program,
/// and (unless the probe itself is what mints metadata) a minted public
/// metadata handle, ready to drive the one method under test.
fn probe_executor(
    target: MetadataProbeTarget,
    fault: MetadataProbeFault,
) -> (
    OutOfProcessJavaScriptPageExecutor<MetadataProbeChild>,
    TabId,
    ProbeTarget,
) {
    let (tabs, tab_id) = loaded_tabs(
        "<script>let probed = true;</script>",
        "https://example.test/metadata-probe.html",
    );
    let mut executor =
        OutOfProcessJavaScriptPageExecutor::new(MetadataProbeChild::new(target, fault));
    executor.synchronize_and_execute(&tabs).unwrap();
    let program = executor.debugger_programs(tab_id, 1).unwrap()[0];
    let metadata_target = if target == MetadataProbeTarget::Inventory {
        ProbeTarget {
            program_handle: program.program_handle,
            program_generation: program.program_generation,
            metadata_handle: 0,
            metadata_generation: 0,
        }
    } else {
        let metadata = executor
            .debugger_static_metadata(
                tab_id,
                1,
                program.program_handle,
                program.program_generation,
            )
            .unwrap()[0];
        ProbeTarget {
            program_handle: program.program_handle,
            program_generation: program.program_generation,
            metadata_handle: metadata.metadata_handle,
            metadata_generation: metadata.metadata_generation,
        }
    };
    (executor, tab_id, metadata_target)
}

#[test]
fn core_rejects_an_inventory_reply_of_the_wrong_variant() {
    let (mut executor, tab_id, target) =
        probe_executor(MetadataProbeTarget::Inventory, MetadataProbeFault::WrongVariant);
    assert_eq!(
        executor.debugger_static_metadata(
            tab_id,
            1,
            target.program_handle,
            target.program_generation,
        ),
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    );
}

#[test]
fn core_rejects_an_inventory_reply_echoing_the_wrong_tab() {
    let (mut executor, tab_id, target) = probe_executor(
        MetadataProbeTarget::Inventory,
        MetadataProbeFault::MismatchedTuple,
    );
    assert_eq!(
        executor.debugger_static_metadata(
            tab_id,
            1,
            target.program_handle,
            target.program_generation,
        ),
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    );
}

#[test]
fn core_rejects_an_inventory_reply_with_a_zero_placeholder_handle() {
    let (mut executor, tab_id, target) = probe_executor(
        MetadataProbeTarget::Inventory,
        MetadataProbeFault::NotWellFormed,
    );
    assert_eq!(
        executor.debugger_static_metadata(
            tab_id,
            1,
            target.program_handle,
            target.program_generation,
        ),
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    );
}

#[test]
fn core_rejects_an_inventory_reply_with_a_duplicate_handle() {
    let (mut executor, tab_id, target) =
        probe_executor(MetadataProbeTarget::Inventory, MetadataProbeFault::Duplicate);
    assert_eq!(
        executor.debugger_static_metadata(
            tab_id,
            1,
            target.program_handle,
            target.program_generation,
        ),
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    );
}

#[test]
fn core_rejects_an_inventory_reply_exceeding_the_per_program_cap() {
    let (mut executor, tab_id, target) = probe_executor(
        MetadataProbeTarget::Inventory,
        MetadataProbeFault::ExceedsMax,
    );
    assert_eq!(
        executor.debugger_static_metadata(
            tab_id,
            1,
            target.program_handle,
            target.program_generation,
        ),
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    );
}

#[test]
fn core_rejects_a_summary_reply_of_the_wrong_variant() {
    let (mut executor, tab_id, target) =
        probe_executor(MetadataProbeTarget::Summary, MetadataProbeFault::WrongVariant);
    assert_eq!(
        executor.debugger_static_metadata_summary(
            tab_id,
            1,
            target.program_handle,
            target.program_generation,
            target.metadata_handle,
            target.metadata_generation,
        ),
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    );
}

#[test]
fn core_rejects_a_summary_reply_echoing_the_wrong_metadata_handle() {
    let (mut executor, tab_id, target) = probe_executor(
        MetadataProbeTarget::Summary,
        MetadataProbeFault::MismatchedTuple,
    );
    assert_eq!(
        executor.debugger_static_metadata_summary(
            tab_id,
            1,
            target.program_handle,
            target.program_generation,
            target.metadata_handle,
            target.metadata_generation,
        ),
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    );
}

#[test]
fn core_rejects_a_sources_reply_of_the_wrong_variant() {
    let (mut executor, tab_id, target) =
        probe_executor(MetadataProbeTarget::Sources, MetadataProbeFault::WrongVariant);
    assert_eq!(
        executor.debugger_static_metadata_sources(
            tab_id,
            1,
            target.program_handle,
            target.program_generation,
            target.metadata_handle,
            target.metadata_generation,
        ),
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    );
}

#[test]
fn core_rejects_a_sources_reply_with_a_duplicate_source_id() {
    let (mut executor, tab_id, target) =
        probe_executor(MetadataProbeTarget::Sources, MetadataProbeFault::Duplicate);
    assert_eq!(
        executor.debugger_static_metadata_sources(
            tab_id,
            1,
            target.program_handle,
            target.program_generation,
            target.metadata_handle,
            target.metadata_generation,
        ),
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    );
}

#[test]
fn core_rejects_a_sources_reply_exceeding_the_max_source_count() {
    let (mut executor, tab_id, target) =
        probe_executor(MetadataProbeTarget::Sources, MetadataProbeFault::ExceedsMax);
    assert_eq!(
        executor.debugger_static_metadata_sources(
            tab_id,
            1,
            target.program_handle,
            target.program_generation,
            target.metadata_handle,
            target.metadata_generation,
        ),
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    );
}

#[test]
fn core_rejects_a_type_display_reply_of_the_wrong_variant() {
    let (mut executor, tab_id, target) = probe_executor(
        MetadataProbeTarget::TypeDisplay,
        MetadataProbeFault::WrongVariant,
    );
    let type_target = JavaScriptPageDebuggerStaticMetadataTypeTarget {
        program_handle: target.program_handle,
        program_generation: target.program_generation,
        metadata_handle: target.metadata_handle,
        metadata_generation: target.metadata_generation,
        type_id: 1,
    };
    assert_eq!(
        executor.debugger_static_metadata_type_display(tab_id, 1, type_target),
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    );
}

#[test]
fn core_rejects_a_type_display_reply_echoing_the_wrong_type_id() {
    let (mut executor, tab_id, target) = probe_executor(
        MetadataProbeTarget::TypeDisplay,
        MetadataProbeFault::MismatchedTuple,
    );
    let type_target = JavaScriptPageDebuggerStaticMetadataTypeTarget {
        program_handle: target.program_handle,
        program_generation: target.program_generation,
        metadata_handle: target.metadata_handle,
        metadata_generation: target.metadata_generation,
        type_id: 1,
    };
    assert_eq!(
        executor.debugger_static_metadata_type_display(tab_id, 1, type_target),
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    );
}

#[test]
fn core_rejects_a_type_display_reply_with_an_empty_display() {
    let (mut executor, tab_id, target) = probe_executor(
        MetadataProbeTarget::TypeDisplay,
        MetadataProbeFault::EmptyDisplay,
    );
    let type_target = JavaScriptPageDebuggerStaticMetadataTypeTarget {
        program_handle: target.program_handle,
        program_generation: target.program_generation,
        metadata_handle: target.metadata_handle,
        metadata_generation: target.metadata_generation,
        type_id: 1,
    };
    assert_eq!(
        executor.debugger_static_metadata_type_display(tab_id, 1, type_target),
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    );
}

#[test]
fn core_rejects_a_type_display_reply_exceeding_the_max_display_length() {
    let (mut executor, tab_id, target) = probe_executor(
        MetadataProbeTarget::TypeDisplay,
        MetadataProbeFault::DisplayTooLong,
    );
    let type_target = JavaScriptPageDebuggerStaticMetadataTypeTarget {
        program_handle: target.program_handle,
        program_generation: target.program_generation,
        metadata_handle: target.metadata_handle,
        metadata_generation: target.metadata_generation,
        type_id: 1,
    };
    assert_eq!(
        executor.debugger_static_metadata_type_display(tab_id, 1, type_target),
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    );
}

#[test]
fn core_rejects_a_lowering_summary_reply_of_the_wrong_variant() {
    let (mut executor, tab_id, target) = probe_executor(
        MetadataProbeTarget::LoweringSummary,
        MetadataProbeFault::WrongVariant,
    );
    assert_eq!(
        executor.debugger_static_metadata_lowering_summary(
            tab_id,
            1,
            target.program_handle,
            target.program_generation,
            target.metadata_handle,
            target.metadata_generation,
        ),
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    );
}

#[test]
fn core_rejects_a_lowering_summary_reply_echoing_the_wrong_metadata_handle() {
    let (mut executor, tab_id, target) = probe_executor(
        MetadataProbeTarget::LoweringSummary,
        MetadataProbeFault::MismatchedTuple,
    );
    assert_eq!(
        executor.debugger_static_metadata_lowering_summary(
            tab_id,
            1,
            target.program_handle,
            target.program_generation,
            target.metadata_handle,
            target.metadata_generation,
        ),
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    );
}

#[test]
fn core_rejects_a_lowering_summary_reply_with_an_empty_abi_field() {
    let (mut executor, tab_id, target) = probe_executor(
        MetadataProbeTarget::LoweringSummary,
        MetadataProbeFault::EmptyDisplay,
    );
    assert_eq!(
        executor.debugger_static_metadata_lowering_summary(
            tab_id,
            1,
            target.program_handle,
            target.program_generation,
            target.metadata_handle,
            target.metadata_generation,
        ),
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    );
}

#[test]
fn core_rejects_a_lowering_summary_reply_exceeding_the_max_bound_safe_points() {
    let (mut executor, tab_id, target) = probe_executor(
        MetadataProbeTarget::LoweringSummary,
        MetadataProbeFault::ExceedsMax,
    );
    assert_eq!(
        executor.debugger_static_metadata_lowering_summary(
            tab_id,
            1,
            target.program_handle,
            target.program_generation,
            target.metadata_handle,
            target.metadata_generation,
        ),
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    );
}

#[test]
fn core_rejects_a_source_provenance_reply_of_the_wrong_variant() {
    let (mut executor, tab_id, target) = probe_executor(
        MetadataProbeTarget::SourceProvenance,
        MetadataProbeFault::WrongVariant,
    );
    let source_target = JavaScriptPageDebuggerStaticMetadataSourceTarget {
        program_handle: target.program_handle,
        program_generation: target.program_generation,
        metadata_handle: target.metadata_handle,
        metadata_generation: target.metadata_generation,
        source_id: 1,
    };
    assert_eq!(
        executor.debugger_static_metadata_source_provenance(tab_id, 1, source_target),
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    );
}

#[test]
fn core_rejects_a_source_provenance_reply_echoing_the_wrong_source_id() {
    let (mut executor, tab_id, target) = probe_executor(
        MetadataProbeTarget::SourceProvenance,
        MetadataProbeFault::MismatchedTuple,
    );
    let source_target = JavaScriptPageDebuggerStaticMetadataSourceTarget {
        program_handle: target.program_handle,
        program_generation: target.program_generation,
        metadata_handle: target.metadata_handle,
        metadata_generation: target.metadata_generation,
        source_id: 1,
    };
    assert_eq!(
        executor.debugger_static_metadata_source_provenance(tab_id, 1, source_target),
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    );
}

#[test]
fn core_rejects_a_types_reply_of_the_wrong_variant() {
    let (mut executor, tab_id, target) =
        probe_executor(MetadataProbeTarget::Types, MetadataProbeFault::WrongVariant);
    assert_eq!(
        executor.debugger_static_metadata_types(
            tab_id,
            1,
            target.program_handle,
            target.program_generation,
            target.metadata_handle,
            target.metadata_generation,
        ),
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    );
}

#[test]
fn core_rejects_a_types_reply_with_a_duplicate_type_id() {
    let (mut executor, tab_id, target) =
        probe_executor(MetadataProbeTarget::Types, MetadataProbeFault::Duplicate);
    assert_eq!(
        executor.debugger_static_metadata_types(
            tab_id,
            1,
            target.program_handle,
            target.program_generation,
            target.metadata_handle,
            target.metadata_generation,
        ),
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    );
}

#[test]
fn core_rejects_a_types_reply_exceeding_the_max_type_count() {
    let (mut executor, tab_id, target) =
        probe_executor(MetadataProbeTarget::Types, MetadataProbeFault::ExceedsMax);
    assert_eq!(
        executor.debugger_static_metadata_types(
            tab_id,
            1,
            target.program_handle,
            target.program_generation,
            target.metadata_handle,
            target.metadata_generation,
        ),
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    );
}

#[test]
fn core_rejects_a_symbols_reply_of_the_wrong_variant() {
    let (mut executor, tab_id, target) = probe_executor(
        MetadataProbeTarget::Symbols,
        MetadataProbeFault::WrongVariant,
    );
    assert_eq!(
        executor.debugger_static_metadata_symbols(
            tab_id,
            1,
            target.program_handle,
            target.program_generation,
            target.metadata_handle,
            target.metadata_generation,
        ),
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    );
}

#[test]
fn core_rejects_a_symbols_reply_with_a_duplicate_symbol_id() {
    let (mut executor, tab_id, target) =
        probe_executor(MetadataProbeTarget::Symbols, MetadataProbeFault::Duplicate);
    assert_eq!(
        executor.debugger_static_metadata_symbols(
            tab_id,
            1,
            target.program_handle,
            target.program_generation,
            target.metadata_handle,
            target.metadata_generation,
        ),
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    );
}

#[test]
fn core_rejects_a_symbols_reply_exceeding_the_max_symbol_count() {
    let (mut executor, tab_id, target) =
        probe_executor(MetadataProbeTarget::Symbols, MetadataProbeFault::ExceedsMax);
    assert_eq!(
        executor.debugger_static_metadata_symbols(
            tab_id,
            1,
            target.program_handle,
            target.program_generation,
            target.metadata_handle,
            target.metadata_generation,
        ),
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    );
}

#[test]
fn core_rejects_a_contracts_reply_of_the_wrong_variant() {
    let (mut executor, tab_id, target) = probe_executor(
        MetadataProbeTarget::Contracts,
        MetadataProbeFault::WrongVariant,
    );
    assert_eq!(
        executor.debugger_static_metadata_contracts(
            tab_id,
            1,
            target.program_handle,
            target.program_generation,
            target.metadata_handle,
            target.metadata_generation,
        ),
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    );
}

#[test]
fn core_rejects_a_contracts_reply_with_a_duplicate_contract_id() {
    let (mut executor, tab_id, target) = probe_executor(
        MetadataProbeTarget::Contracts,
        MetadataProbeFault::Duplicate,
    );
    assert_eq!(
        executor.debugger_static_metadata_contracts(
            tab_id,
            1,
            target.program_handle,
            target.program_generation,
            target.metadata_handle,
            target.metadata_generation,
        ),
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    );
}

#[test]
fn core_rejects_a_contracts_reply_exceeding_the_max_contract_count() {
    let (mut executor, tab_id, target) = probe_executor(
        MetadataProbeTarget::Contracts,
        MetadataProbeFault::ExceedsMax,
    );
    assert_eq!(
        executor.debugger_static_metadata_contracts(
            tab_id,
            1,
            target.program_handle,
            target.program_generation,
            target.metadata_handle,
            target.metadata_generation,
        ),
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    );
}

#[test]
fn core_rejects_a_contract_display_reply_of_the_wrong_variant() {
    let (mut executor, tab_id, target) = probe_executor(
        MetadataProbeTarget::ContractDisplay,
        MetadataProbeFault::WrongVariant,
    );
    let contract_target = JavaScriptPageDebuggerStaticMetadataContractTarget {
        program_handle: target.program_handle,
        program_generation: target.program_generation,
        metadata_handle: target.metadata_handle,
        metadata_generation: target.metadata_generation,
        contract_id: 1,
    };
    assert_eq!(
        executor.debugger_static_metadata_contract_display(tab_id, 1, contract_target),
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    );
}

#[test]
fn core_rejects_a_contract_display_reply_echoing_the_wrong_contract_id() {
    let (mut executor, tab_id, target) = probe_executor(
        MetadataProbeTarget::ContractDisplay,
        MetadataProbeFault::MismatchedTuple,
    );
    let contract_target = JavaScriptPageDebuggerStaticMetadataContractTarget {
        program_handle: target.program_handle,
        program_generation: target.program_generation,
        metadata_handle: target.metadata_handle,
        metadata_generation: target.metadata_generation,
        contract_id: 1,
    };
    assert_eq!(
        executor.debugger_static_metadata_contract_display(tab_id, 1, contract_target),
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    );
}

#[test]
fn core_rejects_a_contract_display_reply_with_an_empty_display() {
    let (mut executor, tab_id, target) = probe_executor(
        MetadataProbeTarget::ContractDisplay,
        MetadataProbeFault::EmptyDisplay,
    );
    let contract_target = JavaScriptPageDebuggerStaticMetadataContractTarget {
        program_handle: target.program_handle,
        program_generation: target.program_generation,
        metadata_handle: target.metadata_handle,
        metadata_generation: target.metadata_generation,
        contract_id: 1,
    };
    assert_eq!(
        executor.debugger_static_metadata_contract_display(tab_id, 1, contract_target),
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    );
}

#[test]
fn core_rejects_a_contract_display_reply_exceeding_the_max_display_length() {
    let (mut executor, tab_id, target) = probe_executor(
        MetadataProbeTarget::ContractDisplay,
        MetadataProbeFault::DisplayTooLong,
    );
    let contract_target = JavaScriptPageDebuggerStaticMetadataContractTarget {
        program_handle: target.program_handle,
        program_generation: target.program_generation,
        metadata_handle: target.metadata_handle,
        metadata_generation: target.metadata_generation,
        contract_id: 1,
    };
    assert_eq!(
        executor.debugger_static_metadata_contract_display(tab_id, 1, contract_target),
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    );
}

#[test]
fn core_rejects_a_contract_validation_reply_of_the_wrong_variant() {
    let (mut executor, tab_id, target) = probe_executor(
        MetadataProbeTarget::ContractValidation,
        MetadataProbeFault::WrongVariant,
    );
    let contract_target = JavaScriptPageDebuggerStaticMetadataContractTarget {
        program_handle: target.program_handle,
        program_generation: target.program_generation,
        metadata_handle: target.metadata_handle,
        metadata_generation: target.metadata_generation,
        contract_id: 1,
    };
    assert_eq!(
        executor.debugger_static_metadata_contract_validation(
            tab_id,
            1,
            contract_target,
            CompilerContractValue::Null,
        ),
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    );
}

#[test]
fn core_rejects_a_contract_validation_reply_echoing_the_wrong_contract_id() {
    let (mut executor, tab_id, target) = probe_executor(
        MetadataProbeTarget::ContractValidation,
        MetadataProbeFault::MismatchedTuple,
    );
    let contract_target = JavaScriptPageDebuggerStaticMetadataContractTarget {
        program_handle: target.program_handle,
        program_generation: target.program_generation,
        metadata_handle: target.metadata_handle,
        metadata_generation: target.metadata_generation,
        contract_id: 1,
    };
    assert_eq!(
        executor.debugger_static_metadata_contract_validation(
            tab_id,
            1,
            contract_target,
            CompilerContractValue::Null,
        ),
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    );
}

#[test]
fn core_rejects_a_symbol_display_reply_of_the_wrong_variant() {
    let (mut executor, tab_id, target) = probe_executor(
        MetadataProbeTarget::SymbolDisplay,
        MetadataProbeFault::WrongVariant,
    );
    let symbol_target = JavaScriptPageDebuggerStaticMetadataSymbolTarget {
        program_handle: target.program_handle,
        program_generation: target.program_generation,
        metadata_handle: target.metadata_handle,
        metadata_generation: target.metadata_generation,
        symbol_id: 1,
    };
    assert_eq!(
        executor.debugger_static_metadata_symbol_display(tab_id, 1, symbol_target),
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    );
}

#[test]
fn core_rejects_a_symbol_display_reply_echoing_the_wrong_symbol_id() {
    let (mut executor, tab_id, target) = probe_executor(
        MetadataProbeTarget::SymbolDisplay,
        MetadataProbeFault::MismatchedTuple,
    );
    let symbol_target = JavaScriptPageDebuggerStaticMetadataSymbolTarget {
        program_handle: target.program_handle,
        program_generation: target.program_generation,
        metadata_handle: target.metadata_handle,
        metadata_generation: target.metadata_generation,
        symbol_id: 1,
    };
    assert_eq!(
        executor.debugger_static_metadata_symbol_display(tab_id, 1, symbol_target),
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    );
}

#[test]
fn core_rejects_a_symbol_display_reply_with_an_empty_display() {
    let (mut executor, tab_id, target) = probe_executor(
        MetadataProbeTarget::SymbolDisplay,
        MetadataProbeFault::EmptyDisplay,
    );
    let symbol_target = JavaScriptPageDebuggerStaticMetadataSymbolTarget {
        program_handle: target.program_handle,
        program_generation: target.program_generation,
        metadata_handle: target.metadata_handle,
        metadata_generation: target.metadata_generation,
        symbol_id: 1,
    };
    assert_eq!(
        executor.debugger_static_metadata_symbol_display(tab_id, 1, symbol_target),
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    );
}

#[test]
fn core_rejects_a_symbol_display_reply_exceeding_the_max_display_length() {
    let (mut executor, tab_id, target) = probe_executor(
        MetadataProbeTarget::SymbolDisplay,
        MetadataProbeFault::DisplayTooLong,
    );
    let symbol_target = JavaScriptPageDebuggerStaticMetadataSymbolTarget {
        program_handle: target.program_handle,
        program_generation: target.program_generation,
        metadata_handle: target.metadata_handle,
        metadata_generation: target.metadata_generation,
        symbol_id: 1,
    };
    assert_eq!(
        executor.debugger_static_metadata_symbol_display(tab_id, 1, symbol_target),
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    );
}

/// Every `core_debugger_static_metadata*` capability check is a stand-alone
/// early return -- no live document, program, or metadata registration is
/// needed to observe it refuse when the underlying child capability is off.
#[test]
fn core_rejects_every_static_metadata_operation_when_its_capability_is_unavailable() {
    struct NoCapabilityChild;
    impl PageHostClient for NoCapabilityChild {
        fn synchronize_document(
            &mut self,
            document: PageHostDocument,
        ) -> io::Result<PageHostReply> {
            Ok(PageHostReply::Synchronized {
                tab_id: document.tab_id,
                document_generation: document.document_generation,
                already_current: false,
                reports: Vec::new(),
            })
        }

        fn close_realm(
            &mut self,
            tab_id: u64,
            document_generation: u64,
        ) -> io::Result<PageHostReply> {
            Ok(PageHostReply::RealmClosed {
                tab_id,
                document_generation,
            })
        }
    }

    let (tabs, tab_id) = loaded_tabs(
        "<script>let uncapable = true;</script>",
        "https://example.test/no-metadata-capability.html",
    );
    let mut executor = OutOfProcessJavaScriptPageExecutor::new(NoCapabilityChild);
    executor.synchronize_and_execute(&tabs).unwrap();

    macro_rules! assert_no_live_realm {
        ($result:expr) => {
            assert!(matches!(
                $result,
                Err(JavaScriptPageDebuggerError::NoLiveRealm)
            ));
        };
    }
    assert_no_live_realm!(executor.debugger_static_metadata(tab_id, 1, 1, 1));
    assert_no_live_realm!(executor.debugger_static_metadata_summary(tab_id, 1, 1, 1, 1, 1));
    assert_no_live_realm!(executor.debugger_static_metadata_lowering_summary(tab_id, 1, 1, 1, 1, 1));
    assert_no_live_realm!(executor.debugger_static_metadata_sources(tab_id, 1, 1, 1, 1, 1));
    assert_no_live_realm!(executor.debugger_static_metadata_types(tab_id, 1, 1, 1, 1, 1));
    assert_no_live_realm!(executor.debugger_static_metadata_symbols(tab_id, 1, 1, 1, 1, 1));
    assert_no_live_realm!(executor.debugger_static_metadata_contracts(tab_id, 1, 1, 1, 1, 1));
    let source_provenance_target = JavaScriptPageDebuggerStaticMetadataSourceTarget {
        program_handle: 1,
        program_generation: 1,
        metadata_handle: 1,
        metadata_generation: 1,
        source_id: 1,
    };
    assert_no_live_realm!(
        executor.debugger_static_metadata_source_provenance(tab_id, 1, source_provenance_target)
    );
    let type_target = JavaScriptPageDebuggerStaticMetadataTypeTarget {
        program_handle: 1,
        program_generation: 1,
        metadata_handle: 1,
        metadata_generation: 1,
        type_id: 1,
    };
    assert_no_live_realm!(executor.debugger_static_metadata_type_display(tab_id, 1, type_target));
    let contract_target = JavaScriptPageDebuggerStaticMetadataContractTarget {
        program_handle: 1,
        program_generation: 1,
        metadata_handle: 1,
        metadata_generation: 1,
        contract_id: 1,
    };
    assert_no_live_realm!(
        executor.debugger_static_metadata_contract_display(tab_id, 1, contract_target)
    );
    assert_no_live_realm!(executor.debugger_static_metadata_contract_validation(
        tab_id,
        1,
        contract_target,
        CompilerContractValue::Null,
    ));
    let symbol_target = JavaScriptPageDebuggerStaticMetadataSymbolTarget {
        program_handle: 1,
        program_generation: 1,
        metadata_handle: 1,
        metadata_generation: 1,
        symbol_id: 1,
    };
    assert_no_live_realm!(executor.debugger_static_metadata_symbol_display(tab_id, 1, symbol_target));

    let symbol_location_target = JavaScriptPageDebuggerStaticMetadataSymbolLocationTarget {
        program_handle: 1,
        program_generation: 1,
        metadata_handle: 1,
        metadata_generation: 1,
        symbol_id: 1,
        source_id: 1,
    };
    assert_no_live_realm!(
        executor.debugger_static_metadata_symbol_location(tab_id, 1, symbol_location_target)
    );
    let contract_location_target = JavaScriptPageDebuggerStaticMetadataContractLocationTarget {
        program_handle: 1,
        program_generation: 1,
        metadata_handle: 1,
        metadata_generation: 1,
        contract_id: 1,
        source_id: 1,
    };
    assert_no_live_realm!(
        executor.debugger_static_metadata_contract_location(tab_id, 1, contract_location_target)
    );
    let symbol_type_target = JavaScriptPageDebuggerStaticMetadataSymbolTypeTarget {
        program_handle: 1,
        program_generation: 1,
        metadata_handle: 1,
        metadata_generation: 1,
        symbol_id: 1,
        type_id: 1,
    };
    assert_no_live_realm!(executor.debugger_static_metadata_symbol_type(tab_id, 1, symbol_type_target));
    let symbol_contract_target = JavaScriptPageDebuggerStaticMetadataSymbolContractTarget {
        program_handle: 1,
        program_generation: 1,
        metadata_handle: 1,
        metadata_generation: 1,
        symbol_id: 1,
        contract_id: 1,
    };
    assert_no_live_realm!(executor
        .debugger_static_metadata_symbol_contract(tab_id, 1, symbol_contract_target));
}

#[test]
fn core_resolves_a_live_bluets_symbol_location() {
    let (mut executor, tab_id, target) =
        probe_executor(MetadataProbeTarget::SymbolLocation, MetadataProbeFault::None);
    let location_target = JavaScriptPageDebuggerStaticMetadataSymbolLocationTarget {
        program_handle: target.program_handle,
        program_generation: target.program_generation,
        metadata_handle: target.metadata_handle,
        metadata_generation: target.metadata_generation,
        symbol_id: 7,
        source_id: 3,
    };
    assert_eq!(
        executor.debugger_static_metadata_symbol_location(tab_id, 1, location_target),
        Ok(JavaScriptPageDebuggerStaticMetadataSymbolLocation {
            symbol_id: 7,
            source_id: 3,
            start_byte: 2,
            end_byte: 8,
            coordinates: blueice_ipc::debugger::DebuggerSourceCoordinates {
                start_line: 0,
                start_column_utf16: 2,
                end_line: 0,
                end_column_utf16: 8,
            },
        })
    );
}

#[test]
fn core_rejects_a_symbol_location_reply_of_the_wrong_variant() {
    let (mut executor, tab_id, target) = probe_executor(
        MetadataProbeTarget::SymbolLocation,
        MetadataProbeFault::WrongVariant,
    );
    let location_target = JavaScriptPageDebuggerStaticMetadataSymbolLocationTarget {
        program_handle: target.program_handle,
        program_generation: target.program_generation,
        metadata_handle: target.metadata_handle,
        metadata_generation: target.metadata_generation,
        symbol_id: 7,
        source_id: 3,
    };
    assert_eq!(
        executor.debugger_static_metadata_symbol_location(tab_id, 1, location_target),
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    );
}

#[test]
fn core_rejects_a_symbol_location_reply_echoing_the_wrong_tab() {
    let (mut executor, tab_id, target) = probe_executor(
        MetadataProbeTarget::SymbolLocation,
        MetadataProbeFault::MismatchedTuple,
    );
    let location_target = JavaScriptPageDebuggerStaticMetadataSymbolLocationTarget {
        program_handle: target.program_handle,
        program_generation: target.program_generation,
        metadata_handle: target.metadata_handle,
        metadata_generation: target.metadata_generation,
        symbol_id: 7,
        source_id: 3,
    };
    assert_eq!(
        executor.debugger_static_metadata_symbol_location(tab_id, 1, location_target),
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    );
}

#[test]
fn core_rejects_a_symbol_location_reply_echoing_the_wrong_symbol_id() {
    let (mut executor, tab_id, target) = probe_executor(
        MetadataProbeTarget::SymbolLocation,
        MetadataProbeFault::NotWellFormed,
    );
    let location_target = JavaScriptPageDebuggerStaticMetadataSymbolLocationTarget {
        program_handle: target.program_handle,
        program_generation: target.program_generation,
        metadata_handle: target.metadata_handle,
        metadata_generation: target.metadata_generation,
        symbol_id: 7,
        source_id: 3,
    };
    assert_eq!(
        executor.debugger_static_metadata_symbol_location(tab_id, 1, location_target),
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    );
}

#[test]
fn core_resolves_a_live_bluets_contract_location() {
    let (mut executor, tab_id, target) = probe_executor(
        MetadataProbeTarget::ContractLocation,
        MetadataProbeFault::None,
    );
    let location_target = JavaScriptPageDebuggerStaticMetadataContractLocationTarget {
        program_handle: target.program_handle,
        program_generation: target.program_generation,
        metadata_handle: target.metadata_handle,
        metadata_generation: target.metadata_generation,
        contract_id: 11,
        source_id: 3,
    };
    assert_eq!(
        executor.debugger_static_metadata_contract_location(tab_id, 1, location_target),
        Ok(JavaScriptPageDebuggerStaticMetadataContractLocation {
            contract_id: 11,
            source_id: 3,
            start_byte: 2,
            end_byte: 8,
            coordinates: blueice_ipc::debugger::DebuggerSourceCoordinates {
                start_line: 0,
                start_column_utf16: 2,
                end_line: 0,
                end_column_utf16: 8,
            },
        })
    );
}

#[test]
fn core_rejects_a_contract_location_reply_of_the_wrong_variant() {
    let (mut executor, tab_id, target) = probe_executor(
        MetadataProbeTarget::ContractLocation,
        MetadataProbeFault::WrongVariant,
    );
    let location_target = JavaScriptPageDebuggerStaticMetadataContractLocationTarget {
        program_handle: target.program_handle,
        program_generation: target.program_generation,
        metadata_handle: target.metadata_handle,
        metadata_generation: target.metadata_generation,
        contract_id: 11,
        source_id: 3,
    };
    assert_eq!(
        executor.debugger_static_metadata_contract_location(tab_id, 1, location_target),
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    );
}

#[test]
fn core_rejects_a_contract_location_reply_echoing_the_wrong_tab() {
    let (mut executor, tab_id, target) = probe_executor(
        MetadataProbeTarget::ContractLocation,
        MetadataProbeFault::MismatchedTuple,
    );
    let location_target = JavaScriptPageDebuggerStaticMetadataContractLocationTarget {
        program_handle: target.program_handle,
        program_generation: target.program_generation,
        metadata_handle: target.metadata_handle,
        metadata_generation: target.metadata_generation,
        contract_id: 11,
        source_id: 3,
    };
    assert_eq!(
        executor.debugger_static_metadata_contract_location(tab_id, 1, location_target),
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    );
}

#[test]
fn core_rejects_a_contract_location_reply_with_an_inverted_byte_range() {
    let (mut executor, tab_id, target) = probe_executor(
        MetadataProbeTarget::ContractLocation,
        MetadataProbeFault::NotWellFormed,
    );
    let location_target = JavaScriptPageDebuggerStaticMetadataContractLocationTarget {
        program_handle: target.program_handle,
        program_generation: target.program_generation,
        metadata_handle: target.metadata_handle,
        metadata_generation: target.metadata_generation,
        contract_id: 11,
        source_id: 3,
    };
    assert_eq!(
        executor.debugger_static_metadata_contract_location(tab_id, 1, location_target),
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    );
}

#[test]
fn core_rejects_a_well_formed_contract_location_reply_naming_the_wrong_contract() {
    let (mut executor, tab_id, target) = probe_executor(
        MetadataProbeTarget::ContractLocation,
        MetadataProbeFault::MismatchedRelation,
    );
    let location_target = JavaScriptPageDebuggerStaticMetadataContractLocationTarget {
        program_handle: target.program_handle,
        program_generation: target.program_generation,
        metadata_handle: target.metadata_handle,
        metadata_generation: target.metadata_generation,
        contract_id: 11,
        source_id: 3,
    };
    assert_eq!(
        executor.debugger_static_metadata_contract_location(tab_id, 1, location_target),
        Err(JavaScriptPageDebuggerError::UnknownProgram)
    );
}

#[test]
fn core_resolves_a_live_bluets_symbol_type() {
    let (mut executor, tab_id, target) =
        probe_executor(MetadataProbeTarget::SymbolType, MetadataProbeFault::None);
    let type_target = JavaScriptPageDebuggerStaticMetadataSymbolTypeTarget {
        program_handle: target.program_handle,
        program_generation: target.program_generation,
        metadata_handle: target.metadata_handle,
        metadata_generation: target.metadata_generation,
        symbol_id: 7,
        type_id: 13,
    };
    assert_eq!(
        executor.debugger_static_metadata_symbol_type(tab_id, 1, type_target),
        Ok(JavaScriptPageDebuggerStaticMetadataSymbolType {
            symbol_id: 7,
            type_id: 13,
        })
    );
}

#[test]
fn core_rejects_a_symbol_type_reply_of_the_wrong_variant() {
    let (mut executor, tab_id, target) = probe_executor(
        MetadataProbeTarget::SymbolType,
        MetadataProbeFault::WrongVariant,
    );
    let type_target = JavaScriptPageDebuggerStaticMetadataSymbolTypeTarget {
        program_handle: target.program_handle,
        program_generation: target.program_generation,
        metadata_handle: target.metadata_handle,
        metadata_generation: target.metadata_generation,
        symbol_id: 7,
        type_id: 13,
    };
    assert_eq!(
        executor.debugger_static_metadata_symbol_type(tab_id, 1, type_target),
        Err(JavaScriptPageDebuggerError::UnknownProgram)
    );
}

#[test]
fn core_rejects_a_symbol_type_reply_echoing_the_wrong_tab() {
    let (mut executor, tab_id, target) = probe_executor(
        MetadataProbeTarget::SymbolType,
        MetadataProbeFault::MismatchedTuple,
    );
    let type_target = JavaScriptPageDebuggerStaticMetadataSymbolTypeTarget {
        program_handle: target.program_handle,
        program_generation: target.program_generation,
        metadata_handle: target.metadata_handle,
        metadata_generation: target.metadata_generation,
        symbol_id: 7,
        type_id: 13,
    };
    assert_eq!(
        executor.debugger_static_metadata_symbol_type(tab_id, 1, type_target),
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    );
}

#[test]
fn core_rejects_a_symbol_type_reply_echoing_the_wrong_type_id() {
    let (mut executor, tab_id, target) = probe_executor(
        MetadataProbeTarget::SymbolType,
        MetadataProbeFault::NotWellFormed,
    );
    let type_target = JavaScriptPageDebuggerStaticMetadataSymbolTypeTarget {
        program_handle: target.program_handle,
        program_generation: target.program_generation,
        metadata_handle: target.metadata_handle,
        metadata_generation: target.metadata_generation,
        symbol_id: 7,
        type_id: 13,
    };
    assert_eq!(
        executor.debugger_static_metadata_symbol_type(tab_id, 1, type_target),
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    );
}

#[test]
fn core_resolves_a_live_bluets_symbol_contract() {
    let (mut executor, tab_id, target) = probe_executor(
        MetadataProbeTarget::SymbolContract,
        MetadataProbeFault::None,
    );
    let contract_target = JavaScriptPageDebuggerStaticMetadataSymbolContractTarget {
        program_handle: target.program_handle,
        program_generation: target.program_generation,
        metadata_handle: target.metadata_handle,
        metadata_generation: target.metadata_generation,
        symbol_id: 7,
        contract_id: 11,
    };
    assert_eq!(
        executor.debugger_static_metadata_symbol_contract(tab_id, 1, contract_target),
        Ok(JavaScriptPageDebuggerStaticMetadataSymbolContract {
            symbol_id: 7,
            contract_id: 11,
        })
    );
}

#[test]
fn core_rejects_a_symbol_contract_reply_of_the_wrong_variant() {
    let (mut executor, tab_id, target) = probe_executor(
        MetadataProbeTarget::SymbolContract,
        MetadataProbeFault::WrongVariant,
    );
    let contract_target = JavaScriptPageDebuggerStaticMetadataSymbolContractTarget {
        program_handle: target.program_handle,
        program_generation: target.program_generation,
        metadata_handle: target.metadata_handle,
        metadata_generation: target.metadata_generation,
        symbol_id: 7,
        contract_id: 11,
    };
    assert_eq!(
        executor.debugger_static_metadata_symbol_contract(tab_id, 1, contract_target),
        Err(JavaScriptPageDebuggerError::UnknownProgram)
    );
}

#[test]
fn core_rejects_a_symbol_contract_reply_echoing_the_wrong_tab() {
    let (mut executor, tab_id, target) = probe_executor(
        MetadataProbeTarget::SymbolContract,
        MetadataProbeFault::MismatchedTuple,
    );
    let contract_target = JavaScriptPageDebuggerStaticMetadataSymbolContractTarget {
        program_handle: target.program_handle,
        program_generation: target.program_generation,
        metadata_handle: target.metadata_handle,
        metadata_generation: target.metadata_generation,
        symbol_id: 7,
        contract_id: 11,
    };
    assert_eq!(
        executor.debugger_static_metadata_symbol_contract(tab_id, 1, contract_target),
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    );
}

#[test]
fn core_rejects_a_symbol_contract_reply_echoing_the_wrong_contract_id() {
    let (mut executor, tab_id, target) = probe_executor(
        MetadataProbeTarget::SymbolContract,
        MetadataProbeFault::NotWellFormed,
    );
    let contract_target = JavaScriptPageDebuggerStaticMetadataSymbolContractTarget {
        program_handle: target.program_handle,
        program_generation: target.program_generation,
        metadata_handle: target.metadata_handle,
        metadata_generation: target.metadata_generation,
        symbol_id: 7,
        contract_id: 11,
    };
    assert_eq!(
        executor.debugger_static_metadata_symbol_contract(tab_id, 1, contract_target),
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    );
}
