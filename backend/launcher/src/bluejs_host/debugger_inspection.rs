// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

impl BlueJsChildHost {
    /// Joins one freshly active root slot to its retained compiler
    /// symbol/type IDs without inspecting a VM value.
    pub(super) fn debugger_static_scope_relation(
        &self,
        target: PageHostDebuggerStaticScopeTarget,
    ) -> PageHostReply {
        if !target.is_well_formed() {
            return invalid_request();
        }
        let (metadata, scope_target) = match &target {
            PageHostDebuggerStaticScopeTarget::Ordinary { metadata, target } => {
                (*metadata, *target)
            }
            PageHostDebuggerStaticScopeTarget::Linked { .. } => {
                return self.debugger_static_scope_linked_relation(target);
            }
        };
        let stack = match self.debugger_stack_snapshot(
            scope_target.tab_id,
            scope_target.document_generation,
            scope_target.program,
            scope_target.frame,
            2,
            PAGE_HOST_DEBUGGER_MAX_SCOPE_ENTRIES,
        ) {
            PageHostReply::DebuggerStackSnapshot {
                tab_id,
                document_generation,
                program,
                frame,
                snapshot,
            } if tab_id == scope_target.tab_id
                && document_generation == scope_target.document_generation
                && program == scope_target.program
                && frame == scope_target.frame =>
            {
                snapshot
            }
            PageHostReply::Error { code, message } => {
                return PageHostReply::Error { code, message };
            }
            _ => return invalid_debugger_state(),
        };
        if stack.stack_truncated
            || stack.frames.iter().any(|frame| frame.scope_truncated)
            || stack.frames.len() != if scope_target.frame.is_some() { 2 } else { 1 }
        {
            return invalid_debugger_state();
        }
        let Some(frame) = stack.frames.get(scope_target.frame_index as usize) else {
            return invalid_debugger_state();
        };
        if frame.scope_truncated
            || frame.code_unit_ordinal != 0
            || frame.bytecode_offset != scope_target.safe_point.bytecode_offset
            || frame
                .scope_entries
                .iter()
                .filter(|entry| entry.slot_ordinal == scope_target.scope_entry.slot_ordinal)
                .count()
                != 1
            || !frame.scope_entries.contains(&scope_target.scope_entry)
        {
            return invalid_debugger_state();
        }
        let slots = match self.live_bluets_root_symbol_slots(
            scope_target.tab_id,
            scope_target.document_generation,
            scope_target.program,
            metadata,
        ) {
            Ok(slots) => slots,
            Err(_) => return invalid_debugger_state(),
        };
        let mut matching = slots.iter().filter(|slot| {
            slot.code_unit.ordinal() == 0
                && slot.slot_ordinal == scope_target.scope_entry.slot_ordinal
        });
        let Some(slot) = matching.next() else {
            return invalid_debugger_state();
        };
        if matching.next().is_some() {
            return invalid_debugger_state();
        }
        PageHostReply::DebuggerStaticScopeRelation(Box::new(PageHostDebuggerStaticScopeRelation {
            target,
            symbol_type: PageHostDebuggerBlueTsMetadataSymbolType {
                symbol_id: slot.symbol_id.0,
                type_id: slot.type_id.0,
            },
        }))
    }

