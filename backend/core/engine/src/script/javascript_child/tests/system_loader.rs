// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

const LOADER: PageHostDebuggerProgram = PageHostDebuggerProgram {
    program_handle: 11,
    program_generation: 13,
};
const EMITTED: PageHostDebuggerProgram = PageHostDebuggerProgram {
    program_handle: 17,
    program_generation: 19,
};
const TARGET: JavaScriptPageDebuggerSafePoint = JavaScriptPageDebuggerSafePoint {
    code_unit_ordinal: 2,
    bytecode_offset: 4,
};

#[derive(Clone, Copy)]
enum ArmReply {
    Exact,
    WrongTab,
    WrongGeneration,
    WrongLoader,
    WrongTarget,
    PrivateError,
}

#[derive(Clone, Copy)]
enum StackReply {
    Exact,
    WrongFrame,
    WrongLoader,
    WrongMiddle,
    WrongTopOffset,
    Truncated,
}

#[derive(Clone, Copy)]
enum LookupReply {
    Exact,
    Unbound,
    WrongTab,
    WrongGeneration,
    WrongLoader,
    WrongProgram,
    WrongCoordinate,
    RootTarget,
    ForeignTarget,
}

#[derive(Clone, Copy)]
enum LocationReply {
    Exact,
    WrongFrame,
    WrongLoader,
    WrongMiddle,
    WrongStackPoint,
    WrongOriginal,
    WrongCoordinate,
    MovedPause,
}

struct SystemLoaderChild {
    lifecycle: RecordingChild,
    arm_reply: ArmReply,
    arm_calls: usize,
    stack_reply: StackReply,
    stack_calls: usize,
    lookup_reply: LookupReply,
    lookup_calls: usize,
    location_reply: LocationReply,
    location_calls: usize,
}

impl PageHostClient for SystemLoaderChild {
    fn synchronize_document(&mut self, document: PageHostDocument) -> io::Result<PageHostReply> {
        self.lifecycle.synchronize_document(document)
    }

    fn close_realm(&mut self, tab_id: u64, generation: u64) -> io::Result<PageHostReply> {
        self.lifecycle.close_realm(tab_id, generation)
    }

    fn debugger_realm_stats(&mut self, tab_id: u64, generation: u64) -> io::Result<PageHostReply> {
        self.lifecycle.debugger_realm_stats(tab_id, generation)
    }

    fn debugger_execution_control_available(&self) -> bool {
        true
    }

    fn debugger_nested_frames_available(&self) -> bool {
        true
    }

    fn debugger_emitted_map_available(&self) -> bool {
        true
    }

    fn debugger_programs(&mut self, tab_id: u64, generation: u64) -> io::Result<PageHostReply> {
        Ok(PageHostReply::DebuggerPrograms {
            tab_id,
            document_generation: generation,
            programs: vec![LOADER, EMITTED],
        })
    }

    fn validate_debugger_safe_point(
        &mut self,
        tab_id: u64,
        generation: u64,
        safe_point: PageHostDebuggerSafePoint,
    ) -> io::Result<PageHostReply> {
        assert!(safe_point.program == EMITTED || safe_point.program == LOADER);
        Ok(PageHostReply::DebuggerSafePointValidated {
            tab_id,
            document_generation: generation,
            safe_point,
        })
    }

    fn debugger_system_emitted_safe_point(
        &mut self,
        tab_id: u64,
        generation: u64,
        loader_program: PageHostDebuggerProgram,
        program: PageHostDebuggerProgram,
        generated_line: u32,
        generated_column_utf16: u32,
    ) -> io::Result<PageHostReply> {
        assert_eq!(loader_program, LOADER);
        assert_eq!(program, EMITTED);
        self.lookup_calls += 1;
        Ok(PageHostReply::DebuggerSystemEmittedSafePoint {
            tab_id: tab_id + u64::from(matches!(self.lookup_reply, LookupReply::WrongTab)),
            document_generation: generation
                + u64::from(matches!(self.lookup_reply, LookupReply::WrongGeneration)),
            loader_program: if matches!(self.lookup_reply, LookupReply::WrongLoader) {
                EMITTED
            } else {
                loader_program
            },
            program: if matches!(self.lookup_reply, LookupReply::WrongProgram) {
                LOADER
            } else {
                program
            },
            generated_line: generated_line
                + u32::from(matches!(self.lookup_reply, LookupReply::WrongCoordinate)),
            generated_column_utf16,
            safe_point: if matches!(self.lookup_reply, LookupReply::Unbound) {
                None
            } else {
                Some(PageHostDebuggerSafePoint {
                    program: if matches!(self.lookup_reply, LookupReply::ForeignTarget) {
                        LOADER
                    } else {
                        program
                    },
                    code_unit_ordinal: if matches!(self.lookup_reply, LookupReply::RootTarget) {
                        0
                    } else {
                        TARGET.code_unit_ordinal
                    },
                    bytecode_offset: TARGET.bytecode_offset,
                })
            },
        })
    }

