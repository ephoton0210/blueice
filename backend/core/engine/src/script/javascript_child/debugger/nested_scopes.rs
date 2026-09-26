// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

impl<C: PageHostClient> OutOfProcessJavaScriptPageExecutor<C> {
    pub(super) fn core_arm_debugger_nested_safe_point_breakpoint(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        program_handle: u64,
        program_generation: u64,
        code_unit_ordinal: u32,
        bytecode_offset: u32,
    ) -> Result<(), JavaScriptPageDebuggerError> {
        if !self.debugger_nested_frames_available() || code_unit_ordinal == 0 {
            return Err(JavaScriptPageDebuggerError::ExecutionControlUnavailable);
        }
        let safe_point = PageHostDebuggerSafePoint {
            program: self.child_program_for_core(
                tab_id,
                document_generation,
                program_handle,
                program_generation,
            )?,
            code_unit_ordinal,
            bytecode_offset,
        };
        validate_child_safe_point_reply(self, tab_id, document_generation, safe_point)?;
        let reply = self
            .child
            .arm_debugger_nested_safe_point_breakpoint(
                tab_id.as_u64(),
                document_generation,
                safe_point,
            )
            .map_err(|_| JavaScriptPageDebuggerError::NoLiveRealm)?;
        match reply {
            PageHostReply::DebuggerNestedSafePointBreakpointArmed {
                tab_id: reply_tab_id,
                document_generation: reply_generation,
                safe_point: reply_safe_point,
            } if reply_tab_id == tab_id.as_u64()
                && reply_generation == document_generation
                && reply_safe_point == safe_point =>
            {
                Ok(())
            }
            PageHostReply::Error { .. } => Err(child_debugger_reply_error(&reply)),
            _ => Err(JavaScriptPageDebuggerError::NoLiveRealm),
        }
    }

