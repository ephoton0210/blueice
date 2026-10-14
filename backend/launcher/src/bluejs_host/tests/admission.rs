// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn emitted_map_admission_rejects_an_invalid_complete_stream() {
    let generated = PageHostSource::new("blueice://page/generated.js", "const value = 1;");
    let map = SourceMap {
        file: "generated.js".to_string(),
        sources: vec!["source.ts".to_string()],
        sources_content: vec!["const value: number = 1;".to_string()],
        inline_sources: true,
        source_root: String::new(),
        names: Vec::new(),
        mappings: "AAAA;!".to_string(),
    };
    assert!(BoundEmittedMap::new(
        generated.clone(),
        map.clone(),
        PageHostBlueTsEmissionProfile::Standard,
        Vec::new(),
        Vec::new()
    )
    .is_err());
    let mut oversized_allocation = generated;
    oversized_allocation
        .source
        .reserve_exact(BoundEmittedMap::MAX_RETAINED_BYTES + 1);
    let mut valid_map = map;
    valid_map.mappings = "AAAA".to_string();
    assert!(
        BoundEmittedMap::new(
            oversized_allocation,
            valid_map,
            PageHostBlueTsEmissionProfile::Standard,
            Vec::new(),
            Vec::new()
        )
        .is_err(),
        "retention must account for allocation capacity, not only text length"
    );
}

#[test]
fn corrupted_live_emitted_map_discloses_no_partial_location_or_safe_point() {
    let mut host = BlueJsChildHost::default();
    let reply = host.handle_request(PageHostRequest::SynchronizeDocument {
        document: debugger_document(
            1,
            vec![blue_ts_emitted_classic(0, "const answer: number = 42;")],
        ),
    });
    assert!(matches!(
        reply,
        PageHostReply::Synchronized { reports, .. } if reports.is_empty()
    ));
    let program = *host.documents[&7].emitted_maps.keys().next().unwrap();
    let bound = host
        .documents
        .get_mut(&7)
        .unwrap()
        .emitted_maps
        .get_mut(&program)
        .unwrap();
    bound.map.mappings.push_str(";!");
    for reply in [
        host.handle_request(PageHostRequest::ResolveDebuggerEmittedOriginalPosition {
            tab_id: 7,
            document_generation: 1,
            program,
            generated_line: 0,
            generated_column_utf16: 0,
        }),
        host.handle_request(PageHostRequest::ResolveDebuggerEmittedSafePoint {
            tab_id: 7,
            document_generation: 1,
            program,
            generated_line: 0,
            generated_column_utf16: 0,
        }),
    ] {
        assert!(matches!(
            reply,
            PageHostReply::Error {
                code: PageHostErrorCode::HostFailure,
                ..
            }
        ));
    }
    assert!(host.documents[&7].debugger_breakpoints.is_empty());
    assert_eq!(
        host.documents[&7].debugger_execution_states[&program],
        ChildDebuggerExecutionStatus::Pending
    );
}

#[test]
fn page_host_compiles_and_executes_an_emitted_bluets_artifact_in_the_child() {
    let fixture = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../bluets/tests/fixtures/k11_source_maps/decorated.ts"
    ));
    let source = fixture.replace(
        "console.log(new Example().method());",
        "const finalAnswer: number = new Example().method();",
    );
    assert_ne!(source, fixture);
    let mut host = BlueJsChildHost::default();
    let reply = host.handle_request(PageHostRequest::SynchronizeDocument {
        document: document(
            1,
            vec![
                blue_ts_emitted_classic(0, &source),
                classic(1, "if (finalAnswer !== 42) throw 'emitted decorator';"),
            ],
        ),
    });
    let PageHostReply::Synchronized { reports, .. } = reply else {
        panic!("expected synchronized reply");
    };
    assert_eq!(reports.len(), 2);
    assert_eq!(reports[0].language, PageHostScriptLanguage::BlueTsEmitted);
    assert_eq!(reports[0].outcome, PageHostScriptOutcome::Executed);
    assert_eq!(reports[1].outcome, PageHostScriptOutcome::Executed);
}

#[test]
fn emitted_bluets_module_executes_its_closed_emitted_graph_without_debugger_control() {
    let entry = "blueice://page/emitted-entry.ts";
    let dependency = "blueice://page/emitted-dependency.ts";
    let mut source_graph = graph(
        entry,
        vec![
            PageHostSource::new(
                entry,
                "import { value } from './emitted-dependency.ts'; const record = { value }; if (record.value !== 41) throw 'wrong emitted import';",
            ),
            PageHostSource::new(dependency, "export const value: number = 41;"),
        ],
    );
    source_graph.resolutions.push(PageHostStaticResolution {
        from_module: entry.into(),
        specifier: "./emitted-dependency.ts".into(),
        canonical_target: dependency.into(),
    });
    let (emitted_graph, programs, maps) =
        prepare_bluets_emitted_module_graph(source_graph.clone(), PageDomProfile::Snapshot, false)
            .expect("the child must preflight the entire emitted module graph");
    assert_eq!(programs.len(), 2);
    assert_eq!(maps.len(), 2);
    assert!(programs.keys().eq(maps.keys()));
    assert_eq!(emitted_graph.modules.len(), 2);
    assert_eq!(
        emitted_graph.resolutions[0].specifier,
        "./emitted-dependency.js"
    );
    for generated in &emitted_graph.modules {
        let bound = &maps[&generated.canonical_module_id];
        assert_eq!(bound.generated.source_hash, generated.source_hash);
        assert_eq!(bound.generated.source, generated.source);
        assert!(!generated.source.contains(": number"));
        assert_eq!(
            bound.top_level_spans.len(),
            blueice_bluejs::parse_module(&generated.source)
                .unwrap()
                .body
                .len()
        );
        assert!(!bound.top_level_spans.is_empty());
        for identifier in &bound.identifier_spans {
            assert_eq!(
                &generated.source[identifier.start_byte..identifier.end_byte],
                identifier.name,
                "an emitted module identifier must keep its exact parser token"
            );
        }
    }
    assert!(
        maps[entry]
            .identifier_spans
            .iter()
            .any(|span| span.name == "value"),
        "the imported object-literal shorthand value must retain its token"
    );
    let mut host = BlueJsChildHost::default();
    let reply = host.handle_request(PageHostRequest::SynchronizeDocument {
        document: document(
            1,
            vec![PageHostScript {
                ordinal: 0,
                language: PageHostScriptLanguage::BlueTsEmitted,
                kind: PageHostScriptKind::Module,
                graph: source_graph,
            }],
        ),
    });
    assert!(
        matches!(
            &reply,
            PageHostReply::Synchronized { reports, .. }
                if matches!(reports.as_slice(), [report]
                    if report.outcome == PageHostScriptOutcome::Executed)
        ),
        "{reply:?}"
    );
}

