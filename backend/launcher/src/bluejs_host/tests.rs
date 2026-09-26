// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;
mod debugger_control;
mod dom_transport;
mod exception_sites;
mod metadata_inventory;
mod metadata_roots;
mod nested_scheduler;
mod resource_accounting;
mod source_spans;
mod static_scope_values;

fn graph(entry: &str, modules: Vec<PageHostSource>) -> PageHostModuleGraph {
    PageHostModuleGraph {
        entry: entry.to_string(),
        modules,
        resolutions: Vec::new(),
        resolver_fingerprint: "core-page-loader-v1".to_string(),
    }
}

fn document(generation: u64, scripts: Vec<PageHostScript>) -> PageHostDocument {
    document_with_snapshot(
        generation,
        "test document snapshot".to_string(),
        "https://example.test".to_string(),
        scripts,
    )
}

fn document_with_snapshot(
    generation: u64,
    document_text: String,
    document_origin: String,
    scripts: Vec<PageHostScript>,
) -> PageHostDocument {
    PageHostDocument {
        tab_id: 7,
        document_generation: generation,
        snapshot: PageHostDocumentSnapshot {
            document_text,
            document_origin,
        },
        debugger_execution_control: false,
        scripts,
    }
}

fn debugger_document(generation: u64, scripts: Vec<PageHostScript>) -> PageHostDocument {
    let mut document = document(generation, scripts);
    document.debugger_execution_control = true;
    document
}

fn classic(ordinal: u32, source: &str) -> PageHostScript {
    let id = format!("blueice://page/inline-{ordinal}.js");
    PageHostScript {
        ordinal,
        language: PageHostScriptLanguage::JavaScript,
        kind: PageHostScriptKind::Classic,
        graph: graph(&id, vec![PageHostSource::new(id.clone(), source)]),
    }
}

fn blue_ts_classic(ordinal: u32, source: &str) -> PageHostScript {
    let id = format!("blueice://page/inline-{ordinal}.ts");
    PageHostScript {
        ordinal,
        language: PageHostScriptLanguage::BlueTs,
        kind: PageHostScriptKind::Classic,
        graph: graph(&id, vec![PageHostSource::new(id.clone(), source)]),
    }
}

