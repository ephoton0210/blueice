// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn private_child_classic_scheduler_steps_only_its_paused_nested_invocation() {
    let mut host = BlueJsChildHost::default();
    let source =
        "var calls = 0; function inner() { calls++; return 4; } globalThis.answer = inner() + 1;";
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
            .expect("the inner function has a first verified instruction"),
        reply => panic!("expected child safe points: {reply:?}"),
    };
    host.arm_debugger_nested_target(7, 1, target).unwrap();
    assert!(host.arm_debugger_nested_target(7, 1, target).is_err());
    let root = match host.handle_request(PageHostRequest::ListDebuggerSafePoints {
        tab_id: 7,
        document_generation: 1,
        program,
    }) {
        PageHostReply::DebuggerSafePoints { safe_points, .. } => safe_points
            .into_iter()
            .find(|point| point.code_unit_ordinal == 0)
            .unwrap(),
        reply => panic!("expected root safe points: {reply:?}"),
    };
    assert!(matches!(
        host.handle_request(PageHostRequest::ArmDebuggerRootSafePointBreakpoint {
            tab_id: 7,
            document_generation: 1,
            safe_point: root,
        }),
        PageHostReply::Error { .. }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::DebuggerExecutionAdvanced { reports, .. } if reports.is_empty()
    ));
    let frame = match host.documents[&7].debugger_execution_states[&program] {
        ChildDebuggerExecutionStatus::NestedPaused { frame, safe_point } => {
            assert_eq!(safe_point, target);
            frame
        }
        status => panic!("the actual inner invocation must pause: {status:?}"),
    };
    assert!(matches!(
        host.handle_request(PageHostRequest::StepDebuggerRootInstruction {
            tab_id: 7,
            document_generation: 1,
            program,
        }),
        PageHostReply::Error { .. }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::ResumeDebuggerExecution {
            tab_id: 7,
            document_generation: 1,
            program,
        }),
        PageHostReply::Error { .. }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::GetDebuggerExecutionState {
            tab_id: 7,
            document_generation: 1,
            program,
        }),
        PageHostReply::DebuggerExecutionState {
            state: PageHostDebuggerExecutionState::NestedPaused {
                frame: wire_frame,
                safe_point,
            },
            ..
        } if wire_frame == child_debugger_frame(7, 1, program, frame)
            && safe_point == target
    ));
    assert!(host
        .request_debugger_nested_advance(7, 2, frame, false)
        .is_err());
    let mut returned = false;
    for _ in 0..96 {
        host.request_debugger_nested_advance(7, 1, frame, false)
            .unwrap();
        assert!(matches!(
            host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
                tab_id: 7,
                document_generation: 1,
            }),
            PageHostReply::DebuggerExecutionAdvanced { reports, .. } if reports.is_empty()
        ));
        match host.documents[&7].debugger_execution_states[&program] {
            ChildDebuggerExecutionStatus::NestedPaused {
                frame: same_frame,
                safe_point,
            } => {
                assert_eq!(same_frame, frame);
                host.exact_debugger_safe_point(7, 1, safe_point).unwrap();
            }
            ChildDebuggerExecutionStatus::Paused(root) => {
                assert_eq!(root.code_unit_ordinal, 0);
                host.exact_debugger_safe_point(7, 1, root).unwrap();
                returned = true;
                break;
            }
            status => panic!("unexpected nested step: {status:?}"),
        }
    }
    assert!(returned);
    assert!(host
        .request_debugger_nested_advance(7, 1, frame, false)
        .is_err());
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
fn child_nested_wire_requires_the_exact_live_frame_and_reports_step_state() {
    let mut host = BlueJsChildHost::default();
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: debugger_document(
                1,
                vec![classic(0, "function inner() { return 4; } inner();")],
            ),
        }),
        PageHostReply::Synchronized { reports, .. } if reports.is_empty()
    ));
    let program = match host.handle_request(PageHostRequest::ListDebuggerPrograms {
        tab_id: 7,
        document_generation: 1,
    }) {
        PageHostReply::DebuggerPrograms { programs, .. } => programs[0],
        reply => panic!("expected a child program: {reply:?}"),
    };
    let point = match host.handle_request(PageHostRequest::ListDebuggerSafePoints {
        tab_id: 7,
        document_generation: 1,
        program,
    }) {
        PageHostReply::DebuggerSafePoints { safe_points, .. } => safe_points
            .into_iter()
            .find(|point| point.code_unit_ordinal == 1 && point.bytecode_offset == 0)
            .unwrap(),
        reply => panic!("expected child points: {reply:?}"),
    };
    assert_eq!(
        host.handle_request(PageHostRequest::ArmDebuggerNestedSafePointBreakpoint {
            tab_id: 7,
            document_generation: 1,
            safe_point: point,
        }),
        PageHostReply::DebuggerNestedSafePointBreakpointArmed {
            tab_id: 7,
            document_generation: 1,
            safe_point: point,
        }
    );
    assert!(matches!(
        host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::DebuggerExecutionAdvanced { reports, .. } if reports.is_empty()
    ));
    let frame = match host.handle_request(PageHostRequest::GetDebuggerExecutionState {
        tab_id: 7,
        document_generation: 1,
        program,
    }) {
        PageHostReply::DebuggerExecutionState {
            state: PageHostDebuggerExecutionState::NestedPaused { frame, safe_point },
            ..
        } => {
            assert_eq!(safe_point, point);
            frame
        }
        reply => panic!("expected active frame state: {reply:?}"),
    };
    assert!(frame.is_well_formed());
    let snapshot = |frame: Option<PageHostDebuggerFrame>, max_frames, max_scope_entries| {
        PageHostRequest::GetDebuggerStackSnapshot {
            tab_id: 7,
            document_generation: 1,
            program,
            frame,
            max_frames,
            max_scope_entries,
        }
    };
    assert!(matches!(
        host.handle_request(snapshot(Some(frame), 1, 1)),
        PageHostReply::DebuggerStackSnapshot {
            frame: Some(reply_frame),
            snapshot: PageHostDebuggerStackSnapshot { frames, stack_truncated: true },
            ..
        } if reply_frame == frame && frames.len() == 1 && frames[0].code_unit_ordinal == 1
    ));
    assert!(matches!(
        host.handle_request(snapshot(Some(frame), 2, 256)),
        PageHostReply::DebuggerStackSnapshot {
            snapshot: PageHostDebuggerStackSnapshot { frames, stack_truncated: false },
            ..
        } if frames.len() == 2 && frames[0].code_unit_ordinal == 1 && frames[1].code_unit_ordinal == 0
    ));
    assert!(matches!(
        host.handle_request(snapshot(None, 2, 256)),
        PageHostReply::Error { .. }
    ));
    assert!(matches!(
        host.handle_request(snapshot(Some(frame), 0, 1)),
        PageHostReply::Error { .. }
    ));
    assert!(matches!(
        host.handle_request(snapshot(Some(frame), 2, 257)),
        PageHostReply::Error { .. }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::GetDebuggerStackSnapshot {
            tab_id: 7,
            document_generation: 1,
            program: PageHostDebuggerProgram {
                program_generation: program.program_generation + 1,
                ..program
            },
            frame: Some(frame),
            max_frames: 2,
            max_scope_entries: 256,
        }),
        PageHostReply::Error { .. }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::GetDebuggerStackSnapshot {
            tab_id: 7,
            document_generation: 2,
            program,
            frame: Some(frame),
            max_frames: 2,
            max_scope_entries: 256,
        }),
        PageHostReply::Error {
            code: PageHostErrorCode::StaleDocument,
            ..
        }
    ));
    for wrong in [
        PageHostDebuggerFrame {
            invocation_serial: frame.invocation_serial + 1,
            ..frame
        },
        PageHostDebuggerFrame {
            code_unit_ordinal: frame.code_unit_ordinal + 1,
            ..frame
        },
        PageHostDebuggerFrame {
            document_generation: frame.document_generation + 1,
            ..frame
        },
        PageHostDebuggerFrame { tab_id: 8, ..frame },
    ] {
        assert!(matches!(
            host.handle_request(PageHostRequest::StepDebuggerNestedInstruction { frame: wrong }),
            PageHostReply::Error { .. }
        ));
        assert!(matches!(
            host.handle_request(snapshot(Some(wrong), 2, 256)),
            PageHostReply::Error { .. }
        ));
    }
    assert_eq!(
        host.handle_request(PageHostRequest::StepDebuggerNestedInstruction { frame }),
        PageHostReply::DebuggerNestedStepRequested { frame }
    );
    assert!(matches!(
        host.handle_request(PageHostRequest::GetDebuggerExecutionState {
            tab_id: 7,
            document_generation: 1,
            program,
        }),
        PageHostReply::DebuggerExecutionState {
            state: PageHostDebuggerExecutionState::NestedStepping {
                frame: same_frame,
            },
            ..
        } if same_frame == frame
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::StepDebuggerNestedInstruction { frame }),
        PageHostReply::Error { .. }
    ));
    assert!(matches!(
        host.handle_request(snapshot(Some(frame), 2, 256)),
        PageHostReply::Error { .. }
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
            state: PageHostDebuggerExecutionState::NestedPaused { frame: same, .. },
            ..
        } if same == frame
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::ResumeDebuggerNestedExecution {
            frame: PageHostDebuggerFrame {
                invocation_serial: frame.invocation_serial + 1,
                ..frame
            },
        }),
        PageHostReply::Error { .. }
    ));
    assert_eq!(
        host.handle_request(PageHostRequest::ResumeDebuggerNestedExecution { frame }),
        PageHostReply::DebuggerNestedResumeRequested { frame }
    );
    assert!(matches!(
        host.handle_request(PageHostRequest::GetDebuggerExecutionState {
            tab_id: 7,
            document_generation: 1,
            program,
        }),
        PageHostReply::DebuggerExecutionState {
            state: PageHostDebuggerExecutionState::NestedResuming { frame: same },
            ..
        } if same == frame
    ));
    assert!(matches!(
        host.handle_request(snapshot(Some(frame), 2, 256)),
        PageHostReply::Error { .. }
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
        } if safe_point.code_unit_ordinal == 0
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::ResumeDebuggerNestedExecution { frame }),
        PageHostReply::Error { .. }
    ));
    assert!(matches!(
        host.handle_request(snapshot(None, 2, 256)),
        PageHostReply::DebuggerStackSnapshot { frame: None, snapshot: PageHostDebuggerStackSnapshot { frames, stack_truncated: false }, .. }
            if frames.len() == 1 && frames[0].code_unit_ordinal == 0
    ));
}