#[test]
fn emitted_bluets_module_debugger_attaches_every_map_to_its_live_program() {
    let entry = "blueice://page/emitted-module-debugger.ts";
    let dependency = "blueice://page/emitted-module-dependency.ts";
    let entry_source = "import { value } from './emitted-module-dependency.ts'; const marker: number = 0; export const answer: number = value + 1;";
    let mut source_graph = graph(
        entry,
        vec![
            PageHostSource::new(entry, entry_source),
            PageHostSource::new(dependency, "export const value: number = 41;"),
        ],
    );
    source_graph.resolutions.push(PageHostStaticResolution {
        from_module: entry.into(),
        specifier: "./emitted-module-dependency.ts".into(),
        canonical_target: dependency.into(),
    });
    let mut host = BlueJsChildHost::default();
    let reply = host.handle_request(PageHostRequest::SynchronizeDocument {
        document: debugger_document(
            1,
            vec![PageHostScript {
                ordinal: 0,
                language: PageHostScriptLanguage::BlueTsEmitted,
                kind: PageHostScriptKind::Module,
                graph: source_graph,
            }],
        ),
    });
    assert!(matches!(
        &reply,
        PageHostReply::Synchronized { reports, .. } if reports.is_empty()
    ));
    let document = host.documents.get(&7).unwrap();
    assert_eq!(document.emitted_maps.len(), 2);
    assert_eq!(document.debugger_programs.len(), 2);
    let pending = document.pending_debugger_executions.front().unwrap();
    let entry_program = pending.program.unwrap();
    let DeferredChildExecution::BlueTsModule {
        attachment: DeferredModuleAttachment::Emitted(attachment),
        ..
    } = &pending.execution
    else {
        panic!("the emitted graph must own a live debugger attachment");
    };
    assert_eq!(attachment.modules.len(), 2);
    assert_eq!(attachment.entry, attachment.modules[entry]);
    for (module_id, handle) in &attachment.modules {
        let program = document
            .debugger_programs
            .iter()
            .find(|(_, record)| record.runtime_handle == *handle)
            .map(|(opaque, record)| PageHostDebuggerProgram {
                program_handle: *opaque,
                program_generation: record.program_generation,
            })
            .unwrap();
        let bound = &document.emitted_maps[&program];
        let compiled = host.runtime.program_registry().get(*handle).unwrap();
        assert_eq!(compiled.source().canonical_module_id(), module_id);
        assert_eq!(compiled.source().source_hash(), bound.generated.source_hash);
    }
    let point = host
        .runtime
        .module_evaluate_entry_safe_point(7, attachment.entry)
        .unwrap();
    let safe_point = PageHostDebuggerSafePoint {
        program: entry_program,
        code_unit_ordinal: 0,
        bytecode_offset: point.bytecode_offset,
    };
    let generated = &document.emitted_maps[&entry_program].generated.source;
    let value_byte = generated.find("value + 1").unwrap();
    let before_value = &generated[..value_byte];
    let generated_line = before_value.bytes().filter(|byte| *byte == b'\n').count() as u32;
    let generated_column_utf16 = before_value
        .rsplit('\n')
        .next()
        .unwrap()
        .encode_utf16()
        .count() as u32;
    let marker_byte = document.emitted_maps[&entry_program].top_level_spans[0].start_byte;
    let before_marker = &generated[..marker_byte];
    let marker_line = before_marker.bytes().filter(|byte| *byte == b'\n').count() as u32;
    let marker_column_utf16 = before_marker
        .rsplit('\n')
        .next()
        .unwrap()
        .encode_utf16()
        .count() as u32;
    assert_eq!(
        host.handle_request(PageHostRequest::ResolveDebuggerEmittedOriginalPosition {
            tab_id: 7,
            document_generation: 1,
            program: entry_program,
            generated_line,
            generated_column_utf16,
        }),
        PageHostReply::DebuggerEmittedOriginalPosition {
            tab_id: 7,
            document_generation: 1,
            program: entry_program,
            generated_line,
            generated_column_utf16,
            original: Some(PageHostDebuggerEmittedOriginalPosition {
                source_index: 0,
                line: 0,
                column_utf16: entry_source.find("value + 1").unwrap() as u32,
            }),
        }
    );
    assert!(matches!(
        host.handle_request(PageHostRequest::ResolveDebuggerEmittedSafePoint {
            tab_id: 7,
            document_generation: 1,
            program: entry_program,
            generated_line,
            generated_column_utf16,
        }),
        PageHostReply::DebuggerEmittedSafePoint {
            safe_point: Some(_),
            ..
        }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::ArmDebuggerRootSafePointBreakpoint {
            tab_id: 7,
            document_generation: 1,
            safe_point,
        }),
        PageHostReply::DebuggerRootSafePointBreakpointArmed { .. }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::DebuggerExecutionAdvanced { reports, .. } if reports.is_empty()
    ));
    assert_eq!(
        host.handle_request(PageHostRequest::GetDebuggerExecutionState {
            tab_id: 7,
            document_generation: 1,
            program: entry_program,
        }),
        PageHostReply::DebuggerExecutionState {
            tab_id: 7,
            document_generation: 1,
            program: entry_program,
            state: PageHostDebuggerExecutionState::Paused { safe_point },
        }
    );
    assert_eq!(
        host.handle_request(PageHostRequest::GetDebuggerEmittedPausedRootLocation {
            tab_id: 7,
            document_generation: 1,
            program: entry_program,
        }),
        PageHostReply::DebuggerEmittedPausedRootLocation {
            tab_id: 7,
            document_generation: 1,
            program: entry_program,
            safe_point,
            location: Some(PageHostDebuggerEmittedPausedRootLocation {
                generated_line: marker_line,
                generated_column_utf16: marker_column_utf16,
                original: Some(PageHostDebuggerEmittedOriginalPosition {
                    source_index: 0,
                    line: 0,
                    column_utf16: entry_source.find("const marker").unwrap() as u32,
                }),
            }),
        }
    );
}

#[test]
fn emitted_bluets_module_debugger_rolls_back_an_incomplete_graph_install() {
    let entry = "blueice://page/rollback-entry.ts";
    let dependency = "blueice://page/rollback-dependency.ts";
    let mut source_graph = graph(
        entry,
        vec![
            PageHostSource::new(
                entry,
                "import { value } from './rollback-dependency.ts'; export const answer: number = value + 1;",
            ),
            PageHostSource::new(dependency, "export const value: number = 41;"),
        ],
    );
    source_graph.resolutions.push(PageHostStaticResolution {
        from_module: entry.into(),
        specifier: "./rollback-dependency.ts".into(),
        canonical_target: dependency.into(),
    });
    let mut host = BlueJsChildHost::with_runtime_limits(BlueJsHostRuntimeLimits {
        max_programs_per_realm: 1,
        ..BlueJsHostRuntimeLimits::default()
    })
    .unwrap();
    let reply = host.handle_request(PageHostRequest::SynchronizeDocument {
        document: debugger_document(
            1,
            vec![PageHostScript {
                ordinal: 0,
                language: PageHostScriptLanguage::BlueTsEmitted,
                kind: PageHostScriptKind::Module,
                graph: source_graph,
            }],
        ),
    });
    assert!(matches!(
        reply,
        PageHostReply::Synchronized { reports, .. }
            if matches!(reports.as_slice(), [report]
                if matches!(report.outcome, PageHostScriptOutcome::Rejected { .. }))
    ));
    let document = &host.documents[&7];
    assert!(document.debugger_programs.is_empty());
    assert!(document.emitted_maps.is_empty());
    assert!(document.pending_debugger_executions.is_empty());
    assert!(host.runtime.program_handles(7).unwrap().is_empty());
}

#[test]
fn emitted_bluets_module_debugger_rejects_a_substituted_map_before_install() {
    let entry = "blueice://page/map-entry.ts";
    let source_graph = graph(
        entry,
        vec![PageHostSource::new(
            entry,
            "export const value: number = 41;",
        )],
    );
    let (generated_graph, programs, mut maps) =
        prepare_bluets_emitted_module_graph(source_graph, PageDomProfile::Snapshot, false).unwrap();
    maps.get_mut(entry).unwrap().generated.source_hash.push('x');
    let mut host = BlueJsChildHost::default();
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: debugger_document(1, vec![]),
        }),
        PageHostReply::Synchronized { .. }
    ));
    let origin = host.documents[&7].origin.clone();
    assert!(matches!(
        attach_emitted_module_graph(
            &mut host.runtime,
            7,
            &origin,
            &generated_graph,
            &programs,
            &maps,
        ),
        Err("BlueTS emitted module map does not match its generated source")
    ));
    assert!(host.runtime.program_handles(7).unwrap().is_empty());
    assert!(host.documents[&7].emitted_maps.is_empty());
}