fn paused_value_targets(
    host: &mut BlueJsChildHost,
    program: PageHostDebuggerProgram,
    frame: Option<PageHostDebuggerFrame>,
    frame_index: usize,
) -> Vec<PageHostDebuggerValueTarget> {
    let snapshot = match host.handle_request(PageHostRequest::GetDebuggerStackSnapshot {
        tab_id: 7,
        document_generation: 1,
        program,
        frame,
        max_frames: 2,
        max_scope_entries: 256,
    }) {
        PageHostReply::DebuggerStackSnapshot { snapshot, .. } => snapshot,
        reply => panic!("expected active stack: {reply:?}"),
    };
    let selected = &snapshot.frames[frame_index];
    selected
        .scope_entries
        .iter()
        .copied()
        .map(|scope_entry| PageHostDebuggerValueTarget {
            tab_id: 7,
            document_generation: 1,
            program,
            frame,
            frame_index: frame_index as u32,
            safe_point: PageHostDebuggerSafePoint {
                program,
                code_unit_ordinal: selected.code_unit_ordinal,
                bytecode_offset: selected.bytecode_offset,
            },
            scope_entry,
        })
        .collect()
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
fn bluets_module_child_pauses_steps_and_resumes_the_exact_pending_entry() {
    let entry = "blueice://page/debug-entry.ts";
    let dependency = "blueice://page/debug-dependency.ts";
    let mut module = PageHostScript {
        ordinal: 0,
        language: PageHostScriptLanguage::BlueTs,
        kind: PageHostScriptKind::Module,
        graph: graph(
            entry,
            vec![
                PageHostSource::new(
                    entry,
                    "import { answer } from './debug-dependency.ts'; export const result: number = answer + 1;",
                ),
                PageHostSource::new(dependency, "export const answer: number = 41;"),
            ],
        ),
    };
    module.graph.resolutions.push(PageHostStaticResolution {
        from_module: entry.to_string(),
        specifier: "./debug-dependency.ts".to_string(),
        canonical_target: dependency.to_string(),
    });
    let mut host = BlueJsChildHost::default();
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: debugger_document(1, vec![module]),
        }),
        PageHostReply::Synchronized { reports, .. } if reports.is_empty()
    ));
    let pending = host.documents[&7]
        .pending_debugger_executions
        .front()
        .unwrap();
    let program = pending.program.unwrap();
    let DeferredChildExecution::BlueTsModule { attachment, .. } = &pending.execution else {
        panic!("the attached module is pending");
    };
    let entry_handle = attachment.entry.handle;
    let point = host
        .runtime
        .module_evaluate_entry_safe_point(7, entry_handle)
        .unwrap();
    let target = PageHostDebuggerSafePoint {
        program,
        code_unit_ordinal: 0,
        bytecode_offset: point.bytecode_offset,
    };
    let wrong_entry_point = match host.handle_request(PageHostRequest::ListDebuggerSafePoints {
        tab_id: 7,
        document_generation: 1,
        program,
    }) {
        PageHostReply::DebuggerSafePoints { safe_points, .. } => safe_points
            .into_iter()
            .find(|point| point.code_unit_ordinal == 0 && *point != target)
            .unwrap(),
        reply => panic!("expected entry safe points, got {reply:?}"),
    };
    assert!(matches!(
        host.handle_request(PageHostRequest::ArmDebuggerRootSafePointBreakpoint {
            tab_id: 7,
            document_generation: 1,
            safe_point: wrong_entry_point,
        }),
        PageHostReply::Error {
            code: PageHostErrorCode::InvalidDebuggerState,
            ..
        }
    ));
    let dependency_program = match host.handle_request(PageHostRequest::ListDebuggerPrograms {
        tab_id: 7,
        document_generation: 1,
    }) {
        PageHostReply::DebuggerPrograms { programs, .. } => *programs
            .iter()
            .find(|candidate| **candidate != program)
            .unwrap(),
        reply => panic!("expected both module programs, got {reply:?}"),
    };
    let dependency_point = match host.handle_request(PageHostRequest::ListDebuggerSafePoints {
        tab_id: 7,
        document_generation: 1,
        program: dependency_program,
    }) {
        PageHostReply::DebuggerSafePoints { safe_points, .. } => safe_points
            .into_iter()
            .find(|point| point.code_unit_ordinal == 0)
            .unwrap(),
        reply => panic!("expected dependency safe points, got {reply:?}"),
    };
    assert!(matches!(
        host.handle_request(PageHostRequest::ArmDebuggerRootSafePointBreakpoint {
            tab_id: 7,
            document_generation: 1,
            safe_point: dependency_point,
        }),
        PageHostReply::Error {
            code: PageHostErrorCode::InvalidDebuggerState,
            ..
        }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::ArmDebuggerRootSafePointBreakpoint {
            tab_id: 7,
            document_generation: 1,
            safe_point: target,
        }),
        PageHostReply::DebuggerRootSafePointBreakpointArmed { .. }
    ));
    for _ in 0..2 {
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
                program,
            }),
            PageHostReply::DebuggerExecutionState {
                tab_id: 7,
                document_generation: 1,
                program,
                state: PageHostDebuggerExecutionState::Paused { safe_point: target },
            }
        );
    }
    assert!(matches!(
        host.handle_request(PageHostRequest::StepDebuggerRootInstruction {
            tab_id: 7,
            document_generation: 1,
            program: dependency_program,
        }),
        PageHostReply::Error {
            code: PageHostErrorCode::InvalidDebuggerState,
            ..
        }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::StepDebuggerRootInstruction {
            tab_id: 7,
            document_generation: 1,
            program,
        }),
        PageHostReply::DebuggerExecutionStepRequested { .. }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::DebuggerExecutionAdvanced { reports, .. } if reports.is_empty()
    ));
    let successor = match host.handle_request(PageHostRequest::GetDebuggerExecutionState {
        tab_id: 7,
        document_generation: 1,
        program,
    }) {
        PageHostReply::DebuggerExecutionState {
            state: PageHostDebuggerExecutionState::Paused { safe_point },
            ..
        } => safe_point,
        reply => panic!("expected verified module step successor, got {reply:?}"),
    };
    assert_eq!(successor.program, program);
    assert_eq!(successor.code_unit_ordinal, 0);
    assert_ne!(successor, target);
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
            program,
        }),
        PageHostReply::DebuggerExecutionState {
            tab_id: 7,
            document_generation: 1,
            program,
            state: PageHostDebuggerExecutionState::Paused {
                safe_point: successor
            },
        }
    );
    assert!(matches!(
        host.handle_request(PageHostRequest::ValidateDebuggerSafePoint {
            tab_id: 7,
            document_generation: 1,
            safe_point: successor,
        }),
        PageHostReply::DebuggerSafePointValidated { .. }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::ResumeDebuggerExecution {
            tab_id: 7,
            document_generation: 1,
            program,
        }),
        PageHostReply::DebuggerExecutionResumed { .. }
    ));
    assert_eq!(
        host.handle_request(PageHostRequest::GetDebuggerExecutionState {
            tab_id: 7,
            document_generation: 1,
            program,
        }),
        PageHostReply::DebuggerExecutionState {
            tab_id: 7,
            document_generation: 1,
            program,
            state: PageHostDebuggerExecutionState::Resuming,
        }
    );
    assert!(matches!(
        host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::DebuggerExecutionAdvanced { reports, .. }
            if reports.len() == 1 && reports[0].outcome == PageHostScriptOutcome::Executed
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::GetDebuggerExecutionState {
            tab_id: 7,
            document_generation: 1,
            program,
        }),
        PageHostReply::DebuggerExecutionState {
            state: PageHostDebuggerExecutionState::Completed,
            ..
        }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: debugger_document(2, Vec::new()),
        }),
        PageHostReply::Synchronized { .. }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::ArmDebuggerRootSafePointBreakpoint {
            tab_id: 7,
            document_generation: 1,
            safe_point: target,
        }),
        PageHostReply::Error {
            code: PageHostErrorCode::StaleDocument,
            ..
        }
    ));
}