    fn debugger_emitted_original_position(
        &mut self,
        tab_id: u64,
        generation: u64,
        program: PageHostDebuggerProgram,
        generated_line: u32,
        generated_column_utf16: u32,
    ) -> io::Result<PageHostReply> {
        assert_eq!(program, EMITTED);
        Ok(PageHostReply::DebuggerEmittedOriginalPosition {
            tab_id,
            document_generation: generation,
            program,
            generated_line,
            generated_column_utf16,
            original: (generated_line == 5 && generated_column_utf16 == 8).then_some(
                page_host::PageHostDebuggerEmittedOriginalPosition {
                    source_index: 0,
                    line: 2,
                    column_utf16: 3,
                },
            ),
        })
    }

    fn debugger_system_stack_locations(
        &mut self,
        frame: PageHostDebuggerFrame,
        loader_program: PageHostDebuggerProgram,
        max_scope_entries: u32,
    ) -> io::Result<PageHostReply> {
        self.location_calls += 1;
        let PageHostReply::DebuggerSystemStackSnapshot { mut snapshot, .. } =
            self.debugger_system_stack_snapshot(frame, loader_program, max_scope_entries)?
        else {
            panic!("mock stack must be available");
        };
        if matches!(self.location_reply, LocationReply::WrongStackPoint) {
            snapshot.frames[0].safe_point.bytecode_offset += 1;
        }
        let location = page_host::PageHostDebuggerEmittedPausedRootLocation {
            generated_line: 5,
            generated_column_utf16: 8 + u32::from(matches!(
                self.location_reply,
                LocationReply::WrongCoordinate
            )),
            original: Some(page_host::PageHostDebuggerEmittedOriginalPosition {
                source_index: 0,
                line: 2 + u32::from(matches!(self.location_reply, LocationReply::WrongOriginal)),
                column_utf16: 3,
            }),
        };
        Ok(PageHostReply::DebuggerSystemStackLocations {
            frame: PageHostDebuggerFrame {
                invocation_serial: frame.invocation_serial
                    + u64::from(matches!(self.location_reply, LocationReply::WrongFrame)),
                ..frame
            },
            loader_program: if matches!(self.location_reply, LocationReply::WrongLoader) {
                EMITTED
            } else {
                loader_program
            },
            snapshot,
            locations: Box::new([
                Some(location),
                matches!(self.location_reply, LocationReply::WrongMiddle).then_some(location),
                None,
            ]),
        })
    }

    fn debugger_execution_state(
        &mut self,
        tab_id: u64,
        generation: u64,
        program: PageHostDebuggerProgram,
    ) -> io::Result<PageHostReply> {
        assert_eq!(program, EMITTED);
        Ok(PageHostReply::DebuggerExecutionState {
            tab_id,
            document_generation: generation,
            program,
            state: PageHostDebuggerExecutionState::NestedPaused {
                frame: PageHostDebuggerFrame {
                    tab_id,
                    document_generation: generation,
                    program,
                    code_unit_ordinal: TARGET.code_unit_ordinal,
                    invocation_serial: 23,
                },
                safe_point: PageHostDebuggerSafePoint {
                    program,
                    code_unit_ordinal: TARGET.code_unit_ordinal,
                    bytecode_offset: TARGET.bytecode_offset
                        + u32::from(
                            matches!(self.location_reply, LocationReply::MovedPause)
                                && self.location_calls > 0,
                        ),
                },
            },
        })
    }