#[test]
fn debugger_admission_retains_the_exact_emitted_map_until_realm_replacement() {
    let fixture = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../bluets/tests/fixtures/k11_source_maps/decorated.ts"
    ));
    let source = fixture.replace(
        "console.log(new Example().method());",
        "const finalAnswer: number = new Example().method();",
    );
    let mut host = BlueJsChildHost::default();
    let reply = host.handle_request(PageHostRequest::SynchronizeDocument {
        document: debugger_document(1, vec![blue_ts_emitted_classic(0, &source)]),
    });
    assert!(matches!(reply, PageHostReply::Synchronized { .. }));
    let programs = match host.handle_request(PageHostRequest::ListDebuggerPrograms {
        tab_id: 7,
        document_generation: 1,
    }) {
        PageHostReply::DebuggerPrograms { programs, .. } => programs,
        reply => panic!("expected a live emitted program, got {reply:?}"),
    };
    assert_eq!(programs.len(), 1);
    let live_document = host.documents.get(&7).unwrap();
    let bound = live_document.emitted_maps.get(&programs[0]).unwrap();
    let bound_bytes = bound.retained_bytes().unwrap();
    assert_eq!(
        host.retained_static_payload_bytes(7, 1).unwrap(),
        bound_bytes
    );
    let runtime_handle = live_document
        .debugger_programs
        .get(&programs[0].program_handle)
        .unwrap()
        .runtime_handle;
    let installed = host.runtime.program_registry().get(runtime_handle).unwrap();
    assert_eq!(
        installed.source().source_hash(),
        bound.generated.source_hash
    );
    assert_eq!(
        installed.source().canonical_module_id(),
        bound.generated.canonical_module_id
    );
    let javascript = &bound.generated.source;
    let generated_position = |offset| {
        let prefix = &javascript[..offset];
        (
            prefix.bytes().filter(|byte| *byte == b'\n').count(),
            prefix.rsplit('\n').next().unwrap().encode_utf16().count(),
        )
    };
    let relocated = javascript.find("_method_decorators = [decoSite]").unwrap()
        + "_method_decorators = [".len();
    let (relocated_line, relocated_column) = generated_position(relocated);
    let relocated_statement_index = bound
        .top_level_spans
        .iter()
        .position(|span| span.start_byte <= relocated && relocated < span.end_byte)
        .unwrap();
    let relocated_runtime_point = installed
        .unique_identifier_safe_point(relocated_statement_index, "decoSite")
        .unwrap();
    assert_eq!(
        installed.code_units()[relocated_runtime_point.code_unit.ordinal() as usize]
            .nesting_depth(),
        2,
        "the emitted decorator runs below an IIFE and a class static block"
    );
    let relocated_safe_point = PageHostDebuggerSafePoint {
        program: programs[0],
        code_unit_ordinal: relocated_runtime_point.code_unit.ordinal(),
        bytecode_offset: relocated_runtime_point.bytecode_offset,
    };
    let (line, column) = generated_position(javascript.find("var __bluetsEsDecorate").unwrap());
    let helper_start = javascript.find("var __bluetsEsDecorate").unwrap();
    let helper_index = bound
        .top_level_spans
        .iter()
        .position(|span| span.start_byte == helper_start)
        .unwrap();
    let helper_node = installed
        .ast_nodes()
        .iter()
        .filter(|node| node.is_top_level_statement())
        .nth(helper_index)
        .unwrap();
    let helper_runtime_point = installed.safe_point_for_ast_node(helper_node.id()).unwrap();
    let helper_safe_point = PageHostDebuggerSafePoint {
        program: programs[0],
        code_unit_ordinal: helper_runtime_point.code_unit.ordinal(),
        bytecode_offset: helper_runtime_point.bytecode_offset,
    };
    let PageHostReply::RealmStats(stats) = host.handle_request(PageHostRequest::GetRealmStats {
        tab_id: 7,
        document_generation: 1,
    }) else {
        panic!("the admitted map must be charged to child realm stats");
    };
    assert_eq!(stats.static_metadata_bytes, bound_bytes as u64);
    let (source_line, source_column) = {
        let prefix = &source[..source.find("@decoSite").unwrap() + 1];
        (
            prefix.bytes().filter(|byte| *byte == b'\n').count(),
            prefix.rsplit('\n').next().unwrap().encode_utf16().count(),
        )
    };
    assert_eq!(
        host.handle_request(PageHostRequest::ResolveDebuggerEmittedOriginalPosition {
            tab_id: 7,
            document_generation: 1,
            program: programs[0],
            generated_line: relocated_line as u32,
            generated_column_utf16: relocated_column as u32,
        }),
        PageHostReply::DebuggerEmittedOriginalPosition {
            tab_id: 7,
            document_generation: 1,
            program: programs[0],
            generated_line: relocated_line as u32,
            generated_column_utf16: relocated_column as u32,
            original: Some(PageHostDebuggerEmittedOriginalPosition {
                source_index: 0,
                line: source_line as u32,
                column_utf16: source_column as u32,
            }),
        }
    );
    assert_eq!(
        host.handle_request(PageHostRequest::ResolveDebuggerEmittedSafePoint {
            tab_id: 7,
            document_generation: 1,
            program: programs[0],
            generated_line: relocated_line as u32,
            generated_column_utf16: relocated_column as u32,
        }),
        PageHostReply::DebuggerEmittedSafePoint {
            tab_id: 7,
            document_generation: 1,
            program: programs[0],
            generated_line: relocated_line as u32,
            generated_column_utf16: relocated_column as u32,
            safe_point: Some(relocated_safe_point),
        },
        "the depth-two decorator token must bind its verified compiler instruction"
    );
    assert_eq!(
        host.handle_request(PageHostRequest::ResolveDebuggerEmittedOriginalPosition {
            tab_id: 7,
            document_generation: 1,
            program: programs[0],
            generated_line: line as u32,
            generated_column_utf16: column as u32,
        }),
        PageHostReply::DebuggerEmittedOriginalPosition {
            tab_id: 7,
            document_generation: 1,
            program: programs[0],
            generated_line: line as u32,
            generated_column_utf16: column as u32,
            original: None,
        }
    );
    assert_eq!(
        host.handle_request(PageHostRequest::ResolveDebuggerEmittedSafePoint {
            tab_id: 7,
            document_generation: 1,
            program: programs[0],
            generated_line: line as u32,
            generated_column_utf16: column as u32,
        }),
        PageHostReply::DebuggerEmittedSafePoint {
            tab_id: 7,
            document_generation: 1,
            program: programs[0],
            generated_line: line as u32,
            generated_column_utf16: column as u32,
            safe_point: None,
        },
        "unmapped helper definition must never bind"
    );
    assert!(matches!(
        host.handle_request(PageHostRequest::ArmDebuggerRootSafePointBreakpoint {
            tab_id: 7,
            document_generation: 1,
            safe_point: helper_safe_point,
        }),
        PageHostReply::DebuggerRootSafePointBreakpointArmed { .. }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::DebuggerExecutionAdvanced { .. }
    ));
    assert_eq!(
        host.handle_request(PageHostRequest::GetDebuggerEmittedPausedRootLocation {
            tab_id: 7,
            document_generation: 1,
            program: programs[0],
        }),
        PageHostReply::DebuggerEmittedPausedRootLocation {
            tab_id: 7,
            document_generation: 1,
            program: programs[0],
            safe_point: helper_safe_point,
            location: Some(PageHostDebuggerEmittedPausedRootLocation {
                generated_line: line as u32,
                generated_column_utf16: column as u32,
                original: None,
            }),
        },
        "a paused generated helper must never become an original user location"
    );
    assert!(matches!(
        host.handle_request(PageHostRequest::ResolveDebuggerEmittedOriginalPosition {
            tab_id: 7,
            document_generation: 1,
            program: PageHostDebuggerProgram {
                program_generation: programs[0].program_generation + 1,
                ..programs[0]
            },
            generated_line: line as u32,
            generated_column_utf16: column as u32,
        }),
        PageHostReply::Error {
            code: PageHostErrorCode::InvalidRequest,
            ..
        }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::ResolveDebuggerEmittedSafePoint {
            tab_id: 7,
            document_generation: 1,
            program: PageHostDebuggerProgram {
                program_generation: programs[0].program_generation + 1,
                ..programs[0]
            },
            generated_line: relocated_line as u32,
            generated_column_utf16: relocated_column as u32,
        }),
        PageHostReply::Error {
            code: PageHostErrorCode::InvalidRequest,
            ..
        }
    ));
    host.documents
        .get_mut(&7)
        .unwrap()
        .emitted_maps
        .get_mut(&programs[0])
        .unwrap()
        .generated
        .source_hash
        .push('x');
    assert!(matches!(
        host.handle_request(PageHostRequest::ResolveDebuggerEmittedOriginalPosition {
            tab_id: 7,
            document_generation: 1,
            program: programs[0],
            generated_line: line as u32,
            generated_column_utf16: column as u32,
        }),
        PageHostReply::Error {
            code: PageHostErrorCode::InvalidRequest,
            ..
        }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::ResolveDebuggerEmittedSafePoint {
            tab_id: 7,
            document_generation: 1,
            program: programs[0],
            generated_line: relocated_line as u32,
            generated_column_utf16: relocated_column as u32,
        }),
        PageHostReply::Error {
            code: PageHostErrorCode::InvalidRequest,
            ..
        }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: document(2, vec![]),
        }),
        PageHostReply::Synchronized { .. }
    ));
    assert!(host.documents.get(&7).unwrap().emitted_maps.is_empty());
    assert_eq!(host.retained_static_payload_bytes(7, 2).unwrap(), 0);
    assert!(matches!(
        host.handle_request(PageHostRequest::ResolveDebuggerEmittedOriginalPosition {
            tab_id: 7,
            document_generation: 1,
            program: programs[0],
            generated_line: line as u32,
            generated_column_utf16: column as u32,
        }),
        PageHostReply::Error {
            code: PageHostErrorCode::StaleDocument,
            ..
        }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::ResolveDebuggerEmittedSafePoint {
            tab_id: 7,
            document_generation: 1,
            program: programs[0],
            generated_line: relocated_line as u32,
            generated_column_utf16: relocated_column as u32,
        }),
        PageHostReply::Error {
            code: PageHostErrorCode::StaleDocument,
            ..
        }
    ));
}

