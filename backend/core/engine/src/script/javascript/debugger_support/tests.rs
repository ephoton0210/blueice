// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

fn loaded_tabs(html: &str, url: &str) -> (TabManager, TabId) {
    let mut tabs = TabManager::new(320.0, 200.0);
    let tab_id = tabs.default_tab();
    tabs.get_mut(tab_id)
        .unwrap()
        .load_html_str(html, Some(url.to_string()));
    (tabs, tab_id)
}

#[test]
fn breakpoint_configuration_is_exact_bounded_idempotent_and_realm_scoped() {
    let (mut tabs, tab_id) = loaded_tabs(
        "<script>const first = 1; const second = 2;</script>",
        "https://example.test/breakpoint-configuration.html",
    );
    let mut executor = JavaScriptPageExecutor::with_config(JavaScriptPageExecutorConfig {
        max_debugger_breakpoints_per_realm: 1,
        ..JavaScriptPageExecutorConfig::default()
    })
    .unwrap();

    executor.synchronize_and_execute(&tabs).unwrap();
    let program = executor.debugger_programs(tab_id, 1).unwrap()[0];
    let safe_points = executor
        .debugger_safe_points(
            tab_id,
            1,
            program.program_handle,
            program.program_generation,
        )
        .unwrap();
    let first = *safe_points
        .first()
        .expect("the compiled declaration has a first safe point");
    let second = *safe_points
        .iter()
        .find(|safe_point| *safe_point != &first)
        .expect("the two declarations provide distinct safe points");

    executor
        .set_debugger_breakpoint(
            tab_id,
            1,
            program.program_handle,
            program.program_generation,
            first.code_unit_ordinal,
            first.bytecode_offset,
        )
        .unwrap();
    // A socket retry cannot consume an additional bounded record.
    executor
        .set_debugger_breakpoint(
            tab_id,
            1,
            program.program_handle,
            program.program_generation,
            first.code_unit_ordinal,
            first.bytecode_offset,
        )
        .unwrap();
    assert_eq!(
        executor.debugger_breakpoints(tab_id, 1).unwrap(),
        vec![JavaScriptPageDebuggerBreakpoint {
            program_handle: program.program_handle,
            program_generation: program.program_generation,
            code_unit_ordinal: first.code_unit_ordinal,
            bytecode_offset: first.bytecode_offset,
        }]
    );
    assert_eq!(
        executor.set_debugger_breakpoint(
            tab_id,
            1,
            program.program_handle,
            program.program_generation,
            second.code_unit_ordinal,
            second.bytecode_offset,
        ),
        Err(JavaScriptPageDebuggerError::BreakpointLimit)
    );
    assert_eq!(
        executor.clear_debugger_breakpoint(
            tab_id,
            1,
            program.program_handle,
            program.program_generation,
            first.code_unit_ordinal,
            u32::MAX,
        ),
        Err(JavaScriptPageDebuggerError::InvalidSafePoint)
    );
    assert!(executor
        .clear_debugger_breakpoint(
            tab_id,
            1,
            program.program_handle,
            program.program_generation,
            first.code_unit_ordinal,
            first.bytecode_offset,
        )
        .unwrap());
    assert!(!executor
        .clear_debugger_breakpoint(
            tab_id,
            1,
            program.program_handle,
            program.program_generation,
            first.code_unit_ordinal,
            first.bytecode_offset,
        )
        .unwrap());
    executor
        .set_debugger_breakpoint(
            tab_id,
            1,
            program.program_handle,
            program.program_generation,
            second.code_unit_ordinal,
            second.bytecode_offset,
        )
        .unwrap();

    tabs.get_mut(tab_id).unwrap().load_html_str(
        "<script>const successor = 3;</script>",
        Some("https://example.test/breakpoint-successor.html".to_string()),
    );
    executor.synchronize_and_execute(&tabs).unwrap();
    assert_eq!(
        executor.debugger_breakpoints(tab_id, 1),
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    );
    assert!(executor.debugger_breakpoints(tab_id, 2).unwrap().is_empty());
}

