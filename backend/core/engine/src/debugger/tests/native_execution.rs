// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn discovery_rejects_a_malformed_or_unknown_target() {
    let (tabs, realm) = loaded_tabs();
    assert!(matches!(
        handle_debugger_request(
            &tabs,
            DebuggerRequest::DescribeCapabilities {
                realm: DebuggerPageRealm {
                    realm_generation: 0,
                    ..realm
                },
            },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            ..
        }
    ));
    assert!(matches!(
        handle_debugger_request(
            &tabs,
            DebuggerRequest::DescribeCapabilities {
                realm: DebuggerPageRealm {
                    tab_id: 999,
                    ..realm
                },
            },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            ..
        }
    ));
}

#[test]
fn enabled_javascript_realm_exposes_only_exact_opaque_program_locations() {
    let mut tabs = TabManager::new(320.0, 200.0);
    let first_tab = tabs.default_tab();
    tabs.get_mut(first_tab).unwrap().load_html_str(
        "<main>first</main><script>const answer = 40 + 2; answer;</script>",
        Some("https://example.test/first".to_string()),
    );
    let second_tab = tabs.open_tab();
    tabs.get_mut(second_tab).unwrap().load_html_str(
        "<main>second</main><script>const answer = 43;</script>",
        Some("https://example.test/second".to_string()),
    );
    let mut executor = JavaScriptPageExecutor::default();
    executor.synchronize_and_execute(&tabs).unwrap();
    let first_realm = DebuggerPageRealm {
        browser_context_id: DEFAULT_BROWSER_CONTEXT_ID,
        tab_id: first_tab.as_u64(),
        realm_generation: 1,
    };
    let second_realm = DebuggerPageRealm {
        browser_context_id: DEFAULT_BROWSER_CONTEXT_ID,
        tab_id: second_tab.as_u64(),
        realm_generation: 1,
    };

    let DebuggerReply::Capabilities(capabilities) =
        handle_debugger_request_with_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::DescribeCapabilities { realm: first_realm },
        )
    else {
        panic!("an enabled JavaScript realm must describe its live location capability")
    };
    assert!(capabilities.reports.iter().any(|report| {
        report.capability == DebuggerCapability::ProgramLocations
            && report.state == DebuggerCapabilityState::Available
    }));
    assert!(capabilities
        .reports
        .iter()
        .filter(|report| {
            !matches!(
                report.capability,
                DebuggerCapability::ProgramLocations | DebuggerCapability::BreakpointConfiguration
            )
        })
        .all(|report| report.state == DebuggerCapabilityState::Planned));
    assert!(capabilities.reports.iter().any(|report| {
        report.capability == DebuggerCapability::BreakpointConfiguration
            && report.state == DebuggerCapabilityState::Available
            && report.detail.contains("does not interrupt execution")
    }));

    let DebuggerReply::Programs(first_programs) = handle_debugger_request_with_javascript_executor(
        &tabs,
        Some(&mut executor),
        DebuggerRequest::ListPrograms { realm: first_realm },
    ) else {
        panic!("the current realm must expose opaque program identities")
    };
    assert_eq!(first_programs.len(), 1);
    let first_program = first_programs[0];
    assert_eq!(first_program.realm, first_realm);

    let DebuggerReply::SafePoints(safe_points) = handle_debugger_request_with_javascript_executor(
        &tabs,
        Some(&mut executor),
        DebuggerRequest::ListSafePoints {
            program: first_program,
        },
    ) else {
        panic!("a current program must expose compiler-verified boundaries")
    };
    let safe_point = *safe_points
        .first()
        .expect("a non-empty JavaScript program has an instruction boundary");
    assert_eq!(safe_point.program, first_program);
    assert_eq!(
        handle_debugger_request_with_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::ValidateSafePoint { safe_point },
        ),
        DebuggerReply::SafePointValidated { safe_point }
    );
    assert_eq!(
        handle_debugger_request_with_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::SetBreakpoint { safe_point },
        ),
        DebuggerReply::BreakpointSet { safe_point }
    );
    assert_eq!(
        handle_debugger_request_with_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::ListBreakpoints { realm: first_realm },
        ),
        DebuggerReply::Breakpoints(vec![safe_point])
    );
    assert_eq!(
        handle_debugger_request_with_javascript_executor(
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
        handle_debugger_request_with_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::ClearBreakpoint { safe_point },
        ),
        DebuggerReply::BreakpointCleared {
            safe_point,
            was_present: false,
        }
    );

    assert!(matches!(
        handle_debugger_request_with_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::ValidateSafePoint {
                safe_point: DebuggerSafePoint {
                    bytecode_offset: u32::MAX,
                    ..safe_point
                },
            },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidSafePoint,
            ..
        }
    ));
    assert!(matches!(
        handle_debugger_request_with_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::SetBreakpoint {
                safe_point: DebuggerSafePoint {
                    bytecode_offset: u32::MAX,
                    ..safe_point
                },
            },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidSafePoint,
            ..
        }
    ));

    assert!(matches!(
        handle_debugger_request_with_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::ListSafePoints {
                program: DebuggerProgram {
                    realm: second_realm,
                    ..first_program
                },
            },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            ..
        }
    ));
    assert!(matches!(
        handle_debugger_request_with_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::SetBreakpoint {
                safe_point: DebuggerSafePoint {
                    program: DebuggerProgram {
                        realm: second_realm,
                        ..first_program
                    },
                    ..safe_point
                },
            },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            ..
        }
    ));

    tabs.get_mut(first_tab).unwrap().load_html_str(
        "<main>replacement</main><script>const successor = 44;</script>",
        Some("https://example.test/replacement".to_string()),
    );
    executor.synchronize_and_execute(&tabs).unwrap();
    assert!(matches!(
        handle_debugger_request_with_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::ListSafePoints {
                program: first_program,
            },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::StaleRealm,
            ..
        }
    ));
}