#[test]
fn bluets_module_source_span_step_stops_at_the_next_bound_statement() {
    let entry = "blueice://page/module-source-step.ts";
    let module = PageHostScript {
        ordinal: 0,
        language: PageHostScriptLanguage::BlueTs,
        kind: PageHostScriptKind::Module,
        graph: graph(
            entry,
            vec![PageHostSource::new(
                entry,
                "let first: number = 1; let second: number = first + 1; export const answer: number = second + 1;",
            )],
        ),
    };
    let mut host = BlueJsChildHost::default();
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: debugger_document(1, vec![module]),
        }),
        PageHostReply::Synchronized { reports, .. } if reports.is_empty()
    ));
    let pending = host.documents[&7]
        .pending_debugger_executions
        .front()
        .unwrap();
    let program = pending.program.unwrap();
    let DeferredChildExecution::BlueTsModule { attachment, .. } = &pending.execution else {
        panic!("the checked entry module must be pending");
    };
    let point = host
        .runtime
        .module_evaluate_entry_safe_point(7, attachment.entry.handle)
        .unwrap();
    let target = PageHostDebuggerSafePoint {
        program,
        code_unit_ordinal: 0,
        bytecode_offset: point.bytecode_offset,
    };
    let metadata = match host.handle_request(PageHostRequest::ListDebuggerBlueTsMetadata {
        tab_id: 7,
        document_generation: 1,
        program,
    }) {
        PageHostReply::DebuggerBlueTsMetadata { metadata, .. } => metadata[0],
        reply => panic!("expected entry metadata, got {reply:?}"),
    };
    assert!(matches!(
        host.handle_request(PageHostRequest::ArmDebuggerRootSafePointBreakpoint {
            tab_id: 7,
            document_generation: 1,
            safe_point: target,
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
    let mut current = target;
    let origin = loop {
        if let Some(span) = host
            .debugger_bluets_source_span_key(7, 1, metadata, current)
            .unwrap()
        {
            break span;
        }
        assert!(matches!(
            host.handle_request(PageHostRequest::StepDebuggerRootInstruction {
                tab_id: 7,
                document_generation: 1,
                program,
            }),
            PageHostReply::DebuggerExecutionStepRequested { .. }
        ));
        assert!(matches!(
            host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
                tab_id: 7,
                document_generation: 1,
            }),
            PageHostReply::DebuggerExecutionAdvanced { reports, .. } if reports.is_empty()
        ));
        current = match host.handle_request(PageHostRequest::GetDebuggerExecutionState {
            tab_id: 7,
            document_generation: 1,
            program,
        }) {
            PageHostReply::DebuggerExecutionState {
                state: PageHostDebuggerExecutionState::Paused { safe_point },
                ..
            } => safe_point,
            reply => panic!("module must remain paused until a bound span: {reply:?}"),
        };
    };
    assert!(matches!(
        host.handle_request(PageHostRequest::StepDebuggerBlueTsSourceSpan {
            tab_id: 7,
            document_generation: 1,
            metadata,
            source_id: origin.source_id + 1,
            safe_point: current,
        }),
        PageHostReply::Error {
            code: PageHostErrorCode::InvalidRequest,
            ..
        }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::StepDebuggerBlueTsSourceSpan {
            tab_id: 7,
            document_generation: 1,
            metadata,
            source_id: origin.source_id,
            safe_point: current,
        }),
        PageHostReply::DebuggerBlueTsSourceStepRequested { .. }
    ));
    let mut successor = None;
    for _ in 0..MAX_BLUETS_SOURCE_STEP_ROOT_INSTRUCTIONS {
        assert!(matches!(
            host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
                tab_id: 7,
                document_generation: 1,
            }),
            PageHostReply::DebuggerExecutionAdvanced { reports, .. } if reports.is_empty()
        ));
        match host.handle_request(PageHostRequest::GetDebuggerExecutionState {
            tab_id: 7,
            document_generation: 1,
            program,
        }) {
            PageHostReply::DebuggerExecutionState {
                state: PageHostDebuggerExecutionState::Stepping,
                ..
            } => {}
            PageHostReply::DebuggerExecutionState {
                state: PageHostDebuggerExecutionState::Paused { safe_point },
                ..
            } => {
                successor = Some(safe_point);
                break;
            }
            reply => panic!("module source step did not find a new span: {reply:?}"),
        }
    }
    let successor = successor.expect("bounded module step must find another statement");
    assert_ne!(successor, current);
    assert_ne!(
        host.debugger_bluets_source_span_key(7, 1, metadata, successor)
            .unwrap(),
        Some(origin)
    );
    assert!(matches!(
        host.handle_request(PageHostRequest::ResumeDebuggerExecution {
            tab_id: 7,
            document_generation: 1,
            program,
        }),
        PageHostReply::DebuggerExecutionResumed { .. }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::DebuggerExecutionAdvanced { reports, .. }
            if reports.len() == 1 && reports[0].outcome == PageHostScriptOutcome::Executed
    ));
}