#[test]
fn root_entry_breakpoint_pauses_before_vm_execution_and_resumes_once() {
    let (mut tabs, tab_id) = loaded_tabs(
        "<script>throw 1;</script>",
        "https://example.test/debugger-entry-pause.html",
    );
    let mut executor = JavaScriptPageExecutor::with_config(JavaScriptPageExecutorConfig {
        native_debugger_execution_control: true,
        ..JavaScriptPageExecutorConfig::default()
    })
    .unwrap();

    // Admission has compiled and registered the program, but execution is
    // deliberately deferred for one owner-session turn.
    executor.synchronize_and_execute(&tabs).unwrap();
    let program = executor.debugger_programs(tab_id, 1).unwrap()[0];
    assert_eq!(
        executor
            .debugger_execution_state(
                tab_id,
                1,
                program.program_handle,
                program.program_generation
            )
            .unwrap(),
        JavaScriptPageDebuggerExecutionState::Pending
    );
    assert!(executor.drain_reports_for_tab(tab_id).is_empty());
    let entry = executor
        .debugger_safe_points(
            tab_id,
            1,
            program.program_handle,
            program.program_generation,
        )
        .unwrap()
        .into_iter()
        .find(|safe_point| safe_point.code_unit_ordinal == 0 && safe_point.bytecode_offset == 0)
        .expect("every admitted BlueJS root has a verified instruction-zero boundary");
    executor
        .arm_debugger_entry_breakpoint(
            tab_id,
            1,
            program.program_handle,
            program.program_generation,
            entry.code_unit_ordinal,
            entry.bytecode_offset,
        )
        .unwrap();
    // v4 root-entry arming retains its idempotent configuration behavior;
    // only the v5 non-entry continuation arm is one-shot.
    executor
        .arm_debugger_entry_breakpoint(
            tab_id,
            1,
            program.program_handle,
            program.program_generation,
            entry.code_unit_ordinal,
            entry.bytecode_offset,
        )
        .unwrap();

    // The second lifecycle turn observes the armed exact boundary before
    // passing anything to `Vm::execute_script`; the throwing bytecode has
    // not run, so there is neither execution report nor runtime error.
    executor.synchronize_and_execute(&tabs).unwrap();
    assert_eq!(
        executor
            .debugger_execution_state(
                tab_id,
                1,
                program.program_handle,
                program.program_generation
            )
            .unwrap(),
        JavaScriptPageDebuggerExecutionState::Paused {
            code_unit_ordinal: entry.code_unit_ordinal,
            bytecode_offset: entry.bytecode_offset,
        }
    );
    assert!(executor.drain_reports_for_tab(tab_id).is_empty());

    executor
        .resume_debugger_execution(
            tab_id,
            1,
            program.program_handle,
            program.program_generation,
        )
        .unwrap();
    assert_eq!(
        executor
            .debugger_execution_state(
                tab_id,
                1,
                program.program_handle,
                program.program_generation
            )
            .unwrap(),
        JavaScriptPageDebuggerExecutionState::Resuming
    );
    executor.synchronize_and_execute(&tabs).unwrap();
    assert_eq!(
        executor
            .debugger_execution_state(
                tab_id,
                1,
                program.program_handle,
                program.program_generation
            )
            .unwrap(),
        JavaScriptPageDebuggerExecutionState::Completed
    );
    assert_eq!(
        executor.drain_reports_for_tab(tab_id),
        vec![JavaScriptPageExecutionReport::Rejected {
            tab_id: tab_id.as_u64(),
            document_generation: 1,
            ordinal: 0,
            kind: BlueJsPageScriptKind::Classic,
            category: "BlueJS page execution failed",
        }]
    );

    // Replacement clears both paused/completed scheduler records and
    // opaque program identities before admitting a successor realm.
    tabs.get_mut(tab_id).unwrap().load_html_str(
        "<script>const successor = 2;</script>",
        Some("https://example.test/debugger-entry-successor.html".to_string()),
    );
    executor.synchronize_and_execute(&tabs).unwrap();
    assert_eq!(
        executor.debugger_execution_state(
            tab_id,
            1,
            program.program_handle,
            program.program_generation,
        ),
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    );
}

