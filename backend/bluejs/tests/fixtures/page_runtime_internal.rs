// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

pub(super) fn debug_starts_with(value: &impl std::fmt::Debug, prefix: &str) -> bool {
    format!("{value:?}").starts_with(prefix)
}

use super::*;
use crate::{parse, parse_module, BlueJsProgramV1, BlueJsSafePoint, Opcode};

fn origin() -> BlueJsPageOrigin {
    BlueJsPageOrigin::new("https://example.test").unwrap()
}

fn source(name: &str) -> BlueJsSourceIdentity {
    BlueJsSourceIdentity::new(name, format!("sha256:{name}")).unwrap()
}

fn install(
    runtime: &mut BlueJsPageRuntime,
    tab: u64,
    name: &str,
    program: &BlueJsProgramV1,
) -> BlueJsProgramHandle {
    runtime
        .install_program(tab, &origin(), source(name), program)
        .unwrap()
}

fn first_nested_point(
    runtime: &BlueJsPageRuntime,
    tab_id: u64,
    handle: BlueJsProgramHandle,
) -> BlueJsSafePoint {
    runtime
        .safe_points(tab_id, handle, 1024)
        .unwrap()
        .into_iter()
        .find(|point| point.code_unit.ordinal() == 1 && point.bytecode_offset == 0)
        .expect("the direct child has a first verified instruction")
}

fn expected_throw_site(
    runtime: &BlueJsPageRuntime,
    handle: BlueJsProgramHandle,
    nested: bool,
) -> VmDebuggerThrowSite {
    let root = runtime.program_registry().get(handle).unwrap().bytecode();
    let code = if nested {
        root.child_code_units().next().unwrap()
    } else {
        root
    };
    VmDebuggerThrowSite {
        program_generation: handle.generation().as_u64(),
        code_unit_ordinal: code.debugger_code_unit_ordinal.unwrap(),
        bytecode_offset: code
            .instructions()
            .find(|instruction| instruction.opcode == Opcode::Throw)
            .unwrap()
            .offset as u32,
    }
}

#[cfg_attr(test, test)]
fn uncaught_site_read_requires_exact_page_owned_program_and_last_execution() {
    let mut runtime = BlueJsPageRuntime::default();
    runtime.open_realm(7, origin()).unwrap();
    runtime.open_realm(8, origin()).unwrap();
    let throwing = runtime
        .install_program(
            7,
            &origin(),
            source("page:///throw.js"),
            &BlueJsProgramV1::Script(parse("function inner() { throw 7; } inner();").unwrap()),
        )
        .unwrap();
    let successor = runtime
        .install_program(
            7,
            &origin(),
            source("page:///successor.js"),
            &BlueJsProgramV1::Script(parse("42;").unwrap()),
        )
        .unwrap();
    assert!(matches!(
        runtime.execute_program(7, throwing),
        Err(BlueJsPageRuntimeError::Runtime(RuntimeError::Thrown(_)))
    ));
    assert_eq!(
        runtime.debugger_uncaught_throw_site(7, throwing),
        Ok(Some(expected_throw_site(&runtime, throwing, true)))
    );
    assert_eq!(runtime.debugger_uncaught_throw_site(7, successor), Ok(None));
    assert!(matches!(
        runtime.debugger_uncaught_throw_site(8, throwing),
        Err(BlueJsPageRuntimeError::ProgramNotOwnedByRealm { .. })
    ));

    runtime.execute_program(7, successor).unwrap();
    assert_eq!(runtime.debugger_uncaught_throw_site(7, throwing), Ok(None));
    assert!(runtime.execute_program(7, throwing).is_err());
    runtime.discard_program(7, throwing).unwrap();
    assert!(matches!(
        runtime.debugger_uncaught_throw_site(7, throwing),
        Err(BlueJsPageRuntimeError::ProgramNotOwnedByRealm { .. })
    ));
    runtime.navigate(7, origin()).unwrap();
    assert!(matches!(
        runtime.debugger_uncaught_throw_site(7, successor),
        Err(BlueJsPageRuntimeError::ProgramNotOwnedByRealm { .. })
    ));
}

#[cfg_attr(test, test)]
fn uncaught_module_root_and_child_sites_obey_the_same_page_boundary() {
    let mut runtime = BlueJsPageRuntime::default();
    runtime.open_realm(7, origin()).unwrap();
    let root = runtime
        .install_program(
            7,
            &origin(),
            source("page:///throw-root.mjs"),
            &BlueJsProgramV1::Module(parse_module("throw 8;").unwrap()),
        )
        .unwrap();
    assert!(runtime.execute_module_graph(7, root, [root]).is_err());
    assert_eq!(
        runtime.debugger_uncaught_throw_site(7, root),
        Ok(Some(expected_throw_site(&runtime, root, false)))
    );

    let nested = runtime
        .install_program(
            7,
            &origin(),
            source("page:///throw-nested.mjs"),
            &BlueJsProgramV1::Module(
                parse_module("function inner() { throw 9; } export const value = inner();")
                    .unwrap(),
            ),
        )
        .unwrap();
    assert!(runtime.execute_module_graph(7, nested, [nested]).is_err());
    assert_eq!(runtime.debugger_uncaught_throw_site(7, root), Ok(None));
    assert_eq!(
        runtime.debugger_uncaught_throw_site(7, nested),
        Ok(Some(expected_throw_site(&runtime, nested, true)))
    );
}

#[cfg_attr(test, test)]
fn page_value_preview_requires_owned_exact_paused_slot() {
    let mut runtime = BlueJsPageRuntime::default();
    runtime.open_realm(7, origin()).unwrap();
    runtime.open_realm(8, origin()).unwrap();
    let program = runtime
        .install_program(
            7,
            &origin(),
            source("page:///value.js"),
            &BlueJsProgramV1::Script(parse("let answer = 41;").unwrap()),
        )
        .unwrap();
    let halt = runtime
        .safe_points(7, program, 1024)
        .unwrap()
        .into_iter()
        .rfind(|point| point.code_unit.ordinal() == 0)
        .unwrap();
    runtime
        .execute_program_until_debugger_pause(7, program, halt)
        .unwrap();
    let frame = &runtime
        .debugger_stack_snapshot(7, program, None, 1, 256)
        .unwrap()
        .frames[0];
    let target = BlueJsPageDebuggerValueTarget {
        frame_index: 0,
        code_unit_ordinal: frame.code_unit_ordinal,
        bytecode_offset: frame.bytecode_offset,
        scope_entry: frame.scope_entries[0],
    };
    assert_eq!(
        runtime.debugger_value_preview(7, program, None, target),
        Ok(VmDebuggerValuePreview::NumberBits(41.0_f64.to_bits()))
    );
    assert!(runtime
        .debugger_value_preview(
            7,
            program,
            None,
            BlueJsPageDebuggerValueTarget {
                bytecode_offset: target.bytecode_offset - 1,
                ..target
            },
        )
        .is_err());
    assert!(runtime
        .debugger_value_preview(8, program, None, target)
        .is_err());
    assert!(runtime.close_realm(7));
    assert!(runtime
        .debugger_value_preview(7, program, None, target)
        .is_err());
}

