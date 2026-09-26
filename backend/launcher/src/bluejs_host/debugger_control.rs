// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

impl BlueJsChildHost {
    pub(super) fn debugger_safe_points(
        &self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
    ) -> PageHostReply {
        if !program.is_well_formed() {
            return invalid_request();
        }
        let document = match self.exact_document(tab_id, document_generation) {
            Ok(document) => document,
            Err(reply) => return reply,
        };
        let Some(record) = document.debugger_programs.get(&program.program_handle) else {
            return invalid_request();
        };
        if record.program_generation != program.program_generation {
            return invalid_request();
        }
        let safe_points = match self.runtime.safe_points(
            tab_id,
            record.runtime_handle,
            usize::try_from(PAGE_HOST_DEBUGGER_MAX_SAFE_POINTS_PER_PROGRAM)
                .expect("page-host debugger safe-point cap fits usize"),
        ) {
            Ok(safe_points) => safe_points,
            Err(BlueJsPageRuntimeError::SafePointLimit { .. }) => return resource_limit(),
            Err(_) => return invalid_request(),
        };
        PageHostReply::DebuggerSafePoints {
            tab_id,
            document_generation,
            program,
            safe_points: safe_points
                .into_iter()
                .map(|safe_point| PageHostDebuggerSafePoint {
                    program,
                    code_unit_ordinal: safe_point.code_unit.ordinal(),
                    bytecode_offset: safe_point.bytecode_offset,
                })
                .collect(),
        }
    }

    pub(super) fn validate_debugger_safe_point(
        &self,
        tab_id: u64,
        document_generation: u64,
        safe_point: PageHostDebuggerSafePoint,
    ) -> PageHostReply {
        match self.exact_debugger_safe_point(tab_id, document_generation, safe_point) {
            Ok(()) => PageHostReply::DebuggerSafePointValidated {
                tab_id,
                document_generation,
                safe_point,
            },
            Err(reply) => reply,
        }
    }

    /// Revalidates an exact child-private safe-point tuple without returning
    /// a runtime handle, source, bytecode, or VM object to the caller.
    pub(super) fn exact_debugger_safe_point(
        &self,
        tab_id: u64,
        document_generation: u64,
        safe_point: PageHostDebuggerSafePoint,
    ) -> Result<(), PageHostReply> {
        if !safe_point.is_well_formed() {
            return Err(invalid_request());
        }
        let document = self.exact_document(tab_id, document_generation)?;
        let Some(record) = document
            .debugger_programs
            .get(&safe_point.program.program_handle)
        else {
            return Err(invalid_request());
        };
        if record.program_generation != safe_point.program.program_generation {
            return Err(invalid_request());
        }
        // Constructing a BlueJS safe point is intentionally not exposed by
        // its public runtime API. Find the exact compiler-recorded boundary
        // first, then ask the runtime to revalidate that authentic tuple.
        let found = match self.runtime.safe_points(
            tab_id,
            record.runtime_handle,
            usize::try_from(PAGE_HOST_DEBUGGER_MAX_SAFE_POINTS_PER_PROGRAM)
                .expect("page-host debugger safe-point cap fits usize"),
        ) {
            Ok(safe_points) => safe_points.into_iter().find(|candidate| {
                candidate.code_unit.ordinal() == safe_point.code_unit_ordinal
                    && candidate.bytecode_offset == safe_point.bytecode_offset
            }),
            Err(BlueJsPageRuntimeError::SafePointLimit { .. }) => return Err(resource_limit()),
            Err(_) => return Err(invalid_request()),
        };
        let Some(found) = found else {
            return Err(invalid_request());
        };
        if self
            .runtime
            .validate_safe_point(tab_id, record.runtime_handle, found)
            .is_err()
        {
            return Err(invalid_request());
        }
        Ok(())
    }