#[test]
fn root_safe_point_breakpoint_pauses_after_real_execution_and_resumes_same_frame() {
    let (tabs, tab_id) = loaded_tabs(
        "<script>globalThis.before = 1; throw 2;</script>",
        "https://example.test/debugger-root-safe-point.html",
    );
    let mut executor = JavaScriptPageExecutor::with_config(JavaScriptPageExecutorConfig {
        native_debugger_execution_control: true,
        ..JavaScriptPageExecutorConfig::default()
    })
    .unwrap();

    executor.synchronize_and_execute(&tabs).unwrap();
    let program = executor.debugger_programs(tab_id, 1).unwrap()[0];
    let safe_point = executor
        .debugger_safe_points(
            tab_id,
            1,
            program.program_handle,
            program.program_generation,
        )
        .unwrap()
        .into_iter()
        .find(|safe_point| safe_point.code_unit_ordinal == 0 && safe_point.bytecode_offset != 0)
        .expect("fixture has a non-entry root instruction boundary");
    executor
        .arm_debugger_root_safe_point_breakpoint(
            tab_id,
            1,
            program.program_handle,
            program.program_generation,
            safe_point.code_unit_ordinal,
            safe_point.bytecode_offset,
        )
        .unwrap();
    // The continuation target is committed while the declaration is
    // still pending. A second request cannot retarget it before the
    // scheduler reaches the first boundary.
    assert_eq!(
        executor.arm_debugger_root_safe_point_breakpoint(
            tab_id,
            1,
            program.program_handle,
            program.program_generation,
            safe_point.code_unit_ordinal,
            safe_point.bytecode_offset,
        ),
        Err(JavaScriptPageDebuggerError::InvalidExecutionState)
    );

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
        JavaScriptPageDebuggerExecutionState::Paused {
            code_unit_ordinal: safe_point.code_unit_ordinal,
            bytecode_offset: safe_point.bytecode_offset,
        }
    );
    assert!(executor.drain_reports_for_tab(tab_id).is_empty());

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
    assert_eq!(
        executor.drain_reports_for_tab(tab_id),
        vec![JavaScriptPageExecutionReport::Rejected {
            tab_id: tab_id.as_u64(),
            document_generation: 1,
            ordinal: 0,
            kind: BlueJsPageScriptKind::Classic,
            category: "BlueJS page execution failed",
        }]
    );
}

#[test]
fn entry_paused_classic_steps_one_root_instruction_per_owner_turn() {
    let (tabs, tab_id) = loaded_tabs(
        "<script>let index = 0; while (index < 2) { index++; } globalThis.done = index;</script>",
        "https://example.test/debugger-step.html",
    );
    let mut executor = JavaScriptPageExecutor::with_config(JavaScriptPageExecutorConfig {
        native_debugger_execution_control: true,
        ..JavaScriptPageExecutorConfig::default()
    })
    .unwrap();
    executor.synchronize_and_execute(&tabs).unwrap();
    let program = executor.debugger_programs(tab_id, 1).unwrap()[0];
    let safe_points = executor
        .debugger_safe_points(
            tab_id,
            1,
            program.program_handle,
            program.program_generation,
        )
        .unwrap();
    let entry = safe_points
        .iter()
        .find(|point| point.code_unit_ordinal == 0 && point.bytecode_offset == 0)
        .copied()
        .unwrap();
    executor
        .arm_debugger_entry_breakpoint(
            tab_id,
            1,
            program.program_handle,
            program.program_generation,
            entry.code_unit_ordinal,
            entry.bytecode_offset,
        )
        .unwrap();
    executor.synchronize_and_execute(&tabs).unwrap();
    let mut seen_offsets = Vec::new();
    let mut completed = false;
    for _ in 0..256 {
        executor
            .step_debugger_root_instruction(
                tab_id,
                1,
                program.program_handle,
                program.program_generation,
            )
            .unwrap();
        assert_eq!(
            executor
                .debugger_execution_state(
                    tab_id,
                    1,
                    program.program_handle,
                    program.program_generation,
                )
                .unwrap(),
            JavaScriptPageDebuggerExecutionState::Stepping
        );
        assert_eq!(
            executor.step_debugger_root_instruction(
                tab_id,
                1,
                program.program_handle,
                program.program_generation,
            ),
            Err(JavaScriptPageDebuggerError::InvalidExecutionState)
        );
        executor.synchronize_and_execute(&tabs).unwrap();
        match executor
            .debugger_execution_state(
                tab_id,
                1,
                program.program_handle,
                program.program_generation,
            )
            .unwrap()
        {
            JavaScriptPageDebuggerExecutionState::Paused {
                code_unit_ordinal: 0,
                bytecode_offset,
            } => {
                assert!(safe_points.iter().any(|point| {
                    point.code_unit_ordinal == 0 && point.bytecode_offset == bytecode_offset
                }));
                seen_offsets.push(bytecode_offset);
                assert!(executor.drain_reports_for_tab(tab_id).is_empty());
            }
            JavaScriptPageDebuggerExecutionState::Completed => {
                completed = true;
                break;
            }
            state => panic!("unexpected step state: {state:?}"),
        }
    }
    assert!(completed);
    assert!(
        seen_offsets
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len()
            < seen_offsets.len(),
        "loop stepping must revisit a real root boundary"
    );
    assert_eq!(
        executor.drain_reports_for_tab(tab_id),
        vec![JavaScriptPageExecutionReport::Executed {
            tab_id: tab_id.as_u64(),
            document_generation: 1,
            ordinal: 0,
            kind: BlueJsPageScriptKind::Classic,
        }]
    );
}