#[cfg_attr(test, test)]
fn page_stack_snapshot_requires_the_paused_program_and_exact_nested_frame() {
    let mut runtime = BlueJsPageRuntime::default();
    runtime.open_realm(7, origin()).unwrap();
    runtime.open_realm(8, origin()).unwrap();
    let program = runtime
        .install_program(
            7,
            &origin(),
            source("page:///stack.js"),
            &BlueJsProgramV1::Script(parse("function inner() { return 1; } inner();").unwrap()),
        )
        .unwrap();
    let other = runtime
        .install_program(
            7,
            &origin(),
            source("page:///other-stack.js"),
            &BlueJsProgramV1::Script(parse("1;").unwrap()),
        )
        .unwrap();
    let point = first_nested_point(&runtime, 7, program);
    let BlueJsPageDebuggerNestedExecutionState::Paused { frame, .. } = runtime
        .execute_program_until_nested_debugger_pause(7, program, point)
        .unwrap()
    else {
        panic!("child must pause");
    };
    let snapshot = runtime
        .debugger_stack_snapshot(7, program, Some(frame), 2, 256)
        .unwrap();
    assert_eq!(snapshot.program_generation, program.generation().as_u64());
    assert_eq!(snapshot.frames.len(), 2);
    assert_eq!(snapshot.frames[0].code_unit_ordinal, 1);
    assert_eq!(snapshot.frames[1].code_unit_ordinal, 0);
    assert!(runtime
        .debugger_stack_snapshot(7, program, None, 2, 256)
        .is_err());
    let mut stale = frame;
    stale.invocation_serial += 1;
    assert_eq!(
        runtime.debugger_stack_snapshot(7, program, Some(stale), 2, 256),
        Err(BlueJsPageRuntimeError::DebuggerNestedFrameUnavailable)
    );
    assert!(matches!(
        runtime.debugger_stack_snapshot(8, program, Some(frame), 2, 256),
        Err(BlueJsPageRuntimeError::ProgramNotOwnedByRealm { .. })
    ));
    runtime.resume_debugger_nested_execution(frame).unwrap();
    assert!(runtime
        .debugger_stack_snapshot(7, program, Some(frame), 2, 256)
        .is_err());
    assert_eq!(
        runtime
            .debugger_stack_snapshot(7, program, None, 2, 256)
            .unwrap()
            .frames
            .len(),
        1
    );
    assert_eq!(
        runtime.debugger_stack_snapshot(7, other, None, 2, 256),
        Err(BlueJsPageRuntimeError::ProgramNotOwnedByRealm {
            tab_id: 7,
            handle: other,
        })
    );
    runtime.resume_debugger_execution(7).unwrap();
    assert!(runtime
        .debugger_stack_snapshot(7, program, None, 2, 256)
        .is_err());
}

#[cfg_attr(test, test)]
fn nested_page_frame_is_exact_and_revoked_after_return_or_navigation() {
    let mut runtime = BlueJsPageRuntime::default();
    runtime.open_realm(7, origin()).unwrap();
    runtime.open_realm(8, origin()).unwrap();
    let program = runtime
        .install_program(
            7,
            &origin(),
            source("page:///nested.js"),
            &BlueJsProgramV1::Script(
                parse("var calls = 0; function inner() { calls++; return 4; } inner();").unwrap(),
            ),
        )
        .unwrap();
    let point = first_nested_point(&runtime, 7, program);
    let other_program = runtime
        .install_program(
            7,
            &origin(),
            source("page:///other.js"),
            &BlueJsProgramV1::Script(parse("42").unwrap()),
        )
        .unwrap();
    let root_point = runtime
        .safe_points(7, program, 1024)
        .unwrap()
        .into_iter()
        .find(|point| point.code_unit.ordinal() == 0)
        .unwrap();
    assert_eq!(
        runtime.execute_program_until_nested_debugger_pause(7, program, root_point),
        Err(BlueJsPageRuntimeError::DebuggerNestedCodeUnitOnly)
    );
    assert!(matches!(
        runtime.execute_program_until_nested_debugger_pause(8, program, point),
        Err(BlueJsPageRuntimeError::ProgramNotOwnedByRealm { tab_id: 8, .. })
    ));
    let BlueJsPageDebuggerNestedExecutionState::Paused { frame, .. } = runtime
        .execute_program_until_nested_debugger_pause(7, program, point)
        .unwrap()
    else {
        panic!("the child must pause");
    };
    assert_eq!(frame.tab_id(), 7);
    assert_eq!(frame.program(), program);
    assert_eq!(frame.code_unit_ordinal(), 1);
    assert_ne!(frame.invocation_serial(), 0);
    let mut other_tab = frame;
    other_tab.tab_id = 8;
    assert_eq!(
        runtime.step_debugger_nested_instruction(other_tab),
        Err(BlueJsPageRuntimeError::DebuggerNestedFrameUnavailable)
    );
    let mut other_serial = frame;
    other_serial.invocation_serial += 1;
    assert_eq!(
        runtime.step_debugger_nested_instruction(other_serial),
        Err(BlueJsPageRuntimeError::DebuggerNestedFrameUnavailable)
    );
    let mut other_generation = frame;
    other_generation.program = other_program;
    assert_eq!(
        runtime.step_debugger_nested_instruction(other_generation),
        Err(BlueJsPageRuntimeError::DebuggerNestedFrameUnavailable)
    );
    let mut returned = false;
    for _ in 0..64 {
        match runtime.step_debugger_nested_instruction(frame).unwrap() {
            BlueJsPageDebuggerNestedExecutionState::Paused {
                frame: same_frame,
                bytecode_offset,
            } => {
                assert_eq!(same_frame, frame);
                assert!(runtime
                    .safe_points(7, program, 1024)
                    .unwrap()
                    .iter()
                    .any(|candidate| candidate.code_unit.ordinal() == 1
                        && candidate.bytecode_offset == bytecode_offset));
            }
            BlueJsPageDebuggerNestedExecutionState::FrameReturned { .. } => {
                returned = true;
                break;
            }
            state => panic!("unexpected nested state: {state:?}"),
        }
    }
    assert!(returned);
    assert_eq!(
        runtime.step_debugger_nested_instruction(frame),
        Err(BlueJsPageRuntimeError::DebuggerNestedFrameUnavailable)
    );
    assert_eq!(
        runtime.resume_debugger_execution(7),
        Ok(BlueJsPageDebuggerExecutionState::Completed)
    );
    let BlueJsPageDebuggerNestedExecutionState::Paused {
        frame: next_frame, ..
    } = runtime
        .execute_program_until_nested_debugger_pause(7, program, point)
        .unwrap()
    else {
        panic!("the second invocation must pause");
    };
    assert_ne!(next_frame, frame);
    assert_eq!(
        runtime.step_debugger_nested_instruction(frame),
        Err(BlueJsPageRuntimeError::DebuggerNestedFrameUnavailable)
    );
    runtime.navigate(7, origin()).unwrap();
    assert_eq!(
        runtime.step_debugger_nested_instruction(next_frame),
        Err(BlueJsPageRuntimeError::DebuggerNestedFrameUnavailable)
    );
}