    fn debugger_system_stack_snapshot(
        &mut self,
        frame: PageHostDebuggerFrame,
        loader_program: PageHostDebuggerProgram,
        max_scope_entries: u32,
    ) -> io::Result<PageHostReply> {
        assert_eq!(frame.program, EMITTED);
        assert_eq!(loader_program, LOADER);
        self.stack_calls += 1;
        let entry = |program, code_unit_ordinal, bytecode_offset| {
            page_host::PageHostDebuggerSystemStackFrame {
                safe_point: PageHostDebuggerSafePoint {
                    program,
                    code_unit_ordinal,
                    bytecode_offset,
                },
                scope_entries: Vec::new(),
                scope_truncated: false,
            }
        };
        let mut snapshot = page_host::PageHostDebuggerSystemStackSnapshot {
            frames: [
                entry(EMITTED, 2, 4),
                entry(LOADER, 1, 4),
                entry(EMITTED, 0, 4),
            ],
            stack_truncated: false,
            max_scope_entries,
        };
        if matches!(self.stack_reply, StackReply::WrongMiddle) {
            snapshot.frames[1].safe_point.program = EMITTED;
        }
        if matches!(self.stack_reply, StackReply::WrongTopOffset) {
            snapshot.frames[0].safe_point.bytecode_offset += 1;
        }
        if matches!(self.stack_reply, StackReply::Truncated) {
            snapshot.stack_truncated = true;
        }
        Ok(PageHostReply::DebuggerSystemStackSnapshot {
            frame: PageHostDebuggerFrame {
                invocation_serial: frame.invocation_serial
                    + u64::from(matches!(self.stack_reply, StackReply::WrongFrame)),
                ..frame
            },
            loader_program: if matches!(self.stack_reply, StackReply::WrongLoader) {
                EMITTED
            } else {
                loader_program
            },
            snapshot: Box::new(snapshot),
        })
    }

    fn arm_debugger_system_nested_breakpoint(
        &mut self,
        tab_id: u64,
        generation: u64,
        loader_program: PageHostDebuggerProgram,
        safe_point: PageHostDebuggerSafePoint,
    ) -> io::Result<PageHostReply> {
        assert_eq!(loader_program, LOADER);
        assert_eq!(safe_point.program, EMITTED);
        self.arm_calls += 1;
        if matches!(self.arm_reply, ArmReply::PrivateError) {
            return Ok(PageHostReply::Error {
                code: page_host::PageHostErrorCode::InvalidDebuggerState,
                message: "not pending".to_string(),
            });
        }
        Ok(PageHostReply::DebuggerSystemNestedBreakpointArmed {
            tab_id: tab_id + u64::from(matches!(self.arm_reply, ArmReply::WrongTab)),
            document_generation: generation
                + u64::from(matches!(self.arm_reply, ArmReply::WrongGeneration)),
            loader_program: if matches!(self.arm_reply, ArmReply::WrongLoader) {
                EMITTED
            } else {
                loader_program
            },
            safe_point: if matches!(self.arm_reply, ArmReply::WrongTarget) {
                PageHostDebuggerSafePoint {
                    bytecode_offset: safe_point.bytecode_offset + 1,
                    ..safe_point
                }
            } else {
                safe_point
            },
        })
    }
}

fn setup(
    arm_reply: ArmReply,
) -> (
    OutOfProcessJavaScriptPageExecutor<SystemLoaderChild>,
    TabId,
    JavaScriptPageDebuggerProgram,
    JavaScriptPageDebuggerProgram,
) {
    let (tabs, tab_id) = loaded_tabs(
        "<script>globalThis.loader = true;</script><script type=\"application/x-blueice-typescript\">const value: number = 1;</script>",
        "https://example.test/system-loader.html",
    );
    let mut executor = OutOfProcessJavaScriptPageExecutor::new_with_debugger_execution_control(
        SystemLoaderChild {
            lifecycle: RecordingChild::default(),
            arm_reply,
            arm_calls: 0,
            stack_reply: StackReply::Exact,
            stack_calls: 0,
            lookup_reply: LookupReply::Exact,
            lookup_calls: 0,
            location_reply: LocationReply::Exact,
            location_calls: 0,
        },
    );
    executor.synchronize_and_execute(&tabs).unwrap();
    let programs = executor.debugger_programs(tab_id, 1).unwrap();
    assert_eq!(programs.len(), 2);
    assert_ne!(programs[0].program_handle, LOADER.program_handle);
    assert_ne!(programs[1].program_handle, EMITTED.program_handle);
    (executor, tab_id, programs[0], programs[1])
}

