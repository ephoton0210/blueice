// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn core_proxies_real_child_root_safe_point_pause_and_resume_without_private_ids() {
    use crate::debugger::handle_debugger_request_with_page_javascript_executor;
    use blueice_ipc::debugger::{
        DebuggerCapability, DebuggerCapabilityState, DebuggerPageRealm, DebuggerReply,
        DebuggerRequest,
    };

    let (path, token, child) = spawn_child();
    let (tabs, tab_id) = loaded_tabs(
        "<script>let first = 1; first += 1; globalThis.answer = first;</script>",
        "https://example.test/root-safe-point.html",
    );
    let mut executor =
        OutOfProcessJavaScriptPageExecutor::connect_with_debugger_execution_control(&path, &token)
            .unwrap();
    executor.synchronize_and_execute(&tabs).unwrap();
    let realm = DebuggerPageRealm {
        browser_context_id: crate::debugger::DEFAULT_BROWSER_CONTEXT_ID,
        tab_id: tab_id.as_u64(),
        realm_generation: 1,
    };
    let DebuggerReply::Capabilities(capabilities) =
        handle_debugger_request_with_page_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::DescribeCapabilities { realm },
        )
    else {
        panic!("expected OOP debugger capabilities");
    };
    assert!(capabilities.reports.iter().any(|report| {
        report.capability == DebuggerCapability::PauseResume
            && report.state == DebuggerCapabilityState::Available
    }));
    let program = match handle_debugger_request_with_page_javascript_executor(
        &tabs,
        Some(&mut executor),
        DebuggerRequest::ListPrograms { realm },
    ) {
        DebuggerReply::Programs(programs) => programs[0],
        reply => panic!("expected public OOP debugger program, got {reply:?}"),
    };
    assert!(
        program.program_handle >= CORE_CHILD_DEBUGGER_ID_NAMESPACE_START,
        "core must not disclose child-private program IDs"
    );
    let safe_point = match handle_debugger_request_with_page_javascript_executor(
        &tabs,
        Some(&mut executor),
        DebuggerRequest::ListSafePoints { program },
    ) {
        DebuggerReply::SafePoints(safe_points) => safe_points
            .into_iter()
            .find(|safe_point| safe_point.code_unit_ordinal == 0 && safe_point.bytecode_offset != 0)
            .expect("fixture has a resumable root safe point"),
        reply => panic!("expected public OOP safe points, got {reply:?}"),
    };
    assert_eq!(
        handle_debugger_request_with_page_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::ArmRootSafePointBreakpoint { safe_point },
        ),
        DebuggerReply::RootSafePointBreakpointArmed { safe_point }
    );
    executor.synchronize_and_execute(&tabs).unwrap();
    assert_eq!(
        handle_debugger_request_with_page_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::GetExecutionState { program },
        ),
        DebuggerReply::ExecutionState {
            program,
            state: blueice_ipc::debugger::DebuggerExecutionState::Paused { safe_point },
        }
    );
    assert_eq!(
        handle_debugger_request_with_page_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::ResumeExecution { program },
        ),
        DebuggerReply::ExecutionResumed { program }
    );
    executor.synchronize_and_execute(&tabs).unwrap();
    assert_eq!(
        handle_debugger_request_with_page_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::GetExecutionState { program },
        ),
        DebuggerReply::ExecutionState {
            program,
            state: blueice_ipc::debugger::DebuggerExecutionState::Completed,
        }
    );
    assert!(matches!(
        executor.drain_reports_for_tab(tab_id).as_slice(),
        [JavaScriptPageExecutionReport::Executed { ordinal: 0, .. }]
    ));
    drop(executor);
    shutdown_child(&path, &token);
    child.join().unwrap();
    let _ = std::fs::remove_file(path);
}