#[cfg_attr(test, test)]
fn nested_page_module_frame_preserves_graph_and_rejoins_entry() {
    let mut runtime = BlueJsPageRuntime::default();
    runtime.open_realm(7, origin()).unwrap();
    let dependency = runtime
        .install_program(
            7,
            &origin(),
            source("page:///dep.mjs"),
            &BlueJsProgramV1::Module(
                parse_module("globalThis.depRuns = (globalThis.depRuns || 0) + 1;").unwrap(),
            ),
        )
        .unwrap();
    let entry = runtime
        .install_program(
            7,
            &origin(),
            source("page:///entry.mjs"),
            &BlueJsProgramV1::Module(
                parse_module(
                    "import './dep.mjs'; function inner() { return 4; } globalThis.answer = inner() + 1;",
                )
                .unwrap(),
            ),
        )
        .unwrap();
    let point = first_nested_point(&runtime, 7, entry);
    let BlueJsPageDebuggerNestedExecutionState::Paused { frame, .. } = runtime
        .execute_module_graph_until_nested_debugger_pause(7, entry, vec![dependency, entry], point)
        .unwrap()
    else {
        panic!("the module child must pause");
    };
    let snapshot = runtime
        .debugger_stack_snapshot(7, entry, Some(frame), 2, 256)
        .unwrap();
    assert_eq!(snapshot.frames.len(), 2);
    assert_eq!(snapshot.frames[0].code_unit_ordinal, 1);
    assert_eq!(snapshot.frames[1].code_unit_ordinal, 0);
    assert_eq!(snapshot.program_generation, entry.generation().as_u64());
    let mut returned = false;
    for _ in 0..64 {
        if matches!(
            runtime.step_debugger_nested_instruction(frame).unwrap(),
            BlueJsPageDebuggerNestedExecutionState::FrameReturned { .. }
        ) {
            returned = true;
            break;
        }
    }
    assert!(returned);
    let root_snapshot = runtime
        .debugger_stack_snapshot(7, entry, None, 2, 256)
        .unwrap();
    assert_eq!(root_snapshot.frames.len(), 1);
    assert_eq!(root_snapshot.frames[0].code_unit_ordinal, 0);
    assert!(!root_snapshot.stack_truncated);
    assert_eq!(
        runtime.resume_debugger_module_execution(7),
        Ok(BlueJsPageDebuggerExecutionState::Completed)
    );
    let dep_reader = runtime
        .install_program(
            7,
            &origin(),
            source("page:///dep-reader.js"),
            &BlueJsProgramV1::Script(parse("globalThis.depRuns").unwrap()),
        )
        .unwrap();
    assert_eq!(
        runtime.execute_program(7, dep_reader),
        Ok(Value::Number(1.0))
    );
    let answer_reader = runtime
        .install_program(
            7,
            &origin(),
            source("page:///answer-reader.js"),
            &BlueJsProgramV1::Script(parse("globalThis.answer").unwrap()),
        )
        .unwrap();
    assert_eq!(
        runtime.execute_program(7, answer_reader),
        Ok(Value::Number(5.0))
    );
}

#[cfg_attr(test, test)]
fn linked_module_frame_keeps_both_page_programs_and_expires_on_resume() {
    let mut runtime = BlueJsPageRuntime::default();
    runtime.open_realm(7, origin()).unwrap();
    runtime.open_realm(8, origin()).unwrap();
    let dependency = runtime
        .install_program(
            7,
            &origin(),
            source("page:///linked-dep.mjs"),
            &BlueJsProgramV1::Module(
                parse_module("export function inner() { return 41; }").unwrap(),
            ),
        )
        .unwrap();
    let entry = runtime
        .install_program(
            7,
            &origin(),
            source("page:///linked-entry.mjs"),
            &BlueJsProgramV1::Module(
                parse_module("import { inner } from './linked-dep.mjs'; export const rootValue = 9; export const answer = inner() + rootValue;")
                    .unwrap(),
            ),
        )
        .unwrap();
    let point = first_nested_point(&runtime, 7, dependency);
    let BlueJsPageDebuggerLinkedExecutionState::Paused { frame, .. } = runtime
        .execute_module_graph_until_linked_nested_debugger_pause(
            7,
            entry,
            dependency,
            vec![dependency, entry],
            point,
        )
        .unwrap()
    else {
        panic!("entry must pause in its dependency child");
    };
    assert_eq!(frame.entry_program(), entry);
    assert_eq!(frame.dependency_program(), dependency);
    assert_eq!(frame.tab_id(), 7);
    let snapshot = runtime
        .debugger_linked_stack_snapshot(7, frame, 2, 256)
        .unwrap();
    let unrelated = install(
        &mut runtime,
        7,
        "page:///other-owned.mjs",
        &BlueJsProgramV1::Module(parse_module("export const other = 42;").unwrap()),
    );
    // Simulate stale host-side association metadata using live, admitted
    // handles. The VM's paused frames and all heap objects remain unchanged.
    for stale in [
        BlueJsPageDebuggerLinkedFrame {
            entry_program: unrelated,
            ..frame
        },
        BlueJsPageDebuggerLinkedFrame {
            dependency_program: unrelated,
            ..frame
        },
        BlueJsPageDebuggerLinkedFrame {
            code_unit_ordinal: frame.code_unit_ordinal + 1,
            ..frame
        },
    ] {
        runtime.realms.get_mut(&7).unwrap().debugger_linked_frame = Some(stale);
        assert_eq!(
            runtime.debugger_linked_stack_snapshot(7, stale, 2, 256),
            Err(BlueJsPageRuntimeError::DebuggerNestedFrameUnavailable)
        );
    }
    runtime.realms.get_mut(&7).unwrap().debugger_linked_frame = Some(frame);
    assert_eq!(
        runtime.debugger_linked_stack_snapshot(7, frame, 2, 256),
        Ok(snapshot.clone())
    );
    assert_eq!(snapshot.program_generation, entry.generation().as_u64());
    assert_eq!(
        snapshot.frames[0].program_generation,
        dependency.generation().as_u64()
    );
    assert_eq!(
        snapshot.frames[1].program_generation,
        entry.generation().as_u64()
    );
    let root_entry = snapshot.frames[1]
        .scope_entries
        .iter()
        .find(|entry| {
            runtime.debugger_linked_value_preview(7, frame, &snapshot, **entry)
                == Ok(VmDebuggerValuePreview::NumberBits(9.0_f64.to_bits()))
        })
        .copied()
        .expect("the exact entry root must retain its initialized binding");
    assert!(runtime
        .debugger_linked_value_preview(8, frame, &snapshot, root_entry)
        .is_err());
    let mut moved_snapshot = snapshot.clone();
    moved_snapshot.frames[0].bytecode_offset += 1;
    assert!(runtime
        .debugger_linked_value_preview(7, frame, &moved_snapshot, root_entry)
        .is_err());
    assert!(runtime
        .debugger_linked_value_preview(
            7,
            frame,
            &snapshot,
            VmDebuggerScopeEntry {
                slot_ordinal: u32::MAX,
                ..root_entry
            },
        )
        .is_err());
    assert!(runtime
        .debugger_stack_snapshot(7, entry, None, 2, 256)
        .is_err());
    assert!(runtime
        .debugger_linked_stack_snapshot(8, frame, 2, 256)
        .is_err());
    assert!(runtime
        .debugger_linked_stack_snapshot(7, frame, 1, 256)
        .is_err());
    let wrong_dependency = BlueJsPageDebuggerLinkedFrame {
        dependency_program: entry,
        ..frame
    };
    assert!(runtime
        .debugger_linked_value_preview(7, wrong_dependency, &snapshot, root_entry)
        .is_err());
    assert!(runtime
        .debugger_linked_stack_snapshot(7, wrong_dependency, 2, 256)
        .is_err());
    assert!(runtime
        .resume_debugger_linked_nested_execution(wrong_dependency)
        .is_err());
    let wrong_serial = BlueJsPageDebuggerLinkedFrame {
        invocation_serial: frame.invocation_serial() + 1,
        ..frame
    };
    assert!(runtime
        .resume_debugger_linked_nested_execution(wrong_serial)
        .is_err());
    assert!(matches!(
        runtime.resume_debugger_linked_nested_execution(frame),
        Ok(BlueJsPageDebuggerLinkedExecutionState::FrameReturned { .. })
    ));
    assert!(runtime
        .debugger_linked_stack_snapshot(7, frame, 2, 256)
        .is_err());
    assert!(runtime
        .debugger_linked_value_preview(7, frame, &snapshot, root_entry)
        .is_err());
    assert_eq!(
        runtime.resume_debugger_module_execution(7),
        Ok(BlueJsPageDebuggerExecutionState::Completed)
    );
    assert!(runtime
        .resume_debugger_linked_nested_execution(frame)
        .is_err());
}