    pub(super) fn set_debugger_breakpoint(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        safe_point: PageHostDebuggerSafePoint,
    ) -> PageHostReply {
        if let Err(reply) = self.exact_debugger_safe_point(tab_id, document_generation, safe_point)
        {
            return reply;
        }
        let document = self
            .documents
            .get_mut(&tab_id)
            .expect("the exact child document remains live after validation");
        let max_breakpoints = usize::try_from(PAGE_HOST_DEBUGGER_MAX_BREAKPOINTS_PER_REALM)
            .expect("page-host debugger breakpoint cap fits usize");
        if !document.debugger_breakpoints.contains(&safe_point)
            && document.debugger_breakpoints.len() == max_breakpoints
        {
            return resource_limit();
        }
        document.debugger_breakpoints.insert(safe_point);
        PageHostReply::DebuggerBreakpointSet {
            tab_id,
            document_generation,
            safe_point,
        }
    }

    pub(super) fn debugger_breakpoints(
        &self,
        tab_id: u64,
        document_generation: u64,
    ) -> PageHostReply {
        let document = match self.exact_document(tab_id, document_generation) {
            Ok(document) => document,
            Err(reply) => return reply,
        };
        let max_breakpoints = usize::try_from(PAGE_HOST_DEBUGGER_MAX_BREAKPOINTS_PER_REALM)
            .expect("page-host debugger breakpoint cap fits usize");
        if document.debugger_breakpoints.len() > max_breakpoints {
            return resource_limit();
        }
        PageHostReply::DebuggerBreakpoints {
            tab_id,
            document_generation,
            safe_points: document.debugger_breakpoints.iter().copied().collect(),
        }
    }

    pub(super) fn clear_debugger_breakpoint(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        safe_point: PageHostDebuggerSafePoint,
    ) -> PageHostReply {
        if let Err(reply) = self.exact_debugger_safe_point(tab_id, document_generation, safe_point)
        {
            return reply;
        }
        let was_present = self
            .documents
            .get_mut(&tab_id)
            .expect("the exact child document remains live after validation")
            .debugger_breakpoints
            .remove(&safe_point);
        PageHostReply::DebuggerBreakpointCleared {
            tab_id,
            document_generation,
            safe_point,
            was_present,
        }
    }

    pub(super) fn arm_debugger_root_safe_point_breakpoint(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        safe_point: PageHostDebuggerSafePoint,
    ) -> PageHostReply {
        if safe_point.code_unit_ordinal != 0 {
            return invalid_debugger_state();
        }
        if let Err(reply) = self.exact_debugger_safe_point(tab_id, document_generation, safe_point)
        {
            return reply;
        }
        let Some(document) = self.documents.get_mut(&tab_id) else {
            return unknown_realm();
        };
        if !document.debugger_execution_control {
            return invalid_debugger_state();
        }
        if document.debugger_execution_states.get(&safe_point.program)
            != Some(&ChildDebuggerExecutionStatus::Pending)
        {
            return invalid_debugger_state();
        }
        let Some(pending) = document
            .pending_debugger_executions
            .iter_mut()
            .find(|pending| pending.program == Some(safe_point.program))
        else {
            return invalid_debugger_state();
        };
        let root_safe_point = match &mut pending.execution {
            DeferredChildExecution::JavaScriptClassic {
                root_safe_point, ..
            }
            | DeferredChildExecution::BlueTsClassic {
                root_safe_point, ..
            } => root_safe_point,
            DeferredChildExecution::BlueTsModule {
                attachment,
                root_safe_point,
            } => {
                let Ok(expected) = self
                    .runtime
                    .module_evaluate_entry_safe_point(tab_id, attachment.entry.handle)
                else {
                    return invalid_debugger_state();
                };
                if expected.code_unit.ordinal() != safe_point.code_unit_ordinal
                    || expected.bytecode_offset != safe_point.bytecode_offset
                {
                    return invalid_debugger_state();
                }
                root_safe_point
            }
            DeferredChildExecution::JavaScriptModule { .. } => return invalid_debugger_state(),
        };
        if root_safe_point.is_some()
            || pending.nested_safe_point.is_some()
            || pending.linked_safe_point.is_some()
        {
            return invalid_debugger_state();
        }
        let max = usize::try_from(PAGE_HOST_DEBUGGER_MAX_BREAKPOINTS_PER_REALM)
            .expect("page-host debugger breakpoint cap fits usize");
        if !document.debugger_breakpoints.contains(&safe_point)
            && document.debugger_breakpoints.len() == max
        {
            return resource_limit();
        }
        document.debugger_breakpoints.insert(safe_point);
        *root_safe_point = Some(safe_point);
        PageHostReply::DebuggerRootSafePointBreakpointArmed {
            tab_id,
            document_generation,
            safe_point,
        }
    }

