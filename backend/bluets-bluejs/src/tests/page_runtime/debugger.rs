// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Debugger page-realm regressions.

use super::*;

#[test]
fn direct_page_callback_method_debug_frame_expires_after_navigation() {
    let source = CALLBACK_METHOD_SOURCE;
    let dispatch_start = source.find("function dispatch").unwrap();
    let dispatch_end = source.find(" const visitor").unwrap();
    let artifact = compile_direct_script(
        ENTRY,
        &MapLoader::from([ModuleSource::new(ENTRY, source)]),
        CompilerOptions::default(),
    )
    .unwrap();
    let mut runtime = bluejs::BlueJsPageRuntime::default();
    runtime.open_realm(7, origin()).unwrap();
    let mut debug = DirectDebugRegistry::default();
    let attachment = artifact
        .attach_debug_in_page_realm(&mut runtime, 7, &origin(), &mut debug)
        .unwrap();
    attachment
        .safe_point_map
        .validate_against(runtime.program_registry(), attachment.handle)
        .unwrap();
    let child = attachment
        .safe_point_map
        .entries
        .iter()
        .find(|entry| {
            entry.code_unit.ordinal() > 0
                && attachment
                    .safe_point_map
                    .source_span_for_safe_point(entry.code_unit.ordinal(), entry.bytecode_offset)
                    .is_some_and(|mapped| {
                        (mapped.start_byte, mapped.end_byte) == (dispatch_start, dispatch_end)
                    })
        })
        .expect("dispatch must have a mapped child instruction");
    let point = bluejs::BlueJsSafePoint {
        code_unit: child.code_unit,
        bytecode_offset: child.bytecode_offset,
    };
    let bluejs::BlueJsPageDebuggerNestedExecutionState::Paused {
        frame,
        bytecode_offset,
    } = runtime
        .execute_program_until_nested_debugger_pause(7, attachment.handle, point)
        .unwrap()
    else {
        panic!("the first overloaded method call must pause in dispatch");
    };
    assert_eq!(frame.program(), attachment.handle);
    let mapped = attachment
        .safe_point_map
        .source_span_for_safe_point(frame.code_unit_ordinal(), bytecode_offset)
        .expect("live dispatch instruction must retain source provenance");
    assert_eq!(mapped.source, ENTRY);
    assert_eq!(
        (mapped.start_byte, mapped.end_byte),
        (dispatch_start, dispatch_end)
    );
    assert!(debug
        .get(runtime.program_registry(), attachment.handle)
        .is_ok());
    runtime.navigate(7, origin()).unwrap();
    assert!(attachment
        .safe_point_map
        .validate_against(runtime.program_registry(), attachment.handle)
        .is_err());
    assert!(runtime.step_debugger_nested_instruction(frame).is_err());
    assert!(debug
        .get(runtime.program_registry(), attachment.handle)
        .is_err());
}