    /// Reacquires and compares the *entire* dependency/entry stack before
    /// mapping an entry-root slot. Dependency locals/captures, stale child
    /// frames, and cross-program metadata can never choose this map.
    pub(super) fn debugger_static_scope_linked_relation(
        &self,
        target: PageHostDebuggerStaticScopeTarget,
    ) -> PageHostReply {
        let PageHostDebuggerStaticScopeTarget::Linked {
            frame,
            expected_stack,
            frame_index,
            metadata,
            scope_entry,
        } = &target
        else {
            return invalid_request();
        };
        let active = match self.exact_linked_runtime_frame(*frame) {
            Ok(active) => active,
            Err(reply) => return reply,
        };
        let current = match self.debugger_linked_stack_snapshot(
            frame.tab_id,
            frame.document_generation,
            frame.entry_program,
            active,
            expected_stack.max_scope_entries,
        ) {
            Ok(current) => current.to_wire(),
            Err(reply) => return reply,
        };
        if current != **expected_stack || !current.is_well_formed(*frame) {
            return invalid_debugger_state();
        }
        let selected = &current.frames[*frame_index as usize];
        if selected
            .scope_entries
            .iter()
            .filter(|entry| entry.slot_ordinal == scope_entry.slot_ordinal)
            .count()
            != 1
        {
            return invalid_debugger_state();
        }
        let slots = match self.live_bluets_root_symbol_slots(
            frame.tab_id,
            frame.document_generation,
            frame.entry_program,
            *metadata,
        ) {
            Ok(slots) => slots,
            Err(_) => return invalid_debugger_state(),
        };
        let mut matching = slots.iter().filter(|slot| {
            slot.code_unit.ordinal() == 0 && slot.slot_ordinal == scope_entry.slot_ordinal
        });
        let Some(slot) = matching.next() else {
            return invalid_debugger_state();
        };
        if matching.next().is_some() {
            return invalid_debugger_state();
        }
        PageHostReply::DebuggerStaticScopeRelation(Box::new(PageHostDebuggerStaticScopeRelation {
            target,
            symbol_type: PageHostDebuggerBlueTsMetadataSymbolType {
                symbol_id: slot.symbol_id.0,
                type_id: slot.type_id.0,
            },
        }))
    }