#[test]
fn native_debugger_arms_observes_and_resumes_a_root_entry_pause() {
    let mut tabs = TabManager::new(320.0, 200.0);
    let tab_id = tabs.default_tab();
    tabs.get_mut(tab_id).unwrap().load_html_str(
        "<script>throw 1;</script>",
        Some("https://example.test/native-entry-pause.html".to_string()),
    );
    let realm = DebuggerPageRealm {
        browser_context_id: DEFAULT_BROWSER_CONTEXT_ID,
        tab_id: tab_id.as_u64(),
        realm_generation: 1,
    };
    let mut executor = JavaScriptPageExecutor::with_config(
        crate::script::javascript::JavaScriptPageExecutorConfig {
            native_debugger_execution_control: true,
            ..crate::script::javascript::JavaScriptPageExecutorConfig::default()
        },
    )
    .unwrap();

    // First lifecycle turn admits but does not execute the declaration.
    executor.synchronize_and_execute(&tabs).unwrap();
    let DebuggerReply::Capabilities(capabilities) =
        handle_debugger_request_with_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::DescribeCapabilities { realm },
        )
    else {
        panic!("the controlled live realm must describe native execution control")
    };
    for capability in [
        DebuggerCapability::Breakpoints,
        DebuggerCapability::PauseResume,
    ] {
        assert!(capabilities.reports.iter().any(|report| {
            report.capability == capability && report.state == DebuggerCapabilityState::Available
        }));
    }
    let DebuggerReply::Programs(programs) = handle_debugger_request_with_javascript_executor(
        &tabs,
        Some(&mut executor),
        DebuggerRequest::ListPrograms { realm },
    ) else {
        panic!("admission must expose an opaque program before execution")
    };
    let program = programs[0];
    let DebuggerReply::SafePoints(safe_points) = handle_debugger_request_with_javascript_executor(
        &tabs,
        Some(&mut executor),
        DebuggerRequest::ListSafePoints { program },
    ) else {
        panic!("the opaque program must retain compiler-verified boundaries")
    };
    let entry = *safe_points
        .iter()
        .find(|safe_point| safe_point.code_unit_ordinal == 0 && safe_point.bytecode_offset == 0)
        .expect("BlueJS root bytecode has a verified entry boundary");
    assert_eq!(
        handle_debugger_request_with_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::ArmEntryBreakpoint { safe_point: entry },
        ),
        DebuggerReply::BreakpointArmed { safe_point: entry }
    );

    // The owner-controlled scheduler now stops before `throw 1` reaches
    // the VM. A state query returns only the opaque exact boundary.
    executor.synchronize_and_execute(&tabs).unwrap();
    assert_eq!(
        handle_debugger_request_with_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::GetExecutionState { program },
        ),
        DebuggerReply::ExecutionState {
            program,
            state: DebuggerExecutionState::Paused { safe_point: entry },
        }
    );
    assert!(executor.drain_reports_for_tab(tab_id).is_empty());
    assert_eq!(
        handle_debugger_request_with_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::ResumeExecution { program },
        ),
        DebuggerReply::ExecutionResumed { program }
    );

    executor.synchronize_and_execute(&tabs).unwrap();
    assert_eq!(
        handle_debugger_request_with_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::GetExecutionState { program },
        ),
        DebuggerReply::ExecutionState {
            program,
            state: DebuggerExecutionState::Completed,
        }
    );
    assert_eq!(
        executor.drain_reports_for_tab(tab_id),
        vec![
            crate::script::javascript::JavaScriptPageExecutionReport::Rejected {
                tab_id: tab_id.as_u64(),
                document_generation: 1,
                ordinal: 0,
                kind: crate::script::BlueJsPageScriptKind::Classic,
                category: "BlueJS page execution failed",
            }
        ]
    );
}

