// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn launcher_supervised_bluets_classic_and_module_values_and_static_scopes_cross_core() {
    use blueice_ipc::debugger::{
        DebuggerCapability, DebuggerCapabilityState, DebuggerPageRealm, DebuggerReply,
        DebuggerRequest,
    };
    use blueice_launcher::bluejs_host::SpawnedBlueJsHost;

    let source = "let rootValue: number = 9; function inner(a: number): number { let childValue: number = a + 1; return childValue; } globalThis.answer = inner(3) + rootValue;";
    for (mime, slug) in [
        ("application/x-blueice-typescript", "classic"),
        ("application/x-blueice-typescript-module", "module"),
    ] {
        let (host, config) = SpawnedBlueJsHost::spawn_for_core().unwrap();
        let (mut tabs, tab_id) = loaded_tabs(
            &format!("<script type=\"{mime}\">{source}</script>"),
            &format!("https://example.test/{slug}-private-values.html"),
        );
        let mut executor =
            OutOfProcessJavaScriptPageExecutor::connect_with_debugger_execution_control(
                config.socket_path(),
                config.session_token(),
            )
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
            panic!("{slug} debugger must describe public capabilities");
        };
        assert!(capabilities.reports.iter().any(|report| {
            report.capability == DebuggerCapability::BoundedValues
                && report.state == DebuggerCapabilityState::Planned
        }));
        let program = executor.debugger_programs(tab_id, 1).unwrap()[0];
        assert!(program.program_handle >= CORE_CHILD_DEBUGGER_ID_NAMESPACE_START);
        let nested_entry = executor
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
                nested_entry.code_unit_ordinal,
                nested_entry.bytecode_offset,
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
            panic!("{slug} nested BlueTS frame must pause");
        };
        let mut stack = executor
            .debugger_stack_snapshot(tab_id, 1, program, Some(frame), 2, 256)
            .unwrap();
        assert_eq!(stack.frames.len(), 2);
        let mut values_ready = false;
        for _ in 0..96 {
            values_ready = [(0, 3.0_f64), (1, 9.0_f64)]
                .into_iter()
                .all(|(index, expected)| {
                    stack.frames[index]
                        .scope_entries
                        .iter()
                        .copied()
                        .any(|entry| {
                            executor.debugger_value_snapshot(
                                tab_id,
                                1,
                                JavaScriptPageDebuggerValueTarget {
                                    program,
                                    frame: Some(frame),
                                    frame_index: index as u32,
                                    safe_point: JavaScriptPageDebuggerSafePoint {
                                        code_unit_ordinal: stack.frames[index].code_unit_ordinal,
                                        bytecode_offset: stack.frames[index].bytecode_offset,
                                    },
                                    scope_entry: entry,
                                },
                            ) == Ok(JavaScriptPageDebuggerValuePreview::NumberBits(
                                expected.to_bits(),
                            ))
                        })
                });
            if values_ready {
                break;
            }
            executor.step_debugger_nested_instruction(frame).unwrap();
            executor.synchronize_and_execute(&tabs).unwrap();
            stack = executor
                .debugger_stack_snapshot(tab_id, 1, program, Some(frame), 2, 256)
                .unwrap();
        }
        assert!(
            values_ready,
            "{slug} nested and root values must become readable"
        );
        let target_for = |index: usize, scope_entry| JavaScriptPageDebuggerValueTarget {
            program,
            frame: Some(frame),
            frame_index: index as u32,
            safe_point: JavaScriptPageDebuggerSafePoint {
                code_unit_ordinal: stack.frames[index].code_unit_ordinal,
                bytecode_offset: stack.frames[index].bytecode_offset,
            },
            scope_entry,
        };
        for (index, expected) in [(0, 3.0_f64), (1, 9.0_f64)] {
            let reads: Vec<_> = stack.frames[index]
                .scope_entries
                .iter()
                .copied()
                .map(|entry| {
                    (
                        entry,
                        executor.debugger_value_snapshot(tab_id, 1, target_for(index, entry)),
                    )
                })
                .collect();
            assert!(
                reads.iter().any(|(_, read)| *read
                    == Ok(JavaScriptPageDebuggerValuePreview::NumberBits(
                        expected.to_bits()
                    ))),
                "{slug} frame {index} must expose its own exact active number: {reads:?}"
            );
        }
        let parent_slot = stack.frames[1]
            .scope_entries
            .iter()
            .copied()
            .find(|entry| {
                executor.debugger_value_snapshot(tab_id, 1, target_for(1, *entry))
                    == Ok(JavaScriptPageDebuggerValuePreview::NumberBits(
                        9.0_f64.to_bits(),
                    ))
            })
            .expect("nested parent root must retain the initialized BlueTS binding");
        let metadata = executor
            .debugger_static_metadata(
                tab_id,
                1,
                program.program_handle,
                program.program_generation,
            )
            .unwrap()[0];
        let parent_static = JavaScriptPageDebuggerStaticScopeTarget::Ordinary {
            metadata,
            target: target_for(1, parent_slot),
        };
        assert_eq!(
            executor
                .debugger_static_scope_relation(tab_id, 1, parent_static)
                .unwrap()
                .target,
            parent_static,
            "{slug} nested parent root must cross the real child/core route"
        );
        assert!(executor
            .debugger_static_scope_relation(
                tab_id,
                1,
                JavaScriptPageDebuggerStaticScopeTarget::Ordinary {
                    metadata,
                    target: target_for(0, stack.frames[0].scope_entries[0]),
                },
            )
            .is_err());
        for denied in [
            JavaScriptPageDebuggerStaticScopeTarget::Ordinary {
                metadata: JavaScriptPageDebuggerStaticMetadata {
                    metadata_generation: metadata.metadata_generation + 1,
                    ..metadata
                },
                target: target_for(1, parent_slot),
            },
            JavaScriptPageDebuggerStaticScopeTarget::Ordinary {
                metadata,
                target: JavaScriptPageDebuggerValueTarget {
                    scope_entry: JavaScriptPageDebuggerScopeEntry {
                        slot_ordinal: parent_slot.slot_ordinal + 1_000,
                        ..parent_slot
                    },
                    ..target_for(1, parent_slot)
                },
            },
            JavaScriptPageDebuggerStaticScopeTarget::Ordinary {
                metadata,
                target: JavaScriptPageDebuggerValueTarget {
                    safe_point: JavaScriptPageDebuggerSafePoint {
                        bytecode_offset: stack.frames[1].bytecode_offset + 1,
                        ..target_for(1, parent_slot).safe_point
                    },
                    ..target_for(1, parent_slot)
                },
            },
        ] {
            assert!(executor
                .debugger_static_scope_relation(tab_id, 1, denied)
                .is_err());
        }
        let selected = target_for(0, stack.frames[0].scope_entries[0]);
        assert_eq!(
            executor.debugger_value_snapshot(
                tab_id,
                1,
                JavaScriptPageDebuggerValueTarget {
                    safe_point: JavaScriptPageDebuggerSafePoint {
                        bytecode_offset: selected.safe_point.bytecode_offset + 1,
                        ..selected.safe_point
                    },
                    ..selected
                },
            ),
            Err(JavaScriptPageDebuggerError::InvalidExecutionState)
        );
        assert_eq!(
            executor.debugger_value_snapshot(
                tab_id,
                1,
                JavaScriptPageDebuggerValueTarget {
                    frame: Some(JavaScriptPageDebuggerFrame {
                        frame_handle: frame.frame_handle + 1,
                        ..frame
                    }),
                    ..selected
                },
            ),
            Err(JavaScriptPageDebuggerError::InvalidExecutionState)
        );
        executor.resume_debugger_nested_execution(frame).unwrap();
        executor.synchronize_and_execute(&tabs).unwrap();
        assert!(executor
            .debugger_static_scope_relation(tab_id, 1, parent_static)
            .is_err());
        let root = executor
            .debugger_stack_snapshot(tab_id, 1, program, None, 1, 256)
            .unwrap();
        assert_eq!(root.frames.len(), 1);
        let root_target = root.frames[0]
            .scope_entries
            .iter()
            .copied()
            .find_map(|entry| {
                let target = JavaScriptPageDebuggerValueTarget {
                    program,
                    frame: None,
                    frame_index: 0,
                    safe_point: JavaScriptPageDebuggerSafePoint {
                        code_unit_ordinal: 0,
                        bytecode_offset: root.frames[0].bytecode_offset,
                    },
                    scope_entry: entry,
                };
                (executor.debugger_value_snapshot(tab_id, 1, target)
                    == Ok(JavaScriptPageDebuggerValuePreview::NumberBits(
                        9.0_f64.to_bits(),
                    )))
                .then_some(target)
            })
            .expect("resumed root must retain its own initialized binding");
        let metadata = executor
            .debugger_static_metadata(
                tab_id,
                1,
                program.program_handle,
                program.program_generation,
            )
            .unwrap()[0];
        let static_target = JavaScriptPageDebuggerStaticScopeTarget::Ordinary {
            metadata,
            target: root_target,
        };
        assert_eq!(
            executor
                .debugger_static_scope_relation(tab_id, 1, static_target)
                .unwrap()
                .target,
            static_target,
            "{slug} root static relation must cross the real child/core route"
        );
        tabs.get_mut(tab_id).unwrap().load_html_str(
            "<p>successor</p>",
            Some(format!("https://example.test/{slug}-successor.html")),
        );
        executor.synchronize_and_execute(&tabs).unwrap();
        assert_eq!(
            executor.debugger_value_snapshot(tab_id, 1, root_target),
            Err(JavaScriptPageDebuggerError::NoLiveRealm)
        );
        assert_eq!(
            executor.debugger_static_scope_relation(tab_id, 1, static_target),
            Err(JavaScriptPageDebuggerError::NoLiveRealm)
        );
        drop(executor);
        drop(host);
    }
}

