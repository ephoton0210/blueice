// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// A genuinely uncaught JS exception raised while the debugger is
// single-stepping/resuming -- at the classic root, inside a nested
// frame, inside a BlueTS source-span step, or while advancing to a
// module's own nested-pause target -- is a real, reachable outcome
// distinct from a normal completion. These exercise the `Err(error)`
// arm of every one of those scheduler match blocks.

use super::*;

#[test]
fn private_child_classic_root_step_rejects_an_uncaught_throw() {
    let mut host = BlueJsChildHost::default();
    let source = "let a = 1; throw new Error('boom');";
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: debugger_document(1, vec![classic(0, source)]),
        }),
        PageHostReply::Synchronized { reports, .. } if reports.is_empty()
    ));
    let program = match host.handle_request(PageHostRequest::ListDebuggerPrograms {
        tab_id: 7,
        document_generation: 1,
    }) {
        PageHostReply::DebuggerPrograms { programs, .. } => programs[0],
        reply => panic!("expected one private program: {reply:?}"),
    };
    let root = match host.handle_request(PageHostRequest::ListDebuggerSafePoints {
        tab_id: 7,
        document_generation: 1,
        program,
    }) {
        PageHostReply::DebuggerSafePoints { safe_points, .. } => safe_points
            .into_iter()
            .find(|point| point.code_unit_ordinal == 0 && point.bytecode_offset == 0)
            .unwrap(),
        reply => panic!("expected root safe points: {reply:?}"),
    };
    assert!(matches!(
        host.handle_request(PageHostRequest::ArmDebuggerRootSafePointBreakpoint {
            tab_id: 7,
            document_generation: 1,
            safe_point: root,
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
    let mut rejected = false;
    for _ in 0..32 {
        assert!(matches!(
            host.handle_request(PageHostRequest::StepDebuggerRootInstruction {
                tab_id: 7,
                document_generation: 1,
                program,
            }),
            PageHostReply::DebuggerExecutionStepRequested { .. }
        ));
        match host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
            tab_id: 7,
            document_generation: 1,
        }) {
            PageHostReply::DebuggerExecutionAdvanced { reports, .. } if reports.is_empty() => {}
            PageHostReply::DebuggerExecutionAdvanced { reports, .. }
                if matches!(
                    reports.as_slice(),
                    [PageHostScriptReport {
                        outcome: PageHostScriptOutcome::Rejected { .. },
                        ..
                    }]
                ) =>
            {
                rejected = true;
                break;
            }
            reply => panic!("unexpected root step outcome: {reply:?}"),
        }
    }
    assert!(rejected, "an uncaught root-level throw must reject the script");
    assert_eq!(
        host.documents[&7].debugger_execution_states[&program],
        ChildDebuggerExecutionStatus::Completed
    );
}

#[test]
fn private_child_classic_nested_step_rejects_an_uncaught_throw() {
    let mut host = BlueJsChildHost::default();
    let source = "function inner() { let a = 1; throw new Error('boom'); } inner();";
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: debugger_document(1, vec![classic(0, source)]),
        }),
        PageHostReply::Synchronized { reports, .. } if reports.is_empty()
    ));
    let program = match host.handle_request(PageHostRequest::ListDebuggerPrograms {
        tab_id: 7,
        document_generation: 1,
    }) {
        PageHostReply::DebuggerPrograms { programs, .. } => programs[0],
        reply => panic!("expected one private program: {reply:?}"),
    };
    let target = match host.handle_request(PageHostRequest::ListDebuggerSafePoints {
        tab_id: 7,
        document_generation: 1,
        program,
    }) {
        PageHostReply::DebuggerSafePoints { safe_points, .. } => safe_points
            .into_iter()
            .find(|point| point.code_unit_ordinal == 1 && point.bytecode_offset == 0)
            .unwrap(),
        reply => panic!("expected child safe points: {reply:?}"),
    };
    host.arm_debugger_nested_target(7, 1, target).unwrap();
    assert!(matches!(
        host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::DebuggerExecutionAdvanced { reports, .. } if reports.is_empty()
    ));
    let frame = match host.documents[&7].debugger_execution_states[&program] {
        ChildDebuggerExecutionStatus::NestedPaused { frame, .. } => frame,
        status => panic!("the actual inner invocation must pause: {status:?}"),
    };
    let mut rejected = false;
    for _ in 0..32 {
        if host
            .request_debugger_nested_advance(7, 1, frame, false)
            .is_err()
        {
            break;
        }
        match host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
            tab_id: 7,
            document_generation: 1,
        }) {
            PageHostReply::DebuggerExecutionAdvanced { reports, .. } if reports.is_empty() => {}
            PageHostReply::DebuggerExecutionAdvanced { reports, .. }
                if matches!(
                    reports.as_slice(),
                    [PageHostScriptReport {
                        outcome: PageHostScriptOutcome::Rejected { .. },
                        ..
                    }]
                ) =>
            {
                rejected = true;
                break;
            }
            reply => panic!("unexpected nested step outcome: {reply:?}"),
        }
    }
    assert!(
        rejected,
        "an uncaught throw during a nested single-step must reject the script"
    );
    assert_eq!(
        host.documents[&7].debugger_execution_states[&program],
        ChildDebuggerExecutionStatus::Completed
    );
}

