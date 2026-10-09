// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Native debugger entry and root execution-control methods.

use super::*;

impl JavaScriptPageExecutor {
    /// Arms a compiler-verified root-entry breakpoint for a declaration that
    /// is admitted but has not entered the BlueJS VM. This is intentionally
    /// narrower than generic breakpoint configuration: the root entry has a
    /// zero-execution continuation, so resume can safely invoke the ordinary
    /// VM entry point without claiming an arbitrary interpreter continuation.
    pub fn arm_debugger_entry_breakpoint(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        program_handle: u64,
        program_generation: u64,
        code_unit_ordinal: u32,
        bytecode_offset: u32,
    ) -> Result<(), JavaScriptPageDebuggerError> {
        if !self.debugger_execution_control_available() {
            return Err(JavaScriptPageDebuggerError::ExecutionControlUnavailable);
        }
        self.validate_debugger_safe_point(
            tab_id,
            document_generation,
            program_handle,
            program_generation,
            code_unit_ordinal,
            bytecode_offset,
        )?;
        let key = DebuggerProgramKey {
            program_handle,
            program_generation,
        };
        let entry_breakpoint = self.debugger_entry_breakpoint(tab_id, key)?;
        if entry_breakpoint.code_unit_ordinal != code_unit_ordinal
            || entry_breakpoint.bytecode_offset != bytecode_offset
        {
            return Err(JavaScriptPageDebuggerError::NotExecutableEntry);
        }
        let status = self
            .debugger_execution_states
            .get(&tab_id)
            .and_then(|states| states.get(&key))
            .copied()
            .ok_or(JavaScriptPageDebuggerError::NotExecutableEntry)?;
        if status != DebuggerExecutionStatus::Pending {
            return Err(JavaScriptPageDebuggerError::InvalidExecutionState);
        }
        self.insert_debugger_breakpoint(tab_id, entry_breakpoint)?;
        Ok(())
    }

    /// Arms one verified root-code-unit location for a still-pending classic
    /// script. A nonzero offset is reached by the BlueJS continuation API,
    /// which preserves the root interpreter frame before returning control to
    /// this session thread. Modules and child code units are rejected because
    /// their continuation state is not represented by this scheduler.
    #[allow(clippy::too_many_arguments)]
    pub fn arm_debugger_root_safe_point_breakpoint(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        program_handle: u64,
        program_generation: u64,
        code_unit_ordinal: u32,
        bytecode_offset: u32,
    ) -> Result<(), JavaScriptPageDebuggerError> {
        if !self.debugger_execution_control_available() {
            return Err(JavaScriptPageDebuggerError::ExecutionControlUnavailable);
        }
        self.validate_debugger_safe_point(
            tab_id,
            document_generation,
            program_handle,
            program_generation,
            code_unit_ordinal,
            bytecode_offset,
        )?;
        let record = self.debugger_program_record(
            tab_id,
            document_generation,
            program_handle,
            program_generation,
        )?;
        if code_unit_ordinal != 0 || !record.supports_root_continuation {
            return Err(JavaScriptPageDebuggerError::NotResumableRootSafePoint);
        }
        let key = DebuggerProgramKey {
            program_handle,
            program_generation,
        };
        let status = self
            .debugger_execution_states
            .get(&tab_id)
            .and_then(|states| states.get(&key))
            .copied()
            .ok_or(JavaScriptPageDebuggerError::NotResumableRootSafePoint)?;
        if status != DebuggerExecutionStatus::Pending {
            return Err(JavaScriptPageDebuggerError::InvalidExecutionState);
        }
        let target = DebuggerBreakpointRecord {
            program_handle,
            program_generation,
            code_unit_ordinal,
            bytecode_offset,
        };
        let pending = self
            .pending_debugger_executions
            .get(&tab_id)
            .into_iter()
            .flatten()
            .find(|pending| pending.program == key)
            .ok_or(JavaScriptPageDebuggerError::NotResumableRootSafePoint)?;
        if pending.root_safe_point_armed {
            return Err(JavaScriptPageDebuggerError::InvalidExecutionState);
        }
        if !matches!(
            pending.execution,
            DeferredJavaScriptExecution::Classic { .. }
        ) {
            return Err(JavaScriptPageDebuggerError::NotResumableRootSafePoint);
        }
        self.insert_debugger_breakpoint(tab_id, target)?;
        let pending = self
            .pending_debugger_executions
            .get_mut(&tab_id)
            .expect("the inspected pending debugger queue remains live")
            .iter_mut()
            .find(|pending| pending.program == key)
            .expect("the inspected pending debugger declaration remains queued");
        pending.entry_breakpoint = target;
        pending.root_safe_point_armed = true;
        Ok(())
    }