#[cfg_attr(test, test)]
fn linked_module_page_target_refuses_unrelated_and_stale_handles() {
    let mut runtime = BlueJsPageRuntime::default();
    runtime.open_realm(7, origin()).unwrap();
    let dependency = runtime
        .install_program(
            7,
            &origin(),
            source("page:///linked-dep.mjs"),
            &BlueJsProgramV1::Module(
                parse_module("export function inner() { return 41; }").unwrap(),
            ),
        )
        .unwrap();
    let entry = runtime
        .install_program(
            7,
            &origin(),
            source("page:///linked-entry.mjs"),
            &BlueJsProgramV1::Module(
                parse_module(
                    "import { inner } from './linked-dep.mjs'; export const answer = inner() + 1;",
                )
                .unwrap(),
            ),
        )
        .unwrap();
    let unrelated = runtime
        .install_program(
            7,
            &origin(),
            source("page:///unrelated.mjs"),
            &BlueJsProgramV1::Module(
                parse_module("export function other() { return 1; }").unwrap(),
            ),
        )
        .unwrap();
    let point = first_nested_point(&runtime, 7, dependency);
    assert!(runtime
        .execute_module_graph_until_linked_nested_debugger_pause(
            7,
            entry,
            dependency,
            [entry, unrelated],
            point,
        )
        .is_err());
    let unrelated_point = first_nested_point(&runtime, 7, unrelated);
    assert!(runtime
        .validate_linked_nested_debugger_target(
            7,
            entry,
            unrelated,
            [entry, dependency, unrelated],
            unrelated_point,
        )
        .is_err());
    assert!(runtime
        .execute_module_graph_until_linked_nested_debugger_pause(
            7,
            entry,
            unrelated,
            [entry, dependency, unrelated],
            unrelated_point,
        )
        .is_err());
    assert_eq!(
        runtime.validate_linked_nested_debugger_target(
            7,
            entry,
            dependency,
            vec![entry, dependency],
            point,
        ),
        Ok(())
    );
    runtime.navigate(7, origin()).unwrap();
    assert!(runtime
        .validate_linked_nested_debugger_target(
            7,
            entry,
            dependency,
            vec![entry, dependency],
            point
        )
        .is_err());
    assert!(runtime
        .execute_module_graph_until_linked_nested_debugger_pause(
            7,
            entry,
            dependency,
            vec![entry, dependency],
            point,
        )
        .is_err());
}

#[cfg_attr(test, test)]
fn nested_page_resume_rejoins_one_module_graph_and_revokes_exact_frame() {
    let mut runtime = BlueJsPageRuntime::default();
    runtime.open_realm(7, origin()).unwrap();
    runtime.open_realm(8, origin()).unwrap();
    let dependency = runtime
        .install_program(
            7,
            &origin(),
            source("page:///resume-dep.mjs"),
            &BlueJsProgramV1::Module(
                parse_module("globalThis.depRuns = (globalThis.depRuns || 0) + 1;").unwrap(),
            ),
        )
        .unwrap();
    let entry = runtime
        .install_program(
            7,
            &origin(),
            source("page:///resume-entry.mjs"),
            &BlueJsProgramV1::Module(
                parse_module(
                    "import './resume-dep.mjs'; function inner() { globalThis.childRuns = (globalThis.childRuns || 0) + 1; return 4; } globalThis.answer = inner() + 1;",
                )
                .unwrap(),
            ),
        )
        .unwrap();
    let point = first_nested_point(&runtime, 7, entry);
    let BlueJsPageDebuggerNestedExecutionState::Paused { frame, .. } = runtime
        .execute_module_graph_until_nested_debugger_pause(7, entry, vec![dependency, entry], point)
        .unwrap()
    else {
        panic!("module child must pause before its first instruction");
    };
    let mut wrong = frame;
    wrong.tab_id = 8;
    assert_eq!(
        runtime.resume_debugger_nested_execution(wrong),
        Err(BlueJsPageRuntimeError::DebuggerNestedFrameUnavailable)
    );
    assert!(matches!(
        runtime.resume_debugger_nested_execution(frame),
        Ok(BlueJsPageDebuggerNestedExecutionState::FrameReturned { .. })
    ));
    assert_eq!(
        runtime.resume_debugger_nested_execution(frame),
        Err(BlueJsPageRuntimeError::DebuggerNestedFrameUnavailable)
    );
    assert_eq!(
        runtime.resume_debugger_module_execution(7),
        Ok(BlueJsPageDebuggerExecutionState::Completed)
    );
    let probe = runtime
        .install_program(
            7,
            &origin(),
            source("page:///resume-probe.js"),
            &BlueJsProgramV1::Script(
                parse("globalThis.depRuns + globalThis.childRuns + globalThis.answer").unwrap(),
            ),
        )
        .unwrap();
    assert_eq!(runtime.execute_program(7, probe), Ok(Value::Number(7.0)));
}

#[cfg_attr(test, test)]
fn failed_nested_page_step_revokes_the_frame_without_claiming_completion() {
    let mut config = BlueJsPageRuntimeConfig::default();
    config.vm.instruction_budget = 256;
    let mut runtime = BlueJsPageRuntime::new(config).unwrap();
    runtime.open_realm(7, origin()).unwrap();
    let program = runtime
        .install_program(
            7,
            &origin(),
            source("page:///loop.js"),
            &BlueJsProgramV1::Script(
                parse("function inner() { while (true) {} } inner();").unwrap(),
            ),
        )
        .unwrap();
    let point = first_nested_point(&runtime, 7, program);
    let BlueJsPageDebuggerNestedExecutionState::Paused { frame, .. } = runtime
        .execute_program_until_nested_debugger_pause(7, program, point)
        .unwrap()
    else {
        panic!("the looping child must pause");
    };
    let mut exhausted = false;
    for _ in 0..300 {
        match runtime.step_debugger_nested_instruction(frame) {
            Ok(BlueJsPageDebuggerNestedExecutionState::Paused { .. }) => {}
            Err(BlueJsPageRuntimeError::Runtime(RuntimeError::InstructionLimit)) => {
                exhausted = true;
                break;
            }
            state => panic!("unexpected nested loop state: {state:?}"),
        }
    }
    assert!(exhausted);
    assert_eq!(
        runtime.step_debugger_nested_instruction(frame),
        Err(BlueJsPageRuntimeError::DebuggerNestedFrameUnavailable)
    );
}

#[cfg_attr(test, test)]
fn executes_classic_and_module_programs_in_one_tab_realm() {
    let mut runtime = BlueJsPageRuntime::default();
    runtime.open_realm(7, origin()).unwrap();
    let classic = runtime
        .install_program(
            7,
            &origin(),
            source("page:///main.js"),
            &BlueJsProgramV1::Script(parse("var answer = 40 + 2; answer;").unwrap()),
        )
        .unwrap();
    assert_eq!(
        runtime.execute_program(7, classic).unwrap(),
        Value::Number(42.0)
    );

    let module = runtime
        .install_program(
            7,
            &origin(),
            source("page:///module.js"),
            &BlueJsProgramV1::Module(parse_module("export const answer = 6 * 7;").unwrap()),
        )
        .unwrap();
    assert_eq!(
        runtime.execute_program(7, module).unwrap(),
        Value::Undefined
    );
    assert_eq!(runtime.realm_stats(7).unwrap().program_count, 2);
}