    /// Child-private until the versioned page-host active-frame route is
    /// complete. This never reuses the root-breakpoint request for a child.
    pub(super) fn arm_debugger_nested_target(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        safe_point: PageHostDebuggerSafePoint,
    ) -> Result<(), PageHostReply> {
        self.exact_debugger_safe_point(tab_id, document_generation, safe_point)?;
        if safe_point.code_unit_ordinal == 0 {
            return Err(invalid_debugger_state());
        }
        let document = self.documents.get_mut(&tab_id).expect("validated document");
        if !document.debugger_execution_control
            || document.debugger_execution_states.get(&safe_point.program)
                != Some(&ChildDebuggerExecutionStatus::Pending)
        {
            return Err(invalid_debugger_state());
        }
        let pending = document
            .pending_debugger_executions
            .iter_mut()
            .find(|pending| pending.program == Some(safe_point.program))
            .ok_or_else(invalid_debugger_state)?;
        if pending.nested_safe_point.is_some()
            || pending.linked_safe_point.is_some()
            || !matches!(
                pending.execution,
                DeferredChildExecution::JavaScriptClassic {
                    root_safe_point: None,
                    ..
                } | DeferredChildExecution::BlueTsClassic {
                    root_safe_point: None,
                    ..
                } | DeferredChildExecution::BlueTsModule {
                    root_safe_point: None,
                    ..
                }
            )
        {
            return Err(invalid_debugger_state());
        }
        let max = usize::try_from(PAGE_HOST_DEBUGGER_MAX_BREAKPOINTS_PER_REALM)
            .expect("page-host debugger breakpoint cap fits usize");
        if !document.debugger_breakpoints.contains(&safe_point)
            && document.debugger_breakpoints.len() == max
        {
            return Err(resource_limit());
        }
        document.debugger_breakpoints.insert(safe_point);
        pending.nested_safe_point = Some(safe_point);
        Ok(())
    }

    pub(super) fn arm_debugger_nested_safe_point_breakpoint(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        safe_point: PageHostDebuggerSafePoint,
    ) -> PageHostReply {
        match self.arm_debugger_nested_target(tab_id, document_generation, safe_point) {
            Ok(()) => PageHostReply::DebuggerNestedSafePointBreakpointArmed {
                tab_id,
                document_generation,
                safe_point,
            },
            Err(reply) => reply,
        }
    }

    /// Arms a dependency child only for this entry's pending BlueTS module
    /// graph. This remains child-local until the complete linked wire ships.
    pub(super) fn arm_debugger_linked_target(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        entry_program: PageHostDebuggerProgram,
        safe_point: PageHostDebuggerSafePoint,
    ) -> Result<(), PageHostReply> {
        self.exact_debugger_safe_point(tab_id, document_generation, safe_point)?;
        if !entry_program.is_well_formed()
            || entry_program == safe_point.program
            || safe_point.code_unit_ordinal == 0
        {
            return Err(invalid_request());
        }
        let document = self.documents.get_mut(&tab_id).expect("validated document");
        if !document.debugger_execution_control
            || document.debugger_execution_states.get(&entry_program)
                != Some(&ChildDebuggerExecutionStatus::Pending)
        {
            return Err(invalid_debugger_state());
        }
        let entry_handle = document
            .debugger_programs
            .get(&entry_program.program_handle)
            .filter(|record| record.program_generation == entry_program.program_generation)
            .map(|record| record.runtime_handle)
            .ok_or_else(invalid_request)?;
        let dependency_handle = document
            .debugger_programs
            .get(&safe_point.program.program_handle)
            .filter(|record| record.program_generation == safe_point.program.program_generation)
            .map(|record| record.runtime_handle)
            .ok_or_else(invalid_request)?;
        let pending = document
            .pending_debugger_executions
            .iter_mut()
            .find(|pending| pending.program == Some(entry_program))
            .ok_or_else(invalid_debugger_state)?;
        let DeferredChildExecution::BlueTsModule {
            attachment,
            root_safe_point: None,
        } = &pending.execution
        else {
            return Err(invalid_debugger_state());
        };
        if pending.nested_safe_point.is_some()
            || pending.linked_safe_point.is_some()
            || attachment.entry.handle != entry_handle
            || !attachment
                .modules
                .values()
                .any(|module| module.handle == dependency_handle)
        {
            return Err(invalid_debugger_state());
        }
        let point = self
            .runtime
            .safe_points(
                tab_id,
                dependency_handle,
                usize::try_from(PAGE_HOST_DEBUGGER_MAX_SAFE_POINTS_PER_PROGRAM)
                    .expect("page-host safe-point cap fits usize"),
            )
            .map_err(|_| invalid_request())?
            .into_iter()
            .find(|point| {
                point.code_unit.ordinal() == safe_point.code_unit_ordinal
                    && point.bytecode_offset == safe_point.bytecode_offset
            })
            .ok_or_else(invalid_request)?;
        self.runtime
            .validate_linked_nested_debugger_target(
                tab_id,
                entry_handle,
                dependency_handle,
                attachment.modules.values().map(|module| module.handle),
                point,
            )
            .map_err(|_| invalid_request())?;
        let max = usize::try_from(PAGE_HOST_DEBUGGER_MAX_BREAKPOINTS_PER_REALM)
            .expect("page-host debugger breakpoint cap fits usize");
        if !document.debugger_breakpoints.contains(&safe_point)
            && document.debugger_breakpoints.len() == max
        {
            return Err(resource_limit());
        }
        document.debugger_breakpoints.insert(safe_point);
        pending.linked_safe_point = Some(safe_point);
        Ok(())
    }