    pub(super) fn debugger_value_snapshot(
        &self,
        target: PageHostDebuggerValueTarget,
    ) -> PageHostReply {
        if !target.is_well_formed() {
            return invalid_request();
        }
        let document = match self.exact_document(target.tab_id, target.document_generation) {
            Ok(document) => document,
            Err(reply) => return reply,
        };
        if !document.debugger_execution_control
            || document
                .pending_debugger_executions
                .front()
                .and_then(|pending| pending.program)
                != Some(target.program)
        {
            return invalid_debugger_state();
        }
        let Some(record) = document
            .debugger_programs
            .get(&target.program.program_handle)
        else {
            return invalid_debugger_state();
        };
        if record.program_generation != target.program.program_generation
            || self
                .debug_registry
                .get(self.runtime.program_registry(), record.runtime_handle)
                .is_err()
        {
            return invalid_debugger_state();
        }
        let runtime_frame = match (
            target.frame,
            document
                .debugger_execution_states
                .get(&target.program)
                .copied(),
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
            ) if frame
                == child_debugger_frame(
                    target.tab_id,
                    target.document_generation,
                    target.program,
                    active,
                )
                && frame.matches_safe_point(safe_point) =>
            {
                Some(active)
            }
            _ => return invalid_debugger_state(),
        };
        let preview = match self.runtime.debugger_value_preview(
            target.tab_id,
            record.runtime_handle,
            runtime_frame,
            BlueJsPageDebuggerValueTarget {
                frame_index: target.frame_index,
                code_unit_ordinal: target.safe_point.code_unit_ordinal,
                bytecode_offset: target.safe_point.bytecode_offset,
                scope_entry: VmDebuggerScopeEntry {
                    slot_ordinal: target.scope_entry.slot_ordinal,
                    scope_depth: target.scope_entry.scope_depth,
                },
            },
        ) {
            Ok(preview) => page_host_debugger_value_preview(preview),
            Err(_) => return invalid_debugger_state(),
        };
        let snapshot = PageHostDebuggerValueSnapshot { target, preview };
        if !snapshot.is_well_formed() {
            return invalid_debugger_state();
        }
        PageHostReply::DebuggerValueSnapshot(Box::new(snapshot))
    }

    pub(super) fn debugger_execution_state(
        &self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
    ) -> PageHostReply {
        let document = match self.exact_document(tab_id, document_generation) {
            Ok(document) => document,
            Err(reply) => return reply,
        };
        if !document.debugger_execution_control || !program.is_well_formed() {
            return invalid_debugger_state();
        }
        let Some(status) = document.debugger_execution_states.get(&program).copied() else {
            return invalid_debugger_state();
        };
        match status {
            ChildDebuggerExecutionStatus::LinkedPaused { frame, safe_point } => {
                let linked = child_debugger_linked_frame(
                    tab_id,
                    document_generation,
                    program,
                    safe_point.program,
                    frame,
                );
                if !linked.is_well_formed()
                    || safe_point.code_unit_ordinal != linked.code_unit_ordinal
                {
                    return invalid_debugger_state();
                }
                return PageHostReply::DebuggerLinkedExecutionState {
                    frame: linked,
                    state: PageHostDebuggerLinkedExecutionState::Paused { safe_point },
                };
            }
            ChildDebuggerExecutionStatus::LinkedResumeRequested { frame, safe_point } => {
                let linked = child_debugger_linked_frame(
                    tab_id,
                    document_generation,
                    program,
                    safe_point.program,
                    frame,
                );
                if !linked.is_well_formed() {
                    return invalid_debugger_state();
                }
                return PageHostReply::DebuggerLinkedExecutionState {
                    frame: linked,
                    state: PageHostDebuggerLinkedExecutionState::Resuming,
                };
            }
            _ => {}
        }
        let Some(state) =
            child_debugger_execution_state(tab_id, document_generation, program, status)
        else {
            return invalid_debugger_state();
        };
        PageHostReply::DebuggerExecutionState {
            tab_id,
            document_generation,
            program,
            state,
        }
    }

    pub(super) fn resume_debugger_execution(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
    ) -> PageHostReply {
        let document = match self.documents.get_mut(&tab_id) {
            Some(document) if document.generation == document_generation => document,
            Some(_) => return stale_document(),
            None => return unknown_realm(),
        };
        if !document.debugger_execution_control || !program.is_well_formed() {
            return invalid_debugger_state();
        }
        let Some(status) = document.debugger_execution_states.get_mut(&program) else {
            return invalid_debugger_state();
        };
        if !matches!(
            status,
            ChildDebuggerExecutionStatus::Paused(_)
                | ChildDebuggerExecutionStatus::SourceStepLimitReached(_)
        ) {
            return invalid_debugger_state();
        }
        *status = ChildDebuggerExecutionStatus::ResumeRequested;
        PageHostReply::DebuggerExecutionResumed {
            tab_id,
            document_generation,
            program,
        }
    }

    pub(super) fn step_debugger_root_instruction(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
    ) -> PageHostReply {
        let document = match self.documents.get_mut(&tab_id) {
            Some(document) if document.generation == document_generation => document,
            Some(_) => return stale_document(),
            None => return unknown_realm(),
        };
        if !document.debugger_execution_control || !program.is_well_formed() {
            return invalid_debugger_state();
        }
        let Some(pending) = document.pending_debugger_executions.front() else {
            return invalid_debugger_state();
        };
        if pending.program != Some(program)
            || !matches!(
                &pending.execution,
                DeferredChildExecution::JavaScriptClassic {
                    root_safe_point: Some(_),
                    ..
                } | DeferredChildExecution::BlueTsClassic {
                    root_safe_point: Some(_),
                    ..
                } | DeferredChildExecution::BlueTsModule {
                    root_safe_point: Some(_),
                    ..
                }
            )
        {
            return invalid_debugger_state();
        }
        let Some(status) = document.debugger_execution_states.get_mut(&program) else {
            return invalid_debugger_state();
        };
        if !matches!(
            status,
            ChildDebuggerExecutionStatus::Paused(_)
                | ChildDebuggerExecutionStatus::SourceStepLimitReached(_)
        ) {
            return invalid_debugger_state();
        }
        *status = ChildDebuggerExecutionStatus::StepRequested;
        PageHostReply::DebuggerExecutionStepRequested {
            tab_id,
            document_generation,
            program,
        }
    }

    pub(super) fn debugger_bluets_source_span_key(
        &self,
        tab_id: u64,
        document_generation: u64,
        metadata: PageHostDebuggerMetadataHandle,
        safe_point: PageHostDebuggerSafePoint,
    ) -> Result<Option<BlueTsSourceSpanKey>, PageHostReply> {
        let document = self.exact_document(tab_id, document_generation)?;
        let program = safe_point.program;
        let record = document
            .debugger_programs
            .get(&program.program_handle)
            .filter(|record| {
                record.program_generation == program.program_generation
                    && record.metadata == Some(metadata)
            })
            .ok_or_else(invalid_request)?;
        let retained = self
            .debug_registry
            .get(self.runtime.program_registry(), record.runtime_handle)
            .map_err(|_| invalid_request())?;
        let Some(entry) = retained
            .safe_point_map()
            .source_span_for_safe_point(safe_point.code_unit_ordinal, safe_point.bytecode_offset)
        else {
            return Ok(None);
        };
        let mut sources = retained
            .static_info()
            .sources
            .iter()
            .filter(|source| source.module == entry.source);
        let source = sources.next().ok_or_else(invalid_request)?;
        if sources.next().is_some()
            || entry.start_byte >= entry.end_byte
            || entry.end_byte
                > usize::try_from(DEBUGGER_STATIC_METADATA_MAX_SOURCE_SPAN_BYTES).unwrap()
        {
            return Err(invalid_request());
        }
        Ok(Some(BlueTsSourceSpanKey {
            source_id: source.id.0,
            start_byte: u32::try_from(entry.start_byte).map_err(|_| invalid_request())?,
            end_byte: u32::try_from(entry.end_byte).map_err(|_| invalid_request())?,
        }))
    }

    pub(super) fn step_debugger_bluets_source_span(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        metadata: PageHostDebuggerMetadataHandle,
        source_id: u32,
        safe_point: PageHostDebuggerSafePoint,
    ) -> PageHostReply {
        if !metadata.is_well_formed() || !safe_point.is_well_formed() {
            return invalid_request();
        }
        if let Err(reply) = self.exact_debugger_safe_point(tab_id, document_generation, safe_point)
        {
            return reply;
        }
        let program = safe_point.program;
        {
            let document = match self.exact_document(tab_id, document_generation) {
                Ok(document) => document,
                Err(reply) => return reply,
            };
            if !document.debugger_execution_control
                || !document
                    .pending_debugger_executions
                    .front()
                    .is_some_and(|pending| {
                        pending.program == Some(program)
                            && matches!(
                                pending.execution,
                                DeferredChildExecution::BlueTsClassic { .. }
                                    | DeferredChildExecution::BlueTsModule {
                                        root_safe_point: Some(_),
                                        ..
                                    }
                            )
                    })
                || !matches!(
                    document.debugger_execution_states.get(&program),
                    Some(ChildDebuggerExecutionStatus::Paused(point)
                        | ChildDebuggerExecutionStatus::SourceStepLimitReached(point))
                        if *point == safe_point
                )
            {
                return invalid_debugger_state();
            }
        }
        let origin = match self.debugger_bluets_source_span_key(
            tab_id,
            document_generation,
            metadata,
            safe_point,
        ) {
            Ok(Some(span)) if span.source_id == source_id => span,
            Ok(_) => return invalid_request(),
            Err(reply) => return reply,
        };
        self.documents
            .get_mut(&tab_id)
            .expect("the exact source-step document remains live")
            .debugger_execution_states
            .insert(
                program,
                ChildDebuggerExecutionStatus::BlueTsSourceStepRequested {
                    origin,
                    remaining: MAX_BLUETS_SOURCE_STEP_ROOT_INSTRUCTIONS,
                },
            );
        PageHostReply::DebuggerBlueTsSourceStepRequested {
            tab_id,
            document_generation,
            metadata,
            source_id,
            safe_point,
        }
    }

    pub(super) fn record_nested_pause(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        runtime_handle: BlueJsProgramHandle,
        target: PageHostDebuggerSafePoint,
        result: Option<Result<BlueJsPageDebuggerNestedExecutionState, BlueJsPageRuntimeError>>,
    ) -> (bool, PageHostScriptOutcome) {
        let (status, outcome) = match result {
            Some(Ok(BlueJsPageDebuggerNestedExecutionState::Paused {
                frame,
                bytecode_offset,
            })) if frame.tab_id() == tab_id
                && frame.program() == runtime_handle
                && frame.code_unit_ordinal() == target.code_unit_ordinal =>
            {
                let successor = PageHostDebuggerSafePoint {
                    bytecode_offset,
                    ..target
                };
                if self
                    .exact_debugger_safe_point(tab_id, document_generation, successor)
                    .is_ok()
                {
                    (
                        ChildDebuggerExecutionStatus::NestedPaused {
                            frame,
                            safe_point: successor,
                        },
                        PageHostScriptOutcome::Executed,
                    )
                } else {
                    (
                        ChildDebuggerExecutionStatus::Completed,
                        rejected("BlueJS nested pause lost its verified boundary"),
                    )
                }
            }
            Some(Ok(BlueJsPageDebuggerNestedExecutionState::Completed)) => (
                ChildDebuggerExecutionStatus::Completed,
                PageHostScriptOutcome::Executed,
            ),
            Some(Err(error)) => (
                ChildDebuggerExecutionStatus::Completed,
                rejected(page_runtime_category(error)),
            ),
            _ => (
                ChildDebuggerExecutionStatus::Completed,
                rejected("BlueJS nested pause returned an inconsistent frame"),
            ),
        };
        self.documents
            .get_mut(&tab_id)
            .expect("the nested document remains live")
            .debugger_execution_states
            .insert(program, status);
        (
            matches!(status, ChildDebuggerExecutionStatus::NestedPaused { .. }),
            outcome,
        )
    }

    pub(super) fn advance_nested_frame(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        frame: BlueJsPageDebuggerFrame,
        safe_point: PageHostDebuggerSafePoint,
        resume: bool,
    ) -> (
        bool,
        PageHostScriptOutcome,
        Option<PageHostDebuggerSafePoint>,
    ) {
        let result = if resume {
            self.runtime.resume_debugger_nested_execution(frame)
        } else {
            self.runtime.step_debugger_nested_instruction(frame)
        };
        let (status, outcome, root_successor) = match result {
            Ok(BlueJsPageDebuggerNestedExecutionState::Paused {
                frame: same_frame,
                bytecode_offset,
            }) if same_frame == frame => {
                let successor = PageHostDebuggerSafePoint {
                    bytecode_offset,
                    ..safe_point
                };
                if self
                    .exact_debugger_safe_point(tab_id, document_generation, successor)
                    .is_ok()
                {
                    (
                        ChildDebuggerExecutionStatus::NestedPaused {
                            frame,
                            safe_point: successor,
                        },
                        PageHostScriptOutcome::Executed,
                        None,
                    )
                } else {
                    (
                        ChildDebuggerExecutionStatus::Completed,
                        rejected("BlueJS nested step lost its verified boundary"),
                        None,
                    )
                }
            }
            Ok(BlueJsPageDebuggerNestedExecutionState::FrameReturned {
                root_bytecode_offset,
            }) => {
                let successor = PageHostDebuggerSafePoint {
                    code_unit_ordinal: 0,
                    bytecode_offset: root_bytecode_offset,
                    ..safe_point
                };
                if self
                    .exact_debugger_safe_point(tab_id, document_generation, successor)
                    .is_ok()
                {
                    (
                        ChildDebuggerExecutionStatus::Paused(successor),
                        PageHostScriptOutcome::Executed,
                        Some(successor),
                    )
                } else {
                    (
                        ChildDebuggerExecutionStatus::Completed,
                        rejected("BlueJS nested return lost its root boundary"),
                        None,
                    )
                }
            }
            Ok(_) => (
                ChildDebuggerExecutionStatus::Completed,
                rejected("BlueJS nested step returned an inconsistent frame"),
                None,
            ),
            Err(error) => (
                ChildDebuggerExecutionStatus::Completed,
                rejected(page_runtime_category(error)),
                None,
            ),
        };
        self.documents
            .get_mut(&tab_id)
            .expect("the nested document remains live")
            .debugger_execution_states
            .insert(program, status);
        (
            matches!(
                status,
                ChildDebuggerExecutionStatus::NestedPaused { .. }
                    | ChildDebuggerExecutionStatus::Paused(_)
            ),
            outcome,
            root_successor,
        )
    }

    pub(super) fn advance_bluets_source_step(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        root_safe_point: PageHostDebuggerSafePoint,
        source_step: (BlueTsSourceSpanKey, u16),
        module_root: bool,
    ) -> (bool, PageHostScriptOutcome) {
        let (origin, remaining) = source_step;
        if remaining == 0 {
            self.documents
                .get_mut(&tab_id)
                .expect("the source-step document remains live")
                .debugger_execution_states
                .insert(program, ChildDebuggerExecutionStatus::Completed);
            return (false, rejected("BlueTS source step state was inconsistent"));
        }
        let result = if module_root {
            self.runtime.step_debugger_module_root_instruction(tab_id)
        } else {
            self.runtime.step_debugger_root_instruction(tab_id)
        };
        match result {
            Ok(BlueJsPageDebuggerExecutionState::Paused { bytecode_offset }) => {
                let successor = PageHostDebuggerSafePoint {
                    bytecode_offset,
                    ..root_safe_point
                };
                let metadata = self
                    .documents
                    .get(&tab_id)
                    .and_then(|document| document.debugger_programs.get(&program.program_handle))
                    .and_then(|record| record.metadata);
                let next_span = if self
                    .exact_debugger_safe_point(tab_id, document_generation, successor)
                    .is_ok()
                {
                    metadata.ok_or_else(invalid_request).and_then(|metadata| {
                        self.debugger_bluets_source_span_key(
                            tab_id,
                            document_generation,
                            metadata,
                            successor,
                        )
                    })
                } else {
                    Err(invalid_request())
                };
                if let Ok(next_span) = next_span {
                    let next_status = match next_span {
                        Some(span) if span != origin => {
                            ChildDebuggerExecutionStatus::Paused(successor)
                        }
                        _ if remaining > 1 => {
                            ChildDebuggerExecutionStatus::BlueTsSourceStepRequested {
                                origin,
                                remaining: remaining - 1,
                            }
                        }
                        _ => ChildDebuggerExecutionStatus::SourceStepLimitReached(successor),
                    };
                    self.documents
                        .get_mut(&tab_id)
                        .expect("the source-step document remains live")
                        .debugger_execution_states
                        .insert(program, next_status);
                    (true, PageHostScriptOutcome::Executed)
                } else {
                    self.documents
                        .get_mut(&tab_id)
                        .expect("the source-step document remains live")
                        .debugger_execution_states
                        .insert(program, ChildDebuggerExecutionStatus::Completed);
                    (
                        false,
                        rejected("BlueTS source step lost its verified boundary"),
                    )
                }
            }
            Ok(BlueJsPageDebuggerExecutionState::Completed) => {
                self.documents
                    .get_mut(&tab_id)
                    .expect("the source-step document remains live")
                    .debugger_execution_states
                    .insert(program, ChildDebuggerExecutionStatus::Completed);
                (false, PageHostScriptOutcome::Executed)
            }
            Err(error) => {
                self.documents
                    .get_mut(&tab_id)
                    .expect("the source-step document remains live")
                    .debugger_execution_states
                    .insert(program, ChildDebuggerExecutionStatus::Completed);
                (false, rejected(page_runtime_category(error)))
            }
        }
    }
}
