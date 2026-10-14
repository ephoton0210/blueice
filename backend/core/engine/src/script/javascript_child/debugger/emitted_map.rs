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