#[test]
fn page_host_executes_bluets_emitted_decorator_artifact_as_javascript() {
    let fixture = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../bluets/tests/fixtures/k11_source_maps/decorated.ts"
    ));
    // The page-host test realm has no console sink; the decorator and its
    // relocation stay intact while the result is checked by a later script.
    let source = fixture.replace(
        "console.log(new Example().method());",
        "globalThis.decoratorResult = new Example().method();",
    );
    assert_ne!(source, fixture);
    let module = "memory:///k11-decorator.ts";
    let compilation = blueice_bluets::compile(
        module,
        &blueice_bluets::MapLoader::from([blueice_bluets::ModuleSource::new(
            module,
            source.clone(),
        )]),
        blueice_bluets::CompilerOptions {
            source_map: true,
            inline_sources: true,
            ..blueice_bluets::CompilerOptions::default()
        },
    );
    assert!(!compilation.has_errors(), "{:#?}", compilation.diagnostics);
    let output = compilation.output.unwrap();
    let artifact = &output.artifacts[module];
    let javascript = &artifact.javascript;
    let relocated = javascript.find("_method_decorators = [decoSite]").unwrap()
        + "_method_decorators = [".len();
    let source_position = |text: &str, offset: usize| {
        let prefix = &text[..offset];
        (
            prefix.bytes().filter(|byte| *byte == b'\n').count(),
            prefix.rsplit('\n').next().unwrap().encode_utf16().count(),
        )
    };
    let map = artifact.source_map.as_ref().unwrap();
    let lookup = |offset| {
        let (line, column) = source_position(javascript, offset);
        map.original_position_at(javascript, line, column, 100_000, 16_000_000)
            .unwrap()
    };
    let (line, column_utf16) = source_position(&source, source.find("@decoSite").unwrap() + 1);
    assert_eq!(
        lookup(relocated),
        Some(blueice_bluets::SourceMapOriginalPosition {
            source_index: 0,
            line,
            column_utf16,
            name_index: None,
        })
    );
    assert_eq!(
        lookup(javascript.find("var __bluetsEsDecorate").unwrap()),
        None,
        "the generated helper body must not claim a source location"
    );

    let mut host = BlueJsChildHost::default();
    let reply = host.handle_request(PageHostRequest::SynchronizeDocument {
        document: document(
            1,
            vec![
                classic(0, javascript),
                classic(
                    1,
                    "if (globalThis.decoratorResult !== 42) throw 'decorator result';",
                ),
            ],
        ),
    });
    let PageHostReply::Synchronized { reports, .. } = reply else {
        panic!("expected synchronized reply");
    };
    assert_eq!(reports.len(), 2);
    assert_eq!(reports[0].outcome, PageHostScriptOutcome::Executed);
    assert_eq!(reports[1].outcome, PageHostScriptOutcome::Executed);
}

#[test]
fn page_host_executes_legacy_parameter_metadata_helpers_from_emitted_javascript() {
    let fixture = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../bluets/tests/fixtures/k11_source_maps/legacy_parameter_metadata.ts"
    ));
    let source = fixture
        .replace("console.log(\"method\");", "")
        .replace("console.log(\"parameter\", index);", "")
        .replace(
            "console.log(new Example().method(41));",
            "globalThis.legacyParameterResult = new Example().method(41);",
        );
    assert_ne!(source, fixture);
    let module = "memory:///k11-legacy-parameter.ts";
    let compilation = blueice_bluets::compile(
        module,
        &blueice_bluets::MapLoader::from([blueice_bluets::ModuleSource::new(
            module,
            source.clone(),
        )]),
        blueice_bluets::CompilerOptions {
            experimental_decorators: true,
            emit_decorator_metadata: true,
            source_map: true,
            inline_sources: true,
            ..blueice_bluets::CompilerOptions::default()
        },
    );
    assert!(!compilation.has_errors(), "{:#?}", compilation.diagnostics);
    let output = compilation.output.unwrap();
    let artifact = &output.artifacts[module];
    let javascript = &artifact.javascript;
    let map = artifact.source_map.as_ref().unwrap();
    let original_token = source.find("@parameterDecor").unwrap() + 1;
    let generated_token = javascript.find("__bluetsParam(0, parameterDecor)").unwrap();
    let position = |text: &str, byte: usize| {
        let prefix = &text[..byte];
        (
            prefix.bytes().filter(|byte| *byte == b'\n').count(),
            prefix.rsplit('\n').next().unwrap().encode_utf16().count(),
        )
    };
    let (original_line, original_column) = position(&source, original_token);
    let mapped_at = |byte| {
        let (line, column) = position(javascript, byte);
        map.original_position_at(javascript, line, column, 200_000, 16 * 1024 * 1024)
            .unwrap()
            .map(|position| (position.source_index, position.line, position.column_utf16))
    };
    assert_eq!(
        mapped_at(generated_token),
        Some((0, original_line, original_column))
    );
    let decorate_call = javascript.find("__bluetsDecorate([").unwrap();
    let (method_line, method_column) = position(&source, source.find("method(@").unwrap());
    assert_eq!(
        mapped_at(decorate_call),
        Some((0, method_line, method_column))
    );
    let metadata_call = javascript.find("__bluetsMetadata(\"design:type\"").unwrap();
    assert_eq!(mapped_at(metadata_call), None);
    let helper_body = javascript.find("var __bluetsParam").unwrap();
    assert_eq!(
        mapped_at(helper_body),
        None,
        "the generated parameter helper body must not claim user source"
    );
    assert_eq!(
        mapped_at(javascript.find("var __bluetsMetadata").unwrap()),
        None
    );
    let mut host = BlueJsChildHost::default();
    let reply = host.handle_request(PageHostRequest::SynchronizeDocument {
        document: document(
            1,
            vec![
                classic(0, javascript),
                classic(
                    1,
                    "if (globalThis.legacyParameterResult !== 42) throw 'legacy parameter result';",
                ),
            ],
        ),
    });
    let PageHostReply::Synchronized { reports, .. } = reply else {
        panic!("expected synchronized reply");
    };
    assert_eq!(reports.len(), 2);
    assert_eq!(reports[0].outcome, PageHostScriptOutcome::Executed);
    assert_eq!(reports[1].outcome, PageHostScriptOutcome::Executed);
}

#[test]
fn child_emitted_legacy_profile_is_explicit_and_standard_rejects_parameter_decorators() {
    let fixture = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../bluets/tests/fixtures/k11_source_maps/legacy_parameter_metadata.ts"
    ));
    let source = fixture
        .replace("console.log(\"method\");", "")
        .replace("console.log(\"parameter\", index);", "")
        .replace(
            "console.log(new Example().method(41));",
            "globalThis.legacyParameterResult = new Example().method(41);",
        );
    let scripts = || {
        vec![
            blue_ts_emitted_classic(0, &source),
            classic(
                1,
                "if (globalThis.legacyParameterResult !== 42) throw 'legacy parameter result';",
            ),
        ]
    };
    let mut standard = BlueJsChildHost::default();
    let PageHostReply::Synchronized {
        reports: standard_reports,
        ..
    } = standard.handle_request(PageHostRequest::SynchronizeDocument {
        document: document(1, scripts()),
    })
    else {
        panic!("standard profile must return source-free reports")
    };
    assert!(matches!(
        standard_reports[0].outcome,
        PageHostScriptOutcome::Rejected { .. }
    ));

    let mut selected = document(1, scripts());
    selected.emitted_bluets_profile = PageHostBlueTsEmissionProfile::LegacyDecoratorsWithMetadata;
    let mut host = BlueJsChildHost::default();
    let PageHostReply::Synchronized { reports, .. } =
        host.handle_request(PageHostRequest::SynchronizeDocument { document: selected })
    else {
        panic!("owner-selected legacy profile must return source-free reports")
    };
    assert_eq!(reports.len(), 2);
    assert_eq!(reports[0].outcome, PageHostScriptOutcome::Executed);
    assert_eq!(reports[1].outcome, PageHostScriptOutcome::Executed);
}

#[test]
fn child_legacy_class_member_profile_does_not_force_metadata_annotations() {
    let fixture = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../bluets/tests/fixtures/k11_source_maps/legacy_decorated.ts"
    ));
    let source = fixture
        .replace("console.log(\"class\");", "")
        .replace("console.log(\"method\");", "")
        .replace(
            "console.log(new Example().method());",
            "globalThis.legacyClassMemberResult = new Example().method();",
        );
    let scripts = || {
        vec![
            blue_ts_emitted_classic(0, &source),
            classic(
                1,
                "if (globalThis.legacyClassMemberResult !== 42) throw 'legacy class/member result';",
            ),
        ]
    };
    let mut metadata_document = document(1, scripts());
    metadata_document.emitted_bluets_profile =
        PageHostBlueTsEmissionProfile::LegacyDecoratorsWithMetadata;
    let mut metadata_host = BlueJsChildHost::default();
    let PageHostReply::Synchronized {
        reports: metadata_reports,
        ..
    } = metadata_host.handle_request(PageHostRequest::SynchronizeDocument {
        document: metadata_document,
    })
    else {
        panic!("metadata profile must return source-free reports")
    };
    assert!(matches!(
        metadata_reports[0].outcome,
        PageHostScriptOutcome::Rejected { .. }
    ));

    let mut legacy_document = document(1, scripts());
    legacy_document.emitted_bluets_profile = PageHostBlueTsEmissionProfile::LegacyDecorators;
    let mut legacy_host = BlueJsChildHost::default();
    let PageHostReply::Synchronized { reports, .. } =
        legacy_host.handle_request(PageHostRequest::SynchronizeDocument {
            document: legacy_document,
        })
    else {
        panic!("legacy class/member profile must return source-free reports")
    };
    assert_eq!(reports.len(), 2);
    assert_eq!(reports[0].outcome, PageHostScriptOutcome::Executed);
    assert_eq!(reports[1].outcome, PageHostScriptOutcome::Executed);
}

