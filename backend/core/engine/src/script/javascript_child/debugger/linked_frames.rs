// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

impl<C: PageHostClient> OutOfProcessJavaScriptPageExecutor<C> {
    pub(super) fn core_arm_debugger_linked_nested_safe_point_breakpoint(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        entry: JavaScriptPageDebuggerProgram,
        dependency: JavaScriptPageDebuggerProgram,
        safe_point: JavaScriptPageDebuggerSafePoint,
    ) -> Result<(), JavaScriptPageDebuggerError> {
        if !self.debugger_linked_frames_available() || safe_point.code_unit_ordinal == 0 {
            return Err(JavaScriptPageDebuggerError::ExecutionControlUnavailable);
        }
        let child_entry = self.child_program_for_core(
            tab_id,
            document_generation,
            entry.program_handle,
            entry.program_generation,
        )?;
        let child_dependency = self.child_program_for_core(
            tab_id,
            document_generation,
            dependency.program_handle,
            dependency.program_generation,
        )?;
        if child_entry == child_dependency {
            return Err(JavaScriptPageDebuggerError::InvalidSafePoint);
        }
        let child_point = PageHostDebuggerSafePoint {
            program: child_dependency,
            code_unit_ordinal: safe_point.code_unit_ordinal,
            bytecode_offset: safe_point.bytecode_offset,
        };
        validate_child_safe_point_reply(self, tab_id, document_generation, child_point)?;
        ChildLinkedDebuggerAdapter {
            child: &mut self.child,
        }
        .arm(
            tab_id.as_u64(),
            document_generation,
            child_entry,
            child_point,
        )
    }

    pub(super) fn core_debugger_linked_execution_state(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        entry: JavaScriptPageDebuggerProgram,
    ) -> Result<JavaScriptPageDebuggerLinkedExecutionState, JavaScriptPageDebuggerError> {
        if !self.debugger_linked_frames_available() {
            return Err(JavaScriptPageDebuggerError::ExecutionControlUnavailable);
        }
        let child_entry = self.child_program_for_core(
            tab_id,
            document_generation,
            entry.program_handle,
            entry.program_generation,
        )?;
        let observed = (ChildLinkedDebuggerAdapter {
            child: &mut self.child,
        })
        .lifecycle_state(tab_id.as_u64(), document_generation, child_entry, None);
        match observed {
            Ok(ChildLinkedDebuggerStatus::Pending) => {
                self.debugger_linked_frames.remove(&tab_id);
                Ok(JavaScriptPageDebuggerLinkedExecutionState::Pending)
            }
            Ok(ChildLinkedDebuggerStatus::Completed) => {
                self.debugger_linked_frames.remove(&tab_id);
                Ok(JavaScriptPageDebuggerLinkedExecutionState::Completed)
            }
            Ok(ChildLinkedDebuggerStatus::Paused { frame, safe_point }) => {
                let frames = self.capture_core_linked_pause_observed(
                    tab_id,
                    document_generation,
                    frame,
                    safe_point,
                    1,
                )?;
                Ok(JavaScriptPageDebuggerLinkedExecutionState::Paused {
                    stack: JavaScriptPageDebuggerLinkedStackSnapshot { frames },
                })
            }
            Ok(ChildLinkedDebuggerStatus::Resuming { frame }) => {
                let active = self
                    .debugger_linked_frames
                    .get(&tab_id)
                    .filter(|active| active.child == frame)
                    .ok_or(JavaScriptPageDebuggerError::InvalidExecutionState)?;
                Ok(JavaScriptPageDebuggerLinkedExecutionState::Resuming {
                    frame: active.frames[0].frame,
                })
            }
            Err(error) => {
                self.debugger_linked_frames.remove(&tab_id);
                Err(error)
            }
        }
    }

    pub(super) fn core_debugger_linked_stack_snapshot(
        &mut self,
        top_frame: JavaScriptPageDebuggerFrame,
        max_scope_entries: u32,
    ) -> Result<JavaScriptPageDebuggerLinkedStackSnapshot, JavaScriptPageDebuggerError> {
        if !self.debugger_linked_frames_available()
            || !(1..=PAGE_HOST_DEBUGGER_MAX_SCOPE_ENTRIES).contains(&max_scope_entries)
        {
            return Err(JavaScriptPageDebuggerError::ExecutionControlUnavailable);
        }
        let active = self
            .debugger_linked_frames
            .get(&top_frame.tab_id)
            .filter(|active| active.frames[0].frame == top_frame)
            .ok_or(JavaScriptPageDebuggerError::InvalidExecutionState)?;
        let entry = JavaScriptPageDebuggerProgram {
            program_handle: active.frames[1].frame.program_handle,
            program_generation: active.frames[1].frame.program_generation,
        };
        let frames = self.capture_core_linked_pause(
            top_frame.tab_id,
            top_frame.document_generation,
            entry,
            max_scope_entries,
        )?;
        if frames[0].frame != top_frame {
            return Err(JavaScriptPageDebuggerError::InvalidExecutionState);
        }
        Ok(JavaScriptPageDebuggerLinkedStackSnapshot { frames })
    }

