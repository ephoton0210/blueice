// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn private_system_scheduler_binds_the_earlier_loader_without_public_stack_leak() {
    let frozen = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../bluets/tests/fixtures/k11_source_maps/system_wrapper.ts"
    ));
    let source = format!("{frozen}\ndeclare const console: {{ log(value: number): void }};\n");
    let loader = "var loaderCalls = 0; var consoleCalls = 0; var console = { log: function(value) { consoleCalls++; if (value !== 42) throw 7; } }; var System = { register: function(dependencies, declare) { loaderCalls++; var exports = {}; var body = declare(function(name, value) { exports[name] = value; return value; }, { id: 'main' }); body.execute(); if (exports.answer !== 42) throw 8; } };";
    let mut selected = debugger_document(
        1,
        vec![
            classic(0, loader),
            blue_ts_emitted_classic(1, &source),
            classic(2, "if (loaderCalls !== 1 || consoleCalls !== 1) throw 9;"),
        ],
    );
    selected.emitted_bluets_profile = PageHostBlueTsEmissionProfile::System;
    let mut host = BlueJsChildHost::default();
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument { document: selected }),
        PageHostReply::Synchronized { reports, .. } if reports.is_empty()
    ));
    let programs = match host.handle_request(PageHostRequest::ListDebuggerPrograms {
        tab_id: 7,
        document_generation: 1,
    }) {
        PageHostReply::DebuggerPrograms { programs, .. } => programs,
        reply => panic!("expected three installed programs: {reply:?}"),
    };
    assert_eq!(programs.len(), 3);
    let loader_program = programs[0];
    let emitted_program = programs[1];
    let bound = &host.documents[&7].emitted_maps[&emitted_program];
    let token = bound.generated.source.rfind("console.log(").unwrap();
    let statement_index = bound
        .top_level_spans
        .iter()
        .position(|span| span.start_byte <= token && token < span.end_byte)
        .unwrap();
    let emitted_handle =
        host.documents[&7].debugger_programs[&emitted_program.program_handle].runtime_handle;
    let loader_handle =
        host.documents[&7].debugger_programs[&loader_program.program_handle].runtime_handle;
    let point = host
        .runtime
        .program_registry()
        .get(emitted_handle)
        .unwrap()
        .unique_identifier_safe_point(statement_index, "console")
        .unwrap();
    let target = PageHostDebuggerSafePoint {
        program: emitted_program,
        code_unit_ordinal: point.code_unit.ordinal(),
        bytecode_offset: point.bytecode_offset,
    };
    assert_ne!(target.code_unit_ordinal, 0);
    let before_token = &bound.generated.source[..token];
    let generated_line = before_token.bytes().filter(|byte| *byte == b'\n').count() as u32;
    let generated_column_utf16 = before_token
        .rsplit('\n')
        .next()
        .unwrap()
        .encode_utf16()
        .count() as u32;
    assert!(matches!(
        host.handle_request(PageHostRequest::ResolveDebuggerEmittedSafePoint {
            tab_id: 7,
            document_generation: 1,
            program: emitted_program,
            generated_line,
            generated_column_utf16,
        }),
        PageHostReply::DebuggerEmittedSafePoint {
            safe_point: None,
            ..
        }
    ));
    assert_eq!(
        host.handle_request(PageHostRequest::ResolveDebuggerSystemEmittedSafePoint {
            tab_id: 7,
            document_generation: 1,
            loader_program,
            program: emitted_program,
            generated_line,
            generated_column_utf16,
        }),
        PageHostReply::DebuggerSystemEmittedSafePoint {
            tab_id: 7,
            document_generation: 1,
            loader_program,
            program: emitted_program,
            generated_line,
            generated_column_utf16,
            safe_point: Some(target),
        }
    );
    let (wrapper_line, wrapper_column_utf16) = {
        let generated = &host.documents[&7].emitted_maps[&emitted_program]
            .generated
            .source;
        let before_wrapper = &generated[..generated.find("System.register").unwrap()];
        (
            before_wrapper.bytes().filter(|byte| *byte == b'\n').count() as u32,
            before_wrapper
                .rsplit('\n')
                .next()
                .unwrap()
                .encode_utf16()
                .count() as u32,
        )
    };
    assert!(matches!(
        host.handle_request(PageHostRequest::ResolveDebuggerSystemEmittedSafePoint {
            tab_id: 7,
            document_generation: 1,
            loader_program,
            program: emitted_program,
            generated_line: wrapper_line,
            generated_column_utf16: wrapper_column_utf16,
        }),
        PageHostReply::DebuggerSystemEmittedSafePoint {
            safe_point: None,
            ..
        }
    ));
    assert!(host.documents[&7].debugger_breakpoints.is_empty());
    for wrong_loader in [emitted_program, programs[2]] {
        assert!(matches!(
            host.handle_request(PageHostRequest::ResolveDebuggerSystemEmittedSafePoint {
                tab_id: 7,
                document_generation: 1,
                loader_program: wrong_loader,
                program: emitted_program,
                generated_line,
                generated_column_utf16,
            }),
            PageHostReply::Error { .. }
        ));
    }
    assert!(matches!(
        host.handle_request(PageHostRequest::ResolveDebuggerSystemEmittedSafePoint {
            tab_id: 7,
            document_generation: 1,
            loader_program: PageHostDebuggerProgram {
                program_generation: loader_program.program_generation + 1,
                ..loader_program
            },
            program: emitted_program,
            generated_line,
            generated_column_utf16,
        }),
        PageHostReply::Error { .. }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::ResolveDebuggerSystemEmittedSafePoint {
            tab_id: 7,
            document_generation: 2,
            loader_program,
            program: emitted_program,
            generated_line,
            generated_column_utf16,
        }),
        PageHostReply::Error { .. }
    ));
    assert!(host.documents[&7].debugger_breakpoints.is_empty());
    assert!(host.arm_debugger_nested_target(7, 1, target).is_err());
    assert!(host.documents[&7].debugger_breakpoints.is_empty());
    assert!(matches!(
        host.handle_request(PageHostRequest::ArmDebuggerSystemNestedBreakpoint {
            tab_id: 7,
            document_generation: 2,
            loader_program,
            safe_point: target,
        }),
        PageHostReply::Error { .. }
    ));
    assert!(host.documents[&7].debugger_breakpoints.is_empty());
    assert!(matches!(
        host.handle_request(PageHostRequest::ArmDebuggerSystemNestedBreakpoint {
            tab_id: 7,
            document_generation: 1,
            loader_program: emitted_program,
            safe_point: target,
        }),
        PageHostReply::Error { .. }
    ));
    assert!(host.documents[&7].debugger_breakpoints.is_empty());
    assert!(matches!(
        host.handle_request(PageHostRequest::ArmDebuggerSystemNestedBreakpoint {
            tab_id: 7,
            document_generation: 1,
            loader_program: programs[2],
            safe_point: target,
        }),
        PageHostReply::Error { .. }
    ));
    assert!(host.documents[&7].debugger_breakpoints.is_empty());
    let stale_loader = PageHostDebuggerProgram {
        program_generation: loader_program.program_generation + 1,
        ..loader_program
    };
    assert!(matches!(
        host.handle_request(PageHostRequest::ArmDebuggerSystemNestedBreakpoint {
            tab_id: 7,
            document_generation: 1,
            loader_program: stale_loader,
            safe_point: target,
        }),
        PageHostReply::Error { .. }
    ));
    assert!(host.documents[&7].debugger_breakpoints.is_empty());
    assert_eq!(
        host.handle_request(PageHostRequest::ArmDebuggerSystemNestedBreakpoint {
            tab_id: 7,
            document_generation: 1,
            loader_program,
            safe_point: target,
        }),
        PageHostReply::DebuggerSystemNestedBreakpointArmed {
            tab_id: 7,
            document_generation: 1,
            loader_program,
            safe_point: target,
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
    let frame = match host.documents[&7].debugger_execution_states[&emitted_program] {
        ChildDebuggerExecutionStatus::NestedPaused { frame, safe_point } => {
            assert_eq!(safe_point, target);
            frame
        }
        status => panic!("System execute body must pause: {status:?}"),
    };
    assert!(matches!(
        host.handle_request(PageHostRequest::GetDebuggerStackSnapshot {
            tab_id: 7,
            document_generation: 1,
            program: emitted_program,
            frame: Some(child_debugger_frame(7, 1, emitted_program, frame)),
            max_frames: 3,
            max_scope_entries: 256,
        }),
        PageHostReply::Error { .. }
    ));
    let snapshot = host
        .runtime
        .debugger_foreign_intermediate_stack_snapshot(7, frame, loader_handle, 3, 256)
        .unwrap();
    assert_eq!(
        snapshot
            .frames
            .iter()
            .map(|frame| frame.program_generation)
            .collect::<Vec<_>>(),
        vec![
            emitted_handle.generation().as_u64(),
            loader_handle.generation().as_u64(),
            emitted_handle.generation().as_u64(),
        ]
    );
    let public_frame = child_debugger_frame(7, 1, emitted_program, frame);
    let PageHostReply::DebuggerSystemStackSnapshot {
        frame: echoed_frame,
        loader_program: echoed_loader,
        snapshot: private_stack,
    } = host.handle_request(PageHostRequest::GetDebuggerSystemStackSnapshot {
        frame: public_frame,
        loader_program,
        max_scope_entries: 256,
    })
    else {
        panic!("exact private System stack must be available");
    };
    assert_eq!(echoed_frame, public_frame);
    assert_eq!(echoed_loader, loader_program);
    assert!(private_stack.is_well_formed(public_frame, loader_program));
    assert_eq!(private_stack.frames[0].safe_point, target);
    assert_eq!(private_stack.frames[1].safe_point.program, loader_program);
    assert_eq!(private_stack.frames[2].safe_point.program, emitted_program);
    assert!(matches!(
        host.handle_request(PageHostRequest::GetDebuggerSystemStackSnapshot {
            frame: public_frame,
            loader_program: PageHostDebuggerProgram {
                program_generation: loader_program.program_generation + 1,
                ..loader_program
            },
            max_scope_entries: 256,
        }),
        PageHostReply::Error { .. }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::GetDebuggerSystemStackSnapshot {
            frame: PageHostDebuggerFrame {
                invocation_serial: public_frame.invocation_serial + 1,
                ..public_frame
            },
            loader_program,
            max_scope_entries: 256,
        }),
        PageHostReply::Error { .. }
    ));
    host.request_debugger_nested_advance(7, 1, frame, true)
        .unwrap();
    assert!(matches!(
        host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::DebuggerExecutionAdvanced { reports, .. } if reports.is_empty()
    ));
    assert!(matches!(
        host.documents[&7].debugger_execution_states[&emitted_program],
        ChildDebuggerExecutionStatus::Paused(_)
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::ResumeDebuggerExecution {
            tab_id: 7,
            document_generation: 1,
            program: emitted_program,
        }),
        PageHostReply::DebuggerExecutionResumed { .. }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::DebuggerExecutionAdvanced { reports, .. }
            if reports.len() == 2 && reports.iter().all(|report| report.outcome == PageHostScriptOutcome::Executed)
    ));
}

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
fn private_child_classic_rejects_a_target_beyond_two_callers() {
    let mut host = BlueJsChildHost::default();
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: debugger_document(
                1,
                vec![classic(
                    0,
                    "function inner() { return 4; } function middle() { return inner(); } function outer() { return middle(); } outer();",
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
