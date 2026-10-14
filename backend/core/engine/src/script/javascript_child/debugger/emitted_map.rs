// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Exact, core-reminted transport for one child-owned emitted artifact.
//!
//! These methods do not grant a public debugger operation. A public route
//! must separately prove owner/client policy and a same-stream program
//! receipt before it may call them.

use super::*;

const MAX_EMITTED_COORDINATE: u32 = 16 * 1024 * 1024;

fn valid_original(position: page_host::PageHostDebuggerEmittedOriginalPosition) -> bool {
    (position.source_index as usize) < page_host::PAGE_HOST_MAX_MODULES_PER_GRAPH
        && position.line <= MAX_EMITTED_COORDINATE
        && position.column_utf16 <= MAX_EMITTED_COORDINATE
}

impl<C: PageHostClient> OutOfProcessJavaScriptPageExecutor<C> {
    pub(super) fn core_debugger_system_stack_locations(
        &mut self,
        frame: JavaScriptPageDebuggerFrame,
        loader: JavaScriptPageDebuggerProgram,
        max_scope_entries: u32,
    ) -> Result<JavaScriptPageDebuggerSystemStackLocations, JavaScriptPageDebuggerError> {
        if !self.child.debugger_emitted_map_available() {
            return Err(JavaScriptPageDebuggerError::ExecutionControlUnavailable);
        }
        let expected =
            self.core_debugger_system_stack_snapshot(frame, loader, max_scope_entries)?;
        let tab_id = frame.tab_id;
        let generation = frame.document_generation;
        let child_loader = self.child_program_for_core(
            tab_id,
            generation,
            loader.program_handle,
            loader.program_generation,
        )?;
        let child_frame = self
            .debugger_nested_frames
            .get(&tab_id)
            .filter(|active| active.public == frame)
            .map(|active| active.child)
            .ok_or(JavaScriptPageDebuggerError::InvalidExecutionState)?;
        let reply = self
            .child
            .debugger_system_stack_locations(child_frame, child_loader, max_scope_entries)
            .map_err(|_| JavaScriptPageDebuggerError::NoLiveRealm)?;
        let PageHostReply::DebuggerSystemStackLocations {
            frame: echoed_frame,
            loader_program: echoed_loader,
            snapshot,
            locations,
        } = reply
        else {
            return Err(child_debugger_reply_error(&reply));
        };
        let same_stack =
            snapshot
                .frames
                .iter()
                .zip(&expected.frames)
                .all(|(child, core)| {
                    child.safe_point.code_unit_ordinal == core.safe_point.code_unit_ordinal
                        && child.safe_point.bytecode_offset == core.safe_point.bytecode_offset
                        && child.scope_truncated == core.scope_truncated
                        && child.scope_entries.len() == core.scope_entries.len()
                        && child.scope_entries.iter().zip(&core.scope_entries).all(
                            |(child, core)| {
                                child.slot_ordinal == core.slot_ordinal
                                    && child.scope_depth == core.scope_depth
                            },
                        )
                });
        if echoed_frame != child_frame
            || echoed_loader != child_loader
            || snapshot.max_scope_entries != max_scope_entries
            || !snapshot.is_well_formed(child_frame, child_loader)
            || !same_stack
            || locations[1].is_some()
        {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        let emitted = JavaScriptPageDebuggerProgram {
            program_handle: frame.program_handle,
            program_generation: frame.program_generation,
        };
        let mut projected = [None; 3];
        for index in [0, 2] {
            if let Some(location) = locations[index] {
                if location.generated_line > MAX_EMITTED_COORDINATE
                    || location.generated_column_utf16 > MAX_EMITTED_COORDINATE
                    || location
                        .original
                        .is_some_and(|original| !valid_original(original))
                {
                    return Err(JavaScriptPageDebuggerError::NoLiveRealm);
                }
                let original = self.core_debugger_emitted_original_position(
                    tab_id,
                    generation,
                    emitted.program_handle,
                    emitted.program_generation,
                    location.generated_line,
                    location.generated_column_utf16,
                )?;
                let claimed = location.original.map(|original| {
                    JavaScriptPageDebuggerEmittedOriginalPosition {
                        source_index: original.source_index,
                        line: original.line,
                        column_utf16: original.column_utf16,
                    }
                });
                if original != claimed {
                    return Err(JavaScriptPageDebuggerError::NoLiveRealm);
                }
                projected[index] = Some(JavaScriptPageDebuggerEmittedPausedRootLocation {
                    generated_line: location.generated_line,
                    generated_column_utf16: location.generated_column_utf16,
                    original: claimed,
                });
            }
        }
        if self.core_debugger_system_stack_snapshot(frame, loader, max_scope_entries)? != expected {
            return Err(JavaScriptPageDebuggerError::InvalidExecutionState);
        }
        Ok(JavaScriptPageDebuggerSystemStackLocations {
            stack: expected,
            locations: projected,
        })
    }

    pub(super) fn core_debugger_system_emitted_safe_point(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        loader: JavaScriptPageDebuggerProgram,
        emitted: JavaScriptPageDebuggerProgram,
        generated_line: u32,
        generated_column_utf16: u32,
    ) -> Result<Option<JavaScriptPageDebuggerSafePoint>, JavaScriptPageDebuggerError> {
        if !self.child.debugger_emitted_map_available()
            || generated_line > MAX_EMITTED_COORDINATE
            || generated_column_utf16 > MAX_EMITTED_COORDINATE
        {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        let child_loader = self.child_program_for_core(
            tab_id,
            document_generation,
            loader.program_handle,
            loader.program_generation,
        )?;
        let child_emitted = self.child_program_for_core(
            tab_id,
            document_generation,
            emitted.program_handle,
            emitted.program_generation,
        )?;
        if child_loader == child_emitted {
            return Err(JavaScriptPageDebuggerError::InvalidSafePoint);
        }
        let reply = self
            .child
            .debugger_system_emitted_safe_point(
                tab_id.as_u64(),
                document_generation,
                child_loader,
                child_emitted,
                generated_line,
                generated_column_utf16,
            )
            .map_err(|_| JavaScriptPageDebuggerError::NoLiveRealm)?;
        let PageHostReply::DebuggerSystemEmittedSafePoint {
            tab_id: reply_tab_id,
            document_generation: reply_generation,
            loader_program: reply_loader,
            program: reply_program,
            generated_line: reply_line,
            generated_column_utf16: reply_column,
            safe_point,
        } = reply
        else {
            return Err(child_debugger_reply_error(&reply));
        };
        if reply_tab_id != tab_id.as_u64()
            || reply_generation != document_generation
            || reply_loader != child_loader
            || reply_program != child_emitted
            || reply_line != generated_line
            || reply_column != generated_column_utf16
            || safe_point.is_some_and(|point| {
                !point.is_well_formed()
                    || point.program != child_emitted
                    || point.code_unit_ordinal == 0
            })
        {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        if let Some(point) = safe_point {
            validate_child_safe_point_reply(self, tab_id, document_generation, point)?;
        }
        Ok(safe_point.map(|point| JavaScriptPageDebuggerSafePoint {
            code_unit_ordinal: point.code_unit_ordinal,
            bytecode_offset: point.bytecode_offset,
        }))
    }

    pub(super) fn core_debugger_emitted_original_position(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        program_handle: u64,
        program_generation: u64,
        generated_line: u32,
        generated_column_utf16: u32,
    ) -> Result<Option<JavaScriptPageDebuggerEmittedOriginalPosition>, JavaScriptPageDebuggerError>
    {
        if !self.child.debugger_emitted_map_available()
            || generated_line > MAX_EMITTED_COORDINATE
            || generated_column_utf16 > MAX_EMITTED_COORDINATE
        {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        let child_program = self.child_program_for_core(
            tab_id,
            document_generation,
            program_handle,
            program_generation,
        )?;
        let reply = self
            .child
            .debugger_emitted_original_position(
                tab_id.as_u64(),
                document_generation,
                child_program,
                generated_line,
                generated_column_utf16,
            )
            .map_err(|_| JavaScriptPageDebuggerError::NoLiveRealm)?;
        let PageHostReply::DebuggerEmittedOriginalPosition {
            tab_id: reply_tab_id,
            document_generation: reply_generation,
            program,
            generated_line: reply_line,
            generated_column_utf16: reply_column,
            original,
        } = reply
        else {
            return Err(child_debugger_reply_error(&reply));
        };
        if reply_tab_id != tab_id.as_u64()
            || reply_generation != document_generation
            || program != child_program
            || reply_line != generated_line
            || reply_column != generated_column_utf16
            || original.is_some_and(|position| !valid_original(position))
        {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        Ok(
            original.map(|position| JavaScriptPageDebuggerEmittedOriginalPosition {
                source_index: position.source_index,
                line: position.line,
                column_utf16: position.column_utf16,
            }),
        )
    }

    pub(super) fn core_debugger_emitted_safe_point(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        program_handle: u64,
        program_generation: u64,
        generated_line: u32,
        generated_column_utf16: u32,
    ) -> Result<Option<JavaScriptPageDebuggerSafePoint>, JavaScriptPageDebuggerError> {
        if !self.child.debugger_emitted_map_available()
            || generated_line > MAX_EMITTED_COORDINATE
            || generated_column_utf16 > MAX_EMITTED_COORDINATE
        {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        let child_program = self.child_program_for_core(
            tab_id,
            document_generation,
            program_handle,
            program_generation,
        )?;
        let reply = self
            .child
            .debugger_emitted_safe_point(
                tab_id.as_u64(),
                document_generation,
                child_program,
                generated_line,
                generated_column_utf16,
            )
            .map_err(|_| JavaScriptPageDebuggerError::NoLiveRealm)?;
        let PageHostReply::DebuggerEmittedSafePoint {
            tab_id: reply_tab_id,
            document_generation: reply_generation,
            program,
            generated_line: reply_line,
            generated_column_utf16: reply_column,
            safe_point,
        } = reply
        else {
            return Err(child_debugger_reply_error(&reply));
        };
        if reply_tab_id != tab_id.as_u64()
            || reply_generation != document_generation
            || program != child_program
            || reply_line != generated_line
            || reply_column != generated_column_utf16
            || safe_point.is_some_and(|point| !point.is_well_formed() || point.program != program)
        {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        Ok(safe_point.map(|point| JavaScriptPageDebuggerSafePoint {
            code_unit_ordinal: point.code_unit_ordinal,
            bytecode_offset: point.bytecode_offset,
        }))
    }

    pub(super) fn core_debugger_emitted_paused_root_location(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        program_handle: u64,
        program_generation: u64,
    ) -> Result<
        (
            JavaScriptPageDebuggerSafePoint,
            Option<JavaScriptPageDebuggerEmittedPausedRootLocation>,
        ),
        JavaScriptPageDebuggerError,
    > {
        if !self.child.debugger_emitted_map_available()
            || !self.child.debugger_emitted_paused_root_location_available()
            || !self.child.debugger_execution_control_available()
        {
            return Err(JavaScriptPageDebuggerError::ExecutionControlUnavailable);
        }
        let child_program = self.child_program_for_core(
            tab_id,
            document_generation,
            program_handle,
            program_generation,
        )?;
        let reply = self
            .child
            .debugger_emitted_paused_root_location(
                tab_id.as_u64(),
                document_generation,
                child_program,
            )
            .map_err(|_| JavaScriptPageDebuggerError::NoLiveRealm)?;
        let PageHostReply::DebuggerEmittedPausedRootLocation {
            tab_id: reply_tab_id,
            document_generation: reply_generation,
            program,
            safe_point,
            location,
        } = reply
        else {
            return Err(child_debugger_reply_error(&reply));
        };
        if reply_tab_id != tab_id.as_u64()
            || reply_generation != document_generation
            || program != child_program
            || !safe_point.is_well_formed()
            || safe_point.program != child_program
            || location.is_some_and(|location| {
                location.generated_line > MAX_EMITTED_COORDINATE
                    || location.generated_column_utf16 > MAX_EMITTED_COORDINATE
                    || location
                        .original
                        .is_some_and(|position| !valid_original(position))
            })
        {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        let location = location.map(|location| JavaScriptPageDebuggerEmittedPausedRootLocation {
            generated_line: location.generated_line,
            generated_column_utf16: location.generated_column_utf16,
            original: location.original.map(|position| {
                JavaScriptPageDebuggerEmittedOriginalPosition {
                    source_index: position.source_index,
                    line: position.line,
                    column_utf16: position.column_utf16,
                }
            }),
        });
        Ok((
            JavaScriptPageDebuggerSafePoint {
                code_unit_ordinal: safe_point.code_unit_ordinal,
                bytecode_offset: safe_point.bytecode_offset,
            },
            location,
        ))
    }
}
