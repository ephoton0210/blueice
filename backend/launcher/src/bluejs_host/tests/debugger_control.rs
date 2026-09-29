// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn private_debugger_locations_are_generation_and_tab_bound_without_runtime_leaks() {
    let mut host = BlueJsChildHost::default();
    let first = document(1, vec![classic(0, "let answer = 42;")]);
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument { document: first }),
        PageHostReply::Synchronized { .. }
    ));
    let programs = match host.handle_request(PageHostRequest::ListDebuggerPrograms {
        tab_id: 7,
        document_generation: 1,
    }) {
        PageHostReply::DebuggerPrograms { programs, .. } => programs,
        reply => panic!("expected source-free child debugger programs, got {reply:?}"),
    };
    assert_eq!(programs.len(), 1);
    let program = programs[0];
    let safe_points = match host.handle_request(PageHostRequest::ListDebuggerSafePoints {
        tab_id: 7,
        document_generation: 1,
        program,
    }) {
        PageHostReply::DebuggerSafePoints { safe_points, .. } => safe_points,
        reply => panic!("expected source-free child debugger safe points, got {reply:?}"),
    };
    let safe_point = *safe_points
        .first()
        .expect("a retained classic program has a root safe point");
    assert_eq!(
        host.handle_request(PageHostRequest::ValidateDebuggerSafePoint {
            tab_id: 7,
            document_generation: 1,
            safe_point,
        }),
        PageHostReply::DebuggerSafePointValidated {
            tab_id: 7,
            document_generation: 1,
            safe_point,
        }
    );
    assert_eq!(
        host.handle_request(PageHostRequest::SetDebuggerBreakpoint {
            tab_id: 7,
            document_generation: 1,
            safe_point,
        }),
        PageHostReply::DebuggerBreakpointSet {
            tab_id: 7,
            document_generation: 1,
            safe_point,
        }
    );
    // Retries are idempotent and cannot consume a second bounded record.
    assert_eq!(
        host.handle_request(PageHostRequest::SetDebuggerBreakpoint {
            tab_id: 7,
            document_generation: 1,
            safe_point,
        }),
        PageHostReply::DebuggerBreakpointSet {
            tab_id: 7,
            document_generation: 1,
            safe_point,
        }
    );
    assert_eq!(
        host.handle_request(PageHostRequest::ListDebuggerBreakpoints {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::DebuggerBreakpoints {
            tab_id: 7,
            document_generation: 1,
            safe_points: vec![safe_point],
        }
    );
    assert_eq!(
        host.handle_request(PageHostRequest::ClearDebuggerBreakpoint {
            tab_id: 7,
            document_generation: 1,
            safe_point,
        }),
        PageHostReply::DebuggerBreakpointCleared {
            tab_id: 7,
            document_generation: 1,
            safe_point,
            was_present: true,
        }
    );
    assert_eq!(
        host.handle_request(PageHostRequest::ClearDebuggerBreakpoint {
            tab_id: 7,
            document_generation: 1,
            safe_point,
        }),
        PageHostReply::DebuggerBreakpointCleared {
            tab_id: 7,
            document_generation: 1,
            safe_point,
            was_present: false,
        }
    );

    let mut other = document(1, vec![classic(0, "let other = 7;")]);
    other.tab_id = 9;
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument { document: other }),
        PageHostReply::Synchronized { .. }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::ListDebuggerSafePoints {
            tab_id: 9,
            document_generation: 1,
            program,
        }),
        PageHostReply::Error {
            code: PageHostErrorCode::InvalidRequest,
            ..
        }
    ));

    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: document(2, vec![classic(0, "let successor = 1;")]),
        }),
        PageHostReply::Synchronized { .. }
    ));
    assert_eq!(
        host.handle_request(PageHostRequest::ListDebuggerBreakpoints {
            tab_id: 7,
            document_generation: 2,
        }),
        PageHostReply::DebuggerBreakpoints {
            tab_id: 7,
            document_generation: 2,
            safe_points: Vec::new(),
        }
    );
    assert!(matches!(
        host.handle_request(PageHostRequest::ListDebuggerPrograms {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::Error {
            code: PageHostErrorCode::StaleDocument,
            ..
        }
    ));
    let reply = format!(
        "{:?}",
        host.handle_request(PageHostRequest::ListDebuggerSafePoints {
            tab_id: 7,
            document_generation: 2,
            program,
        })
    );
    assert!(
        !reply.contains("answer") && !reply.contains("bytecode") && !reply.contains("Value"),
        "private debugger errors must remain source/value-free"
    );
}

#[test]
fn root_classic_breakpoint_pauses_and_resumes_without_vm_disclosure() {
    let mut host = BlueJsChildHost::default();
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: debugger_document(
                1,
                vec![classic(0, "let first = 1; first += 1; globalThis.answer = first;")],
            ),
        }),
        PageHostReply::Synchronized { reports, .. } if reports.is_empty()
    ));
    let program = match host.handle_request(PageHostRequest::ListDebuggerPrograms {
        tab_id: 7,
        document_generation: 1,
    }) {
        PageHostReply::DebuggerPrograms { programs, .. } => programs[0],
        reply => panic!("expected private classic program, got {reply:?}"),
    };
    let target = match host.handle_request(PageHostRequest::ListDebuggerSafePoints {
        tab_id: 7,
        document_generation: 1,
        program,
    }) {
        PageHostReply::DebuggerSafePoints { safe_points, .. } => safe_points
            .into_iter()
            .find(|safe_point| safe_point.code_unit_ordinal == 0 && safe_point.bytecode_offset != 0)
            .expect("fixture must have a non-entry root safe point"),
        reply => panic!("expected source-free safe points, got {reply:?}"),
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
    assert_eq!(
        host.handle_request(PageHostRequest::StepDebuggerRootInstruction {
            tab_id: 7,
            document_generation: 1,
            program,
        }),
        PageHostReply::DebuggerExecutionStepRequested {
            tab_id: 7,
            document_generation: 1,
            program,
        }
    );
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
            state: PageHostDebuggerExecutionState::Stepping,
        }
    );
    assert!(matches!(
        host.handle_request(PageHostRequest::StepDebuggerRootInstruction {
            tab_id: 7,
            document_generation: 1,
            program,
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
    let successor = match host.handle_request(PageHostRequest::GetDebuggerExecutionState {
        tab_id: 7,
        document_generation: 1,
        program,
    }) {
        PageHostReply::DebuggerExecutionState {
            state: PageHostDebuggerExecutionState::Paused { safe_point },
            ..
        } => safe_point,
        reply => panic!("one child root step must pause at its successor: {reply:?}"),
    };
    assert_ne!(successor, target);
    assert_eq!(successor.program, program);
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
            if reports == vec![script_report(
                7,
                1,
                0,
                PageHostScriptLanguage::JavaScript,
                PageHostScriptKind::Classic,
                PageHostScriptOutcome::Executed,
            )]
    ));
    let reply = format!(
        "{:?}",
        host.handle_request(PageHostRequest::GetDebuggerExecutionState {
            tab_id: 7,
            document_generation: 1,
            program,
        })
    );
    assert!(reply.contains("Completed"));
    assert!(
        !reply.contains("answer") && !reply.contains("Value") && !reply.contains("Vm"),
        "execution state remains source/value/VM-free"
    );
}

#[test]
fn root_classic_steps_revisit_loop_boundaries_without_releasing_the_queue() {
    let mut host = BlueJsChildHost::default();
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: debugger_document(
                1,
                vec![classic(
                    0,
                    "let index = 0; while (index < 2) { index++; } globalThis.done = index;",
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
        reply => panic!("expected queued classic program: {reply:?}"),
    };
    let safe_points = match host.handle_request(PageHostRequest::ListDebuggerSafePoints {
        tab_id: 7,
        document_generation: 1,
        program,
    }) {
        PageHostReply::DebuggerSafePoints { safe_points, .. } => safe_points,
        reply => panic!("expected verified root safe points: {reply:?}"),
    };
    let target = *safe_points
        .iter()
        .find(|point| point.code_unit_ordinal == 0 && point.bytecode_offset != 0)
        .unwrap();
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
    let mut seen_offsets = Vec::new();
    let mut completion_reports = None;
    for _ in 0..256 {
        assert!(matches!(
            host.handle_request(PageHostRequest::StepDebuggerRootInstruction {
                tab_id: 7,
                document_generation: 1,
                program,
            }),
            PageHostReply::DebuggerExecutionStepRequested { .. }
        ));
        assert!(matches!(
            host.handle_request(PageHostRequest::GetDebuggerExecutionState {
                tab_id: 7,
                document_generation: 1,
                program,
            }),
            PageHostReply::DebuggerExecutionState {
                state: PageHostDebuggerExecutionState::Stepping,
                ..
            }
        ));
        let reports = match host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
            tab_id: 7,
            document_generation: 1,
        }) {
            PageHostReply::DebuggerExecutionAdvanced { reports, .. } => reports,
            reply => panic!("expected one bounded child advance: {reply:?}"),
        };
        match host.handle_request(PageHostRequest::GetDebuggerExecutionState {
            tab_id: 7,
            document_generation: 1,
            program,
        }) {
            PageHostReply::DebuggerExecutionState {
                state: PageHostDebuggerExecutionState::Paused { safe_point },
                ..
            } => {
                assert!(safe_points.contains(&safe_point));
                seen_offsets.push(safe_point.bytecode_offset);
                assert!(reports.is_empty());
            }
            PageHostReply::DebuggerExecutionState {
                state: PageHostDebuggerExecutionState::Completed,
                ..
            } => {
                completion_reports = Some(reports);
                break;
            }
            reply => panic!("unexpected child step state: {reply:?}"),
        }
    }
    assert_eq!(
        completion_reports,
        Some(vec![script_report(
            7,
            1,
            0,
            PageHostScriptLanguage::JavaScript,
            PageHostScriptKind::Classic,
            PageHostScriptOutcome::Executed,
        )])
    );
    assert!(
        seen_offsets
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len()
            < seen_offsets.len(),
        "the loop must revisit a real root boundary"
    );
}

#[test]
fn private_debugger_breakpoint_configuration_is_idempotent_and_bounded() {
    let mut host = BlueJsChildHost::default();
    let source = (0..=PAGE_HOST_DEBUGGER_MAX_BREAKPOINTS_PER_REALM)
        .map(|index| format!("let breakpoint_{index} = {index};"))
        .collect::<String>();
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: document(1, vec![classic(0, &source)]),
        }),
        PageHostReply::Synchronized { .. }
    ));
    let program = match host.handle_request(PageHostRequest::ListDebuggerPrograms {
        tab_id: 7,
        document_generation: 1,
    }) {
        PageHostReply::DebuggerPrograms { programs, .. } => programs[0],
        reply => panic!("expected child debugger program, got {reply:?}"),
    };
    let safe_points = match host.handle_request(PageHostRequest::ListDebuggerSafePoints {
        tab_id: 7,
        document_generation: 1,
        program,
    }) {
        PageHostReply::DebuggerSafePoints { safe_points, .. } => safe_points,
        reply => panic!("expected child debugger safe points, got {reply:?}"),
    };
    let max = usize::try_from(PAGE_HOST_DEBUGGER_MAX_BREAKPOINTS_PER_REALM)
        .expect("page-host breakpoint cap fits usize");
    assert!(
        safe_points.len() > max,
        "fixture needs one point over the cap"
    );
    for safe_point in safe_points.iter().copied().take(max) {
        assert!(matches!(
            host.handle_request(PageHostRequest::SetDebuggerBreakpoint {
                tab_id: 7,
                document_generation: 1,
                safe_point,
            }),
            PageHostReply::DebuggerBreakpointSet { .. }
        ));
    }
    let overflow = safe_points[max];
    assert!(matches!(
        host.handle_request(PageHostRequest::SetDebuggerBreakpoint {
            tab_id: 7,
            document_generation: 1,
            safe_point: overflow,
        }),
        PageHostReply::Error {
            code: PageHostErrorCode::ResourceLimit,
            ..
        }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::ListDebuggerBreakpoints {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::DebuggerBreakpoints { safe_points, .. } if safe_points.len() == max
    ));
}