#[test]
fn core_proxies_real_child_nested_frame_without_exposing_private_program_ids() {
    let (path, token, child) = spawn_child();
    let (tabs, tab_id) = loaded_tabs(
        "<script>function inner() { return 4; } globalThis.answer = inner() + 1;</script>",
        "https://example.test/nested-frame.html",
    );
    let mut executor =
        OutOfProcessJavaScriptPageExecutor::connect_with_debugger_execution_control(&path, &token)
            .unwrap();
    executor.synchronize_and_execute(&tabs).unwrap();
    assert!(executor.debugger_nested_frames_available());
    let program = executor.debugger_programs(tab_id, 1).unwrap()[0];
    assert!(program.program_handle >= CORE_CHILD_DEBUGGER_ID_NAMESPACE_START);
    let target = executor
        .debugger_safe_points(
            tab_id,
            1,
            program.program_handle,
            program.program_generation,
        )
        .unwrap()
        .into_iter()
        .find(|point| point.code_unit_ordinal == 1 && point.bytecode_offset == 0)
        .unwrap();
    executor
        .arm_debugger_nested_safe_point_breakpoint(
            tab_id,
            1,
            program.program_handle,
            program.program_generation,
            target.code_unit_ordinal,
            target.bytecode_offset,
        )
        .unwrap();
    executor.synchronize_and_execute(&tabs).unwrap();
    let Some(JavaScriptPageDebuggerNestedExecutionState::Paused {
        frame,
        bytecode_offset,
    }) = executor
        .debugger_nested_execution_state(
            tab_id,
            1,
            program.program_handle,
            program.program_generation,
        )
        .unwrap()
    else {
        panic!("the exact nested invocation must be visible to core");
    };
    assert_eq!(bytecode_offset, target.bytecode_offset);
    assert_eq!(frame.tab_id, tab_id);
    assert_eq!(frame.document_generation, 1);
    assert_eq!(frame.program_handle, program.program_handle);
    assert_eq!(frame.program_generation, program.program_generation);
    assert_eq!(frame.code_unit_ordinal, 1);
    assert_ne!(frame.frame_handle, 0);
    let stack = executor
        .debugger_stack_snapshot(tab_id, 1, program, Some(frame), 1, 1)
        .unwrap();
    assert_eq!(stack.frames.len(), 1);
    assert_eq!(stack.frames[0].code_unit_ordinal, 1);
    assert_eq!(stack.frames[0].bytecode_offset, bytecode_offset);
    assert!(stack.stack_truncated);
    assert_eq!(
        executor.debugger_stack_snapshot(
            tab_id,
            1,
            program,
            Some(JavaScriptPageDebuggerFrame {
                frame_handle: frame.frame_handle + 1,
                ..frame
            }),
            2,
            256,
        ),
        Err(JavaScriptPageDebuggerError::InvalidExecutionState)
    );
    assert_eq!(
        executor.debugger_stack_snapshot(tab_id, 1, program, None, 2, 256,),
        Err(JavaScriptPageDebuggerError::InvalidExecutionState)
    );
    assert_eq!(
        executor
            .debugger_nested_execution_state(
                tab_id,
                1,
                program.program_handle,
                program.program_generation,
            )
            .unwrap(),
        Some(JavaScriptPageDebuggerNestedExecutionState::Paused {
            frame,
            bytecode_offset,
        })
    );
    assert_eq!(
        executor.step_debugger_nested_instruction(JavaScriptPageDebuggerFrame {
            frame_handle: frame.frame_handle + 1,
            ..frame
        }),
        Err(JavaScriptPageDebuggerError::InvalidExecutionState)
    );
    let mut returned = false;
    for _ in 0..96 {
        executor.step_debugger_nested_instruction(frame).unwrap();
        assert_eq!(
            executor
                .debugger_nested_execution_state(
                    tab_id,
                    1,
                    program.program_handle,
                    program.program_generation,
                )
                .unwrap(),
            Some(JavaScriptPageDebuggerNestedExecutionState::Stepping { frame })
        );
        executor.synchronize_and_execute(&tabs).unwrap();
        match executor
            .debugger_nested_execution_state(
                tab_id,
                1,
                program.program_handle,
                program.program_generation,
            )
            .unwrap()
        {
            Some(JavaScriptPageDebuggerNestedExecutionState::Paused {
                frame: same_frame, ..
            }) => assert_eq!(same_frame, frame),
            None => {
                returned = true;
                break;
            }
            state => panic!("unexpected child frame state: {state:?}"),
        }
    }
    assert!(returned);
    assert_eq!(
        executor.step_debugger_nested_instruction(frame),
        Err(JavaScriptPageDebuggerError::InvalidExecutionState)
    );
    assert!(matches!(
        executor
            .debugger_execution_state(
                tab_id,
                1,
                program.program_handle,
                program.program_generation,
            )
            .unwrap(),
        JavaScriptPageDebuggerExecutionState::Paused {
            code_unit_ordinal: 0,
            ..
        }
    ));
    let root_stack = executor
        .debugger_stack_snapshot(tab_id, 1, program, None, 2, 256)
        .unwrap();
    assert_eq!(root_stack.frames.len(), 1);
    assert_eq!(root_stack.frames[0].code_unit_ordinal, 0);
    assert!(!root_stack.stack_truncated);
    executor
        .resume_debugger_execution(
            tab_id,
            1,
            program.program_handle,
            program.program_generation,
        )
        .unwrap();
    executor.synchronize_and_execute(&tabs).unwrap();
    assert_eq!(
        executor
            .debugger_execution_state(
                tab_id,
                1,
                program.program_handle,
                program.program_generation,
            )
            .unwrap(),
        JavaScriptPageDebuggerExecutionState::Completed
    );
    drop(executor);
    shutdown_child(&path, &token);
    child.join().unwrap();
    let _ = std::fs::remove_file(path);

    // A replacement executor starts with the same core program counter,
    // but its process-unique frame handle must not alias the predecessor.
    let (next_path, next_token, next_child) = spawn_child();
    let mut replacement =
        OutOfProcessJavaScriptPageExecutor::connect_with_debugger_execution_control(
            &next_path,
            &next_token,
        )
        .unwrap();
    replacement.synchronize_and_execute(&tabs).unwrap();
    let next_program = replacement.debugger_programs(tab_id, 1).unwrap()[0];
    let next_target = replacement
        .debugger_safe_points(
            tab_id,
            1,
            next_program.program_handle,
            next_program.program_generation,
        )
        .unwrap()
        .into_iter()
        .find(|point| point.code_unit_ordinal == 1 && point.bytecode_offset == 0)
        .unwrap();
    replacement
        .arm_debugger_nested_safe_point_breakpoint(
            tab_id,
            1,
            next_program.program_handle,
            next_program.program_generation,
            next_target.code_unit_ordinal,
            next_target.bytecode_offset,
        )
        .unwrap();
    replacement.synchronize_and_execute(&tabs).unwrap();
    let Some(JavaScriptPageDebuggerNestedExecutionState::Paused {
        frame: next_frame, ..
    }) = replacement
        .debugger_nested_execution_state(
            tab_id,
            1,
            next_program.program_handle,
            next_program.program_generation,
        )
        .unwrap()
    else {
        panic!("the replacement child must pause its own frame");
    };
    assert_eq!(next_frame.program_handle, frame.program_handle);
    assert_ne!(next_frame.frame_handle, frame.frame_handle);
    assert_eq!(
        replacement.step_debugger_nested_instruction(frame),
        Err(JavaScriptPageDebuggerError::InvalidExecutionState)
    );
    replacement.close_page(tab_id);
    assert_eq!(
        replacement.step_debugger_nested_instruction(next_frame),
        Err(JavaScriptPageDebuggerError::InvalidExecutionState)
    );
    drop(replacement);
    shutdown_child(&next_path, &next_token);
    next_child.join().unwrap();
    let _ = std::fs::remove_file(next_path);
}