#[test]
fn direct_page_optional_dot_read_debugger_provenance_expires_on_navigation() {
    let source = "function choose(flag: boolean): { value: number } | null { return flag ? { value: 41 } : null; } const receiver: { value: number } | null = choose(false); const read: number | undefined = receiver?.value; read;";
    let read_start = source.find("const read").unwrap();
    let read_end = read_start + source[read_start..].find(';').unwrap() + 1;
    let artifact = compile_direct_script(
        ENTRY,
        &MapLoader::from([ModuleSource::new(ENTRY, source)]),
        CompilerOptions::default(),
    )
    .unwrap();
    let mut runtime = bluejs::BlueJsPageRuntime::default();
    runtime.open_realm(7, origin()).unwrap();
    let mut debug = DirectDebugRegistry::default();
    let attachment = artifact
        .attach_debug_in_page_realm(&mut runtime, 7, &origin(), &mut debug)
        .unwrap();
    attachment
        .safe_point_map
        .validate_against(runtime.program_registry(), attachment.handle)
        .unwrap();
    let read = attachment
        .provenance
        .iter()
        .find(|entry| (entry.source.start, entry.source.end) == (read_start, read_end))
        .expect("optional read declaration must retain original source provenance");
    let DirectSafePointBinding::Bound(point) = read.safe_point else {
        panic!("optional read declaration must bind a root safe point");
    };
    assert_eq!(point.code_unit.ordinal(), 0);
    let mapped = attachment
        .safe_point_map
        .source_span_for_safe_point(0, point.bytecode_offset)
        .expect("root safe point must map to the original optional read declaration");
    assert_eq!(mapped.source, ENTRY);
    assert_eq!((mapped.start_byte, mapped.end_byte), (read_start, read_end));
    assert_eq!(
        attachment.breakpoint_at_or_after(ENTRY, read_start),
        DirectSafePointBinding::Bound(point)
    );
    assert_eq!(
        runtime
            .execute_program_until_debugger_pause(7, attachment.handle, point)
            .unwrap(),
        bluejs::BlueJsPageDebuggerExecutionState::Paused {
            bytecode_offset: point.bytecode_offset
        }
    );
    runtime.navigate(7, origin()).unwrap();
    assert!(attachment
        .safe_point_map
        .validate_against(runtime.program_registry(), attachment.handle)
        .is_err());
    assert!(debug
        .get(runtime.program_registry(), attachment.handle)
        .is_err());
    assert!(runtime.resume_debugger_execution(7).is_err());
}

#[test]
fn direct_page_while_loopback_keeps_its_live_source_map_and_nested_frame() {
    let source = "function count(value: number): number { let total: number = 0; while (value > 0) { total += value; value -= 1; } return total; } count(3);";
    let function_end = source.find(" count(3);").unwrap();
    let artifact = compile_direct_script(
        ENTRY,
        &MapLoader::from([ModuleSource::new(ENTRY, source)]),
        CompilerOptions::default(),
    )
    .unwrap();
    let mut runtime = bluejs::BlueJsPageRuntime::default();
    runtime.open_realm(7, origin()).unwrap();
    let mut debug = DirectDebugRegistry::default();
    let attachment = artifact
        .attach_debug_in_page_realm(&mut runtime, 7, &origin(), &mut debug)
        .unwrap();
    attachment
        .safe_point_map
        .validate_against(runtime.program_registry(), attachment.handle)
        .unwrap();
    let first_child = attachment
        .safe_point_map
        .entries
        .iter()
        .find(|entry| entry.code_unit.ordinal() == 1)
        .expect("the named function must have mapped child instructions");
    let point = bluejs::BlueJsSafePoint {
        code_unit: first_child.code_unit,
        bytecode_offset: first_child.bytecode_offset,
    };
    let bluejs::BlueJsPageDebuggerNestedExecutionState::Paused {
        frame,
        bytecode_offset: mut offset,
    } = runtime
        .execute_program_until_nested_debugger_pause(7, attachment.handle, point)
        .unwrap()
    else {
        panic!("the loop's named function must pause in its child frame");
    };
    assert_eq!(frame.program(), attachment.handle);
    assert_eq!(frame.code_unit_ordinal(), 1);

    let mut visits = BTreeMap::<u32, usize>::new();
    let mut returned = false;
    for _ in 0..256 {
        let mapped = attachment
            .safe_point_map
            .source_span_for_safe_point(frame.code_unit_ordinal(), offset)
            .expect("each paused loop instruction must retain checked BlueTS provenance");
        assert_eq!(mapped.source, ENTRY);
        assert_eq!((mapped.start_byte, mapped.end_byte), (0, function_end));
        assert_eq!(
            &source[mapped.start_byte..mapped.end_byte],
            &source[..function_end]
        );
        *visits.entry(offset).or_default() += 1;
        match runtime.step_debugger_nested_instruction(frame).unwrap() {
            bluejs::BlueJsPageDebuggerNestedExecutionState::Paused {
                frame: same_frame,
                bytecode_offset,
            } => {
                assert_eq!(same_frame, frame);
                offset = bytecode_offset;
            }
            bluejs::BlueJsPageDebuggerNestedExecutionState::FrameReturned { .. } => {
                returned = true;
                break;
            }
            state => panic!("unexpected nested loop state: {state:?}"),
        }
    }
    assert!(
        returned,
        "the finite loop must return within the step bound"
    );
    assert!(
        visits.values().any(|count| *count >= 2),
        "a loop-back instruction must revisit the same mapped safe point"
    );
    assert!(runtime.step_debugger_nested_instruction(frame).is_err());
    assert_eq!(
        runtime.resume_debugger_execution(7),
        Ok(bluejs::BlueJsPageDebuggerExecutionState::Completed)
    );

    let bluejs::BlueJsPageDebuggerNestedExecutionState::Paused {
        frame: second_frame,
        ..
    } = runtime
        .execute_program_until_nested_debugger_pause(7, attachment.handle, point)
        .unwrap()
    else {
        panic!("a new invocation must pause independently");
    };
    assert_ne!(second_frame, frame);
    assert!(matches!(
        runtime.resume_debugger_nested_execution(second_frame),
        Ok(bluejs::BlueJsPageDebuggerNestedExecutionState::FrameReturned { .. })
    ));
    assert_eq!(
        runtime.resume_debugger_execution(7),
        Ok(bluejs::BlueJsPageDebuggerExecutionState::Completed)
    );
    runtime.navigate(7, origin()).unwrap();
    assert!(attachment
        .safe_point_map
        .validate_against(runtime.program_registry(), attachment.handle)
        .is_err());
    assert!(runtime
        .step_debugger_nested_instruction(second_frame)
        .is_err());
}