#[test]
fn private_child_bluets_classic_uses_the_same_nested_frame_scheduler() {
    let mut host = BlueJsChildHost::default();
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: debugger_document(
                1,
                vec![blue_ts_classic(
                    0,
                    "function inner(): number { return 4; } globalThis.answer = inner() + 1;",
                )],
            ),
        }),
        PageHostReply::Synchronized { reports, .. } if reports.is_empty()
    ));
    let program = match host.handle_request(PageHostRequest::ListDebuggerPrograms {
        tab_id: 7,
        document_generation: 1,
    }) {
        PageHostReply::DebuggerPrograms { programs, .. } => programs[0],
        reply => panic!("expected one BlueTS program: {reply:?}"),
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
        reply => panic!("expected BlueTS child points: {reply:?}"),
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
        status => panic!("BlueTS child must pause: {status:?}"),
    };
    host.request_debugger_nested_advance(7, 1, frame, false)
        .unwrap();
    assert!(matches!(
        host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::DebuggerExecutionAdvanced { reports, .. } if reports.is_empty()
    ));
    assert!(matches!(
        host.documents[&7].debugger_execution_states[&program],
        ChildDebuggerExecutionStatus::NestedPaused { frame: same, .. } if same == frame
    ));
}

#[test]
fn private_child_classic_rejects_an_unsupported_deeper_nested_target() {
    let mut host = BlueJsChildHost::default();
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: debugger_document(
                1,
                vec![classic(
                    0,
                    "function inner() { return 4; } function outer() { return inner(); } outer();",
                )],
            ),
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
        PageHostReply::DebuggerExecutionAdvanced { reports, .. }
            if matches!(reports.as_slice(), [PageHostScriptReport {
                outcome: PageHostScriptOutcome::Rejected { .. }, ..
            }])
    ));
    assert_eq!(
        host.documents[&7].debugger_execution_states[&program],
        ChildDebuggerExecutionStatus::Completed
    );
}