    pub(super) fn arm_debugger_linked_safe_point_breakpoint(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        entry_program: PageHostDebuggerProgram,
        safe_point: PageHostDebuggerSafePoint,
    ) -> PageHostReply {
        match self.arm_debugger_linked_target(
            tab_id,
            document_generation,
            entry_program,
            safe_point,
        ) {
            Ok(()) => PageHostReply::DebuggerLinkedNestedSafePointBreakpointArmed {
                tab_id,
                document_generation,
                entry_program,
                safe_point,
            },
            Err(reply) => reply,
        }
    }

    pub(super) fn exact_linked_runtime_frame(
        &self,
        frame: PageHostDebuggerLinkedFrame,
    ) -> Result<BlueJsPageDebuggerLinkedFrame, PageHostReply> {
        if !frame.is_well_formed() {
            return Err(invalid_request());
        }
        let document = self.exact_document(frame.tab_id, frame.document_generation)?;
        if !document.debugger_execution_control
            || document
                .pending_debugger_executions
                .front()
                .and_then(|pending| pending.program)
                != Some(frame.entry_program)
        {
            return Err(invalid_debugger_state());
        }
        let Some(ChildDebuggerExecutionStatus::LinkedPaused {
            frame: active,
            safe_point,
        }) = document
            .debugger_execution_states
            .get(&frame.entry_program)
            .copied()
        else {
            return Err(invalid_debugger_state());
        };
        if child_debugger_linked_frame(
            frame.tab_id,
            frame.document_generation,
            frame.entry_program,
            safe_point.program,
            active,
        ) != frame
        {
            return Err(invalid_debugger_state());
        }
        Ok(active)
    }

    pub(super) fn resume_debugger_linked_nested_execution(
        &mut self,
        frame: PageHostDebuggerLinkedFrame,
    ) -> PageHostReply {
        let active = match self.exact_linked_runtime_frame(frame) {
            Ok(active) => active,
            Err(reply) => return reply,
        };
        match self.request_debugger_linked_resume(
            frame.tab_id,
            frame.document_generation,
            frame.entry_program,
            active,
        ) {
            Ok(()) => PageHostReply::DebuggerLinkedNestedResumeRequested { frame },
            Err(reply) => reply,
        }
    }

    pub(super) fn debugger_linked_stack_reply(
        &self,
        frame: PageHostDebuggerLinkedFrame,
        max_scope_entries: u32,
    ) -> PageHostReply {
        let active = match self.exact_linked_runtime_frame(frame) {
            Ok(active) => active,
            Err(reply) => return reply,
        };
        let snapshot = match self.debugger_linked_stack_snapshot(
            frame.tab_id,
            frame.document_generation,
            frame.entry_program,
            active,
            max_scope_entries,
        ) {
            Ok(snapshot) => snapshot.to_wire(),
            Err(reply) => return reply,
        };
        if !snapshot.is_well_formed(frame) {
            return invalid_debugger_state();
        }
        PageHostReply::DebuggerLinkedStackSnapshot {
            frame,
            snapshot: Box::new(snapshot),
        }
    }