#[test]
fn native_debugger_steps_an_exact_classic_program_through_the_public_route() {
    let mut tabs = TabManager::new(320.0, 200.0);
    let tab_id = tabs.default_tab();
    tabs.get_mut(tab_id).unwrap().load_html_str(
        "<script>let value = 1; globalThis.answer = value + 2;</script>",
        Some("https://example.test/native-step.html".to_string()),
    );
    let realm = DebuggerPageRealm {
        browser_context_id: DEFAULT_BROWSER_CONTEXT_ID,
        tab_id: tab_id.as_u64(),
        realm_generation: 1,
    };
    let mut executor = JavaScriptPageExecutor::with_config(
        crate::script::javascript::JavaScriptPageExecutorConfig {
            native_debugger_execution_control: true,
            ..crate::script::javascript::JavaScriptPageExecutorConfig::default()
        },
    )
    .unwrap();
    executor.synchronize_and_execute(&tabs).unwrap();

    let DebuggerReply::Capabilities(capabilities) =
        handle_debugger_request_with_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::DescribeCapabilities { realm },
        )
    else {
        panic!("the live realm must describe debugger capabilities")
    };
    assert!(capabilities.reports.iter().any(|report| {
        report.capability == DebuggerCapability::Stepping
            && report.state == DebuggerCapabilityState::Available
    }));
    let DebuggerReply::Programs(programs) = handle_debugger_request_with_javascript_executor(
        &tabs,
        Some(&mut executor),
        DebuggerRequest::ListPrograms { realm },
    ) else {
        panic!("the admitted declaration must have an opaque program")
    };
    let program = programs[0];
    let DebuggerReply::SafePoints(safe_points) = handle_debugger_request_with_javascript_executor(
        &tabs,
        Some(&mut executor),
        DebuggerRequest::ListSafePoints { program },
    ) else {
        panic!("the admitted declaration must have safe points")
    };
    let entry = *safe_points
        .iter()
        .find(|point| point.code_unit_ordinal == 0 && point.bytecode_offset == 0)
        .unwrap();
    assert_eq!(
        handle_debugger_request_with_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::ArmEntryBreakpoint { safe_point: entry },
        ),
        DebuggerReply::BreakpointArmed { safe_point: entry }
    );
    executor.synchronize_and_execute(&tabs).unwrap();
    let step = DebuggerRequest::StepRootInstruction { program };
    assert_eq!(
        handle_debugger_request_with_javascript_executor(&tabs, Some(&mut executor), step.clone(),),
        DebuggerReply::ExecutionStepRequested { program }
    );
    assert_eq!(
        handle_debugger_request_with_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::GetExecutionState { program },
        ),
        DebuggerReply::ExecutionState {
            program,
            state: DebuggerExecutionState::Stepping,
        }
    );
    assert!(matches!(
        handle_debugger_request_with_javascript_executor(&tabs, Some(&mut executor), step.clone(),),
        DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidExecutionState,
            ..
        }
    ));
    executor.synchronize_and_execute(&tabs).unwrap();
    let DebuggerReply::ExecutionState {
        state: DebuggerExecutionState::Paused {
            safe_point: advanced,
        },
        ..
    } = handle_debugger_request_with_javascript_executor(
        &tabs,
        Some(&mut executor),
        DebuggerRequest::GetExecutionState { program },
    )
    else {
        panic!("one root instruction must return to an exact paused boundary")
    };
    assert_ne!(advanced, entry);
    assert!(safe_points.contains(&advanced));
    assert!(executor.drain_reports_for_tab(tab_id).is_empty());
    assert!(matches!(
        handle_debugger_request_with_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::StepRootInstruction {
                program: DebuggerProgram {
                    program_generation: program.program_generation + 1,
                    ..program
                },
            },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::StaleProgram,
            ..
        }
    ));
    assert_eq!(
        handle_debugger_request_with_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::ResumeExecution { program },
        ),
        DebuggerReply::ExecutionResumed { program }
    );
    executor.synchronize_and_execute(&tabs).unwrap();
    assert_eq!(
        handle_debugger_request_with_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::GetExecutionState { program },
        ),
        DebuggerReply::ExecutionState {
            program,
            state: DebuggerExecutionState::Completed,
        }
    );
    assert!(matches!(
        handle_debugger_request_with_javascript_executor(&tabs, Some(&mut executor), step),
        DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidExecutionState,
            ..
        }
    ));
}