#[test]
fn paused_bluets_classic_and_module_stack_frames_have_exact_original_spans() {
    let source = "/* 🚀 */ function inner(a: number): number { let value: number = a + 1; return value; } globalThis.answer = inner(3);";
    let function_start = source.find("function inner").unwrap();
    let function_end = source.find("} globalThis").unwrap() + 1;
    let call_start = source.find("globalThis.answer").unwrap();
    for (mime, slug) in [
        ("application/x-blueice-typescript", "classic"),
        ("application/x-blueice-typescript-module", "module"),
    ] {
        let (path, token, child) = spawn_child();
        let (tabs, tab_id) = loaded_tabs(
            &format!("<script type=\"{mime}\">{source}</script>"),
            &format!("https://example.test/{slug}-stack-spans.html"),
        );
        let mut executor =
            OutOfProcessJavaScriptPageExecutor::connect_with_debugger_execution_control(
                &path, &token,
            )
            .unwrap();
        executor.synchronize_and_execute(&tabs).unwrap();
        let program = executor.debugger_programs(tab_id, 1).unwrap()[0];
        let metadata = executor
            .debugger_static_metadata(
                tab_id,
                1,
                program.program_handle,
                program.program_generation,
            )
            .unwrap()[0];
        let source_ids = executor
            .debugger_static_metadata_sources(
                tab_id,
                1,
                program.program_handle,
                program.program_generation,
                metadata.metadata_handle,
                metadata.metadata_generation,
            )
            .unwrap();
        assert!(!source_ids.is_empty());
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
            panic!("{slug} child must pause");
        };
        let stack = executor
            .debugger_stack_snapshot(tab_id, 1, program, Some(frame), 2, 1)
            .unwrap();
        assert_eq!(stack.frames.len(), 2);
        for stack_frame in &stack.frames {
            let spans = source_ids
                .iter()
                .filter_map(|source| {
                    executor
                        .debugger_static_metadata_safe_point_span(
                            tab_id,
                            1,
                            JavaScriptPageDebuggerStaticMetadataSafePointSpanTarget {
                                program_handle: program.program_handle,
                                program_generation: program.program_generation,
                                metadata_handle: metadata.metadata_handle,
                                metadata_generation: metadata.metadata_generation,
                                source_id: source.source_id,
                                code_unit_ordinal: stack_frame.code_unit_ordinal,
                                bytecode_offset: stack_frame.bytecode_offset,
                            },
                        )
                        .ok()
                })
                .collect::<Vec<_>>();
            assert_eq!(
                spans.len(),
                1,
                "{slug} frame {stack_frame:?} must have one exact source mapping"
            );
            let span = spans[0];
            let (expected_start, expected_end) = if stack_frame.code_unit_ordinal == 1 {
                (function_start, function_end)
            } else {
                assert_eq!(stack_frame.code_unit_ordinal, 0);
                (call_start, source.len())
            };
            assert_eq!(
                (span.start_byte as usize, span.end_byte as usize),
                (expected_start, expected_end),
                "{slug} frame must use its owning original BlueTS statement"
            );
            assert_eq!(span.coordinates.start_line, 0);
            assert_eq!(
                span.coordinates.start_column_utf16,
                source[..expected_start].encode_utf16().count() as u32
            );
            assert_eq!(span.coordinates.end_line, 0);
            assert_eq!(
                span.coordinates.end_column_utf16,
                source[..expected_end].encode_utf16().count() as u32
            );
            assert!(source_ids
                .iter()
                .any(|source| source.source_id == span.source_id));
            assert!(span
                .coordinates
                .is_well_formed_for_range(span.start_byte, span.end_byte));
        }
        drop(executor);
        shutdown_child(&path, &token);
        child.join().unwrap();
        let _ = std::fs::remove_file(path);
    }
}