#[test]
fn core_system_loader_arm_remints_both_programs_and_requires_exact_child_acknowledgement() {
    for reply in [
        ArmReply::Exact,
        ArmReply::WrongTab,
        ArmReply::WrongGeneration,
        ArmReply::WrongLoader,
        ArmReply::WrongTarget,
        ArmReply::PrivateError,
    ] {
        let (mut executor, tab_id, loader, emitted) = setup(reply);
        let result =
            executor.arm_debugger_system_nested_breakpoint(tab_id, 1, loader, emitted, TARGET);
        let expected = match reply {
            ArmReply::Exact => Ok(()),
            ArmReply::PrivateError => Err(JavaScriptPageDebuggerError::InvalidExecutionState),
            _ => Err(JavaScriptPageDebuggerError::NoLiveRealm),
        };
        assert_eq!(result, expected);
        assert_eq!(executor.child.arm_calls, 1);
    }
}

#[test]
fn core_system_loader_arm_rejects_stale_or_same_program_before_contacting_child() {
    let (mut executor, tab_id, loader, emitted) = setup(ArmReply::Exact);
    let stale_loader = JavaScriptPageDebuggerProgram {
        program_generation: loader.program_generation + 1,
        ..loader
    };
    assert_eq!(
        executor.arm_debugger_system_nested_breakpoint(tab_id, 1, stale_loader, emitted, TARGET),
        Err(JavaScriptPageDebuggerError::UnknownProgram)
    );
    assert_eq!(
        executor.arm_debugger_system_nested_breakpoint(tab_id, 1, emitted, emitted, TARGET),
        Err(JavaScriptPageDebuggerError::InvalidSafePoint)
    );
    assert_eq!(
        executor.arm_debugger_system_nested_breakpoint(
            tab_id,
            1,
            loader,
            emitted,
            JavaScriptPageDebuggerSafePoint {
                code_unit_ordinal: 0,
                ..TARGET
            },
        ),
        Err(JavaScriptPageDebuggerError::ExecutionControlUnavailable)
    );
    assert_eq!(executor.child.arm_calls, 0);
}

#[test]
fn core_system_stack_remints_all_three_frames_and_rejects_substituted_child_replies() {
    for shape in [
        StackReply::Exact,
        StackReply::WrongFrame,
        StackReply::WrongLoader,
        StackReply::WrongMiddle,
        StackReply::WrongTopOffset,
        StackReply::Truncated,
    ] {
        let (mut executor, tab_id, loader, emitted) = setup(ArmReply::Exact);
        executor.child.stack_reply = shape;
        let Some(JavaScriptPageDebuggerNestedExecutionState::Paused { frame, .. }) = executor
            .debugger_nested_execution_state(
                tab_id,
                1,
                emitted.program_handle,
                emitted.program_generation,
            )
            .unwrap()
        else {
            panic!("expected an exact nested pause");
        };
        let result = executor.debugger_system_stack_snapshot(frame, loader, 1);
        if matches!(shape, StackReply::Exact) {
            let stack = result.unwrap();
            assert_eq!(stack.frame, frame);
            assert_eq!(stack.frames[0].program, emitted);
            assert_eq!(stack.frames[1].program, loader);
            assert_eq!(stack.frames[2].program, emitted);
            assert_eq!(stack.frames[0].safe_point, TARGET);
        } else {
            assert_eq!(result, Err(JavaScriptPageDebuggerError::NoLiveRealm));
        }
        assert_eq!(executor.child.stack_calls, 1);
    }
}