    pub(super) fn core_debugger_nested_execution_state(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        program_handle: u64,
        program_generation: u64,
    ) -> Result<Option<JavaScriptPageDebuggerNestedExecutionState>, JavaScriptPageDebuggerError>
    {
        if !self.debugger_nested_frames_available() {
            return Err(JavaScriptPageDebuggerError::ExecutionControlUnavailable);
        }
        let program = self.child_program_for_core(
            tab_id,
            document_generation,
            program_handle,
            program_generation,
        )?;
        let reply = self
            .child
            .debugger_execution_state(tab_id.as_u64(), document_generation, program)
            .map_err(|_| JavaScriptPageDebuggerError::NoLiveRealm)?;
        let PageHostReply::DebuggerExecutionState {
            tab_id: reply_tab_id,
            document_generation: reply_generation,
            program: reply_program,
            state,
        } = reply
        else {
            return Err(child_debugger_reply_error(&reply));
        };
        if reply_tab_id != tab_id.as_u64()
            || reply_generation != document_generation
            || reply_program != program
        {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        match state {
            PageHostDebuggerExecutionState::NestedPaused { frame, safe_point }
                if frame.tab_id == tab_id.as_u64()
                    && frame.document_generation == document_generation
                    && frame.program == program
                    && frame.matches_safe_point(safe_point) =>
            {
                validate_child_safe_point_reply(self, tab_id, document_generation, safe_point)?;
                Ok(Some(JavaScriptPageDebuggerNestedExecutionState::Paused {
                    frame: self.remint_core_debugger_frame(
                        tab_id,
                        document_generation,
                        program_handle,
                        program_generation,
                        frame,
                    )?,
                    bytecode_offset: safe_point.bytecode_offset,
                }))
            }
            PageHostDebuggerExecutionState::NestedStepping { frame }
            | PageHostDebuggerExecutionState::NestedResuming { frame }
                if frame.is_well_formed()
                    && frame.tab_id == tab_id.as_u64()
                    && frame.document_generation == document_generation
                    && frame.program == program =>
            {
                let active = self
                    .debugger_nested_frames
                    .get(&tab_id)
                    .filter(|active| {
                        active.child == frame
                            && active.public.document_generation == document_generation
                            && active.public.program_handle == program_handle
                            && active.public.program_generation == program_generation
                    })
                    .ok_or(JavaScriptPageDebuggerError::InvalidExecutionState)?;
                Ok(Some(match state {
                    PageHostDebuggerExecutionState::NestedStepping { .. } => {
                        JavaScriptPageDebuggerNestedExecutionState::Stepping {
                            frame: active.public,
                        }
                    }
                    PageHostDebuggerExecutionState::NestedResuming { .. } => {
                        JavaScriptPageDebuggerNestedExecutionState::Resuming {
                            frame: active.public,
                        }
                    }
                    _ => unreachable!("only nested advance states pass this guard"),
                }))
            }
            PageHostDebuggerExecutionState::NestedPaused { .. }
            | PageHostDebuggerExecutionState::NestedStepping { .. }
            | PageHostDebuggerExecutionState::NestedResuming { .. } => {
                Err(JavaScriptPageDebuggerError::NoLiveRealm)
            }
            _ => {
                if self
                    .debugger_nested_frames
                    .get(&tab_id)
                    .is_some_and(|active| {
                        active.public.document_generation == document_generation
                            && active.public.program_handle == program_handle
                            && active.public.program_generation == program_generation
                    })
                {
                    self.debugger_nested_frames.remove(&tab_id);
                }
                Ok(None)
            }
        }
    }

    pub(super) fn core_step_debugger_nested_instruction(
        &mut self,
        frame: JavaScriptPageDebuggerFrame,
    ) -> Result<(), JavaScriptPageDebuggerError> {
        if !self.debugger_nested_frames_available()
            || frame.code_unit_ordinal == 0
            || frame.frame_handle == 0
        {
            return Err(JavaScriptPageDebuggerError::ExecutionControlUnavailable);
        }
        let active = self
            .debugger_nested_frames
            .get(&frame.tab_id)
            .copied()
            .filter(|active| active.public == frame)
            .ok_or(JavaScriptPageDebuggerError::InvalidExecutionState)?;
        let program = self.child_program_for_core(
            frame.tab_id,
            frame.document_generation,
            frame.program_handle,
            frame.program_generation,
        )?;
        if active.child.program != program {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        let child_frame = active.child;
        let reply = self
            .child
            .step_debugger_nested_instruction(child_frame)
            .map_err(|_| JavaScriptPageDebuggerError::NoLiveRealm)?;
        match reply {
            PageHostReply::DebuggerNestedStepRequested { frame: reply_frame }
                if reply_frame == child_frame =>
            {
                Ok(())
            }
            PageHostReply::Error { .. } => {
                self.debugger_nested_frames.remove(&frame.tab_id);
                Err(child_debugger_reply_error(&reply))
            }
            _ => {
                self.debugger_nested_frames.remove(&frame.tab_id);
                Err(JavaScriptPageDebuggerError::NoLiveRealm)
            }
        }
    }

    pub(super) fn core_resume_debugger_nested_execution(
        &mut self,
        frame: JavaScriptPageDebuggerFrame,
    ) -> Result<(), JavaScriptPageDebuggerError> {
        if !self.debugger_nested_frames_available()
            || frame.code_unit_ordinal == 0
            || frame.frame_handle == 0
        {
            return Err(JavaScriptPageDebuggerError::ExecutionControlUnavailable);
        }
        let active = self
            .debugger_nested_frames
            .get(&frame.tab_id)
            .copied()
            .filter(|active| active.public == frame)
            .ok_or(JavaScriptPageDebuggerError::InvalidExecutionState)?;
        let program = self.child_program_for_core(
            frame.tab_id,
            frame.document_generation,
            frame.program_handle,
            frame.program_generation,
        )?;
        if active.child.program != program {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        let child_frame = active.child;
        let reply = self
            .child
            .resume_debugger_nested_execution(child_frame)
            .map_err(|_| JavaScriptPageDebuggerError::NoLiveRealm)?;
        match reply {
            PageHostReply::DebuggerNestedResumeRequested { frame: reply_frame }
                if reply_frame == child_frame =>
            {
                Ok(())
            }
            PageHostReply::Error { .. } => {
                self.debugger_nested_frames.remove(&frame.tab_id);
                Err(child_debugger_reply_error(&reply))
            }
            _ => {
                self.debugger_nested_frames.remove(&frame.tab_id);
                Err(JavaScriptPageDebuggerError::NoLiveRealm)
            }
        }
    }

    pub(super) fn core_debugger_stack_snapshot(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        core_program: JavaScriptPageDebuggerProgram,
        frame: Option<JavaScriptPageDebuggerFrame>,
        max_frames: u32,
        max_scope_entries: u32,
    ) -> Result<JavaScriptPageDebuggerStackSnapshot, JavaScriptPageDebuggerError> {
        let JavaScriptPageDebuggerProgram {
            program_handle,
            program_generation,
        } = core_program;
        if !self.debugger_execution_control_available()
            || !self.child.debugger_stack_snapshot_available()
        {
            return Err(JavaScriptPageDebuggerError::ExecutionControlUnavailable);
        }
        if !(1..=PAGE_HOST_DEBUGGER_MAX_STACK_FRAMES).contains(&max_frames)
            || !(1..=PAGE_HOST_DEBUGGER_MAX_SCOPE_ENTRIES).contains(&max_scope_entries)
        {
            return Err(JavaScriptPageDebuggerError::ResourceLimit);
        }
        let program = self.child_program_for_core(
            tab_id,
            document_generation,
            program_handle,
            program_generation,
        )?;
        let (child_frame, top_code_unit, top_offset) = if let Some(frame) = frame {
            if frame.tab_id != tab_id
                || frame.document_generation != document_generation
                || frame.program_handle != program_handle
                || frame.program_generation != program_generation
                || frame.code_unit_ordinal == 0
                || frame.frame_handle == 0
            {
                return Err(JavaScriptPageDebuggerError::InvalidExecutionState);
            }
            let active = self
                .debugger_nested_frames
                .get(&tab_id)
                .copied()
                .filter(|active| active.public == frame && active.child.program == program)
                .ok_or(JavaScriptPageDebuggerError::InvalidExecutionState)?;
            let Some(JavaScriptPageDebuggerNestedExecutionState::Paused {
                frame: still_paused,
                bytecode_offset,
            }) = self.debugger_nested_execution_state(
                tab_id,
                document_generation,
                program_handle,
                program_generation,
            )?
            else {
                return Err(JavaScriptPageDebuggerError::InvalidExecutionState);
            };
            if still_paused != frame {
                return Err(JavaScriptPageDebuggerError::InvalidExecutionState);
            }
            (Some(active.child), frame.code_unit_ordinal, bytecode_offset)
        } else {
            let state = self.debugger_execution_state(
                tab_id,
                document_generation,
                program_handle,
                program_generation,
            );
            let bytecode_offset = match state {
                Err(JavaScriptPageDebuggerError::ExecutionControlUnavailable) => {
                    return Err(JavaScriptPageDebuggerError::InvalidExecutionState);
                }
                Err(error) => return Err(error),
                Ok(state) => match state {
                    JavaScriptPageDebuggerExecutionState::Paused {
                        code_unit_ordinal: 0,
                        bytecode_offset,
                    }
                    | JavaScriptPageDebuggerExecutionState::SourceStepLimitReached {
                        code_unit_ordinal: 0,
                        bytecode_offset,
                    } => bytecode_offset,
                    _ => return Err(JavaScriptPageDebuggerError::InvalidExecutionState),
                },
            };
            (None, 0, bytecode_offset)
        };
        let reply = self
            .child
            .debugger_stack_snapshot(
                tab_id.as_u64(),
                document_generation,
                program,
                child_frame,
                max_frames,
                max_scope_entries,
            )
            .map_err(|_| JavaScriptPageDebuggerError::NoLiveRealm)?;
        let PageHostReply::DebuggerStackSnapshot {
            tab_id: reply_tab_id,
            document_generation: reply_generation,
            program: reply_program,
            frame: reply_frame,
            snapshot,
        } = reply
        else {
            return Err(child_debugger_reply_error(&reply));
        };
        if reply_tab_id != tab_id.as_u64()
            || reply_generation != document_generation
            || reply_program != program
            || reply_frame != child_frame
            || !valid_child_debugger_stack_snapshot(
                &snapshot,
                child_frame,
                top_code_unit,
                top_offset,
                max_frames,
                max_scope_entries,
            )
        {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        for stack_frame in &snapshot.frames {
            validate_child_safe_point_reply(
                self,
                tab_id,
                document_generation,
                PageHostDebuggerSafePoint {
                    program,
                    code_unit_ordinal: stack_frame.code_unit_ordinal,
                    bytecode_offset: stack_frame.bytecode_offset,
                },
            )?;
        }
        Ok(JavaScriptPageDebuggerStackSnapshot {
            frames: snapshot
                .frames
                .into_iter()
                .map(|frame| JavaScriptPageDebuggerStackFrame {
                    code_unit_ordinal: frame.code_unit_ordinal,
                    bytecode_offset: frame.bytecode_offset,
                    scope_entries: frame
                        .scope_entries
                        .into_iter()
                        .map(|entry| JavaScriptPageDebuggerScopeEntry {
                            slot_ordinal: entry.slot_ordinal,
                            scope_depth: entry.scope_depth,
                        })
                        .collect(),
                    scope_truncated: frame.scope_truncated,
                })
                .collect(),
            stack_truncated: snapshot.stack_truncated,
        })
    }

    pub(super) fn core_debugger_static_scope_relation(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        target: JavaScriptPageDebuggerStaticScopeTarget,
    ) -> Result<JavaScriptPageDebuggerStaticScopeRelation, JavaScriptPageDebuggerError> {
        let (metadata, slot) = match target {
            JavaScriptPageDebuggerStaticScopeTarget::Ordinary { metadata, target } => {
                (metadata, target)
            }
            JavaScriptPageDebuggerStaticScopeTarget::Linked { .. } => {
                return self.core_linked_static_scope_relation(tab_id, document_generation, target);
            }
        };
        if !self.debugger_execution_control_available()
            || !self.child.debugger_stack_snapshot_available()
        {
            return Err(JavaScriptPageDebuggerError::ExecutionControlUnavailable);
        }
        let frame_count = match (slot.frame, slot.frame_index) {
            (None, 0) => 1,
            (Some(_), 1) => 2,
            _ => return Err(JavaScriptPageDebuggerError::InvalidExecutionState),
        };
        if slot.safe_point.code_unit_ordinal != 0 {
            return Err(JavaScriptPageDebuggerError::InvalidExecutionState);
        }
        let stack = self.debugger_stack_snapshot(
            tab_id,
            document_generation,
            slot.program,
            slot.frame,
            frame_count,
            PAGE_HOST_DEBUGGER_MAX_SCOPE_ENTRIES,
        )?;
        if stack.stack_truncated
            || stack.frames.len() != frame_count as usize
            || stack.frames.iter().any(|frame| frame.scope_truncated)
        {
            return Err(JavaScriptPageDebuggerError::InvalidExecutionState);
        }
        let selected = &stack.frames[slot.frame_index as usize];
        if selected.code_unit_ordinal != 0
            || selected.bytecode_offset != slot.safe_point.bytecode_offset
            || selected
                .scope_entries
                .iter()
                .filter(|entry| entry.slot_ordinal == slot.scope_entry.slot_ordinal)
                .count()
                != 1
            || !selected.scope_entries.contains(&slot.scope_entry)
        {
            return Err(JavaScriptPageDebuggerError::InvalidExecutionState);
        }
        let child_program = self.child_program_for_core(
            tab_id,
            document_generation,
            slot.program.program_handle,
            slot.program.program_generation,
        )?;
        let child_metadata = self.child_static_metadata_for_core(
            tab_id,
            document_generation,
            child_program,
            metadata.metadata_handle,
            metadata.metadata_generation,
        )?;
        let child_frame = slot
            .frame
            .map(|frame| {
                self.debugger_nested_frames
                    .get(&tab_id)
                    .filter(|active| {
                        active.public == frame && active.child.program == child_program
                    })
                    .map(|active| active.child)
                    .ok_or(JavaScriptPageDebuggerError::InvalidExecutionState)
            })
            .transpose()?;
        let child_target = PageHostDebuggerStaticScopeTarget::Ordinary {
            metadata: child_metadata,
            target: PageHostDebuggerValueTarget {
                tab_id: tab_id.as_u64(),
                document_generation,
                program: child_program,
                frame: child_frame,
                frame_index: slot.frame_index,
                safe_point: PageHostDebuggerSafePoint {
                    program: child_program,
                    code_unit_ordinal: 0,
                    bytecode_offset: slot.safe_point.bytecode_offset,
                },
                scope_entry: PageHostDebuggerScopeEntry {
                    slot_ordinal: slot.scope_entry.slot_ordinal,
                    scope_depth: slot.scope_entry.scope_depth,
                },
            },
        };
        let symbol_type = (ChildStaticScopeAdapter {
            child: &mut self.child,
        })
        .describe(child_target)?;
        Ok(JavaScriptPageDebuggerStaticScopeRelation {
            target,
            symbol_type: JavaScriptPageDebuggerStaticMetadataSymbolType {
                symbol_id: symbol_type.symbol_id,
                type_id: symbol_type.type_id,
            },
        })
    }

    pub(super) fn core_debugger_value_snapshot(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        target: JavaScriptPageDebuggerValueTarget,
    ) -> Result<JavaScriptPageDebuggerValuePreview, JavaScriptPageDebuggerError> {
        if !self.debugger_execution_control_available()
            || !self.child.debugger_value_snapshot_available()
        {
            return Err(JavaScriptPageDebuggerError::ExecutionControlUnavailable);
        }
        let frame_count = match (target.frame, target.frame_index) {
            (None, 0) => 1,
            (Some(_), 0 | 1) => 2,
            _ => return Err(JavaScriptPageDebuggerError::InvalidExecutionState),
        };
        let stack = self.debugger_stack_snapshot(
            tab_id,
            document_generation,
            target.program,
            target.frame,
            frame_count,
            PAGE_HOST_DEBUGGER_MAX_SCOPE_ENTRIES,
        )?;
        if stack.stack_truncated || stack.frames.len() != frame_count as usize {
            return Err(JavaScriptPageDebuggerError::InvalidExecutionState);
        }
        let selected = &stack.frames[target.frame_index as usize];
        if selected.code_unit_ordinal != target.safe_point.code_unit_ordinal
            || selected.bytecode_offset != target.safe_point.bytecode_offset
            || !selected.scope_entries.contains(&target.scope_entry)
        {
            return Err(JavaScriptPageDebuggerError::InvalidExecutionState);
        }
        let program = self.child_program_for_core(
            tab_id,
            document_generation,
            target.program.program_handle,
            target.program.program_generation,
        )?;
        let frame = target
            .frame
            .map(|public| {
                self.debugger_nested_frames
                    .get(&tab_id)
                    .filter(|active| active.public == public && active.child.program == program)
                    .map(|active| active.child)
                    .ok_or(JavaScriptPageDebuggerError::InvalidExecutionState)
            })
            .transpose()?;
        let child_target = PageHostDebuggerValueTarget {
            tab_id: tab_id.as_u64(),
            document_generation,
            program,
            frame,
            frame_index: target.frame_index,
            safe_point: PageHostDebuggerSafePoint {
                program,
                code_unit_ordinal: target.safe_point.code_unit_ordinal,
                bytecode_offset: target.safe_point.bytecode_offset,
            },
            scope_entry: PageHostDebuggerScopeEntry {
                slot_ordinal: target.scope_entry.slot_ordinal,
                scope_depth: target.scope_entry.scope_depth,
            },
        };
        if !child_target.is_well_formed() {
            return Err(JavaScriptPageDebuggerError::InvalidExecutionState);
        }
        let reply = self
            .child
            .debugger_value_snapshot(child_target)
            .map_err(|_| JavaScriptPageDebuggerError::NoLiveRealm)?;
        let PageHostReply::DebuggerValueSnapshot(snapshot) = reply else {
            return Err(child_debugger_reply_error(&reply));
        };
        if snapshot.target != child_target || !snapshot.is_well_formed() {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        Ok(remint_child_debugger_value(snapshot.preview))
    }
}