    pub(super) fn debugger_linked_stack_spans_reply(
        &self,
        frame: PageHostDebuggerLinkedFrame,
        expected_stack: PageHostDebuggerLinkedStackSnapshot,
        sources: [PageHostDebuggerLinkedSource; 2],
    ) -> PageHostReply {
        if !expected_stack.is_well_formed(frame)
            || sources
                .iter()
                .any(|source| !source.metadata.is_well_formed())
        {
            return invalid_request();
        }
        let active = match self.exact_linked_runtime_frame(frame) {
            Ok(active) => active,
            Err(reply) => return reply,
        };
        let spans = match self.debugger_linked_source_spans(
            frame.tab_id,
            frame.document_generation,
            frame.entry_program,
            active,
            &ChildLinkedStackSnapshot::from_wire(&expected_stack),
            [
                (sources[0].metadata, sources[0].source_id),
                (sources[1].metadata, sources[1].source_id),
            ],
        ) {
            Ok(spans) => spans,
            Err(reply) => return reply,
        };
        PageHostReply::DebuggerLinkedStackSpans {
            frame,
            snapshot: Box::new(expected_stack),
            spans: Box::new(spans),
        }
    }

    pub(super) fn request_debugger_linked_resume(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        entry_program: PageHostDebuggerProgram,
        frame: BlueJsPageDebuggerLinkedFrame,
    ) -> Result<(), PageHostReply> {
        let document = self.exact_document(tab_id, document_generation)?;
        if !document.debugger_execution_control
            || document
                .pending_debugger_executions
                .front()
                .and_then(|pending| pending.program)
                != Some(entry_program)
        {
            return Err(invalid_debugger_state());
        }
        let Some(ChildDebuggerExecutionStatus::LinkedPaused {
            frame: active,
            safe_point,
        }) = document
            .debugger_execution_states
            .get(&entry_program)
            .copied()
        else {
            return Err(invalid_debugger_state());
        };
        if frame != active || frame.tab_id() != tab_id {
            return Err(invalid_debugger_state());
        }
        self.documents
            .get_mut(&tab_id)
            .expect("validated document")
            .debugger_execution_states
            .insert(
                entry_program,
                ChildDebuggerExecutionStatus::LinkedResumeRequested { frame, safe_point },
            );
        Ok(())
    }

    /// Schedules one step only for the exact paused child invocation.
    pub(super) fn request_debugger_nested_advance(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        frame: BlueJsPageDebuggerFrame,
        resume: bool,
    ) -> Result<(), PageHostReply> {
        let document = self.exact_document(tab_id, document_generation)?;
        if frame.tab_id() != tab_id || !document.debugger_execution_control {
            return Err(invalid_debugger_state());
        }
        let pending = document
            .pending_debugger_executions
            .front()
            .ok_or_else(invalid_debugger_state)?;
        let program = pending.program.ok_or_else(invalid_debugger_state)?;
        let status = document
            .debugger_execution_states
            .get(&program)
            .copied()
            .ok_or_else(invalid_debugger_state)?;
        let ChildDebuggerExecutionStatus::NestedPaused {
            frame: active,
            safe_point,
        } = status
        else {
            return Err(invalid_debugger_state());
        };
        if active != frame
            || !document
                .debugger_programs
                .get(&program.program_handle)
                .is_some_and(|record| {
                    record.program_generation == program.program_generation
                        && record.runtime_handle == frame.program()
                })
            || safe_point.code_unit_ordinal != frame.code_unit_ordinal()
        {
            return Err(invalid_debugger_state());
        }
        let requested = if resume {
            ChildDebuggerExecutionStatus::NestedResumeRequested { frame, safe_point }
        } else {
            ChildDebuggerExecutionStatus::NestedStepRequested { frame, safe_point }
        };
        self.documents
            .get_mut(&tab_id)
            .expect("validated document")
            .debugger_execution_states
            .insert(program, requested);
        Ok(())
    }