#[test]
fn private_child_classic_nested_resume_rejects_an_uncaught_throw() {
    let mut host = BlueJsChildHost::default();
    let source = "function inner() { let a = 1; throw new Error('boom'); } inner();";
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: debugger_document(1, vec![classic(0, source)]),
        }),
        PageHostReply::Synchronized { reports, .. } if reports.is_empty()
    ));
    let program = match host.handle_request(PageHostRequest::ListDebuggerPrograms {
        tab_id: 7,
        document_generation: 1,
    }) {
        PageHostReply::DebuggerPrograms { programs, .. } => programs[0],
        reply => panic!("expected one private program: {reply:?}"),
    };
    let target = match host.handle_request(PageHostRequest::ListDebuggerSafePoints {
        tab_id: 7,
        document_generation: 1,
        program,
    }) {
        PageHostReply::DebuggerSafePoints { safe_points, .. } => safe_points
            .into_iter()
            .find(|point| point.code_unit_ordinal == 1 && point.bytecode_offset == 0)
            .unwrap(),
        reply => panic!("expected child safe points: {reply:?}"),
    };
    host.arm_debugger_nested_target(7, 1, target).unwrap();
    assert!(matches!(
        host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::DebuggerExecutionAdvanced { reports, .. } if reports.is_empty()
    ));
    let frame = match host.documents[&7].debugger_execution_states[&program] {
        ChildDebuggerExecutionStatus::NestedPaused { frame, .. } => frame,
        status => panic!("the actual inner invocation must pause: {status:?}"),
    };
    host.request_debugger_nested_advance(7, 1, frame, true)
        .unwrap();
    assert!(matches!(
        host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::DebuggerExecutionAdvanced { reports, .. }
            if matches!(
                reports.as_slice(),
                [PageHostScriptReport {
                    outcome: PageHostScriptOutcome::Rejected { .. },
                    ..
                }]
            )
    ));
    assert_eq!(
        host.documents[&7].debugger_execution_states[&program],
        ChildDebuggerExecutionStatus::Completed
    );
}