#[test]
fn direct_page_try_nested_safe_points_expire_with_the_page_generation() {
    let source = "function f(): number { try { throw 1; } catch (caught) { if (caught === 1) { return 2; } } finally { 3; } return 0; } f();";
    let function_end = source.find(" f();").unwrap();
    let artifact = compile_direct_script(
        ENTRY,
        &MapLoader::from([ModuleSource::new(ENTRY, source)]),
        CompilerOptions::default(),
    )
    .unwrap();
    let mut runtime = bluejs::BlueJsPageRuntime::default();
    runtime.open_realm(7, origin()).unwrap();
    let mut debug = DirectDebugRegistry::default();
    let attachment = artifact
        .attach_debug_in_page_realm(&mut runtime, 7, &origin(), &mut debug)
        .unwrap();
    attachment
        .safe_point_map
        .validate_against(runtime.program_registry(), attachment.handle)
        .unwrap();
    let child = attachment
        .safe_point_map
        .entries
        .iter()
        .find(|entry| entry.code_unit.ordinal() == 1)
        .expect("the try function must have mapped nested instructions");
    let point = bluejs::BlueJsSafePoint {
        code_unit: child.code_unit,
        bytecode_offset: child.bytecode_offset,
    };
    let bluejs::BlueJsPageDebuggerNestedExecutionState::Paused {
        frame,
        bytecode_offset: mut offset,
    } = runtime
        .execute_program_until_nested_debugger_pause(7, attachment.handle, point)
        .unwrap()
    else {
        panic!("the try function must pause in its nested frame");
    };
    assert_eq!(frame.program(), attachment.handle);
    assert_eq!(frame.code_unit_ordinal(), 1);
    let mut visited = BTreeSet::new();
    let mut returned = false;
    for _ in 0..256 {
        let mapped = attachment
            .safe_point_map
            .source_span_for_safe_point(frame.code_unit_ordinal(), offset)
            .expect("every paused try instruction must retain original BlueTS provenance");
        assert_eq!(mapped.source, ENTRY);
        assert_eq!((mapped.start_byte, mapped.end_byte), (0, function_end));
        visited.insert(offset);
        match runtime.step_debugger_nested_instruction(frame).unwrap() {
            bluejs::BlueJsPageDebuggerNestedExecutionState::Paused {
                frame: same_frame,
                bytecode_offset,
            } => {
                assert_eq!(same_frame, frame);
                offset = bytecode_offset;
            }
            bluejs::BlueJsPageDebuggerNestedExecutionState::FrameReturned { .. } => {
                returned = true;
                break;
            }
            state => panic!("unexpected nested try state: {state:?}"),
        }
    }
    assert!(
        returned,
        "the finite try call must return within the step bound"
    );
    assert!(
        visited.len() > 3,
        "try/catch/finally must map multiple live instructions"
    );
    assert_eq!(
        runtime.resume_debugger_execution(7),
        Ok(bluejs::BlueJsPageDebuggerExecutionState::Completed)
    );
    runtime.navigate(7, origin()).unwrap();
    assert!(attachment
        .safe_point_map
        .validate_against(runtime.program_registry(), attachment.handle)
        .is_err());
    assert!(runtime.step_debugger_nested_instruction(frame).is_err());
    assert!(debug
        .get(runtime.program_registry(), attachment.handle)
        .is_err());
    let successor = artifact
        .attach_debug_in_page_realm(&mut runtime, 7, &origin(), &mut debug)
        .unwrap();
    assert_ne!(successor.handle, attachment.handle);
    successor
        .safe_point_map
        .validate_against(runtime.program_registry(), successor.handle)
        .unwrap();
}