#[cfg_attr(test, test)]
fn resumes_a_non_entry_root_safe_point_without_exposing_vm_state() {
    let mut runtime = BlueJsPageRuntime::default();
    runtime.open_realm(7, origin()).unwrap();
    let paused_program = runtime
        .install_program(
            7,
            &origin(),
            source("page:///paused.js"),
            &BlueJsProgramV1::Script(
                parse("globalThis.before = 1; globalThis.after = 2;").unwrap(),
            ),
        )
        .unwrap();
    let probe = runtime
        .install_program(
            7,
            &origin(),
            source("page:///probe.js"),
            &BlueJsProgramV1::Script(parse("globalThis.before + globalThis.after").unwrap()),
        )
        .unwrap();
    let safe_point = runtime
        .safe_points(7, paused_program, 128)
        .unwrap()
        .into_iter()
        .find(|safe_point| safe_point.code_unit.ordinal() == 0 && safe_point.bytecode_offset != 0)
        .expect("fixture has a non-entry root safe point");

    assert_eq!(
        runtime
            .execute_program_until_debugger_pause(7, paused_program, safe_point)
            .unwrap(),
        BlueJsPageDebuggerExecutionState::Paused {
            bytecode_offset: safe_point.bytecode_offset
        }
    );
    assert_eq!(
        runtime.execute_program(7, probe),
        Err(BlueJsPageRuntimeError::Runtime(RuntimeError::Unsupported(
            "a debugger-paused root script must resume before another execution starts"
        )))
    );
    assert_eq!(
        runtime.resume_debugger_execution(7).unwrap(),
        BlueJsPageDebuggerExecutionState::Completed
    );
    assert_eq!(
        runtime.execute_program(7, probe).unwrap(),
        Value::Number(3.0)
    );
}

#[cfg_attr(test, test)]
fn selects_only_a_live_module_evaluate_body_root_safe_point() {
    let mut runtime = BlueJsPageRuntime::default();
    runtime.open_realm(7, origin()).unwrap();
    let module = runtime
        .install_program(
            7,
            &origin(),
            source("page:///entry.js"),
            &BlueJsProgramV1::Module(parse_module("export const answer = 6 * 7;").unwrap()),
        )
        .unwrap();
    let classic = runtime
        .install_program(
            7,
            &origin(),
            source("page:///classic.js"),
            &BlueJsProgramV1::Script(parse("globalThis.answer = 42;").unwrap()),
        )
        .unwrap();
    let point = runtime.module_evaluate_entry_safe_point(7, module).unwrap();
    let entry = runtime
        .program_registry()
        .get(module)
        .unwrap()
        .bytecode()
        .module_evaluate_entry
        .unwrap();
    assert_eq!(point.code_unit.ordinal(), 0);
    assert!(point.bytecode_offset >= entry);
    assert_eq!(
        point.bytecode_offset,
        runtime
            .safe_points(7, module, 128)
            .unwrap()
            .into_iter()
            .filter(|candidate| {
                candidate.code_unit.ordinal() == 0 && candidate.bytecode_offset >= entry
            })
            .map(|candidate| candidate.bytecode_offset)
            .min()
            .unwrap()
    );
    assert_eq!(
        runtime.module_evaluate_entry_safe_point(7, classic),
        Err(BlueJsPageRuntimeError::ProgramShape)
    );
    runtime.navigate(7, origin()).unwrap();
    assert_eq!(
        runtime.module_evaluate_entry_safe_point(7, module),
        Err(BlueJsPageRuntimeError::ProgramNotOwnedByRealm {
            tab_id: 7,
            handle: module,
        })
    );
}

#[cfg_attr(test, test)]
fn steps_only_the_paused_tab_root_and_remains_generation_bound() {
    let mut runtime = BlueJsPageRuntime::default();
    runtime.open_realm(7, origin()).unwrap();
    runtime.open_realm(8, origin()).unwrap();
    let program = runtime
        .install_program(
            7,
            &origin(),
            source("page:///stepped.js"),
            &BlueJsProgramV1::Script(
                parse("globalThis.stepped = (globalThis.stepped || 0) + 1;").unwrap(),
            ),
        )
        .unwrap();
    let root_offsets: Vec<_> = runtime
        .safe_points(7, program, 128)
        .unwrap()
        .into_iter()
        .filter(|point| point.code_unit.ordinal() == 0)
        .map(|point| point.bytecode_offset)
        .collect();
    assert!(root_offsets.len() > 2);
    assert_eq!(
        runtime
            .execute_program_until_debugger_pause_at_root_offset(7, program, root_offsets[0])
            .unwrap(),
        BlueJsPageDebuggerExecutionState::Paused {
            bytecode_offset: root_offsets[0]
        }
    );
    assert!(matches!(
        runtime.step_debugger_root_instruction(8),
        Err(BlueJsPageRuntimeError::Runtime(RuntimeError::Unsupported(
            "no debugger-paused root script is available"
        )))
    ));
    assert_eq!(
        runtime.step_debugger_root_instruction(7).unwrap(),
        BlueJsPageDebuggerExecutionState::Paused {
            bytecode_offset: root_offsets[1]
        }
    );
    assert!(runtime.close_realm(7));
    runtime.open_realm(7, origin()).unwrap();
    assert!(matches!(
        runtime.step_debugger_root_instruction(7),
        Err(BlueJsPageRuntimeError::Runtime(RuntimeError::Unsupported(
            "no debugger-paused root script is available"
        )))
    ));
}

#[cfg_attr(test, test)]
fn close_and_reopen_discard_a_paused_root_continuation_with_its_generation() {
    let mut runtime = BlueJsPageRuntime::default();
    runtime.open_realm(7, origin()).unwrap();
    let paused_program = runtime
        .install_program(
            7,
            &origin(),
            source("page:///discarded.js"),
            &BlueJsProgramV1::Script(parse("globalThis.discarded = 1;").unwrap()),
        )
        .unwrap();
    let safe_point = runtime
        .safe_points(7, paused_program, 128)
        .unwrap()
        .into_iter()
        .find(|safe_point| safe_point.code_unit.ordinal() == 0)
        .unwrap();
    assert!(debug_starts_with(
        &runtime.execute_program_until_debugger_pause(7, paused_program, safe_point),
        "Ok(Paused { "
    ));

    assert!(runtime.close_realm(7));
    runtime.open_realm(7, origin()).unwrap();
    assert_eq!(
        runtime.resume_debugger_execution(7),
        Err(BlueJsPageRuntimeError::Runtime(RuntimeError::Unsupported(
            "no debugger-paused root script is available"
        )))
    );
    assert_eq!(
        runtime.execute_program(7, paused_program),
        Err(BlueJsPageRuntimeError::ProgramNotOwnedByRealm {
            tab_id: 7,
            handle: paused_program
        })
    );
}

#[cfg_attr(test, test)]
fn debugger_continuation_rejects_child_function_code_units_exactly() {
    let mut runtime = BlueJsPageRuntime::default();
    runtime.open_realm(7, origin()).unwrap();
    let program = runtime
        .install_program(
            7,
            &origin(),
            source("page:///nested.js"),
            &BlueJsProgramV1::Script(parse("function f() { return 1; } f();").unwrap()),
        )
        .unwrap();
    let child_safe_point = runtime
        .safe_points(7, program, 128)
        .unwrap()
        .into_iter()
        .find(|safe_point| safe_point.code_unit.ordinal() != 0)
        .expect("fixture emits a child function code unit");
    assert_eq!(
        runtime.execute_program_until_debugger_pause(7, program, child_safe_point),
        Err(BlueJsPageRuntimeError::DebuggerRootCodeUnitOnly)
    );
    assert_eq!(
        runtime.execute_program(7, program).unwrap(),
        Value::Number(1.0)
    );
}