    pub(super) fn step_debugger_nested_instruction(
        &mut self,
        frame: PageHostDebuggerFrame,
    ) -> PageHostReply {
        self.continue_debugger_nested_execution(frame, false)
    }

    pub(super) fn resume_debugger_nested_execution(
        &mut self,
        frame: PageHostDebuggerFrame,
    ) -> PageHostReply {
        self.continue_debugger_nested_execution(frame, true)
    }

    pub(super) fn continue_debugger_nested_execution(
        &mut self,
        frame: PageHostDebuggerFrame,
        resume: bool,
    ) -> PageHostReply {
        if !frame.is_well_formed() {
            return invalid_request();
        }
        let document = match self.exact_document(frame.tab_id, frame.document_generation) {
            Ok(document) => document,
            Err(reply) => return reply,
        };
        let Some(ChildDebuggerExecutionStatus::NestedPaused {
            frame: active,
            safe_point,
        }) = document
            .debugger_execution_states
            .get(&frame.program)
            .copied()
        else {
            return invalid_debugger_state();
        };
        if frame
            != child_debugger_frame(
                frame.tab_id,
                frame.document_generation,
                frame.program,
                active,
            )
            || !frame.matches_safe_point(safe_point)
        {
            return invalid_debugger_state();
        }
        match self.request_debugger_nested_advance(
            frame.tab_id,
            frame.document_generation,
            active,
            resume,
        ) {
            Ok(()) if resume => PageHostReply::DebuggerNestedResumeRequested { frame },
            Ok(()) => PageHostReply::DebuggerNestedStepRequested { frame },
            Err(reply) => reply,
        }
    }

    pub(super) fn debugger_stack_snapshot(
        &self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        frame: Option<PageHostDebuggerFrame>,
        max_frames: u32,
        max_scope_entries: u32,
    ) -> PageHostReply {
        if max_frames == 0
            || max_frames > PAGE_HOST_DEBUGGER_MAX_STACK_FRAMES
            || max_frames > VM_DEBUGGER_MAX_STACK_FRAMES
            || max_scope_entries == 0
            || max_scope_entries > PAGE_HOST_DEBUGGER_MAX_SCOPE_ENTRIES
            || max_scope_entries > VM_DEBUGGER_MAX_SCOPE_ENTRIES
            || !program.is_well_formed()
            || frame.is_some_and(|frame| !frame.is_well_formed())
        {
            return invalid_request();
        }
        let document = match self.exact_document(tab_id, document_generation) {
            Ok(document) => document,
            Err(reply) => return reply,
        };
        if !document.debugger_execution_control
            || document
                .pending_debugger_executions
                .front()
                .and_then(|pending| pending.program)
                != Some(program)
        {
            return invalid_debugger_state();
        }
        let Some(record) = document.debugger_programs.get(&program.program_handle) else {
            return invalid_debugger_state();
        };
        if record.program_generation != program.program_generation {
            return invalid_debugger_state();
        }
        let runtime_frame = match (
            frame,
            document.debugger_execution_states.get(&program).copied(),
        ) {
            (
                None,
                Some(
                    ChildDebuggerExecutionStatus::Paused(_)
                    | ChildDebuggerExecutionStatus::SourceStepLimitReached(_),
                ),
            ) => None,
            (
                Some(frame),
                Some(ChildDebuggerExecutionStatus::NestedPaused {
                    frame: active,
                    safe_point,
                }),
            ) if frame == child_debugger_frame(tab_id, document_generation, program, active)
                && frame.matches_safe_point(safe_point) =>
            {
                Some(active)
            }
            _ => return invalid_debugger_state(),
        };
        let snapshot = match self.runtime.debugger_stack_snapshot(
            tab_id,
            record.runtime_handle,
            runtime_frame,
            max_frames,
            max_scope_entries,
        ) {
            Ok(snapshot) => snapshot,
            Err(_) => return invalid_debugger_state(),
        };
        PageHostReply::DebuggerStackSnapshot {
            tab_id,
            document_generation,
            program,
            frame,
            snapshot: PageHostDebuggerStackSnapshot {
                frames: snapshot
                    .frames
                    .into_iter()
                    .map(|frame| PageHostDebuggerStackFrame {
                        code_unit_ordinal: frame.code_unit_ordinal,
                        bytecode_offset: frame.bytecode_offset,
                        scope_entries: frame
                            .scope_entries
                            .into_iter()
                            .map(|entry| PageHostDebuggerScopeEntry {
                                slot_ordinal: entry.slot_ordinal,
                                scope_depth: entry.scope_depth,
                            })
                            .collect(),
                        scope_truncated: frame.scope_truncated,
                    })
                    .collect(),
                stack_truncated: snapshot.stack_truncated,
            },
        }
    }