#[test]
fn child_es5_profile_executes_object_spread_helper_from_exact_emitted_artifact() {
    let fixture = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../bluets/tests/fixtures/k11_source_maps/object_spread_helper.ts"
    ));
    let source = fixture.replace(
        "console.log(merged.a + merged.b);",
        "if (merged.a + merged.b !== 3) throw 'ES5 object spread result';",
    );
    let module = "memory:///k11-object-spread.ts";
    let compilation = blueice_bluets::compile(
        module,
        &blueice_bluets::MapLoader::from([blueice_bluets::ModuleSource::new(
            module,
            source.clone(),
        )]),
        blueice_bluets::CompilerOptions {
            target: blueice_bluets::EcmaTarget::Es5,
            source_map: true,
            inline_sources: true,
            ..Default::default()
        },
    );
    assert!(!compilation.has_errors(), "{:#?}", compilation.diagnostics);
    let output = compilation.output.unwrap();
    let generated = &output.artifacts[module].javascript;
    assert!(generated.contains("function __blueice_target_object_properties_"));
    assert!(generated.contains("var merged = __blueice_target_object_properties_"));
    let mut selected = document(1, vec![blue_ts_emitted_classic(0, &source)]);
    selected.emitted_bluets_profile = PageHostBlueTsEmissionProfile::Es5;
    let mut host = BlueJsChildHost::default();
    let PageHostReply::Synchronized { reports, .. } =
        host.handle_request(PageHostRequest::SynchronizeDocument { document: selected })
    else {
        panic!("ES5 profile must return source-free reports")
    };
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].outcome, PageHostScriptOutcome::Executed);
}

const SYSTEM_PAGE_LOADER: &str = r#"
    var console = { log: function(value) {
        if (value !== 42) throw 'System console result';
    } };
    var System = { register: function(dependencies, declare) {
        if (dependencies.length !== 0) throw 'unexpected System dependency';
        var exports = {};
        var body = declare(function(name, value) {
            exports[name] = value;
            return value;
        }, { id: 'main' });
        if (body.setters.length !== 0) throw 'unexpected System setter';
        body.execute();
        if (exports.answer !== 42) throw 'System export result';
    } };
"#;

#[test]
fn page_host_executes_system_wrapper_with_page_supplied_loader() {
    let fixture = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../bluets/tests/fixtures/k11_source_maps/system_wrapper.ts"
    ));
    let source = fixture;
    let module = "memory:///k11-system.ts";
    let compilation = blueice_bluets::compile(
        module,
        &blueice_bluets::MapLoader::from([blueice_bluets::ModuleSource::new(module, source)]),
        blueice_bluets::CompilerOptions {
            module_kind: blueice_bluets::ModuleKind::System,
            source_map: true,
            inline_sources: true,
            ..Default::default()
        },
    );
    assert!(!compilation.has_errors(), "{:#?}", compilation.diagnostics);
    let output = compilation.output.unwrap();
    let generated = &output.artifacts[module].javascript;
    assert!(generated.contains("System.register"));
    let mut host = BlueJsChildHost::default();
    let PageHostReply::Synchronized { reports, .. } =
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: document(
                1,
                vec![classic(0, SYSTEM_PAGE_LOADER), classic(1, generated)],
            ),
        })
    else {
        panic!("System wrapper must return source-free reports")
    };
    assert_eq!(reports.len(), 2);
    assert_eq!(reports[0].outcome, PageHostScriptOutcome::Executed);
    assert_eq!(reports[1].outcome, PageHostScriptOutcome::Executed);
}

#[test]
fn child_owner_selected_system_profile_executes_with_page_loader() {
    let fixture = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../bluets/tests/fixtures/k11_source_maps/system_wrapper.ts"
    ));
    let source = fixture;
    let script = blue_ts_emitted_classic(1, source);
    let graph = script.graph.clone();
    let options = emitted_bluets_compiler_options(
        &graph,
        PageDomProfile::Snapshot,
        PageHostBlueTsEmissionProfile::System,
    )
    .unwrap();
    let loader = bluets_loader(&graph, validate_graph(&graph).unwrap()).unwrap();
    let compilation = blueice_bluets::compile(&graph.entry, &loader, options);
    assert!(!compilation.has_errors(), "{:#?}", compilation.diagnostics);
    let output = compilation.output.unwrap();
    let artifact = &output.artifacts[&graph.entry];
    let mut selected = document(1, vec![classic(0, SYSTEM_PAGE_LOADER), script]);
    selected.emitted_bluets_profile = PageHostBlueTsEmissionProfile::System;
    let mut host = BlueJsChildHost::default();
    let PageHostReply::Synchronized { reports, .. } =
        host.handle_request(PageHostRequest::SynchronizeDocument { document: selected })
    else {
        panic!("owner-selected System profile must return source-free reports")
    };
    assert_eq!(reports.len(), 2);
    assert_eq!(reports[0].outcome, PageHostScriptOutcome::Executed);
    assert_eq!(reports[1].outcome, PageHostScriptOutcome::Executed);
    let mut selected = debugger_document(
        1,
        vec![
            classic(0, SYSTEM_PAGE_LOADER),
            blue_ts_emitted_classic(1, source),
        ],
    );
    selected.emitted_bluets_profile = PageHostBlueTsEmissionProfile::System;
    let mut debugger_host = BlueJsChildHost::default();
    assert!(matches!(
        debugger_host.handle_request(PageHostRequest::SynchronizeDocument { document: selected }),
        PageHostReply::Synchronized { .. }
    ));
    let programs = match debugger_host.handle_request(PageHostRequest::ListDebuggerPrograms {
        tab_id: 7,
        document_generation: 1,
    }) {
        PageHostReply::DebuggerPrograms { programs, .. } => programs,
        reply => panic!("expected installed loader and System programs: {reply:?}"),
    };
    assert_eq!(programs.len(), 2);
    let document = &debugger_host.documents[&7];
    let maps = &document.emitted_maps;
    assert_eq!(maps.len(), 1);
    let (program, bound) = maps.iter().next().unwrap();
    assert!(programs.contains(program));
    let runtime_handle = document
        .debugger_programs
        .get(&program.program_handle)
        .unwrap()
        .runtime_handle;
    let installed = debugger_host
        .runtime
        .program_registry()
        .get(runtime_handle)
        .unwrap();
    assert_eq!(
        installed.source().source_hash(),
        bound.generated.source_hash
    );
    assert_eq!(
        bound.emission_profile,
        PageHostBlueTsEmissionProfile::System
    );
    assert_eq!(bound.generated.source, artifact.javascript);
    assert_eq!(
        bound.generated.source_hash,
        page_host::source_hash(&artifact.javascript)
    );
    assert_eq!(bound.map, artifact.source_map.clone().unwrap());
    assert_eq!(bound.map.sources_content, vec![source.to_string()]);
}

#[test]
fn system_page_loader_console_typing_is_profile_scoped_and_source_separate() {
    let fixture = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../bluets/tests/fixtures/k11_source_maps/system_wrapper.ts"
    ));
    let script = blue_ts_emitted_classic(1, fixture);
    let system = emitted_bluets_compiler_options(
        &script.graph,
        PageDomProfile::Snapshot,
        PageHostBlueTsEmissionProfile::System,
    )
    .unwrap();
    let standard = emitted_bluets_compiler_options(
        &script.graph,
        PageDomProfile::Snapshot,
        PageHostBlueTsEmissionProfile::Standard,
    )
    .unwrap();
    assert_eq!(system.module_kind, ModuleKind::System);
    assert!(system.ambient_declaration_modules.iter().any(|module| {
        module.id.ends_with("system-loader-console.d.ts")
            && module.text == "declare const console: { log(value: number): void };\n"
    }));
    assert!(standard
        .ambient_declaration_modules
        .iter()
        .all(|module| !module.id.ends_with("system-loader-console.d.ts")));
    assert_eq!(script.graph.modules[0].source, fixture);
}

#[test]
fn system_console_typing_does_not_install_a_runtime_loader() {
    let fixture = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../bluets/tests/fixtures/k11_source_maps/system_wrapper.ts"
    ));
    let mut selected = document(1, vec![blue_ts_emitted_classic(0, fixture)]);
    selected.emitted_bluets_profile = PageHostBlueTsEmissionProfile::System;
    let mut host = BlueJsChildHost::default();
    let PageHostReply::Synchronized { reports, .. } =
        host.handle_request(PageHostRequest::SynchronizeDocument { document: selected })
    else {
        panic!("System profile must return source-free reports")
    };
    assert_eq!(reports.len(), 1);
    assert!(matches!(
        reports[0].outcome,
        PageHostScriptOutcome::Rejected { .. }
    ));
}

#[test]
fn module_page_console_typing_is_owner_scoped_and_source_separate() {
    let fixture = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../bluets/tests/fixtures/k11_source_maps/basic.ts"
    ));
    let module_id = "blueice://page/k11-primitive.ts";
    let graph = graph(module_id, vec![PageHostSource::new(module_id, fixture)]);
    let selected = emitted_bluets_compiler_options(
        &graph,
        PageDomProfile::Snapshot,
        PageHostBlueTsEmissionProfile::ModulePageConsole,
    )
    .unwrap();
    let standard = emitted_bluets_compiler_options(
        &graph,
        PageDomProfile::Snapshot,
        PageHostBlueTsEmissionProfile::Standard,
    )
    .unwrap();
    assert_eq!(selected.module_kind, standard.module_kind);
    assert!(selected.ambient_declaration_modules.iter().any(|module| {
        module.id.ends_with("module-page-console.d.ts")
            && module.text == "declare const console: { log(...values: unknown[]): void };\n"
    }));
    assert!(standard
        .ambient_declaration_modules
        .iter()
        .all(|module| !module.id.ends_with("module-page-console.d.ts")));
    assert_eq!(graph.modules[0].source, fixture);
}