#[test]
fn core_system_stack_rejects_stale_loader_before_private_inspection() {
    let (mut executor, tab_id, loader, emitted) = setup(ArmReply::Exact);
    let Some(JavaScriptPageDebuggerNestedExecutionState::Paused { frame, .. }) = executor
        .debugger_nested_execution_state(
            tab_id,
            1,
            emitted.program_handle,
            emitted.program_generation,
        )
        .unwrap()
    else {
        panic!("expected an exact nested pause");
    };
    assert_eq!(
        executor.debugger_system_stack_snapshot(
            frame,
            JavaScriptPageDebuggerProgram {
                program_generation: loader.program_generation + 1,
                ..loader
            },
            1,
        ),
        Err(JavaScriptPageDebuggerError::UnknownProgram)
    );
    assert_eq!(executor.child.stack_calls, 0);
}

#[test]
fn core_system_generated_lookup_remints_both_programs_and_rejects_forged_replies() {
    for reply in [
        LookupReply::Exact,
        LookupReply::Unbound,
        LookupReply::WrongTab,
        LookupReply::WrongGeneration,
        LookupReply::WrongLoader,
        LookupReply::WrongProgram,
        LookupReply::WrongCoordinate,
        LookupReply::RootTarget,
        LookupReply::ForeignTarget,
    ] {
        let (mut executor, tab_id, loader, emitted) = setup(ArmReply::Exact);
        executor.child.lookup_reply = reply;
        let result = executor.debugger_system_emitted_safe_point(tab_id, 1, loader, emitted, 5, 8);
        let expected = match reply {
            LookupReply::Exact => Ok(Some(TARGET)),
            LookupReply::Unbound => Ok(None),
            _ => Err(JavaScriptPageDebuggerError::NoLiveRealm),
        };
        assert_eq!(result, expected);
        assert_eq!(executor.child.lookup_calls, 1);
        assert_eq!(executor.child.arm_calls, 0);
    }
}

#[test]
fn core_system_generated_lookup_rejects_stale_or_same_program_without_child_query() {
    let (mut executor, tab_id, loader, emitted) = setup(ArmReply::Exact);
    assert_eq!(
        executor.debugger_system_emitted_safe_point(
            tab_id,
            1,
            JavaScriptPageDebuggerProgram {
                program_generation: loader.program_generation + 1,
                ..loader
            },
            emitted,
            5,
            8,
        ),
        Err(JavaScriptPageDebuggerError::UnknownProgram)
    );
    assert_eq!(
        executor.debugger_system_emitted_safe_point(tab_id, 1, emitted, emitted, 5, 8),
        Err(JavaScriptPageDebuggerError::InvalidSafePoint)
    );
    assert_eq!(executor.child.lookup_calls, 0);
}

#[test]
fn core_system_locations_compare_full_stack_and_each_map_projection() {
    for reply in [
        LocationReply::Exact,
        LocationReply::WrongFrame,
        LocationReply::WrongLoader,
        LocationReply::WrongMiddle,
        LocationReply::WrongStackPoint,
        LocationReply::WrongOriginal,
        LocationReply::WrongCoordinate,
        LocationReply::MovedPause,
    ] {
        let (mut executor, tab_id, loader, emitted) = setup(ArmReply::Exact);
        executor.child.location_reply = reply;
        let Some(JavaScriptPageDebuggerNestedExecutionState::Paused { frame, .. }) = executor
            .debugger_nested_execution_state(
                tab_id,
                1,
                emitted.program_handle,
                emitted.program_generation,
            )
            .unwrap()
        else {
            panic!("expected an exact System pause");
        };
        let result = executor.debugger_system_stack_locations(frame, loader, 1);
        if matches!(reply, LocationReply::Exact) {
            let locations = result.unwrap();
            assert_eq!(locations.stack.frame, frame);
            assert_eq!(locations.stack.frames[1].program, loader);
            assert_eq!(locations.locations[1], None);
            assert_eq!(locations.locations[2], None);
            assert_eq!(
                locations.locations[0].unwrap().original,
                Some(JavaScriptPageDebuggerEmittedOriginalPosition {
                    source_index: 0,
                    line: 2,
                    column_utf16: 3,
                })
            );
        } else {
            assert_eq!(result, Err(JavaScriptPageDebuggerError::NoLiveRealm));
        }
        assert_eq!(executor.child.location_calls, 1);
    }
}