#[test]
fn public_core_route_steps_one_real_child_nested_frame_without_root_aliasing() {
    use crate::debugger::handle_debugger_request_with_page_javascript_executor;
    use blueice_ipc::debugger::{
        DebuggerCapability, DebuggerCapabilityState, DebuggerExecutionState, DebuggerPageRealm,
        DebuggerReply, DebuggerRequest,
    };

    let (path, token, child) = spawn_child();
    let (tabs, tab_id) = loaded_tabs(
        "<script>function inner() { return 4; } globalThis.answer = inner() + 1;</script>",
        "https://example.test/public-nested.html",
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
        panic!("expected live debugger capabilities");
    };
    assert!(capabilities.reports.iter().any(|report| {
        report.capability == DebuggerCapability::NestedFrames
            && report.state == DebuggerCapabilityState::Available
    }));
    let program = match handle_debugger_request_with_page_javascript_executor(
        &tabs,
        Some(&mut executor),
        DebuggerRequest::ListPrograms { realm },
    ) {
        DebuggerReply::Programs(programs) => programs[0],
        reply => panic!("expected one program: {reply:?}"),
    };
    let target = match handle_debugger_request_with_page_javascript_executor(
        &tabs,
        Some(&mut executor),
        DebuggerRequest::ListSafePoints { program },
    ) {
        DebuggerReply::SafePoints(points) => points
            .into_iter()
            .find(|point| point.code_unit_ordinal == 1 && point.bytecode_offset == 0)
            .unwrap(),
        reply => panic!("expected nested safe points: {reply:?}"),
    };
    assert_eq!(
        handle_debugger_request_with_page_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::ArmNestedSafePointBreakpoint { safe_point: target },
        ),
        DebuggerReply::NestedSafePointBreakpointArmed { safe_point: target }
    );
    executor.synchronize_and_execute(&tabs).unwrap();
    let frame = match handle_debugger_request_with_page_javascript_executor(
        &tabs,
        Some(&mut executor),
        DebuggerRequest::GetExecutionState { program },
    ) {
        DebuggerReply::ExecutionState {
            state: DebuggerExecutionState::NestedPaused { frame, safe_point },
            ..
        } => {
            assert_eq!(safe_point, target);
            frame
        }
        reply => panic!("expected a public active frame: {reply:?}"),
    };
    assert_eq!(frame.program, program);
    assert_ne!(frame.frame_handle, 0);
    assert!(matches!(
        handle_debugger_request_with_page_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::StepRootInstruction { program },
        ),
        DebuggerReply::Error { .. }
    ));
    let mut wrong = frame;
    wrong.frame_handle += 1;
    assert!(matches!(
        handle_debugger_request_with_page_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::StepNestedInstruction { frame: wrong },
        ),
        DebuggerReply::Error { .. }
    ));
    let mut returned = false;
    for _ in 0..96 {
        assert_eq!(
            handle_debugger_request_with_page_javascript_executor(
                &tabs,
                Some(&mut executor),
                DebuggerRequest::StepNestedInstruction { frame },
            ),
            DebuggerReply::NestedStepRequested { frame }
        );
        assert_eq!(
            handle_debugger_request_with_page_javascript_executor(
                &tabs,
                Some(&mut executor),
                DebuggerRequest::GetExecutionState { program },
            ),
            DebuggerReply::ExecutionState {
                program,
                state: DebuggerExecutionState::NestedStepping { frame },
            }
        );
        executor.synchronize_and_execute(&tabs).unwrap();
        match handle_debugger_request_with_page_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::GetExecutionState { program },
        ) {
            DebuggerReply::ExecutionState {
                state:
                    DebuggerExecutionState::NestedPaused {
                        frame: same_frame, ..
                    },
                ..
            } => assert_eq!(same_frame, frame),
            DebuggerReply::ExecutionState {
                state: DebuggerExecutionState::Paused { safe_point },
                ..
            } => {
                assert_eq!(safe_point.code_unit_ordinal, 0);
                returned = true;
                break;
            }
            reply => panic!("unexpected nested successor: {reply:?}"),
        }
    }
    assert!(returned);
    assert!(matches!(
        handle_debugger_request_with_page_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::StepNestedInstruction { frame },
        ),
        DebuggerReply::Error { .. }
    ));
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
            state: DebuggerExecutionState::Completed,
        }
    );
    drop(executor);
    shutdown_child(&path, &token);
    child.join().unwrap();
    let _ = std::fs::remove_file(path);
}