#[test]
fn frozen_basic_erasure_and_astral_modules_execute_with_page_console() {
    for (name, fixture, expected) in [
        (
            "basic",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../bluets/tests/fixtures/k11_source_maps/basic.ts"
            )),
            3,
        ),
        (
            "erased",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../bluets/tests/fixtures/k11_source_maps/erased.ts"
            )),
            42,
        ),
        (
            "astral",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../bluets/tests/fixtures/k11_source_maps/crlf_astral.ts"
            )),
            2,
        ),
    ] {
        let loader = format!(
            "var console = {{ log: function(value) {{ if (value !== {expected}) throw '{name} result'; }} }};"
        );
        let module_id = "blueice://page/k11-primitive.ts";
        let script = PageHostScript {
            ordinal: 1,
            language: PageHostScriptLanguage::BlueTsEmitted,
            kind: PageHostScriptKind::Module,
            graph: graph(module_id, vec![PageHostSource::new(module_id, fixture)]),
        };
        let frozen_compilation = blueice_bluets::compile(
            module_id,
            &blueice_bluets::MapLoader::from([blueice_bluets::ModuleSource::new(
                module_id, fixture,
            )]),
            blueice_bluets::CompilerOptions {
                source_map: true,
                inline_sources: true,
                ..Default::default()
            },
        );
        assert!(
            !frozen_compilation.has_errors(),
            "{name}: {:#?}",
            frozen_compilation.diagnostics
        );
        let frozen_output = frozen_compilation.output.unwrap();
        let artifact = &frozen_output.artifacts[module_id];
        let mut selected = document(1, vec![classic(0, &loader), script.clone()]);
        selected.emitted_module_page_console_ambient = true;
        let mut host = BlueJsChildHost::default();
        let reply =
            host.handle_request(PageHostRequest::SynchronizeDocument { document: selected });
        assert!(
            matches!(
                reply,
                PageHostReply::Synchronized { reports, .. }
                    if reports.len() == 2
                        && reports.iter().all(|report| report.outcome == PageHostScriptOutcome::Executed)
            ),
            "{name} emitted module did not execute"
        );
        let mut pending = debugger_document(1, vec![classic(0, &loader), script.clone()]);
        pending.emitted_module_page_console_ambient = true;
        let mut debugger_host = BlueJsChildHost::default();
        assert!(matches!(
            debugger_host.handle_request(PageHostRequest::SynchronizeDocument { document: pending }),
            PageHostReply::Synchronized { reports, .. } if reports.is_empty()
        ));
        let live = &debugger_host.documents[&7];
        assert_eq!(live.emitted_maps.len(), 1, "{name}");
        let (program, bound) = live.emitted_maps.iter().next().unwrap();
        assert_eq!(
            bound.emission_profile,
            PageHostBlueTsEmissionProfile::ModulePageConsole
        );
        let runtime_handle = live.debugger_programs[&program.program_handle].runtime_handle;
        let installed = debugger_host
            .runtime
            .program_registry()
            .get(runtime_handle)
            .unwrap();
        assert_eq!(
            installed.source().source_hash(),
            bound.generated.source_hash,
            "{name}"
        );
        assert_eq!(bound.generated.source, artifact.javascript, "{name}");
        assert_eq!(bound.map, artifact.source_map.clone().unwrap(), "{name}");
        assert_eq!(
            bound.map.sources_content,
            vec![fixture.to_string()],
            "{name}"
        );
        let mut missing_loader = document(1, vec![script]);
        missing_loader.emitted_module_page_console_ambient = true;
        let mut no_console_host = BlueJsChildHost::default();
        assert!(
            matches!(
                no_console_host.handle_request(PageHostRequest::SynchronizeDocument {
                    document: missing_loader,
                }),
                PageHostReply::Synchronized { reports, .. }
                    if reports.len() == 1
                        && matches!(reports[0].outcome, PageHostScriptOutcome::Rejected { .. })
            ),
            "{name} ambient typing must not install a runtime console"
        );
    }
}

#[test]
fn emitted_exported_module_entry_can_arm_while_page_loader_is_pending() {
    let module_id = "blueice://page/k11-exported-entry.ts";
    let source = format!(
        "{}\ndeclare const console: {{ log(value: number): void }};\n",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../bluets/tests/fixtures/k11_source_maps/basic.ts"
        ))
    );
    let script = PageHostScript {
        ordinal: 1,
        language: PageHostScriptLanguage::BlueTsEmitted,
        kind: PageHostScriptKind::Module,
        graph: graph(module_id, vec![PageHostSource::new(module_id, source)]),
    };
    let mut host = BlueJsChildHost::default();
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: debugger_document(1, vec![classic(0, "var console = { log: function(value) {} };"), script]),
        }),
        PageHostReply::Synchronized { reports, .. } if reports.is_empty()
    ));
    let document = &host.documents[&7];
    let program = *document.emitted_maps.keys().next().unwrap();
    let generated = &document.emitted_maps[&program].generated.source;
    let byte = generated.find("export const alpha").unwrap();
    let prefix = &generated[..byte];
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count() as u32;
    let column = prefix.rsplit('\n').next().unwrap().encode_utf16().count() as u32;
    let entry_handle = document.debugger_programs[&program.program_handle].runtime_handle;
    let entry = host
        .runtime
        .module_evaluate_entry_safe_point(7, entry_handle)
        .unwrap();
    let installed = host.runtime.program_registry().get(entry_handle).unwrap();
    let declaration = installed
        .ast_nodes()
        .iter()
        .find(|node| node.is_top_level_statement())
        .and_then(|node| installed.safe_point_for_ast_node(node.id()).ok())
        .unwrap();
    assert_eq!(declaration.bytecode_offset, entry.bytecode_offset);
    let expected = PageHostDebuggerSafePoint {
        program,
        code_unit_ordinal: 0,
        bytecode_offset: entry.bytecode_offset,
    };
    assert_eq!(
        host.handle_request(PageHostRequest::ResolveDebuggerEmittedSafePoint {
            tab_id: 7,
            document_generation: 1,
            program,
            generated_line: line,
            generated_column_utf16: column,
        }),
        PageHostReply::DebuggerEmittedSafePoint {
            tab_id: 7,
            document_generation: 1,
            program,
            generated_line: line,
            generated_column_utf16: column,
            safe_point: Some(expected),
        }
    );
    assert_eq!(
        host.handle_request(PageHostRequest::ArmDebuggerRootSafePointBreakpoint {
            tab_id: 7,
            document_generation: 1,
            safe_point: expected,
        }),
        PageHostReply::DebuggerRootSafePointBreakpointArmed {
            tab_id: 7,
            document_generation: 1,
            safe_point: expected,
        }
    );
}

#[test]
fn erased_module_entry_prefers_its_exact_callee_over_the_statement_boundary() {
    let module_id = "blueice://page/k11-erased-entry.ts";
    let source = format!(
        "{}\ndeclare const console: {{ log(value: number): void }};\n",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../bluets/tests/fixtures/k11_source_maps/erased.ts"
        ))
    );
    let script = PageHostScript {
        ordinal: 0,
        language: PageHostScriptLanguage::BlueTsEmitted,
        kind: PageHostScriptKind::Module,
        graph: graph(module_id, vec![PageHostSource::new(module_id, source)]),
    };
    let mut host = BlueJsChildHost::default();
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: debugger_document(1, vec![script]),
        }),
        PageHostReply::Synchronized { reports, .. } if reports.is_empty()
    ));
    let document = &host.documents[&7];
    let program = *document.emitted_maps.keys().next().unwrap();
    let bound = &document.emitted_maps[&program];
    let record = &document.debugger_programs[&program.program_handle];
    let installed = host
        .runtime
        .program_registry()
        .get(record.runtime_handle)
        .unwrap();
    let entry = host
        .runtime
        .module_evaluate_entry_safe_point(7, record.runtime_handle)
        .unwrap();
    let candidates = installed
        .ast_nodes()
        .iter()
        .filter(|node| node.is_top_level_statement())
        .zip(&bound.top_level_spans)
        .enumerate()
        .filter_map(|(index, (node, span))| {
            let point = installed.safe_point_for_ast_node(node.id()).ok()?;
            (point.bytecode_offset == entry.bytecode_offset).then(|| {
                (
                    index,
                    span.start_byte,
                    bound.generated.source[span.start_byte..span.end_byte]
                        .chars()
                        .take(40)
                        .collect::<String>(),
                    bound.non_evaluating_top_level_statements.contains(&index),
                )
            })
        })
        .collect::<Vec<_>>();
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].0, 1);
    assert!(candidates[0].2.starts_with("export const result"));
    assert!(!candidates[0].3);
    assert!(bound.non_evaluating_top_level_statements.contains(&0));
    assert_eq!(
        installed.unique_identifier_at_safe_point(entry),
        Some((1, "double"))
    );
}