    /// Reads only source-free scheduling state for one pending, paused, or
    /// completed root-entry declaration. Programs admitted in the ordinary
    /// immediate-scheduling mode never expose this operation.
    pub fn debugger_execution_state(
        &self,
        tab_id: TabId,
        document_generation: u64,
        program_handle: u64,
        program_generation: u64,
    ) -> Result<JavaScriptPageDebuggerExecutionState, JavaScriptPageDebuggerError> {
        if !self.debugger_execution_control_available() {
            return Err(JavaScriptPageDebuggerError::ExecutionControlUnavailable);
        }
        self.debugger_program_record(
            tab_id,
            document_generation,
            program_handle,
            program_generation,
        )?;
        let status = self
            .debugger_execution_states
            .get(&tab_id)
            .and_then(|states| {
                states.get(&DebuggerProgramKey {
                    program_handle,
                    program_generation,
                })
            })
            .copied()
            .ok_or(JavaScriptPageDebuggerError::NotExecutableEntry)?;
        Ok(public_execution_state(status))
    }

    /// Authorizes one paused root-entry declaration to pass through the
    /// session-owned scheduler. The reply does not carry a completion value;
    /// callers observe only a later source-free completion state.
    pub fn resume_debugger_execution(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        program_handle: u64,
        program_generation: u64,
    ) -> Result<(), JavaScriptPageDebuggerError> {
        if !self.debugger_execution_control_available() {
            return Err(JavaScriptPageDebuggerError::ExecutionControlUnavailable);
        }
        self.debugger_program_record(
            tab_id,
            document_generation,
            program_handle,
            program_generation,
        )?;
        let Some(status) = self
            .debugger_execution_states
            .get_mut(&tab_id)
            .and_then(|states| {
                states.get_mut(&DebuggerProgramKey {
                    program_handle,
                    program_generation,
                })
            })
        else {
            return Err(JavaScriptPageDebuggerError::NotExecutableEntry);
        };
        if !matches!(status, DebuggerExecutionStatus::Paused(_)) {
            return Err(JavaScriptPageDebuggerError::InvalidExecutionState);
        }
        *status = DebuggerExecutionStatus::ResumeRequested;
        Ok(())
    }

    /// Schedules exactly one root instruction for the paused classic at the
    /// head of this tab's document-order queue. An entry-only pause gets a
    /// real VM continuation on the following scheduler turn; module roots,
    /// stale programs, and any non-paused state fail before VM execution.
    pub fn step_debugger_root_instruction(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        program_handle: u64,
        program_generation: u64,
    ) -> Result<(), JavaScriptPageDebuggerError> {
        if !self.debugger_execution_control_available() {
            return Err(JavaScriptPageDebuggerError::ExecutionControlUnavailable);
        }
        self.debugger_program_record(
            tab_id,
            document_generation,
            program_handle,
            program_generation,
        )?;
        let program = DebuggerProgramKey {
            program_handle,
            program_generation,
        };
        let pending = self
            .pending_debugger_executions
            .get(&tab_id)
            .and_then(|queue| queue.front())
            .ok_or(JavaScriptPageDebuggerError::InvalidExecutionState)?;
        if pending.program != program
            || !matches!(
                &pending.execution,
                DeferredJavaScriptExecution::Classic { .. }
            )
        {
            return Err(JavaScriptPageDebuggerError::InvalidExecutionState);
        }
        let status = self
            .debugger_execution_states
            .get_mut(&tab_id)
            .and_then(|states| states.get_mut(&program))
            .ok_or(JavaScriptPageDebuggerError::NotExecutableEntry)?;
        if !matches!(status, DebuggerExecutionStatus::Paused(_)) {
            return Err(JavaScriptPageDebuggerError::InvalidExecutionState);
        }
        *status = DebuggerExecutionStatus::StepRequested;
        Ok(())
    }

    /// Returns the exact, source-free breakpoint configuration retained for a
    /// current realm. The records are sorted by opaque identity and compiler
    /// boundary, never by source text or a VM address.
    pub fn debugger_breakpoints(
        &self,
        tab_id: TabId,
        document_generation: u64,
    ) -> Result<Vec<JavaScriptPageDebuggerBreakpoint>, JavaScriptPageDebuggerError> {
        self.require_live_debugger_realm(tab_id, document_generation)?;
        Ok(self
            .debugger_breakpoints
            .get(&tab_id)
            .into_iter()
            .flatten()
            .map(|breakpoint| JavaScriptPageDebuggerBreakpoint {
                program_handle: breakpoint.program_handle,
                program_generation: breakpoint.program_generation,
                code_unit_ordinal: breakpoint.code_unit_ordinal,
                bytecode_offset: breakpoint.bytecode_offset,
            })
            .collect())
    }

    /// Removes one current exact breakpoint record after revalidating its
    /// complete program-generation and compiler-boundary tuple. An already
    /// absent exact record returns `false`; a stale/malformed target remains
    /// an error rather than silently affecting a successor program.
    pub fn clear_debugger_breakpoint(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        program_handle: u64,
        program_generation: u64,
        code_unit_ordinal: u32,
        bytecode_offset: u32,
    ) -> Result<bool, JavaScriptPageDebuggerError> {
        self.validate_debugger_safe_point(
            tab_id,
            document_generation,
            program_handle,
            program_generation,
            code_unit_ordinal,
            bytecode_offset,
        )?;
        Ok(self
            .debugger_breakpoints
            .get_mut(&tab_id)
            .is_some_and(|breakpoints| {
                breakpoints.remove(&DebuggerBreakpointRecord {
                    program_handle,
                    program_generation,
                    code_unit_ordinal,
                    bytecode_offset,
                })
            }))
    }
}