#[test]
fn private_child_bluets_module_nested_target_completes_when_never_reached() {
    let mut host = BlueJsChildHost::default();
    let entry = "blueice://page/error-paths-entry.ts";
    let dependency = "blueice://page/error-paths-dependency.ts";
    let module = PageHostScript {
        ordinal: 0,
        language: PageHostScriptLanguage::BlueTs,
        kind: PageHostScriptKind::Module,
        graph: graph(
            entry,
            vec![
                PageHostSource::new(
                    entry,
                    "function inner(): number { let a: number = 1; return a; } globalThis.entryDone = 1;",
                ),
                PageHostSource::new(dependency, "export const unused: number = 0;"),
            ],
        ),
    };
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: debugger_document(1, vec![module]),
        }),
        PageHostReply::Synchronized { reports, .. } if reports.is_empty()
    ));
    let program = host.documents[&7]
        .pending_debugger_executions
        .front()
        .unwrap()
        .program
        .unwrap();
    let target = match host.handle_request(PageHostRequest::ListDebuggerSafePoints {
        tab_id: 7,
        document_generation: 1,
        program,
    }) {
        PageHostReply::DebuggerSafePoints { safe_points, .. } => safe_points
            .into_iter()
            .find(|point| point.code_unit_ordinal == 1 && point.bytecode_offset == 0)
            .unwrap(),
        reply => panic!("expected module child points: {reply:?}"),
    };
    host.arm_debugger_nested_target(7, 1, target).unwrap();
    assert!(matches!(
        host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::DebuggerExecutionAdvanced { reports, .. }
            if matches!(
                reports.as_slice(),
                [PageHostScriptReport {
                    outcome: PageHostScriptOutcome::Executed,
                    ..
                }]
            )
    ));
    assert_eq!(
        host.documents[&7].debugger_execution_states[&program],
        ChildDebuggerExecutionStatus::Completed
    );
}

#[test]
fn private_child_bluets_module_nested_target_rejects_an_uncaught_throw_before_reaching_it() {
    let mut host = BlueJsChildHost::default();
    let entry = "blueice://page/error-paths-throw-entry.ts";
    let dependency = "blueice://page/error-paths-throw-dependency.ts";
    let module = PageHostScript {
        ordinal: 0,
        language: PageHostScriptLanguage::BlueTs,
        kind: PageHostScriptKind::Module,
        graph: graph(
            entry,
            vec![
                PageHostSource::new(
                    entry,
                    "function inner(): number { let a: number = 1; return a; } function fails(): number { throw 'early'; } fails();",
                ),
                PageHostSource::new(dependency, "export const unused: number = 0;"),
            ],
        ),
    };
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: debugger_document(1, vec![module]),
        }),
        PageHostReply::Synchronized { reports, .. } if reports.is_empty()
    ));
    let program = host.documents[&7]
        .pending_debugger_executions
        .front()
        .unwrap()
        .program
        .unwrap();
    let target = match host.handle_request(PageHostRequest::ListDebuggerSafePoints {
        tab_id: 7,
        document_generation: 1,
        program,
    }) {
        PageHostReply::DebuggerSafePoints { safe_points, .. } => safe_points
            .into_iter()
            .find(|point| point.code_unit_ordinal == 1 && point.bytecode_offset == 0)
            .unwrap(),
        reply => panic!("expected module child points: {reply:?}"),
    };
    host.arm_debugger_nested_target(7, 1, target).unwrap();
    assert!(matches!(
        host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::DebuggerExecutionAdvanced { reports, .. }
            if matches!(
                reports.as_slice(),
                [PageHostScriptReport {
                    outcome: PageHostScriptOutcome::Rejected { .. },
                    ..
                }]
            )
    ));
    assert_eq!(
        host.documents[&7].debugger_execution_states[&program],
        ChildDebuggerExecutionStatus::Completed
    );
}