    pub(super) fn core_debugger_linked_scope_snapshot(
        &mut self,
        expected_stack: JavaScriptPageDebuggerLinkedStackSnapshot,
    ) -> Result<JavaScriptPageDebuggerLinkedScopeSnapshot, JavaScriptPageDebuggerError> {
        if !self.debugger_linked_frames_available() {
            return Err(JavaScriptPageDebuggerError::ExecutionControlUnavailable);
        }
        let top = expected_stack.frames[0].frame;
        let active_before = self
            .debugger_linked_frames
            .get(&top.tab_id)
            .filter(|active| active.frames == expected_stack.frames)
            .cloned()
            .ok_or(JavaScriptPageDebuggerError::InvalidExecutionState)?;
        if !self.has_core_live_document(top.tab_id, top.document_generation)
            || active_before.frames.iter().any(|frame| {
                frame.frame.tab_id != top.tab_id
                    || frame.frame.document_generation != top.document_generation
            })
            || !active_before
                .child_stack
                .is_well_formed(active_before.child)
        {
            return Err(JavaScriptPageDebuggerError::InvalidExecutionState);
        }
        let observed =
            self.debugger_linked_stack_snapshot(top, PAGE_HOST_DEBUGGER_MAX_SCOPE_ENTRIES)?;
        let active_after = self
            .debugger_linked_frames
            .get(&top.tab_id)
            .filter(|active| {
                observed == expected_stack
                    && active.child == active_before.child
                    && active.frames == expected_stack.frames
                    && active.child_stack.is_well_formed(active.child)
                    && active.child_stack.max_scope_entries == PAGE_HOST_DEBUGGER_MAX_SCOPE_ENTRIES
                    && active
                        .child_stack
                        .frames
                        .iter()
                        .all(|frame| !frame.scope_truncated)
                    && (active_before
                        .child_stack
                        .frames
                        .iter()
                        .any(|frame| frame.scope_truncated)
                        || active_before.child_stack.frames == active.child_stack.frames)
            })
            .ok_or(JavaScriptPageDebuggerError::InvalidExecutionState)?;
        let scope_entries = active_after.child_stack.frames[1]
            .scope_entries
            .iter()
            .map(|entry| JavaScriptPageDebuggerScopeEntry {
                slot_ordinal: entry.slot_ordinal,
                scope_depth: entry.scope_depth,
            })
            .collect::<Vec<_>>();
        let mut seen = BTreeSet::new();
        if !scope_entries
            .iter()
            .all(|entry| seen.insert(entry.slot_ordinal))
        {
            return Err(JavaScriptPageDebuggerError::InvalidExecutionState);
        }
        Ok(JavaScriptPageDebuggerLinkedScopeSnapshot {
            stack: expected_stack,
            scope_entries,
        })
    }

    pub(super) fn core_resume_debugger_linked_nested_execution(
        &mut self,
        top_frame: JavaScriptPageDebuggerFrame,
    ) -> Result<(), JavaScriptPageDebuggerError> {
        if !self.debugger_linked_frames_available() {
            return Err(JavaScriptPageDebuggerError::ExecutionControlUnavailable);
        }
        let active = self
            .debugger_linked_frames
            .get(&top_frame.tab_id)
            .filter(|active| active.frames[0].frame == top_frame)
            .cloned()
            .ok_or(JavaScriptPageDebuggerError::InvalidExecutionState)?;
        for (index, frame) in active.frames.iter().enumerate() {
            let child = match self.child_program_for_core(
                top_frame.tab_id,
                top_frame.document_generation,
                frame.frame.program_handle,
                frame.frame.program_generation,
            ) {
                Ok(child) => child,
                Err(error) => {
                    self.debugger_linked_frames.remove(&top_frame.tab_id);
                    return Err(error);
                }
            };
            let expected = if index == 0 {
                active.child.dependency_program
            } else {
                active.child.entry_program
            };
            if child != expected {
                self.debugger_linked_frames.remove(&top_frame.tab_id);
                return Err(JavaScriptPageDebuggerError::NoLiveRealm);
            }
        }
        let result = (ChildLinkedDebuggerAdapter {
            child: &mut self.child,
        })
        .resume(active.child);
        if result.is_err() {
            self.debugger_linked_frames.remove(&top_frame.tab_id);
        }
        result
    }

    pub(super) fn core_debugger_linked_stack_spans(
        &mut self,
        expected_stack: JavaScriptPageDebuggerLinkedStackSnapshot,
        access: JavaScriptPageDebuggerLinkedSpanAccess,
    ) -> Result<[JavaScriptPageDebuggerStaticMetadataSafePointSpan; 2], JavaScriptPageDebuggerError>
    {
        if !self.debugger_linked_frames_available() {
            return Err(JavaScriptPageDebuggerError::ExecutionControlUnavailable);
        }
        self.core_linked_stack_spans(expected_stack, access)
    }
}