    /// Re-maps only an exact retained dependency/entry stack. Every frame
    /// names its own child program; no partial stack is returned on failure.
    pub(super) fn debugger_linked_stack_snapshot(
        &self,
        tab_id: u64,
        document_generation: u64,
        entry_program: PageHostDebuggerProgram,
        frame: BlueJsPageDebuggerLinkedFrame,
        max_scope_entries: u32,
    ) -> Result<ChildLinkedStackSnapshot, PageHostReply> {
        if max_scope_entries == 0
            || max_scope_entries > PAGE_HOST_DEBUGGER_MAX_SCOPE_ENTRIES
            || max_scope_entries > VM_DEBUGGER_MAX_SCOPE_ENTRIES
        {
            return Err(invalid_request());
        }
        let document = self.exact_document(tab_id, document_generation)?;
        if !document.debugger_execution_control
            || document
                .pending_debugger_executions
                .front()
                .and_then(|pending| pending.program)
                != Some(entry_program)
        {
            return Err(invalid_debugger_state());
        }
        let Some(ChildDebuggerExecutionStatus::LinkedPaused {
            frame: active,
            safe_point,
        }) = document
            .debugger_execution_states
            .get(&entry_program)
            .copied()
        else {
            return Err(invalid_debugger_state());
        };
        if active != frame || safe_point.program == entry_program || frame.tab_id() != tab_id {
            return Err(invalid_debugger_state());
        }
        let entry_handle = document
            .debugger_programs
            .get(&entry_program.program_handle)
            .filter(|record| record.program_generation == entry_program.program_generation)
            .map(|record| record.runtime_handle)
            .ok_or_else(invalid_debugger_state)?;
        let dependency_handle = document
            .debugger_programs
            .get(&safe_point.program.program_handle)
            .filter(|record| record.program_generation == safe_point.program.program_generation)
            .map(|record| record.runtime_handle)
            .ok_or_else(invalid_debugger_state)?;
        if frame.entry_program() != entry_handle
            || frame.dependency_program() != dependency_handle
            || frame.code_unit_ordinal() != safe_point.code_unit_ordinal
        {
            return Err(invalid_debugger_state());
        }
        let snapshot = self
            .runtime
            .debugger_linked_stack_snapshot(tab_id, frame, 2, max_scope_entries)
            .map_err(|_| invalid_debugger_state())?;
        let [child, root] = snapshot.frames.as_slice() else {
            return Err(invalid_debugger_state());
        };
        let child_point = PageHostDebuggerSafePoint {
            program: safe_point.program,
            code_unit_ordinal: child.code_unit_ordinal,
            bytecode_offset: child.bytecode_offset,
        };
        let root_point = PageHostDebuggerSafePoint {
            program: entry_program,
            code_unit_ordinal: root.code_unit_ordinal,
            bytecode_offset: root.bytecode_offset,
        };
        if child_point != safe_point
            || self
                .exact_debugger_safe_point(tab_id, document_generation, child_point)
                .is_err()
            || self
                .exact_debugger_safe_point(tab_id, document_generation, root_point)
                .is_err()
        {
            return Err(invalid_debugger_state());
        }
        Ok(ChildLinkedStackSnapshot {
            frames: [
                ChildLinkedStackFrame {
                    safe_point: child_point,
                    scope_entries: child
                        .scope_entries
                        .iter()
                        .map(|entry| PageHostDebuggerScopeEntry {
                            slot_ordinal: entry.slot_ordinal,
                            scope_depth: entry.scope_depth,
                        })
                        .collect(),
                    scope_truncated: child.scope_truncated,
                },
                ChildLinkedStackFrame {
                    safe_point: root_point,
                    scope_entries: root
                        .scope_entries
                        .iter()
                        .map(|entry| PageHostDebuggerScopeEntry {
                            slot_ordinal: entry.slot_ordinal,
                            scope_depth: entry.scope_depth,
                        })
                        .collect(),
                    scope_truncated: root.scope_truncated,
                },
            ],
            stack_truncated: snapshot.stack_truncated,
            max_scope_entries,
        })
    }