#[cfg_attr(test, test)]
fn host_bindings_are_realm_local_and_do_not_expose_vm_execution() {
    let mut runtime = BlueJsPageRuntime::default();
    runtime.open_realm(7, origin()).unwrap();
    runtime
        .configure_realm_bindings(7, |bindings| {
            let host = bindings.install_global_object("pageHost").unwrap();
            bindings.install_method(host, "answer", 0, |_args: &[crate::HostValue]| {
                Ok(crate::HostValue::Number(42.0))
            })
        })
        .unwrap();
    let first = runtime
        .install_program(
            7,
            &origin(),
            source("page:///first.js"),
            &BlueJsProgramV1::Script(parse("pageHost.answer();").unwrap()),
        )
        .unwrap();
    assert_eq!(
        runtime.execute_program(7, first).unwrap(),
        Value::Number(42.0)
    );

    runtime.navigate(7, origin()).unwrap();
    let replacement = runtime
        .install_program(
            7,
            &origin(),
            source("page:///replacement.js"),
            &BlueJsProgramV1::Script(parse("pageHost.answer();").unwrap()),
        )
        .unwrap();
    assert!(matches!(
        runtime.execute_program(7, replacement),
        Err(BlueJsPageRuntimeError::Runtime(RuntimeError::ReferenceError(name)))
            if name == "pageHost"
    ));
}

#[cfg_attr(test, test)]
fn host_click_dispatch_is_realm_bound_and_old_document_listeners_expire() {
    let mut runtime = BlueJsPageRuntime::default();
    runtime.open_realm(7, origin()).unwrap();
    runtime.open_realm(8, origin()).unwrap();
    let mut family = None;
    runtime
        .configure_realm_bindings(7, |bindings| {
            let document = bindings.install_global_object("document")?;
            let node_family = bindings.create_host_object_family()?;
            bindings.install_host_click_event_methods(node_family)?;
            bindings.install_host_object_factory_method(
                document,
                "getElementById",
                1,
                node_family,
                |_args: &[crate::HostValue]| Ok(Some(HostObjectKey::new(7, 1, 42))),
            )?;
            family = Some(node_family);
            Ok(())
        })
        .unwrap();
    let family = family.unwrap();
    let program = runtime
        .install_program(
            7,
            &origin(),
            source("page:///click.js"),
            &BlueJsProgramV1::Script(
                parse("globalThis.clicks = 0; document.getElementById('x').addEventListener('click', function(event) { globalThis.clicks += 1; event.preventDefault(); });").unwrap(),
            ),
        )
        .unwrap();
    runtime.execute_program(7, program).unwrap();
    let key = HostObjectKey::new(7, 1, 42);
    assert!(runtime.dispatch_host_click(7, family, key).unwrap());
    assert!(matches!(
        runtime.dispatch_host_click(8, family, key),
        Err(BlueJsPageRuntimeError::Runtime(RuntimeError::TypeError(_)))
    ));
    runtime.navigate(7, origin()).unwrap();
    assert!(matches!(
        runtime.dispatch_host_click(7, family, key),
        Err(BlueJsPageRuntimeError::Runtime(RuntimeError::TypeError(_)))
    ));
    runtime.close_realm(7);
    assert!(matches!(
        runtime.dispatch_host_click(7, family, key),
        Err(BlueJsPageRuntimeError::UnknownRealm(7))
    ));
}

#[cfg_attr(test, test)]
fn host_click_checkpoint_reports_a_pending_chain_that_exceeds_its_job_limit() {
    for listener in [false, true] {
        let mut runtime = BlueJsPageRuntime::default();
        runtime.open_realm(7, origin()).unwrap();
        let mut family = None;
        if listener {
            runtime
                .configure_realm_bindings(7, |bindings| {
                    let document = bindings.install_global_object("document")?;
                    let node_family = bindings.create_host_object_family()?;
                    bindings.install_host_click_event_methods(node_family)?;
                    bindings.install_host_object_factory_method(
                        document,
                        "getElementById",
                        1,
                        node_family,
                        |_args: &[crate::HostValue]| Ok(Some(HostObjectKey::new(7, 1, 42))),
                    )?;
                    family = Some(node_family);
                    Ok(())
                })
                .unwrap();
        }
        let source_text = format!(
            "globalThis.count = 0; function chain() {{var promise = Promise.resolve(); for (var i = 0; i < 300; i++) {{promise = promise.then(() => {{count++;}});}}}} {}",
            if listener {"document.getElementById('x').addEventListener('click',chain);"} else {"chain();"}
        );
        let program = install(
            &mut runtime,
            7,
            "page:///many-jobs.js",
            &BlueJsProgramV1::Script(parse(&source_text).unwrap()),
        );
        runtime.execute_program(7, program).unwrap();
        let result = if let Some(family) = family {
            runtime
                .dispatch_host_click(7, family, HostObjectKey::new(7, 1, 42))
                .map(|_| ())
        } else {
            runtime.run_click_microtask_checkpoint(7)
        };
        assert_eq!(
            result,
            Err(BlueJsPageRuntimeError::Runtime(
                RuntimeError::InstructionLimit
            ))
        );
        runtime.run_click_microtask_checkpoint(7).unwrap();
        let reader = install(
            &mut runtime,
            7,
            "page:///job-count.js",
            &BlueJsProgramV1::Script(parse("count").unwrap()),
        );
        assert_eq!(runtime.execute_program(7, reader), Ok(Value::Number(300.0)));
    }
}

#[cfg_attr(test, test)]
fn unconfigured_document_context_globals_are_rejected_at_runtime() {
    let mut runtime = BlueJsPageRuntime::default();
    runtime.open_realm(7, origin()).unwrap();
    for global in ["blueiceDocumentText", "blueiceDocumentOrigin"] {
        let program = runtime
            .install_program(
                7,
                &origin(),
                source(&format!("page:///unconfigured-{global}.js")),
                &BlueJsProgramV1::Script(parse(&format!("{global}();")).unwrap()),
            )
            .unwrap();

        assert!(matches!(
            runtime.execute_program(7, program),
            Err(BlueJsPageRuntimeError::Runtime(RuntimeError::ReferenceError(name)))
                if name == global
        ));
    }
}

#[cfg_attr(test, test)]
fn a_linked_module_identity_cannot_be_replaced_before_navigation() {
    let mut runtime = BlueJsPageRuntime::default();
    runtime.open_realm(7, origin()).unwrap();
    let module_id = "page:///module.js";
    let first = runtime
        .install_program(
            7,
            &origin(),
            source(module_id),
            &BlueJsProgramV1::Module(parse_module("export const answer = 41;").unwrap()),
        )
        .unwrap();
    runtime.execute_module_graph(7, first, [first]).unwrap();
    runtime.discard_program(7, first).unwrap();

    let replacement = BlueJsProgramV1::Module(parse_module("export const answer = 42;").unwrap());
    assert_eq!(
        runtime.install_program(7, &origin(), source(module_id), &replacement),
        Err(BlueJsPageRuntimeError::DuplicateModuleIdentity(
            module_id.to_string()
        ))
    );

    runtime.navigate(7, origin()).unwrap();
    assert!(runtime
        .install_program(7, &origin(), source(module_id), &replacement)
        .is_ok());
}