#[test]
fn core_proxies_exact_nested_resume_to_real_bluets_child_and_revokes_handle() {
    let (path, token, child) = spawn_child();
    let (tabs, tab_id) = loaded_tabs(
            "<script type=\"application/x-blueice-typescript\">function inner(): number { return 4; } globalThis.answer = inner() + 1;</script>",
            "https://example.test/private-nested-resume.html",
        );
    let mut executor =
        OutOfProcessJavaScriptPageExecutor::connect_with_debugger_execution_control(&path, &token)
            .unwrap();
    executor.synchronize_and_execute(&tabs).unwrap();
    let program = executor.debugger_programs(tab_id, 1).unwrap()[0];
    let target = executor
        .debugger_safe_points(
            tab_id,
            1,
            program.program_handle,
            program.program_generation,
        )
        .unwrap()
        .into_iter()
        .find(|point| point.code_unit_ordinal == 1 && point.bytecode_offset == 0)
        .unwrap();
    executor
        .arm_debugger_nested_safe_point_breakpoint(
            tab_id,
            1,
            program.program_handle,
            program.program_generation,
            target.code_unit_ordinal,
            target.bytecode_offset,
        )
        .unwrap();
    executor.synchronize_and_execute(&tabs).unwrap();
    let Some(JavaScriptPageDebuggerNestedExecutionState::Paused { frame, .. }) = executor
        .debugger_nested_execution_state(
            tab_id,
            1,
            program.program_handle,
            program.program_generation,
        )
        .unwrap()
    else {
        panic!("BlueTS child must pause under a core-owned handle");
    };
    assert_eq!(
        executor.resume_debugger_nested_execution(JavaScriptPageDebuggerFrame {
            frame_handle: frame.frame_handle + 1,
            ..frame
        }),
        Err(JavaScriptPageDebuggerError::InvalidExecutionState)
    );
    executor.resume_debugger_nested_execution(frame).unwrap();
    assert_eq!(
        executor
            .debugger_nested_execution_state(
                tab_id,
                1,
                program.program_handle,
                program.program_generation,
            )
            .unwrap(),
        Some(JavaScriptPageDebuggerNestedExecutionState::Resuming { frame })
    );
    executor.synchronize_and_execute(&tabs).unwrap();
    assert_eq!(
        executor
            .debugger_nested_execution_state(
                tab_id,
                1,
                program.program_handle,
                program.program_generation,
            )
            .unwrap(),
        None
    );
    assert_eq!(
        executor.resume_debugger_nested_execution(frame),
        Err(JavaScriptPageDebuggerError::InvalidExecutionState)
    );
    assert!(matches!(
        executor
            .debugger_execution_state(
                tab_id,
                1,
                program.program_handle,
                program.program_generation,
            )
            .unwrap(),
        JavaScriptPageDebuggerExecutionState::Paused {
            code_unit_ordinal: 0,
            ..
        }
    ));
    executor
        .resume_debugger_execution(
            tab_id,
            1,
            program.program_handle,
            program.program_generation,
        )
        .unwrap();
    executor.synchronize_and_execute(&tabs).unwrap();
    assert_eq!(
        executor
            .debugger_execution_state(
                tab_id,
                1,
                program.program_handle,
                program.program_generation,
            )
            .unwrap(),
        JavaScriptPageDebuggerExecutionState::Completed
    );
    drop(executor);
    shutdown_child(&path, &token);
    child.join().unwrap();
    let _ = std::fs::remove_file(path);
}