#[test]
fn system_classic_profile_does_not_label_emitted_module_maps() {
    let module_id = "blueice://page/mixed-emitted-module.ts";
    let module = PageHostScript {
        ordinal: 1,
        language: PageHostScriptLanguage::BlueTsEmitted,
        kind: PageHostScriptKind::Module,
        graph: graph(
            module_id,
            vec![PageHostSource::new(
                module_id,
                "export const moduleAnswer: number = 42;",
            )],
        ),
    };
    let mut selected = debugger_document(
        1,
        vec![
            blue_ts_emitted_classic(0, "export const classicAnswer: number = 42;"),
            module,
        ],
    );
    selected.emitted_bluets_profile = PageHostBlueTsEmissionProfile::System;
    let mut host = BlueJsChildHost::default();
    let reply = host.handle_request(PageHostRequest::SynchronizeDocument { document: selected });
    assert!(
        matches!(
            &reply,
            PageHostReply::Synchronized { reports, .. } if reports.is_empty()
        ),
        "{reply:?}"
    );
    let maps = &host.documents[&7].emitted_maps;
    assert_eq!(maps.len(), 2);
    assert_eq!(
        maps.values()
            .filter(|map| map.emission_profile == PageHostBlueTsEmissionProfile::System)
            .count(),
        1
    );
    assert_eq!(
        maps.values()
            .filter(|map| map.emission_profile == PageHostBlueTsEmissionProfile::Standard)
            .count(),
        1
    );
    let system_program = *maps
        .iter()
        .find(|(_, map)| map.emission_profile == PageHostBlueTsEmissionProfile::System)
        .unwrap()
        .0;
    let raw_target = match host.handle_request(PageHostRequest::ListDebuggerSafePoints {
        tab_id: 7,
        document_generation: 1,
        program: system_program,
    }) {
        PageHostReply::DebuggerSafePoints { safe_points, .. } => safe_points
            .into_iter()
            .find(|point| point.code_unit_ordinal != 0)
            .expect("System execute body must have a nested safe point"),
        reply => panic!("System safe-point inventory failed: {reply:?}"),
    };
    assert!(matches!(
        host.handle_request(PageHostRequest::ArmDebuggerNestedSafePointBreakpoint {
            tab_id: 7,
            document_generation: 1,
            safe_point: raw_target,
        }),
        PageHostReply::Error {
            code: PageHostErrorCode::InvalidDebuggerState,
            ..
        }
    ));
    assert!(host.documents[&7].debugger_breakpoints.is_empty());
}

#[test]
fn executes_authorized_classic_and_rejects_a_later_parse_failure_independently() {
    let mut host = BlueJsChildHost::default();
    let reply = host.handle_request(PageHostRequest::SynchronizeDocument {
        document: document(
            1,
            vec![
                classic(0, "globalThis.answer = 42;"),
                classic(1, "const = ;"),
            ],
        ),
    });
    let PageHostReply::Synchronized { reports, .. } = reply else {
        panic!("expected synchronized reply");
    };
    assert_eq!(reports.len(), 2);
    assert_eq!(reports[0].outcome, PageHostScriptOutcome::Executed);
    assert!(matches!(
        reports[1].outcome,
        PageHostScriptOutcome::Rejected { .. }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::GetRealmStats {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::RealmStats(PageHostRealmStats {
            program_count: 1,
            ..
        })
    ));
}

#[test]
fn direct_bluets_and_javascript_execute_in_document_order_in_one_realm() {
    let mut host = BlueJsChildHost::default();
    let reply = host.handle_request(PageHostRequest::SynchronizeDocument {
        document: document(
            1,
            vec![
                classic(0, "globalThis.beforeBlueTs = true;"),
                // The TypeScript annotation makes this invalid JavaScript
                // source. Success therefore proves the child used direct
                // BlueTS lowering rather than emitted-JavaScript reparse.
                blue_ts_classic(1, "const sharedAnswer: number = 42;"),
                classic(
                    2,
                    "if (!globalThis.beforeBlueTs || sharedAnswer !== 42) throw 'realm/order failed';",
                ),
            ],
        ),
    });
    let PageHostReply::Synchronized { reports, .. } = reply else {
        panic!("expected synchronized reply");
    };
    assert_eq!(
        reports
            .iter()
            .map(|report| (report.ordinal, report.language, &report.outcome))
            .collect::<Vec<_>>(),
        vec![
            (
                0,
                PageHostScriptLanguage::JavaScript,
                &PageHostScriptOutcome::Executed
            ),
            (
                1,
                PageHostScriptLanguage::BlueTs,
                &PageHostScriptOutcome::Executed
            ),
            (
                2,
                PageHostScriptLanguage::JavaScript,
                &PageHostScriptOutcome::Executed
            ),
        ]
    );
    assert!(matches!(
        host.handle_request(PageHostRequest::GetRealmStats {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::RealmStats(PageHostRealmStats {
            program_count: 3,
            ..
        })
    ));
}

#[test]
fn child_installs_only_fixed_core_snapshot_callbacks_for_javascript() {
    let mut host = BlueJsChildHost::default();
    let reply = host.handle_request(PageHostRequest::SynchronizeDocument {
        document: document_with_snapshot(
            1,
            "private document snapshot".to_string(),
            "https://example.test".to_string(),
            vec![
                classic(
                    0,
                    concat!(
                        "if (blueiceDocumentText() !== 'private document snapshot') throw 'text';",
                        "if (blueiceDocumentOrigin() !== 'https://example.test') throw 'origin';",
                        "if (typeof document !== 'undefined' || typeof fetch !== 'undefined' || typeof blueiceTestHasElementById !== 'undefined' || typeof blueiceTestGetElementById !== 'undefined') throw 'ambient';"
                    ),
                ),
                classic(1, "blueiceDocumentText(1);"),
            ],
        ),
    });
    let PageHostReply::Synchronized { reports, .. } = reply else {
        panic!("expected synchronized reply");
    };
    assert_eq!(reports.len(), 2);
    assert_eq!(reports[0].outcome, PageHostScriptOutcome::Executed);
    assert!(matches!(
        reports[1].outcome,
        PageHostScriptOutcome::Rejected { .. }
    ));
    // Result records remain source/value-free even when a callback
    // returned a private snapshot inside the child VM.
    assert!(!format!("{:?}", reports).contains("private document snapshot"));
}

#[test]
fn snapshot_validation_happens_before_realm_creation_or_replacement() {
    let mut host = BlueJsChildHost::default();
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: document_with_snapshot(
                1,
                "snapshot".to_string(),
                "HTTP://EXAMPLE.test".to_string(),
                vec![classic(0, "globalThis.answer = 42;")],
            ),
        }),
        PageHostReply::Error {
            code: PageHostErrorCode::InvalidRequest,
            ..
        }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::GetRealmStats {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::Error {
            code: PageHostErrorCode::UnknownRealm,
            ..
        }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: document_with_snapshot(
                1,
                "x".repeat(PAGE_HOST_DOCUMENT_TEXT_MAX_BYTES + 1),
                "https://example.test".to_string(),
                vec![classic(0, "globalThis.answer = 42;")],
            ),
        }),
        PageHostReply::Error {
            code: PageHostErrorCode::ResourceLimit,
            ..
        }
    ));
}

#[test]
fn bluets_uses_the_verified_snapshot_profile_and_rejects_untyped_globals() {
    let mut host = BlueJsChildHost::default();
    let reply = host.handle_request(PageHostRequest::SynchronizeDocument {
        document: document(
            1,
            vec![
                // The annotation is invalid JavaScript syntax, so this
                // success also preserves direct BlueTS-to-BlueJS lowering.
                blue_ts_classic(
                    0,
                    concat!(
                        "const snapshotText: string = blueiceDocumentText();",
                        "const snapshotOrigin: string = blueiceDocumentOrigin();"
                    ),
                ),
                classic(
                    1,
                    concat!(
                        "if (snapshotText !== 'test document snapshot') throw 'text';",
                        "if (snapshotOrigin !== 'https://example.test') throw 'origin';"
                    ),
                ),
                // The fixed declaration contains neither fetch nor a
                // general document object. All three compile attempts fail
                // before a program is admitted to the child realm.
                blue_ts_classic(2, "fetch('https://example.test/');"),
                blue_ts_classic(3, "blueiceDocumentText(1);"),
                blue_ts_classic(4, "document.getElementById('target');"),
            ],
        ),
    });
    assert!(matches!(
        reply,
        PageHostReply::Synchronized {
            reports,
            ..
        } if reports.len() == 5
            && reports[0].language == PageHostScriptLanguage::BlueTs
            && reports[0].outcome == PageHostScriptOutcome::Executed
            && reports[1].language == PageHostScriptLanguage::JavaScript
            && reports[1].outcome == PageHostScriptOutcome::Executed
            && matches!(reports[2].outcome, PageHostScriptOutcome::Rejected { .. })
            && matches!(reports[3].outcome, PageHostScriptOutcome::Rejected { .. })
            && matches!(reports[4].outcome, PageHostScriptOutcome::Rejected { .. })
    ));
}