#[test]
fn private_child_bluets_source_step_completes_the_root_script() {
    let mut host = BlueJsChildHost::default();
    let source = "let first: number = 1;";
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: debugger_document(1, vec![blue_ts_classic(0, source)]),
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
    let (source_id, target) = {
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
        let entry = entries.last().expect("fixture needs a bound root span");
        let source_id = retained
            .static_info()
            .sources
            .iter()
            .find(|source| source.module == entry.source)
            .unwrap()
            .id
            .0;
        (
            source_id,
            PageHostDebuggerSafePoint {
                program,
                code_unit_ordinal: 0,
                bytecode_offset: entry.bytecode_offset,
            },
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
    assert_eq!(
        host.handle_request(PageHostRequest::StepDebuggerBlueTsSourceSpan {
            tab_id: 7,
            document_generation: 1,
            metadata,
            source_id,
            safe_point: target,
        }),
        PageHostReply::DebuggerBlueTsSourceStepRequested {
            tab_id: 7,
            document_generation: 1,
            metadata,
            source_id,
            safe_point: target,
        }
    );
    let mut completed = false;
    for _ in 0..MAX_BLUETS_SOURCE_STEP_ROOT_INSTRUCTIONS {
        match host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
            tab_id: 7,
            document_generation: 1,
        }) {
            PageHostReply::DebuggerExecutionAdvanced { reports, .. } if reports.is_empty() => {}
            PageHostReply::DebuggerExecutionAdvanced { reports, .. }
                if matches!(
                    reports.as_slice(),
                    [PageHostScriptReport {
                        outcome: PageHostScriptOutcome::Executed,
                        ..
                    }]
                ) =>
            {
                completed = true;
                break;
            }
            reply => panic!("unexpected source-step outcome: {reply:?}"),
        }
    }
    assert!(
        completed,
        "a source step armed at the script's last statement must run to natural completion"
    );
    assert_eq!(
        host.documents[&7].debugger_execution_states[&program],
        ChildDebuggerExecutionStatus::Completed
    );
}

#[test]
fn private_child_bluets_source_step_rejects_an_uncaught_throw() {
    let mut host = BlueJsChildHost::default();
    let source = "function fails(): number { throw 'boom'; } let first: number = 1; fails();";
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: debugger_document(1, vec![blue_ts_classic(0, source)]),
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
    let (source_id, target) = {
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
        let entry = entries.first().expect("fixture needs a bound root span");
        let source_id = retained
            .static_info()
            .sources
            .iter()
            .find(|source| source.module == entry.source)
            .unwrap()
            .id
            .0;
        (
            source_id,
            PageHostDebuggerSafePoint {
                program,
                code_unit_ordinal: 0,
                bytecode_offset: entry.bytecode_offset,
            },
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
    // A source-span step only walks to the *next* bound statement
    // boundary (a source-line "step over"), so reaching a call's own
    // uncaught throw can take several successive source steps -- one
    // per statement boundary crossed along the way.
    let mut next_target = target;
    let mut rejected = false;
    'source_steps: for _ in 0..8 {
        assert_eq!(
            host.handle_request(PageHostRequest::StepDebuggerBlueTsSourceSpan {
                tab_id: 7,
                document_generation: 1,
                metadata,
                source_id,
                safe_point: next_target,
            }),
            PageHostReply::DebuggerBlueTsSourceStepRequested {
                tab_id: 7,
                document_generation: 1,
                metadata,
                source_id,
                safe_point: next_target,
            }
        );
        for _ in 0..MAX_BLUETS_SOURCE_STEP_ROOT_INSTRUCTIONS {
            match host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
                tab_id: 7,
                document_generation: 1,
            }) {
                PageHostReply::DebuggerExecutionAdvanced { reports, .. } if reports.is_empty() => {}
                PageHostReply::DebuggerExecutionAdvanced { reports, .. }
                    if matches!(
                        reports.as_slice(),
                        [PageHostScriptReport {
                            outcome: PageHostScriptOutcome::Rejected { .. },
                            ..
                        }]
                    ) =>
                {
                    rejected = true;
                    break 'source_steps;
                }
                reply => panic!("unexpected source-step outcome: {reply:?}"),
            }
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
                    assert_ne!(safe_point, next_target);
                    next_target = safe_point;
                    continue 'source_steps;
                }
                reply => panic!("unexpected execution state mid source-step: {reply:?}"),
            }
        }
        panic!("source step never resolved within its fixed root-instruction budget");
    }
    assert!(
        rejected,
        "an uncaught throw during a source-span step must reject the script"
    );
    assert_eq!(
        host.documents[&7].debugger_execution_states[&program],
        ChildDebuggerExecutionStatus::Completed
    );
}

