// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

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
        let slow_start = source.find("let slow:").unwrap();
        let entry = retained
            .safe_point_map()
            .entries
            .iter()
            .filter(|entry| {
                entry.code_unit.ordinal() == 0
                    && entry.start_byte <= slow_start
                    && slow_start < entry.end_byte
            })
            .min_by_key(|entry| entry.bytecode_offset)
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