#[cfg_attr(test, test)]
fn module_entry_pause_retains_exact_page_generation_until_resume() {
    let mut runtime = BlueJsPageRuntime::default();
    runtime.open_realm(7, origin()).unwrap();
    runtime.open_realm(8, origin()).unwrap();
    let module_id = "page:///debug-entry.mjs";
    let entry = runtime
        .install_program(
            7,
            &origin(),
            source(module_id),
            &BlueJsProgramV1::Module(
                parse_module("globalThis.moduleRuns = (globalThis.moduleRuns || 0) + 1; export const answer = 42;").unwrap(),
            ),
        )
        .unwrap();
    let point = runtime.module_evaluate_entry_safe_point(7, entry).unwrap();
    let later = runtime
        .safe_points(7, entry, 1024)
        .unwrap()
        .into_iter()
        .find(|candidate| candidate.code_unit.ordinal() == 0 && *candidate != point)
        .unwrap();
    assert_eq!(
        runtime.execute_module_graph_until_debugger_pause(7, entry, vec![entry], later),
        Err(BlueJsPageRuntimeError::DebuggerModuleEntrySafePointOnly)
    );
    assert!(matches!(
        runtime.execute_module_graph_until_debugger_pause(8, entry, vec![entry], point),
        Err(BlueJsPageRuntimeError::ProgramNotOwnedByRealm { tab_id: 8, .. })
    ));
    assert_eq!(
        runtime.execute_module_graph_until_debugger_pause(7, entry, vec![entry], point),
        Ok(BlueJsPageDebuggerExecutionState::Paused {
            bytecode_offset: point.bytecode_offset
        })
    );
    assert!(matches!(
        runtime.execute_module_graph(7, entry, vec![entry]),
        Err(BlueJsPageRuntimeError::Runtime(RuntimeError::Unsupported(
            _
        )))
    ));
    assert!(matches!(
        runtime.step_debugger_module_root_instruction(8),
        Err(BlueJsPageRuntimeError::Runtime(RuntimeError::Unsupported(
            "no debugger-paused root module is available"
        )))
    ));
    let successor = runtime.step_debugger_module_root_instruction(7).unwrap();
    let BlueJsPageDebuggerExecutionState::Paused { bytecode_offset } = successor else {
        panic!("one module-root instruction must have a verified successor");
    };
    assert!(runtime
        .safe_points(7, entry, 1024)
        .unwrap()
        .iter()
        .any(|candidate| candidate.code_unit.ordinal() == 0
            && candidate.bytecode_offset == bytecode_offset));
    assert_eq!(
        runtime.resume_debugger_module_execution(7),
        Ok(BlueJsPageDebuggerExecutionState::Completed)
    );
    runtime.execute_module_graph(7, entry, vec![entry]).unwrap();
    let reader = runtime
        .install_program(
            7,
            &origin(),
            source("page:///debug-reader.js"),
            &BlueJsProgramV1::Script(parse("globalThis.moduleRuns").unwrap()),
        )
        .unwrap();
    assert_eq!(runtime.execute_program(7, reader), Ok(Value::Number(1.0)));
    runtime.navigate(7, origin()).unwrap();
    assert!(matches!(
        runtime.execute_module_graph_until_debugger_pause(7, entry, vec![entry], point),
        Err(BlueJsPageRuntimeError::ProgramNotOwnedByRealm { tab_id: 7, .. })
    ));
    assert!(matches!(
        runtime.step_debugger_module_root_instruction(7),
        Err(BlueJsPageRuntimeError::Runtime(RuntimeError::Unsupported(
            "no debugger-paused root module is available"
        )))
    ));
}

#[cfg_attr(test, test)]
fn a_failed_module_evaluation_still_reserves_its_identity_until_navigation() {
    let mut runtime = BlueJsPageRuntime::default();
    runtime.open_realm(7, origin()).unwrap();
    let module_id = "page:///failed-module.js";
    let failed = runtime
        .install_program(
            7,
            &origin(),
            source(module_id),
            &BlueJsProgramV1::Module(parse_module("throw 1;").unwrap()),
        )
        .unwrap();
    assert!(debug_starts_with(
        &runtime.execute_module_graph(7, failed, [failed]),
        "Err(Runtime("
    ));
    runtime.discard_program(7, failed).unwrap();

    let replacement = BlueJsProgramV1::Module(parse_module("export const answer = 42;").unwrap());
    assert_eq!(
        runtime.install_program(7, &origin(), source(module_id), &replacement),
        Err(BlueJsPageRuntimeError::DuplicateModuleIdentity(
            module_id.to_string()
        ))
    );
}

#[cfg_attr(test, test)]
fn navigation_invalidates_old_handles_before_the_replacement_realm_runs() {
    let mut runtime = BlueJsPageRuntime::default();
    runtime.open_realm(3, origin()).unwrap();
    let handle = runtime
        .install_program(
            3,
            &origin(),
            source("page:///old.js"),
            &BlueJsProgramV1::Script(parse("1 + 2").unwrap()),
        )
        .unwrap();
    let safe_point = runtime
        .program_registry()
        .get(handle)
        .unwrap()
        .safe_points()
        .next()
        .unwrap();
    runtime.navigate(3, origin()).unwrap();
    assert_eq!(runtime.realm_stats(3).unwrap().program_count, 0);
    assert_eq!(
        runtime.execute_program(3, handle),
        Err(BlueJsPageRuntimeError::ProgramNotOwnedByRealm { tab_id: 3, handle })
    );
    assert_eq!(
        runtime.validate_safe_point(3, handle, safe_point),
        Err(BlueJsPageRuntimeError::ProgramNotOwnedByRealm { tab_id: 3, handle })
    );
    assert!(debug_starts_with(
        &runtime.program_registry().get(handle).err(),
        "Some(UnknownProgram)"
    ));
}

#[cfg_attr(test, test)]
fn closing_a_realm_invalidates_every_program_it_owned() {
    let mut runtime = BlueJsPageRuntime::default();
    runtime.open_realm(3, origin()).unwrap();
    let handle = runtime
        .install_program(
            3,
            &origin(),
            source("page:///close.js"),
            &BlueJsProgramV1::Script(parse("1 + 2").unwrap()),
        )
        .unwrap();

    assert!(runtime.close_realm(3));
    assert!(!runtime.close_realm(3));
    assert!(debug_starts_with(
        &runtime.program_registry().get(handle).err(),
        "Some(UnknownProgram)"
    ));
}

#[cfg_attr(test, test)]
fn origin_and_bytecode_limits_fail_without_program_admission() {
    let mut runtime = BlueJsPageRuntime::new(BlueJsPageRuntimeConfig {
        max_bytecode_bytes_per_realm: 1,
        ..BlueJsPageRuntimeConfig::default()
    })
    .unwrap();
    runtime.open_realm(1, origin()).unwrap();
    let other_origin = BlueJsPageOrigin::new("https://other.test").unwrap();
    let program = BlueJsProgramV1::Script(parse("40 + 2").unwrap());
    assert_eq!(
        runtime.install_program(1, &other_origin, source("page:///wrong.js"), &program),
        Err(BlueJsPageRuntimeError::OriginMismatch)
    );
    assert_eq!(
        runtime.install_program(1, &origin(), source("page:///large.js"), &program),
        Err(BlueJsPageRuntimeError::BytecodeLimit {
            tab_id: 1,
            limit: 1
        })
    );
    assert_eq!(runtime.realm_stats(1).unwrap().program_count, 0);
}

#[cfg_attr(test, test)]
fn discarding_a_program_releases_ownership_and_bytecode_accounting() {
    let mut runtime = BlueJsPageRuntime::default();
    runtime.open_realm(1, origin()).unwrap();
    let handle = runtime
        .install_program(
            1,
            &origin(),
            source("page:///discard.js"),
            &BlueJsProgramV1::Script(parse("42").unwrap()),
        )
        .unwrap();
    assert!(runtime.realm_stats(1).unwrap().bytecode_bytes > 0);

    runtime.discard_program(1, handle).unwrap();
    assert_eq!(runtime.realm_stats(1).unwrap().program_count, 0);
    assert_eq!(runtime.realm_stats(1).unwrap().bytecode_bytes, 0);
    assert!(debug_starts_with(
        &runtime.program_registry().get(handle).err(),
        "Some(UnknownProgram)"
    ));
    assert_eq!(
        runtime.execute_program(1, handle),
        Err(BlueJsPageRuntimeError::ProgramNotOwnedByRealm { tab_id: 1, handle })
    );
}