#[test]
fn private_child_bluets_module_root_step_rejects_an_uncaught_throw() {
    let mut host = BlueJsChildHost::default();
    let entry = "blueice://page/error-paths-module-root-step-entry.ts";
    let dependency = "blueice://page/error-paths-module-root-step-dependency.ts";
    let module = PageHostScript {
        ordinal: 0,
        language: PageHostScriptLanguage::BlueTs,
        kind: PageHostScriptKind::Module,
        graph: graph(
            entry,
            vec![
                PageHostSource::new(
                    entry,
                    "function fails(): number { throw 'boom'; } fails();",
                ),
                PageHostSource::new(dependency, "export const unused: number = 0;"),
            ],
        ),
    };
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
        panic!("the entry module remains pending");
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
    let mut rejected = false;
    for _ in 0..32 {
        assert!(matches!(
            host.handle_request(PageHostRequest::StepDebuggerRootInstruction {
                tab_id: 7,
                document_generation: 1,
                program,
            }),
            PageHostReply::DebuggerExecutionStepRequested { .. }
        ));
        match host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
            tab_id: 7,
            document_generation: 1,
        }) {
            PageHostReply::DebuggerExecutionAdvanced { reports, .. } if reports.is_empty() => {}
            PageHostReply::DebuggerExecutionAdvanced { reports, .. }
                if matches!(
                    reports.as_slice(),
                    [PageHostScriptReport {
                        outcome: PageHostScriptOutcome::Rejected { .. },
                        ..
                    }]
                ) =>
            {
                rejected = true;
                break;
            }
            reply => panic!("unexpected module root step outcome: {reply:?}"),
        }
    }
    assert!(
        rejected,
        "an uncaught throw during a module root single-step must reject the script"
    );
    assert_eq!(
        host.documents[&7].debugger_execution_states[&program],
        ChildDebuggerExecutionStatus::Completed
    );
}

#[test]
fn private_child_bluets_linked_resume_rejects_an_uncaught_throw_in_the_dependency() {
    let entry = "blueice://page/error-paths-linked-resume-entry.ts";
    let dependency = "blueice://page/error-paths-linked-resume-dependency.ts";
    let mut module = PageHostScript {
        ordinal: 0,
        language: PageHostScriptLanguage::BlueTs,
        kind: PageHostScriptKind::Module,
        graph: graph(
            entry,
            vec![
                PageHostSource::new(
                    entry,
                    "import { fails } from './error-paths-linked-resume-dependency.ts'; export const answer: number = fails();",
                ),
                PageHostSource::new(
                    dependency,
                    "export function fails(): number { throw 'boom'; }",
                ),
            ],
        ),
    };
    module.graph.resolutions.push(PageHostStaticResolution {
        from_module: entry.to_string(),
        specifier: "./error-paths-linked-resume-dependency.ts".to_string(),
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
    let wire_frame = match host.handle_request(PageHostRequest::GetDebuggerExecutionState {
        tab_id: 7,
        document_generation: 1,
        program: entry_program,
    }) {
        PageHostReply::DebuggerLinkedExecutionState {
            frame,
            state: PageHostDebuggerLinkedExecutionState::Paused { .. },
        } => frame,
        reply => panic!("expected a linked pause inside the dependency: {reply:?}"),
    };
    assert_eq!(
        host.handle_request(PageHostRequest::ResumeDebuggerLinkedNestedExecution {
            frame: wire_frame,
        }),
        PageHostReply::DebuggerLinkedNestedResumeRequested { frame: wire_frame }
    );
    assert!(matches!(
        host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::DebuggerExecutionAdvanced { reports, .. }
            if matches!(
                reports.as_slice(),
                [PageHostScriptReport {
                    outcome: PageHostScriptOutcome::Rejected { .. },
                    ..
                }]
            )
    ));
    assert_eq!(
        host.documents[&7].debugger_execution_states[&entry_program],
        ChildDebuggerExecutionStatus::Completed
    );
}