#[test]
fn real_bluets_child_private_stack_is_bounded_before_public_queries() {
    use blueice_ipc::debugger::{
        DebuggerCapability, DebuggerCapabilityState, DebuggerPageRealm, DebuggerReply,
        DebuggerRequest,
    };

    let (path, token, child) = spawn_child();
    let (mut tabs, tab_id) = loaded_tabs(
            "<script type=\"application/x-blueice-typescript\">function inner(a: number, b: number): number { let first: number = a; let second: number = b; return first + second; } globalThis.answer = inner(1, 2);</script>",
            "https://example.test/private-stack.html",
        );
    let mut executor =
        OutOfProcessJavaScriptPageExecutor::connect_with_debugger_execution_control(&path, &token)
            .unwrap();
    executor.synchronize_and_execute(&tabs).unwrap();
    let realm = DebuggerPageRealm {
        browser_context_id: crate::debugger::DEFAULT_BROWSER_CONTEXT_ID,
        tab_id: tab_id.as_u64(),
        realm_generation: 1,
    };
    let DebuggerReply::Capabilities(capabilities) =
        crate::debugger::handle_debugger_request_with_page_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::DescribeCapabilities { realm },
        )
    else {
        panic!("the debugger must describe its actual capability boundary");
    };
    for capability in [DebuggerCapability::Stack, DebuggerCapability::Scopes] {
        assert!(capabilities.reports.iter().any(|report| {
            report.capability == capability && report.state == DebuggerCapabilityState::Available
        }));
    }
    let program = executor.debugger_programs(tab_id, 1).unwrap()[0];
    let target = executor
        .debugger_safe_points(
            tab_id,
            1,
            program.program_handle,
            program.program_generation,
        )
        .unwrap()
        .into_iter()
        .find(|point| point.code_unit_ordinal == 1 && point.bytecode_offset == 0)
        .unwrap();
    executor
        .arm_debugger_nested_safe_point_breakpoint(
            tab_id,
            1,
            program.program_handle,
            program.program_generation,
            1,
            0,
        )
        .unwrap();
    executor.synchronize_and_execute(&tabs).unwrap();
    let Some(JavaScriptPageDebuggerNestedExecutionState::Paused { frame, .. }) = executor
        .debugger_nested_execution_state(
            tab_id,
            1,
            program.program_handle,
            program.program_generation,
        )
        .unwrap()
    else {
        panic!("the BlueTS child must pause before its first instruction");
    };
    assert_eq!(frame.code_unit_ordinal, target.code_unit_ordinal);
    let mut full = None;
    for _ in 0..96 {
        let snapshot = executor
            .debugger_stack_snapshot(tab_id, 1, program, Some(frame), 2, 256)
            .unwrap();
        if snapshot.frames[0].scope_entries.len() >= 2 {
            full = Some(snapshot);
            break;
        }
        executor.step_debugger_nested_instruction(frame).unwrap();
        executor.synchronize_and_execute(&tabs).unwrap();
        assert!(matches!(
            executor
                .debugger_nested_execution_state(
                    tab_id,
                    1,
                    program.program_handle,
                    program.program_generation,
                )
                .unwrap(),
            Some(JavaScriptPageDebuggerNestedExecutionState::Paused { frame: same, .. })
                if same == frame
        ));
    }
    let full = full.expect("the child must enter a scope with two active slots");
    assert_eq!(full.frames.len(), 2);
    assert_eq!(full.frames[0].code_unit_ordinal, 1);
    assert_eq!(full.frames[1].code_unit_ordinal, 0);
    assert!(!full.stack_truncated);
    assert!(!full.frames[0].scope_truncated);
    let limited = executor
        .debugger_stack_snapshot(tab_id, 1, program, Some(frame), 1, 1)
        .unwrap();
    assert_eq!(limited.frames.len(), 1);
    assert_eq!(limited.frames[0].scope_entries.len(), 1);
    assert!(limited.stack_truncated);
    assert!(limited.frames[0].scope_truncated);
    assert_eq!(
        executor
            .debugger_stack_snapshot(tab_id, 1, program, Some(frame), 2, 256)
            .unwrap(),
        full
    );
    assert_eq!(
        executor.debugger_stack_snapshot(tab_id, 1, program, Some(frame), 0, 1),
        Err(JavaScriptPageDebuggerError::ResourceLimit)
    );
    assert_eq!(
        executor.debugger_stack_snapshot(tab_id, 1, program, Some(frame), 2, 257),
        Err(JavaScriptPageDebuggerError::ResourceLimit)
    );
    assert_eq!(
        executor.debugger_stack_snapshot(
            tab_id,
            1,
            program,
            Some(JavaScriptPageDebuggerFrame {
                frame_handle: frame.frame_handle + 1,
                ..frame
            }),
            2,
            256,
        ),
        Err(JavaScriptPageDebuggerError::InvalidExecutionState)
    );
    assert_eq!(
        executor.debugger_stack_snapshot(
            tab_id,
            1,
            JavaScriptPageDebuggerProgram {
                program_generation: program.program_generation + 1,
                ..program
            },
            Some(frame),
            2,
            256,
        ),
        Err(JavaScriptPageDebuggerError::UnknownProgram)
    );
    tabs.get_mut(tab_id).unwrap().load_html_str(
        "<script>globalThis.successor = true;</script>",
        Some("https://example.test/successor-stack.html".to_string()),
    );
    executor.synchronize_and_execute(&tabs).unwrap();
    assert_eq!(
        executor.debugger_stack_snapshot(tab_id, 1, program, Some(frame), 2, 256),
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    );
    drop(executor);
    shutdown_child(&path, &token);
    child.join().unwrap();
    let _ = std::fs::remove_file(path);
}