#[test]
fn native_debugger_routes_a_non_entry_root_safe_point_to_same_frame_resume() {
    let mut tabs = TabManager::new(320.0, 200.0);
    let tab_id = tabs.default_tab();
    tabs.get_mut(tab_id).unwrap().load_html_str(
        "<script>globalThis.before = 1; throw 2;</script>",
        Some("https://example.test/native-root-safe-point.html".to_string()),
    );
    let realm = DebuggerPageRealm {
        browser_context_id: DEFAULT_BROWSER_CONTEXT_ID,
        tab_id: tab_id.as_u64(),
        realm_generation: 1,
    };
    let mut executor = JavaScriptPageExecutor::with_config(
        crate::script::javascript::JavaScriptPageExecutorConfig {
            native_debugger_execution_control: true,
            ..crate::script::javascript::JavaScriptPageExecutorConfig::default()
        },
    )
    .unwrap();
    executor.synchronize_and_execute(&tabs).unwrap();
    let DebuggerReply::Programs(programs) = handle_debugger_request_with_javascript_executor(
        &tabs,
        Some(&mut executor),
        DebuggerRequest::ListPrograms { realm },
    ) else {
        panic!("pending classic declaration must expose an opaque program")
    };
    let program = programs[0];
    let DebuggerReply::SafePoints(safe_points) = handle_debugger_request_with_javascript_executor(
        &tabs,
        Some(&mut executor),
        DebuggerRequest::ListSafePoints { program },
    ) else {
        panic!("pending classic declaration must expose exact safe points")
    };
    let safe_point = *safe_points
        .iter()
        .find(|safe_point| safe_point.code_unit_ordinal == 0 && safe_point.bytecode_offset != 0)
        .expect("fixture has a non-entry root safe point");
    assert_eq!(
        handle_debugger_request_with_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::ArmRootSafePointBreakpoint { safe_point },
        ),
        DebuggerReply::RootSafePointBreakpointArmed { safe_point }
    );

    executor.synchronize_and_execute(&tabs).unwrap();
    assert_eq!(
        handle_debugger_request_with_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::GetExecutionState { program },
        ),
        DebuggerReply::ExecutionState {
            program,
            state: DebuggerExecutionState::Paused { safe_point },
        }
    );
    assert!(executor.drain_reports_for_tab(tab_id).is_empty());
    assert_eq!(
        handle_debugger_request_with_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::ResumeExecution { program },
        ),
        DebuggerReply::ExecutionResumed { program }
    );
    executor.synchronize_and_execute(&tabs).unwrap();
    assert_eq!(
        handle_debugger_request_with_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::GetExecutionState { program },
        ),
        DebuggerReply::ExecutionState {
            program,
            state: DebuggerExecutionState::Completed,
        }
    );
    assert_eq!(
        executor.drain_reports_for_tab(tab_id),
        vec![
            crate::script::javascript::JavaScriptPageExecutionReport::Rejected {
                tab_id: tab_id.as_u64(),
                document_generation: 1,
                ordinal: 0,
                kind: crate::script::BlueJsPageScriptKind::Classic,
                category: "BlueJS page execution failed",
            }
        ]
    );
}