#[test]
fn leading_bluets_classic_attaches_metadata_and_pauses_at_a_root_safe_point() {
    let mut host = BlueJsChildHost::default();
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: debugger_document(
                1,
                vec![
                    blue_ts_classic(
                        0,
                        "let first: number = 1; let deferredAnswer: number = first + 41;",
                    ),
                    classic(1, "globalThis.afterBlueTs = 2;"),
                ],
            ),
        }),
        PageHostReply::Synchronized { reports, .. } if reports.is_empty()
    ));
    assert_eq!(host.debug_registry.len(), 1);
    let program = match host.handle_request(PageHostRequest::ListDebuggerPrograms {
        tab_id: 7,
        document_generation: 1,
    }) {
        PageHostReply::DebuggerPrograms { programs, .. } => {
            assert_eq!(programs.len(), 2);
            programs[0]
        }
        reply => panic!("expected attached BlueTS program, got {reply:?}"),
    };
    let target = match host.handle_request(PageHostRequest::ListDebuggerSafePoints {
        tab_id: 7,
        document_generation: 1,
        program,
    }) {
        PageHostReply::DebuggerSafePoints { safe_points, .. } => safe_points
            .into_iter()
            .find(|point| point.code_unit_ordinal == 0 && point.bytecode_offset != 0)
            .expect("typed classic fixture needs a non-entry root safe point"),
        reply => panic!("expected BlueTS root safe points, got {reply:?}"),
    };
    assert_eq!(
        host.handle_request(PageHostRequest::GetDebuggerExecutionState {
            tab_id: 7,
            document_generation: 1,
            program,
        }),
        PageHostReply::DebuggerExecutionState {
            tab_id: 7,
            document_generation: 1,
            program,
            state: PageHostDebuggerExecutionState::Pending,
        }
    );
    assert!(matches!(
        host.handle_request(PageHostRequest::ArmDebuggerRootSafePointBreakpoint {
            tab_id: 7,
            document_generation: 1,
            safe_point: target,
        }),
        PageHostReply::DebuggerRootSafePointBreakpointArmed { safe_point, .. }
            if safe_point == target
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::DebuggerExecutionAdvanced { reports, .. }
            if reports.is_empty()
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::GetDebuggerExecutionState {
            tab_id: 7,
            document_generation: 1,
            program,
        }),
        PageHostReply::DebuggerExecutionState {
            state: PageHostDebuggerExecutionState::Paused { safe_point },
            ..
        } if safe_point == target
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::StepDebuggerRootInstruction {
            tab_id: 7,
            document_generation: 1,
            program,
        }),
        PageHostReply::DebuggerExecutionStepRequested { .. }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::DebuggerExecutionAdvanced { reports, .. } if reports.is_empty()
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::GetDebuggerExecutionState {
            tab_id: 7,
            document_generation: 1,
            program,
        }),
        PageHostReply::DebuggerExecutionState {
            state: PageHostDebuggerExecutionState::Paused { safe_point },
            ..
        } if safe_point.program == program && safe_point != target
    ));
    assert_eq!(host.debug_registry.len(), 1);
    assert!(matches!(
        host.handle_request(PageHostRequest::ResumeDebuggerExecution {
            tab_id: 7,
            document_generation: 1,
            program,
        }),
        PageHostReply::DebuggerExecutionResumed { .. }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::DebuggerExecutionAdvanced { reports, .. }
            if reports == vec![
                script_report(
                    7,
                    1,
                    0,
                    PageHostScriptLanguage::BlueTs,
                    PageHostScriptKind::Classic,
                    PageHostScriptOutcome::Executed,
                ),
                script_report(
                    7,
                    1,
                    1,
                    PageHostScriptLanguage::JavaScript,
                    PageHostScriptKind::Classic,
                    PageHostScriptOutcome::Executed,
                ),
            ]
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::CloseRealm {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::RealmClosed { .. }
    ));
    assert!(host.debug_registry.is_empty());
}