#[test]
fn private_child_bluets_module_nested_frame_rejoins_its_linked_entry_once() {
    let entry = "blueice://page/nested-entry.ts";
    let dependency = "blueice://page/nested-dependency.ts";
    let mut module = PageHostScript {
        ordinal: 0,
        language: PageHostScriptLanguage::BlueTs,
        kind: PageHostScriptKind::Module,
        graph: graph(
            entry,
            vec![
                PageHostSource::new(
                    entry,
                    "import { value } from './nested-dependency.ts'; function inner(): number { return value + 1; } globalThis.moduleAnswer = inner();",
                ),
                PageHostSource::new(
                    dependency,
                    "globalThis.depRuns = (globalThis.depRuns || 0) + 1; export const value: number = 40;",
                ),
            ],
        ),
    };
    module.graph.resolutions.push(PageHostStaticResolution {
        from_module: entry.to_string(),
        specifier: "./nested-dependency.ts".to_string(),
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
        PageHostReply::DebuggerExecutionAdvanced { reports, .. } if reports.is_empty()
    ));
    let frame = match host.documents[&7].debugger_execution_states[&program] {
        ChildDebuggerExecutionStatus::NestedPaused { frame, safe_point } => {
            assert_eq!(safe_point, target);
            frame
        }
        status => panic!("the module entry child must pause: {status:?}"),
    };
    assert!(matches!(
        host.handle_request(PageHostRequest::StepDebuggerRootInstruction {
            tab_id: 7,
            document_generation: 1,
            program,
        }),
        PageHostReply::Error { .. }
    ));
    host.request_debugger_nested_advance(7, 1, frame, false)
        .unwrap();
    assert!(matches!(
        host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::DebuggerExecutionAdvanced { reports, .. } if reports.is_empty()
    ));
    assert!(matches!(
        host.documents[&7].debugger_execution_states[&program],
        ChildDebuggerExecutionStatus::NestedPaused { frame: same, .. } if same == frame
    ));
    let child_frame = child_debugger_frame(7, 1, program, frame);
    assert_eq!(
        host.handle_request(PageHostRequest::ResumeDebuggerNestedExecution { frame: child_frame }),
        PageHostReply::DebuggerNestedResumeRequested { frame: child_frame }
    );
    assert!(matches!(
        host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::DebuggerExecutionAdvanced { reports, .. } if reports.is_empty()
    ));
    let ChildDebuggerExecutionStatus::Paused(root) =
        host.documents[&7].debugger_execution_states[&program]
    else {
        panic!("module child resume must park its original root");
    };
    assert_eq!(root.code_unit_ordinal, 0);
    host.exact_debugger_safe_point(7, 1, root).unwrap();
    assert!(matches!(
        host.handle_request(PageHostRequest::ResumeDebuggerNestedExecution { frame: child_frame }),
        PageHostReply::Error { .. }
    ));
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
    let origin = BlueJsPageOrigin::new("https://example.test").unwrap();
    let probe = host
        .runtime
        .install_program(
            7,
            &origin,
            BlueJsSourceIdentity::new("page:///nested-probe.js", "sha256:nested-probe").unwrap(),
            &BlueJsProgramV1::Script(
                parse("globalThis.depRuns + globalThis.moduleAnswer").unwrap(),
            ),
        )
        .unwrap();
    assert_eq!(
        host.runtime.execute_program(7, probe),
        Ok(Value::Number(42.0))
    );
}