    /// Maps a complete live linked stack through two independent retained
    /// BlueTS attachments. A bad metadata/source pair refuses the whole
    /// vector; numeric source IDs alone are never cross-program authority.
    pub(super) fn debugger_linked_source_spans(
        &self,
        tab_id: u64,
        document_generation: u64,
        entry_program: PageHostDebuggerProgram,
        frame: BlueJsPageDebuggerLinkedFrame,
        expected_stack: &ChildLinkedStackSnapshot,
        sources: [(PageHostDebuggerMetadataHandle, u32); 2],
    ) -> Result<[PageHostDebuggerBlueTsSafePointSpan; 2], PageHostReply> {
        let current = self.debugger_linked_stack_snapshot(
            tab_id,
            document_generation,
            entry_program,
            frame,
            expected_stack.max_scope_entries,
        )?;
        if current != *expected_stack {
            return Err(invalid_request());
        }
        let document = self.exact_document(tab_id, document_generation)?;
        let resolve = |index: usize| {
            let safe_point = current.frames[index].safe_point;
            let (metadata, source_id) = sources[index];
            if !metadata.is_well_formed() {
                return Err(invalid_request());
            }
            let record = document
                .debugger_programs
                .get(&safe_point.program.program_handle)
                .filter(|record| {
                    record.program_generation == safe_point.program.program_generation
                        && record.metadata == Some(metadata)
                })
                .ok_or_else(invalid_request)?;
            let retained = self
                .debug_registry
                .get(self.runtime.program_registry(), record.runtime_handle)
                .map_err(|_| invalid_request())?;
            let span =
                exact_bluets_span_for_site(retained, safe_point).ok_or_else(invalid_request)?;
            if span.source_id != source_id {
                return Err(invalid_request());
            }
            Ok(span)
        };
        Ok([resolve(0)?, resolve(1)?])
    }

    /// Saves one terminal VM throw under its exact live child program before
    /// the scheduler runs another script in the same realm. A dependency may
    /// own the throwing code unit, so inspect only handles in this pending
    /// BlueTS execution rather than assuming the entry module threw.
    pub(super) fn snapshot_bluets_uncaught_location(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        handles: impl IntoIterator<Item = BlueJsProgramHandle>,
    ) {
        for runtime_handle in handles {
            let Some(site) = self
                .runtime
                .debugger_uncaught_throw_site(tab_id, runtime_handle)
                .ok()
                .flatten()
            else {
                continue;
            };
            let Some(program) = self
                .documents
                .get(&tab_id)
                .filter(|document| document.generation == document_generation)
                .and_then(|document| {
                    document
                        .debugger_programs
                        .iter()
                        .find(|(_, record)| record.runtime_handle == runtime_handle)
                        .map(|(program_handle, record)| PageHostDebuggerProgram {
                            program_handle: *program_handle,
                            program_generation: record.program_generation,
                        })
                })
            else {
                return;
            };
            let safe_point = PageHostDebuggerSafePoint {
                program,
                code_unit_ordinal: site.code_unit_ordinal,
                bytecode_offset: site.bytecode_offset,
            };
            if self
                .exact_debugger_safe_point(tab_id, document_generation, safe_point)
                .is_err()
            {
                return;
            }
            let Some(span) = self
                .debug_registry
                .get(self.runtime.program_registry(), runtime_handle)
                .ok()
                .and_then(|retained| exact_bluets_span_for_site(retained, safe_point))
            else {
                return;
            };
            let Some(record) = self
                .documents
                .get_mut(&tab_id)
                .and_then(|document| document.debugger_programs.get_mut(&program.program_handle))
            else {
                return;
            };
            record.exception_location = Some(ChildDebuggerExceptionLocation { safe_point, span });
            return;
        }
    }
}