#[test]
fn child_source_span_step_stops_at_the_next_bound_bluets_statement() {
    let mut host = BlueJsChildHost::default();
    let source = concat!(
        "let first: number = 1; ",
        "let middle: number = first + 1; ",
        "let last: number = middle + 1;"
    );
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: debugger_document(
                1,
                vec![blue_ts_classic(0, source), classic(1, "globalThis.afterStep = 1;")],
            ),
        }),
        PageHostReply::Synchronized { reports, .. } if reports.is_empty()
    ));
    let program = match host.handle_request(PageHostRequest::ListDebuggerPrograms {
        tab_id: 7,
        document_generation: 1,
    }) {
        PageHostReply::DebuggerPrograms { programs, .. } => programs[0],
        reply => panic!("expected a BlueTS program, got {reply:?}"),
    };
    let metadata = match host.handle_request(PageHostRequest::ListDebuggerBlueTsMetadata {
        tab_id: 7,
        document_generation: 1,
        program,
    }) {
        PageHostReply::DebuggerBlueTsMetadata { metadata, .. } => metadata[0],
        reply => panic!("expected retained BlueTS metadata, got {reply:?}"),
    };
    let (entries, source_id) = {
        let handle = host.documents[&7].debugger_programs[&program.program_handle].runtime_handle;
        let retained = host
            .debug_registry
            .get(host.runtime.program_registry(), handle)
            .unwrap();
        let entries = retained
            .safe_point_map()
            .entries
            .iter()
            .filter(|entry| entry.code_unit.ordinal() == 0)
            .cloned()
            .collect::<Vec<_>>();
        let source_id = retained
            .static_info()
            .sources
            .iter()
            .find(|source| source.module == entries[1].source)
            .unwrap()
            .id
            .0;
        (entries, source_id)
    };
    assert!(entries.len() >= 3, "fixture needs three bound root spans");
    let target_entry = &entries[1];
    let target = PageHostDebuggerSafePoint {
        program,
        code_unit_ordinal: 0,
        bytecode_offset: target_entry.bytecode_offset,
    };
    assert_ne!(target.bytecode_offset, 0);
    assert!(matches!(
        host.handle_request(PageHostRequest::ArmDebuggerRootSafePointBreakpoint {
            tab_id: 7,
            document_generation: 1,
            safe_point: target,
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
    let request = PageHostRequest::StepDebuggerBlueTsSourceSpan {
        tab_id: 7,
        document_generation: 1,
        metadata,
        source_id,
        safe_point: target,
    };
    let wrong_source_reply = host.handle_request(PageHostRequest::StepDebuggerBlueTsSourceSpan {
        tab_id: 7,
        document_generation: 1,
        metadata,
        source_id: source_id + 1,
        safe_point: target,
    });
    assert!(
        matches!(
            wrong_source_reply,
            PageHostReply::Error {
                code: PageHostErrorCode::InvalidRequest,
                ..
            }
        ),
        "{wrong_source_reply:?}"
    );
    assert!(matches!(
        host.handle_request(PageHostRequest::StepDebuggerBlueTsSourceSpan {
            tab_id: 7,
            document_generation: 1,
            metadata: PageHostDebuggerMetadataHandle {
                metadata_generation: metadata.metadata_generation + 1,
                ..metadata
            },
            source_id,
            safe_point: target,
        }),
        PageHostReply::Error {
            code: PageHostErrorCode::InvalidRequest,
            ..
        }
    ));
    assert_eq!(
        host.handle_request(request.clone()),
        PageHostReply::DebuggerBlueTsSourceStepRequested {
            tab_id: 7,
            document_generation: 1,
            metadata,
            source_id,
            safe_point: target,
        }
    );
    assert!(matches!(
        host.handle_request(request),
        PageHostReply::Error {
            code: PageHostErrorCode::InvalidDebuggerState,
            ..
        }
    ));
    let mut successor = None;
    for _ in 0..MAX_BLUETS_SOURCE_STEP_ROOT_INSTRUCTIONS {
        assert!(matches!(
            host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
                tab_id: 7,
                document_generation: 1,
            }),
            PageHostReply::DebuggerExecutionAdvanced { reports, .. } if reports.is_empty()
        ));
        match host.handle_request(PageHostRequest::GetDebuggerExecutionState {
            tab_id: 7,
            document_generation: 1,
            program,
        }) {
            PageHostReply::DebuggerExecutionState {
                state: PageHostDebuggerExecutionState::Stepping,
                ..
            } => {}
            PageHostReply::DebuggerExecutionState {
                state: PageHostDebuggerExecutionState::Paused { safe_point },
                ..
            } => {
                successor = Some(safe_point);
                break;
            }
            reply => panic!("source step did not stop at a new bound span: {reply:?}"),
        }
    }
    let successor = successor.expect("source step must reach the third statement");
    assert_eq!(successor.bytecode_offset, entries[2].bytecode_offset);
    assert_eq!(host.debug_registry.len(), 1);
    assert!(matches!(
        host.handle_request(PageHostRequest::ResumeDebuggerExecution {
            tab_id: 7,
            document_generation: 1,
            program,
        }),
        PageHostReply::DebuggerExecutionResumed { .. }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::DebuggerExecutionAdvanced { reports, .. } if reports.len() == 2
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: debugger_document(2, Vec::new()),
        }),
        PageHostReply::Synchronized { .. }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::StepDebuggerBlueTsSourceSpan {
            tab_id: 7,
            document_generation: 1,
            metadata,
            source_id,
            safe_point: target,
        }),
        PageHostReply::Error {
            code: PageHostErrorCode::StaleDocument,
            ..
        }
    ));
}