#[cfg_attr(test, test)]
fn a_handle_cannot_cross_between_tab_realms() {
    let mut runtime = BlueJsPageRuntime::default();
    runtime.open_realm(1, origin()).unwrap();
    runtime.open_realm(2, origin()).unwrap();
    let handle = runtime
        .install_program(
            1,
            &origin(),
            source("page:///first.js"),
            &BlueJsProgramV1::Script(parse("42").unwrap()),
        )
        .unwrap();
    assert_eq!(
        runtime.execute_program(2, handle),
        Err(BlueJsPageRuntimeError::ProgramNotOwnedByRealm { tab_id: 2, handle })
    );
}

#[cfg_attr(test, test)]
fn safe_points_remain_exactly_generation_validated() {
    let mut runtime = BlueJsPageRuntime::default();
    runtime.open_realm(1, origin()).unwrap();
    let handle = runtime
        .install_program(
            1,
            &origin(),
            source("page:///safe.js"),
            &BlueJsProgramV1::Script(parse("42").unwrap()),
        )
        .unwrap();
    let safe_point = runtime
        .program_registry()
        .get(handle)
        .unwrap()
        .safe_points()
        .next()
        .unwrap();
    assert_eq!(runtime.program_handles(1).unwrap(), vec![handle]);
    assert_eq!(runtime.safe_points(1, handle, 32).unwrap()[0], safe_point);
    assert_eq!(
        runtime.safe_points(1, handle, 0),
        Err(BlueJsPageRuntimeError::SafePointLimit {
            tab_id: 1,
            limit: 0,
        })
    );
    runtime.validate_safe_point(1, handle, safe_point).unwrap();
    let malformed = BlueJsSafePoint {
        code_unit: safe_point.code_unit,
        bytecode_offset: safe_point.bytecode_offset + 1,
    };
    assert_eq!(
        runtime.validate_safe_point(1, handle, malformed),
        Err(BlueJsPageRuntimeError::ProgramRegistry(
            BlueJsProgramDebugError::InvalidInstructionBoundary
        ))
    );
}

impl BlueJsPageRuntime {
    /// Runs page runtime contracts in unit and coverage builds.
    #[doc(hidden)]
    pub fn verify_retained_page_runtime_contracts() {
        graph_debugger_entries_validate_owned_graphs_and_complete_without_an_invocation();
        uncaught_site_read_requires_exact_page_owned_program_and_last_execution();
        uncaught_module_root_and_child_sites_obey_the_same_page_boundary();
        page_value_preview_requires_owned_exact_paused_slot();
        page_stack_snapshot_requires_the_paused_program_and_exact_nested_frame();
        nested_page_frame_is_exact_and_revoked_after_return_or_navigation();
        nested_page_module_frame_preserves_graph_and_rejoins_entry();
        linked_module_frame_keeps_both_page_programs_and_expires_on_resume();
        linked_module_page_target_refuses_unrelated_and_stale_handles();
        nested_page_resume_rejoins_one_module_graph_and_revokes_exact_frame();
        failed_nested_page_step_revokes_the_frame_without_claiming_completion();
        executes_classic_and_module_programs_in_one_tab_realm();
        resumes_a_non_entry_root_safe_point_without_exposing_vm_state();
        selects_only_a_live_module_evaluate_body_root_safe_point();
        steps_only_the_paused_tab_root_and_remains_generation_bound();
        close_and_reopen_discard_a_paused_root_continuation_with_its_generation();
        debugger_continuation_rejects_child_function_code_units_exactly();
        host_bindings_are_realm_local_and_do_not_expose_vm_execution();
        host_click_dispatch_is_realm_bound_and_old_document_listeners_expire();
        host_click_checkpoint_reports_a_pending_chain_that_exceeds_its_job_limit();
        unconfigured_document_context_globals_are_rejected_at_runtime();
        a_linked_module_identity_cannot_be_replaced_before_navigation();
        module_entry_pause_retains_exact_page_generation_until_resume();
        a_failed_module_evaluation_still_reserves_its_identity_until_navigation();
        navigation_invalidates_old_handles_before_the_replacement_realm_runs();
        closing_a_realm_invalidates_every_program_it_owned();
        origin_and_bytecode_limits_fail_without_program_admission();
        discarding_a_program_releases_ownership_and_bytecode_accounting();
        a_handle_cannot_cross_between_tab_realms();
        safe_points_remain_exactly_generation_validated();
    }
}

#[cfg_attr(test, test)]
fn graph_debugger_entries_validate_owned_graphs_and_complete_without_an_invocation() {
    for linked in [false, true] {
        let mut runtime = BlueJsPageRuntime::default();
        runtime.open_realm(7, origin()).unwrap();
        let dependency = install(
            &mut runtime,
            7,
            "page:///idle-dep.js",
            &BlueJsProgramV1::Module(
                parse_module("export function answer() {return 42;}").unwrap(),
            ),
        );
        let entry = install(&mut runtime, 7, "page:///idle-entry.js",
            &BlueJsProgramV1::Module(parse_module(
                "import {answer} from './idle-dep.js'; function child() {var value = answer(); return value;} globalThis.result = 7;").unwrap()));
        let classic = install(
            &mut runtime,
            7,
            "page:///classic-shape.js",
            &BlueJsProgramV1::Script(parse("42").unwrap()),
        );
        let entry_point = runtime.module_evaluate_entry_safe_point(7, entry).unwrap();
        let child_point = first_nested_point(&runtime, 7, entry);
        let dependency_point = first_nested_point(&runtime, 7, dependency);
        let classic_point = runtime.safe_points(7, classic, 8).unwrap()[0];
        assert_eq!(
            runtime.execute_module_graph_until_debugger_pause(
                7,
                classic,
                vec![classic],
                classic_point
            ),
            Err(BlueJsPageRuntimeError::ProgramShape)
        );
        assert!(runtime
            .execute_module_graph_until_debugger_pause(7, entry, vec![classic], entry_point)
            .is_err());
        assert_eq!(
            runtime.execute_module_graph_until_nested_debugger_pause(
                7,
                entry,
                vec![entry, dependency],
                entry_point
            ),
            Err(BlueJsPageRuntimeError::DebuggerNestedCodeUnitOnly)
        );
        assert!(runtime
            .execute_module_graph_until_nested_debugger_pause(7, entry, vec![classic], child_point)
            .is_err());
        assert_eq!(
            runtime.execute_module_graph_until_linked_nested_debugger_pause(
                7,
                entry,
                entry,
                vec![entry, dependency],
                child_point
            ),
            Err(BlueJsPageRuntimeError::DebuggerNestedCodeUnitOnly)
        );
        assert!(runtime
            .execute_module_graph_until_linked_nested_debugger_pause(
                7,
                entry,
                dependency,
                vec![classic],
                dependency_point
            )
            .is_err());
        assert_eq!(
            runtime.validate_linked_nested_debugger_target(
                7,
                entry,
                entry,
                vec![entry, dependency],
                child_point
            ),
            Err(BlueJsPageRuntimeError::DebuggerNestedCodeUnitOnly)
        );
        assert!(runtime
            .validate_linked_nested_debugger_target(
                7,
                entry,
                dependency,
                vec![classic],
                dependency_point
            )
            .is_err());
        if linked {
            assert_eq!(
                runtime.execute_module_graph_until_linked_nested_debugger_pause(
                    7,
                    entry,
                    dependency,
                    vec![entry, dependency],
                    dependency_point
                ),
                Ok(BlueJsPageDebuggerLinkedExecutionState::Completed)
            );
        } else {
            assert_eq!(
                runtime.execute_module_graph_until_nested_debugger_pause(
                    7,
                    entry,
                    vec![entry, dependency],
                    child_point
                ),
                Ok(BlueJsPageDebuggerNestedExecutionState::Completed)
            );
        }
    }
}