#[test]
fn successful_execution_controls_invalidate_scope_pause_incarnation() {
    let (_, realm) = loaded_tabs();
    let (_, receiver) = debugger_request_channel();
    let program = DebuggerProgram {
        realm,
        program_handle: 1,
        program_generation: 1,
    };
    assert_eq!(receiver.pause_incarnation.get(), 1);
    receiver.invalidate_pause_receipts_for(&DebuggerReply::Error {
        code: DebuggerErrorCode::InvalidExecutionState,
        message: String::new(),
    });
    assert_eq!(receiver.pause_incarnation.get(), 1);
    receiver.invalidate_pause_receipts_for(&DebuggerReply::ExecutionResumed { program });
    assert_eq!(receiver.pause_incarnation.get(), 2);
    receiver.invalidate_pause_receipts_for(&DebuggerReply::ExecutionStepRequested { program });
    assert_eq!(receiver.pause_incarnation.get(), 3);
    receiver.pause_incarnation.set(u64::MAX);
    receiver.invalidate_pause_receipts_for(&DebuggerReply::ExecutionResumed { program });
    assert_eq!(receiver.pause_incarnation.get(), 0);
    receiver.invalidate_pause_receipts_for(&DebuggerReply::ExecutionResumed { program });
    assert_eq!(receiver.pause_incarnation.get(), 0);
}

#[test]
fn queued_requests_are_applied_only_by_the_session_owner() {
    let (tabs, realm) = loaded_tabs();
    let (sender, receiver) = debugger_request_channel();
    let (reply_sender, reply_receiver) = mpsc::sync_channel(1);
    sender
        .0
        .send(DebuggerRequestEnvelope {
            request: DebuggerRequest::DescribeCapabilities { realm },
            metadata_session: None,
            reply: reply_sender,
        })
        .unwrap();
    assert_eq!(receiver.dispatch_pending(&tabs, None), 1);
    assert!(matches!(
        reply_receiver.recv().unwrap(),
        DebuggerReply::Capabilities(_)
    ));
}

#[test]
fn discovery_refuses_an_unbounded_page_realm_list() {
    let mut tabs = TabManager::new(320.0, 200.0);
    for index in 0..=MAX_DISCOVERABLE_PAGE_REALMS {
        let tab_id = if index == 0 {
            tabs.default_tab()
        } else {
            tabs.open_tab()
        };
        tabs.get_mut(tab_id).unwrap().load_html_str(
            "<main>debugger target</main>",
            Some(format!("https://example.test/{index}")),
        );
    }
    assert!(matches!(
        handle_debugger_request(&tabs, DebuggerRequest::ListPageRealms),
        DebuggerReply::Error {
            code: DebuggerErrorCode::ResourceLimit,
            ..
        }
    ));
}

#[test]
fn source_step_budget_stop_keeps_a_distinct_verified_public_reason() {
    let (_, realm) = loaded_tabs();
    let program = DebuggerProgram {
        realm,
        program_handle: 7,
        program_generation: 1,
    };
    assert_eq!(
        debugger_execution_state(
            JavaScriptPageDebuggerExecutionState::SourceStepLimitReached {
                code_unit_ordinal: 0,
                bytecode_offset: 19,
            },
            program,
        ),
        DebuggerExecutionState::SourceStepLimitReached {
            safe_point: DebuggerSafePoint {
                program,
                code_unit_ordinal: 0,
                bytecode_offset: 19,
            },
        }
    );
}