#[test]
fn direct_page_typeof_guard_keeps_live_nested_provenance_and_rejects_stale_frames() {
    let source = "function choose(input: string | number): number { const value: string | number = input; if (typeof value === 'string') { return 1; } return value; } choose('Ada');";
    let function_end = source.find(" choose('Ada');").unwrap();
    let artifact = compile_direct_script(
        ENTRY,
        &MapLoader::from([ModuleSource::new(ENTRY, source)]),
        CompilerOptions::default(),
    )
    .unwrap();
    let mut runtime = bluejs::BlueJsPageRuntime::default();
    runtime.open_realm(7, origin()).unwrap();
    let mut debug = DirectDebugRegistry::default();
    let attachment = artifact
        .attach_debug_in_page_realm(&mut runtime, 7, &origin(), &mut debug)
        .unwrap();
    attachment
        .safe_point_map
        .validate_against(runtime.program_registry(), attachment.handle)
        .unwrap();
    let child = attachment
        .safe_point_map
        .entries
        .iter()
        .find(|entry| entry.code_unit.ordinal() == 1)
        .expect("guarded named function must have a mapped child frame");
    let point = bluejs::BlueJsSafePoint {
        code_unit: child.code_unit,
        bytecode_offset: child.bytecode_offset,
    };
    let bluejs::BlueJsPageDebuggerNestedExecutionState::Paused {
        frame,
        bytecode_offset,
    } = runtime
        .execute_program_until_nested_debugger_pause(7, attachment.handle, point)
        .unwrap()
    else {
        panic!("guarded function must pause in its nested frame");
    };
    let mapped = attachment
        .safe_point_map
        .source_span_for_safe_point(frame.code_unit_ordinal(), bytecode_offset)
        .expect("the live guarded instruction must retain original provenance");
    assert_eq!(mapped.source, ENTRY);
    assert_eq!((mapped.start_byte, mapped.end_byte), (0, function_end));
    assert!(matches!(
        runtime.step_debugger_nested_instruction(frame),
        Ok(bluejs::BlueJsPageDebuggerNestedExecutionState::Paused { .. })
    ));
    runtime.navigate(7, origin()).unwrap();
    assert!(attachment
        .safe_point_map
        .validate_against(runtime.program_registry(), attachment.handle)
        .is_err());
    assert!(runtime.step_debugger_nested_instruction(frame).is_err());
    assert!(debug
        .get(runtime.program_registry(), attachment.handle)
        .is_err());
}