#[test]
fn bluets_compiler_positions_remain_private_candidates_in_normal_and_deferred_reports() {
    let source = "const broken: number = \"wrong\";";
    let mut host = BlueJsChildHost::default();
    for (generation, debugger_control) in [(1, false), (2, true)] {
        let mut request_document = document(generation, vec![blue_ts_classic(0, source)]);
        request_document.debugger_execution_control = debugger_control;
        let reply = host.handle_request(PageHostRequest::SynchronizeDocument {
            document: request_document,
        });
        let PageHostReply::Synchronized { reports, .. } = reply else {
            panic!("expected synchronized reply: {reply:?}");
        };
        let [report] = reports.as_slice() else {
            panic!("expected one BlueTS report: {reports:?}");
        };
        assert_eq!(report.document_generation, generation);
        assert!(matches!(
            report.outcome,
            PageHostScriptOutcome::Rejected { .. }
        ));
        let position = report.source_position.as_ref().expect("compiler position");
        assert_eq!(position.module_id, "blueice://page/inline-0.ts");
        assert!(position.start < position.end);
        assert!(position.end as usize <= source.len());
        assert!(report.is_well_formed());
        assert!(!format!("{report:?}").contains("wrong"));
    }
}

#[test]
fn module_graph_uses_only_the_explicit_static_resolution_records() {
    let entry = "blueice://page/entry.js";
    let dependency = "blueice://page/dependency.js";
    let mut module = PageHostScript {
        ordinal: 0,
        language: PageHostScriptLanguage::JavaScript,
        kind: PageHostScriptKind::Module,
        graph: graph(
            entry,
            vec![
                PageHostSource::new(
                    entry,
                    "import { answer } from './dependency.js'; export const result = answer;",
                ),
                PageHostSource::new(dependency, "export const answer = 42;"),
            ],
        ),
    };
    module.graph.resolutions.push(PageHostStaticResolution {
        from_module: entry.to_string(),
        specifier: "./dependency.js".to_string(),
        canonical_target: dependency.to_string(),
    });
    let mut host = BlueJsChildHost::default();
    let reply = host.handle_request(PageHostRequest::SynchronizeDocument {
        document: document(1, vec![module]),
    });
    assert!(matches!(
        reply,
        PageHostReply::Synchronized {
            reports,
            ..
        } if reports[0].outcome == PageHostScriptOutcome::Executed
    ));
}

#[test]
fn direct_bluets_module_graph_uses_only_the_explicit_static_resolution_records() {
    let entry = "blueice://page/entry.ts";
    let dependency = "blueice://page/dependency.ts";
    let mut module = PageHostScript {
        ordinal: 0,
        language: PageHostScriptLanguage::BlueTs,
        kind: PageHostScriptKind::Module,
        graph: graph(
            entry,
            vec![
                PageHostSource::new(
                    entry,
                    "import { answer } from './dependency.ts'; export const result: number = answer;",
                ),
                PageHostSource::new(dependency, "export const answer: number = 42;"),
            ],
        ),
    };
    module.graph.resolutions.push(PageHostStaticResolution {
        from_module: entry.to_string(),
        specifier: "./dependency.ts".to_string(),
        canonical_target: dependency.to_string(),
    });
    let mut host = BlueJsChildHost::default();
    let reply = host.handle_request(PageHostRequest::SynchronizeDocument {
        document: document(1, vec![module]),
    });
    assert!(matches!(
        reply,
        PageHostReply::Synchronized {
            reports,
            ..
        } if matches!(
            reports.as_slice(),
            [PageHostScriptReport {
                language: PageHostScriptLanguage::BlueTs,
                kind: PageHostScriptKind::Module,
                outcome: PageHostScriptOutcome::Executed,
                ..
            }]
        )
    ));
    assert_eq!(host.debug_registry.len(), 2);
    let mut retained_modules: Vec<_> = host
        .documents
        .get(&7)
        .expect("the child realm remains live")
        .debugger_programs
        .values()
        .filter_map(|record| {
            host.debug_registry
                .get(host.runtime.program_registry(), record.runtime_handle)
                .ok()
        })
        .map(|metadata| {
            let sources = &metadata.static_info().sources;
            sources
                .iter()
                .find(|source| source.module == entry || source.module == dependency)
                .expect("each module keeps its own static source metadata")
                .module
                .clone()
        })
        .collect();
    retained_modules.sort();
    assert_eq!(
        retained_modules,
        vec![dependency.to_string(), entry.to_string()]
    );
}

#[test]
fn a_missing_module_resolution_admits_no_partial_graph_programs() {
    let entry = "blueice://page/entry.js";
    let dependency = "blueice://page/dependency.js";
    let script = PageHostScript {
        ordinal: 0,
        language: PageHostScriptLanguage::JavaScript,
        kind: PageHostScriptKind::Module,
        graph: graph(
            entry,
            vec![
                PageHostSource::new(entry, "import './dependency.js';"),
                PageHostSource::new(dependency, "export const answer = 42;"),
            ],
        ),
    };
    let mut host = BlueJsChildHost::default();
    let reply = host.handle_request(PageHostRequest::SynchronizeDocument {
        document: document(1, vec![script]),
    });
    assert!(matches!(
        reply,
        PageHostReply::Synchronized {
            reports,
            ..
        } if matches!(reports[0].outcome, PageHostScriptOutcome::Rejected { .. })
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::GetRealmStats {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::RealmStats(PageHostRealmStats {
            program_count: 0,
            bytecode_bytes: 0,
            ..
        })
    ));
}

#[test]
fn document_generations_are_idempotent_and_cannot_close_a_successor() {
    let mut host = BlueJsChildHost::default();
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: document(1, vec![classic(0, "globalThis.first = true;")]),
        }),
        PageHostReply::Synchronized {
            already_current: false,
            ..
        }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: document(1, vec![classic(0, "throw new Error('must not replay');")]),
        }),
        PageHostReply::Synchronized {
            already_current: true,
            reports,
            ..
        } if reports.is_empty()
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: document(2, vec![classic(0, "globalThis.second = true;")]),
        }),
        PageHostReply::Synchronized {
            already_current: false,
            ..
        }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::CloseRealm {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::Error {
            code: PageHostErrorCode::StaleDocument,
            ..
        }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::GetRealmStats {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::Error {
            code: PageHostErrorCode::StaleDocument,
            ..
        }
    ));
}

#[test]
fn source_hash_tampering_is_rejected_before_realm_program_admission() {
    let mut source = PageHostSource::new("blueice://page/main.js", "globalThis.answer = 42;");
    source.source_hash = "fnv1a64:0000000000000000".to_string();
    let script = PageHostScript {
        ordinal: 0,
        language: PageHostScriptLanguage::JavaScript,
        kind: PageHostScriptKind::Classic,
        graph: graph("blueice://page/main.js", vec![source]),
    };
    let mut host = BlueJsChildHost::default();
    let reply = host.handle_request(PageHostRequest::SynchronizeDocument {
        document: document(1, vec![script]),
    });
    assert!(matches!(
        reply,
        PageHostReply::Synchronized {
            reports,
            ..
        } if matches!(reports[0].outcome, PageHostScriptOutcome::Rejected { .. })
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::GetRealmStats {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::RealmStats(PageHostRealmStats {
            program_count: 0,
            ..
        })
    ));
}

#[test]
fn document_source_budget_rejects_before_realm_replacement() {
    let source = PageHostSource::new(
        "blueice://page/too-large.js",
        "x".repeat(MAX_SOURCE_BYTES_PER_DOCUMENT + 1),
    );
    let script = PageHostScript {
        ordinal: 0,
        language: PageHostScriptLanguage::JavaScript,
        kind: PageHostScriptKind::Classic,
        graph: graph("blueice://page/too-large.js", vec![source]),
    };
    let mut host = BlueJsChildHost::default();
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: document(1, vec![script]),
        }),
        PageHostReply::Error {
            code: PageHostErrorCode::ResourceLimit,
            ..
        }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::GetRealmStats {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::Error {
            code: PageHostErrorCode::UnknownRealm,
            ..
        }
    ));
}

#[test]
fn duplicate_script_ordinals_are_rejected_before_realm_replacement() {
    let mut host = BlueJsChildHost::default();
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: document(
                1,
                vec![
                    classic(0, "globalThis.first = true;"),
                    classic(0, "globalThis.second = true;"),
                ],
            ),
        }),
        PageHostReply::Error {
            code: PageHostErrorCode::InvalidRequest,
            ..
        }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::GetRealmStats {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::Error {
            code: PageHostErrorCode::UnknownRealm,
            ..
        }
    ));
}

#[test]
fn private_socket_is_owner_only() {
    let path = PathBuf::from("/tmp").join(format!(
        "blueice-launcher-owner-only-test-{}.sock",
        std::process::id()
    ));
    let listener = bind_bluejs_host_socket(&path).unwrap();
    let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
    drop(listener);
    let _ = fs::remove_file(path);
}