#[test]
fn child_source_span_step_yields_at_its_fixed_root_instruction_limit() {
    let mut host = BlueJsChildHost::default();
    let expression = std::iter::repeat_n("first", 320)
        .collect::<Vec<_>>()
        .join(" + ");
    let source = format!(
        "let first: number = 1; let slow: number = {expression}; let last: number = slow + 1;"
    );
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: debugger_document(1, vec![blue_ts_classic(0, &source)]),
        }),
        PageHostReply::Synchronized { reports, .. } if reports.is_empty()
    ));
    let program = match host.handle_request(PageHostRequest::ListDebuggerPrograms {
        tab_id: 7,
        document_generation: 1,
    }) {
        PageHostReply::DebuggerPrograms { programs, .. } => programs[0],
        reply => panic!("expected a BlueTS program, got {reply:?}"),
    };
    let metadata = match host.handle_request(PageHostRequest::ListDebuggerBlueTsMetadata {
        tab_id: 7,
        document_generation: 1,
        program,
    }) {
        PageHostReply::DebuggerBlueTsMetadata { metadata, .. } => metadata[0],
        reply => panic!("expected retained BlueTS metadata, got {reply:?}"),
    };
    let (target, source_id) = {
        let handle = host.documents[&7].debugger_programs[&program.program_handle].runtime_handle;
        let retained = host
            .debug_registry
            .get(host.runtime.program_registry(), handle)
            .unwrap();
        let first_start = retained
            .safe_point_map()
            .entries
            .iter()
            .filter(|entry| entry.code_unit.ordinal() == 0)
            .map(|entry| entry.start_byte)
            .min()
            .expect("the first statement has a root entry");
        let entry = retained
            .safe_point_map()
            .entries
            .iter()
            .filter(|entry| entry.code_unit.ordinal() == 0 && entry.start_byte > first_start)
            .min_by_key(|entry| (entry.start_byte, entry.bytecode_offset))
            .expect("the long second statement must have a bound root entry");
        let source_id = retained
            .static_info()
            .sources
            .iter()
            .find(|source| source.module == entry.source)
            .unwrap()
            .id
            .0;
        (
            PageHostDebuggerSafePoint {
                program,
                code_unit_ordinal: 0,
                bytecode_offset: entry.bytecode_offset,
            },
            source_id,
        )
    };
    assert!(matches!(
        host.handle_request(PageHostRequest::ArmDebuggerRootSafePointBreakpoint {
            tab_id: 7,
            document_generation: 1,
            safe_point: target,
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
    assert!(matches!(
        host.handle_request(PageHostRequest::StepDebuggerBlueTsSourceSpan {
            tab_id: 7,
            document_generation: 1,
            metadata,
            source_id,
            safe_point: target,
        }),
        PageHostReply::DebuggerBlueTsSourceStepRequested { .. }
    ));
    for turn in 1..=MAX_BLUETS_SOURCE_STEP_ROOT_INSTRUCTIONS {
        assert!(matches!(
            host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
                tab_id: 7,
                document_generation: 1,
            }),
            PageHostReply::DebuggerExecutionAdvanced { reports, .. } if reports.is_empty()
        ));
        let state = host.handle_request(PageHostRequest::GetDebuggerExecutionState {
            tab_id: 7,
            document_generation: 1,
            program,
        });
        if turn < MAX_BLUETS_SOURCE_STEP_ROOT_INSTRUCTIONS {
            assert!(
                matches!(
                    state,
                    PageHostReply::DebuggerExecutionState {
                        state: PageHostDebuggerExecutionState::Stepping,
                        ..
                    }
                ),
                "unexpected pre-limit state on turn {turn}: {state:?}"
            );
        } else {
            assert!(
                matches!(
                    state,
                    PageHostReply::DebuggerExecutionState {
                        state: PageHostDebuggerExecutionState::SourceStepLimitReached { safe_point },
                        ..
                    } if safe_point.program == program && safe_point != target
                ),
                "source step must yield at its exact budget: {state:?}"
            );
        }
    }
    assert!(matches!(
        host.handle_request(PageHostRequest::ResumeDebuggerExecution {
            tab_id: 7,
            document_generation: 1,
            program,
        }),
        PageHostReply::DebuggerExecutionResumed { .. }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::DebuggerExecutionAdvanced { reports, .. } if reports.len() == 1
    ));
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
fn linked_module_child_keeps_distinct_programs_and_resumes_its_entry() {
    let entry = "blueice://page/linked-entry.ts";
    let dependency = "blueice://page/linked-dependency.ts";
    let mut module = PageHostScript {
        ordinal: 0,
        language: PageHostScriptLanguage::BlueTs,
        kind: PageHostScriptKind::Module,
        graph: graph(
            entry,
            vec![
                PageHostSource::new(
                    entry,
                    "import { inner } from './linked-dependency.ts'; export const answer: number = inner() + 1;",
                ),
                PageHostSource::new(
                    dependency,
                    "export function inner(): number { return 41; }",
                ),
            ],
        ),
    };
    module.graph.resolutions.push(PageHostStaticResolution {
        from_module: entry.to_string(),
        specifier: "./linked-dependency.ts".to_string(),
        canonical_target: dependency.to_string(),
    });
    let mut host = BlueJsChildHost::default();
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: debugger_document(1, vec![module]),
        }),
        PageHostReply::Synchronized { reports, .. } if reports.is_empty()
    ));
    let pending = host.documents[&7]
        .pending_debugger_executions
        .front()
        .unwrap();
    let entry_program = pending.program.unwrap();
    let DeferredChildExecution::BlueTsModule { attachment, .. } = &pending.execution else {
        panic!("the linked module remains pending");
    };
    let entry_handle = attachment.entry.handle;
    let dependency_handle = attachment.modules[dependency].handle;
    let dependency_program = host.documents[&7]
        .debugger_programs
        .iter()
        .find(|(_, record)| record.runtime_handle == dependency_handle)
        .map(|(handle, record)| PageHostDebuggerProgram {
            program_handle: *handle,
            program_generation: record.program_generation,
        })
        .unwrap();
    assert_ne!(entry_program, dependency_program);
    let point = host
        .runtime
        .safe_points(7, dependency_handle, 1024)
        .unwrap()
        .into_iter()
        .find(|point| point.code_unit.ordinal() == 1 && point.bytecode_offset == 0)
        .unwrap();
    let target = PageHostDebuggerSafePoint {
        program: dependency_program,
        code_unit_ordinal: point.code_unit.ordinal(),
        bytecode_offset: point.bytecode_offset,
    };
    assert!(matches!(
        host.handle_request(
            PageHostRequest::ArmDebuggerLinkedNestedSafePointBreakpoint {
                tab_id: 7,
                document_generation: 1,
                entry_program: dependency_program,
                safe_point: target,
            }
        ),
        PageHostReply::Error { .. }
    ));
    assert_eq!(
        host.handle_request(
            PageHostRequest::ArmDebuggerLinkedNestedSafePointBreakpoint {
                tab_id: 7,
                document_generation: 1,
                entry_program,
                safe_point: target,
            }
        ),
        PageHostReply::DebuggerLinkedNestedSafePointBreakpointArmed {
            tab_id: 7,
            document_generation: 1,
            entry_program,
            safe_point: target,
        }
    );
    assert!(matches!(
        host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::DebuggerExecutionAdvanced { reports, .. } if reports.is_empty()
    ));
    let ChildDebuggerExecutionStatus::LinkedPaused { frame, safe_point } =
        host.documents[&7].debugger_execution_states[&entry_program]
    else {
        panic!("the linked child must be paused");
    };
    assert_eq!(frame.entry_program(), entry_handle);
    assert_eq!(frame.dependency_program(), dependency_handle);
    assert_eq!(safe_point.program, dependency_program);
    let stack = host
        .debugger_linked_stack_snapshot(7, 1, entry_program, frame, 256)
        .unwrap();
    assert_eq!(stack.frames[0].safe_point.program, dependency_program);
    assert_eq!(stack.frames[1].safe_point.program, entry_program);
    let wire_frame = child_debugger_linked_frame(7, 1, entry_program, dependency_program, frame);
    assert_eq!(
        host.handle_request(PageHostRequest::GetDebuggerExecutionState {
            tab_id: 7,
            document_generation: 1,
            program: entry_program,
        }),
        PageHostReply::DebuggerLinkedExecutionState {
            frame: wire_frame,
            state: PageHostDebuggerLinkedExecutionState::Paused { safe_point },
        }
    );
    let wire_stack = match host.handle_request(PageHostRequest::GetDebuggerLinkedStackSnapshot {
        frame: wire_frame,
        max_scope_entries: 256,
    }) {
        PageHostReply::DebuggerLinkedStackSnapshot {
            frame: returned,
            snapshot,
        } if returned == wire_frame => *snapshot,
        reply => panic!("expected private linked stack, got {reply:?}"),
    };
    assert_eq!(wire_stack.frames[0].safe_point.program, dependency_program);
    assert_eq!(wire_stack.frames[1].safe_point.program, entry_program);
    let mut source_inventory = |program| {
        let metadata = match host.handle_request(PageHostRequest::ListDebuggerBlueTsMetadata {
            tab_id: 7,
            document_generation: 1,
            program,
        }) {
            PageHostReply::DebuggerBlueTsMetadata { metadata, .. } => metadata[0],
            reply => panic!("expected per-program metadata, got {reply:?}"),
        };
        let source_id =
            match host.handle_request(PageHostRequest::ListDebuggerBlueTsMetadataSources {
                tab_id: 7,
                document_generation: 1,
                program,
                metadata,
            }) {
                PageHostReply::DebuggerBlueTsMetadataSources { sources, .. } => {
                    sources[0].source_id
                }
                reply => panic!("expected per-program source IDs, got {reply:?}"),
            };
        (metadata, source_id)
    };
    let child_source = source_inventory(dependency_program);
    let entry_source = source_inventory(entry_program);
    assert_ne!(child_source.0, entry_source.0);
    let spans = host
        .debugger_linked_source_spans(
            7,
            1,
            entry_program,
            frame,
            &stack,
            [child_source, entry_source],
        )
        .unwrap();
    assert_eq!(spans[0].source_id, child_source.1);
    assert_eq!(spans[1].source_id, entry_source.1);
    let wire_sources = [
        PageHostDebuggerLinkedSource {
            metadata: child_source.0,
            source_id: child_source.1,
        },
        PageHostDebuggerLinkedSource {
            metadata: entry_source.0,
            source_id: entry_source.1,
        },
    ];
    assert_eq!(
        host.handle_request(PageHostRequest::DescribeDebuggerLinkedStackSpans {
            frame: wire_frame,
            expected_stack: wire_stack.clone(),
            sources: wire_sources,
        }),
        PageHostReply::DebuggerLinkedStackSpans {
            frame: wire_frame,
            snapshot: Box::new(wire_stack.clone()),
            spans: Box::new(spans),
        }
    );
    for rejected_sources in [
        [wire_sources[1], wire_sources[0]],
        [
            PageHostDebuggerLinkedSource {
                source_id: u32::MAX,
                ..wire_sources[0]
            },
            wire_sources[1],
        ],
        [
            wire_sources[0],
            PageHostDebuggerLinkedSource {
                source_id: u32::MAX,
                ..wire_sources[1]
            },
        ],
    ] {
        assert!(matches!(
            host.handle_request(PageHostRequest::DescribeDebuggerLinkedStackSpans {
                frame: wire_frame,
                expected_stack: wire_stack.clone(),
                sources: rejected_sources,
            }),
            PageHostReply::Error { .. }
        ));
    }
    for (program, (metadata, source_id), expected_module) in [
        (dependency_program, child_source, dependency),
        (entry_program, entry_source, entry),
    ] {
        let provenance =
            match host.handle_request(PageHostRequest::DescribeDebuggerBlueTsMetadataSource {
                tab_id: 7,
                document_generation: 1,
                program,
                metadata,
                source_id,
            }) {
                PageHostReply::DebuggerBlueTsMetadataSourceProvenance { provenance, .. } => {
                    provenance
                }
                reply => panic!("expected exact source provenance, got {reply:?}"),
            };
        assert_eq!(provenance.module, expected_module);
    }
    assert!(host
        .debugger_linked_source_spans(
            7,
            1,
            entry_program,
            frame,
            &stack,
            [entry_source, child_source],
        )
        .is_err());
    assert!(host
        .debugger_linked_source_spans(
            7,
            1,
            entry_program,
            frame,
            &stack,
            [(child_source.0, u32::MAX), entry_source],
        )
        .is_err());
    assert!(host
        .debugger_linked_source_spans(
            7,
            1,
            entry_program,
            frame,
            &stack,
            [child_source, (entry_source.0, u32::MAX)],
        )
        .is_err());
    let mut wrong_stack = stack.clone();
    wrong_stack.frames[0].safe_point = wrong_stack.frames[1].safe_point;
    assert!(host
        .debugger_linked_source_spans(
            7,
            1,
            entry_program,
            frame,
            &wrong_stack,
            [child_source, entry_source],
        )
        .is_err());
    assert!(host
        .debugger_linked_stack_snapshot(7, 1, dependency_program, frame, 256)
        .is_err());
    assert!(host
        .debugger_linked_stack_snapshot(7, 2, entry_program, frame, 256)
        .is_err());
    let mut moved_stack = wire_stack.clone();
    moved_stack.frames[0].safe_point.bytecode_offset += 1;
    assert!(matches!(
        host.handle_request(PageHostRequest::DescribeDebuggerLinkedStackSpans {
            frame: wire_frame,
            expected_stack: moved_stack,
            sources: wire_sources,
        }),
        PageHostReply::Error { .. }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::GetDebuggerStackSnapshot {
            tab_id: 7,
            document_generation: 1,
            program: entry_program,
            frame: None,
            max_frames: 2,
            max_scope_entries: 256,
        }),
        PageHostReply::Error {
            code: PageHostErrorCode::InvalidDebuggerState,
            ..
        }
    ));
    let wrong_serial = PageHostDebuggerLinkedFrame {
        invocation_serial: wire_frame.invocation_serial + 1,
        ..wire_frame
    };
    assert!(matches!(
        host.handle_request(PageHostRequest::GetDebuggerLinkedStackSnapshot {
            frame: wrong_serial,
            max_scope_entries: 256,
        }),
        PageHostReply::Error { .. }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::ResumeDebuggerLinkedNestedExecution {
            frame: wrong_serial,
        }),
        PageHostReply::Error { .. }
    ));
    let stale_document = PageHostDebuggerLinkedFrame {
        document_generation: 2,
        ..wire_frame
    };
    assert!(matches!(
        host.handle_request(PageHostRequest::GetDebuggerLinkedStackSnapshot {
            frame: stale_document,
            max_scope_entries: 256,
        }),
        PageHostReply::Error {
            code: PageHostErrorCode::StaleDocument,
            ..
        }
    ));
    assert!(host
        .request_debugger_linked_resume(7, 1, dependency_program, frame)
        .is_err());
    assert_eq!(
        host.handle_request(PageHostRequest::ResumeDebuggerLinkedNestedExecution {
            frame: wire_frame,
        }),
        PageHostReply::DebuggerLinkedNestedResumeRequested { frame: wire_frame }
    );
    assert_eq!(
        host.handle_request(PageHostRequest::GetDebuggerExecutionState {
            tab_id: 7,
            document_generation: 1,
            program: entry_program,
        }),
        PageHostReply::DebuggerLinkedExecutionState {
            frame: wire_frame,
            state: PageHostDebuggerLinkedExecutionState::Resuming,
        }
    );
    assert!(matches!(
        host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::DebuggerExecutionAdvanced { reports, .. } if reports.is_empty()
    ));
    let ChildDebuggerExecutionStatus::Paused(root_point) =
        host.documents[&7].debugger_execution_states[&entry_program]
    else {
        panic!("the linked child must return to its entry");
    };
    assert_eq!(root_point.program, entry_program);
    assert_eq!(root_point.code_unit_ordinal, 0);
    assert!(host
        .debugger_linked_stack_snapshot(7, 1, entry_program, frame, 256)
        .is_err());
    assert!(host
        .request_debugger_linked_resume(7, 1, entry_program, frame)
        .is_err());
    assert!(matches!(
        host.handle_request(PageHostRequest::ResumeDebuggerLinkedNestedExecution {
            frame: wire_frame,
        }),
        PageHostReply::Error { .. }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::ResumeDebuggerExecution {
            tab_id: 7,
            document_generation: 1,
            program: entry_program,
        }),
        PageHostReply::DebuggerExecutionResumed { .. }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::DebuggerExecutionAdvanced { .. }
    ));
    assert_eq!(
        host.documents[&7].debugger_execution_states[&entry_program],
        ChildDebuggerExecutionStatus::Completed
    );
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
