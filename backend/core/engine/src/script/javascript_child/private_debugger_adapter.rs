// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

/// Strict core-side validator for the complete private linked-module route.
/// Public identities and grants are deliberately not minted at this seam.
pub(super) struct ChildLinkedDebuggerAdapter<'a, C: PageHostClient> {
    pub(super) child: &'a mut C,
}

/// Strict private wire boundary for an already core-checked paused slot. No
/// child handle, partial relation, value, or display can pass an altered echo.
pub(super) struct ChildStaticScopeAdapter<'a, C: PageHostClient> {
    pub(super) child: &'a mut C,
}

impl<C: PageHostClient> ChildStaticScopeAdapter<'_, C> {
    pub(super) fn describe(
        &mut self,
        target: PageHostDebuggerStaticScopeTarget,
    ) -> Result<PageHostDebuggerBlueTsMetadataSymbolType, JavaScriptPageDebuggerError> {
        if !target.is_well_formed() {
            return Err(JavaScriptPageDebuggerError::InvalidExecutionState);
        }
        let reply = self
            .child
            .debugger_static_scope_relation(target.clone())
            .map_err(|_| JavaScriptPageDebuggerError::NoLiveRealm)?;
        match reply {
            PageHostReply::DebuggerStaticScopeRelation(relation)
                if relation.is_well_formed() && relation.target == target =>
            {
                Ok(relation.symbol_type)
            }
            _ => Err(child_debugger_reply_error(&reply)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ChildLinkedDebuggerStatus {
    Pending,
    Paused {
        frame: PageHostDebuggerLinkedFrame,
        safe_point: PageHostDebuggerSafePoint,
    },
    Resuming {
        frame: PageHostDebuggerLinkedFrame,
    },
    Completed,
}

#[allow(dead_code)] // The core reminting leaf connects this staged adapter.
impl<C: PageHostClient> ChildLinkedDebuggerAdapter<'_, C> {
    pub(super) fn arm(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        entry_program: PageHostDebuggerProgram,
        safe_point: PageHostDebuggerSafePoint,
    ) -> Result<(), JavaScriptPageDebuggerError> {
        if tab_id == 0
            || document_generation == 0
            || !entry_program.is_well_formed()
            || !safe_point.is_well_formed()
            || safe_point.program == entry_program
            || safe_point.code_unit_ordinal == 0
        {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        let reply = self
            .child
            .arm_debugger_linked_nested_safe_point_breakpoint(
                tab_id,
                document_generation,
                entry_program,
                safe_point,
            )
            .map_err(|_| JavaScriptPageDebuggerError::NoLiveRealm)?;
        match reply {
            PageHostReply::DebuggerLinkedNestedSafePointBreakpointArmed {
                tab_id: echoed_tab,
                document_generation: echoed_generation,
                entry_program: echoed_entry,
                safe_point: echoed_point,
            } if echoed_tab == tab_id
                && echoed_generation == document_generation
                && echoed_entry == entry_program
                && echoed_point == safe_point =>
            {
                Ok(())
            }
            _ => Err(child_debugger_reply_error(&reply)),
        }
    }

    pub(super) fn state(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        entry_program: PageHostDebuggerProgram,
        expected_frame: Option<PageHostDebuggerLinkedFrame>,
    ) -> Result<
        (
            PageHostDebuggerLinkedFrame,
            PageHostDebuggerLinkedExecutionState,
        ),
        JavaScriptPageDebuggerError,
    > {
        match self.lifecycle_state(tab_id, document_generation, entry_program, expected_frame)? {
            ChildLinkedDebuggerStatus::Paused { frame, safe_point } => Ok((
                frame,
                PageHostDebuggerLinkedExecutionState::Paused { safe_point },
            )),
            ChildLinkedDebuggerStatus::Resuming { frame } => {
                Ok((frame, PageHostDebuggerLinkedExecutionState::Resuming))
            }
            ChildLinkedDebuggerStatus::Pending | ChildLinkedDebuggerStatus::Completed => {
                Err(JavaScriptPageDebuggerError::InvalidExecutionState)
            }
        }
    }

    pub(super) fn lifecycle_state(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        entry_program: PageHostDebuggerProgram,
        expected_frame: Option<PageHostDebuggerLinkedFrame>,
    ) -> Result<ChildLinkedDebuggerStatus, JavaScriptPageDebuggerError> {
        let reply = self
            .child
            .debugger_execution_state(tab_id, document_generation, entry_program)
            .map_err(|_| JavaScriptPageDebuggerError::NoLiveRealm)?;
        match reply {
            PageHostReply::DebuggerExecutionState {
                tab_id: echoed_tab,
                document_generation: echoed_generation,
                program: echoed_program,
                state: PageHostDebuggerExecutionState::Pending,
            } if echoed_tab == tab_id
                && echoed_generation == document_generation
                && echoed_program == entry_program
                && expected_frame.is_none() =>
            {
                Ok(ChildLinkedDebuggerStatus::Pending)
            }
            PageHostReply::DebuggerExecutionState {
                tab_id: echoed_tab,
                document_generation: echoed_generation,
                program: echoed_program,
                state: PageHostDebuggerExecutionState::Completed,
            } if echoed_tab == tab_id
                && echoed_generation == document_generation
                && echoed_program == entry_program =>
            {
                Ok(ChildLinkedDebuggerStatus::Completed)
            }
            PageHostReply::DebuggerLinkedExecutionState { frame, state } => {
                if !frame.is_well_formed()
                    || frame.tab_id != tab_id
                    || frame.document_generation != document_generation
                    || frame.entry_program != entry_program
                    || expected_frame.is_some_and(|expected| expected != frame)
                {
                    return Err(JavaScriptPageDebuggerError::NoLiveRealm);
                }
                match state {
                    PageHostDebuggerLinkedExecutionState::Paused { safe_point }
                        if safe_point.program == frame.dependency_program
                            && safe_point.code_unit_ordinal == frame.code_unit_ordinal =>
                    {
                        Ok(ChildLinkedDebuggerStatus::Paused { frame, safe_point })
                    }
                    PageHostDebuggerLinkedExecutionState::Resuming => {
                        Ok(ChildLinkedDebuggerStatus::Resuming { frame })
                    }
                    _ => Err(JavaScriptPageDebuggerError::NoLiveRealm),
                }
            }
            _ => Err(child_debugger_reply_error(&reply)),
        }
    }

    pub(super) fn stack(
        &mut self,
        frame: PageHostDebuggerLinkedFrame,
        max_scope_entries: u32,
    ) -> Result<PageHostDebuggerLinkedStackSnapshot, JavaScriptPageDebuggerError> {
        if !frame.is_well_formed()
            || !(1..=PAGE_HOST_DEBUGGER_MAX_SCOPE_ENTRIES).contains(&max_scope_entries)
        {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        let reply = self
            .child
            .debugger_linked_stack_snapshot(frame, max_scope_entries)
            .map_err(|_| JavaScriptPageDebuggerError::NoLiveRealm)?;
        let PageHostReply::DebuggerLinkedStackSnapshot {
            frame: echoed_frame,
            snapshot,
        } = reply
        else {
            return Err(child_debugger_reply_error(&reply));
        };
        if echoed_frame != frame
            || snapshot.max_scope_entries != max_scope_entries
            || !snapshot.is_well_formed(frame)
        {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        Ok(*snapshot)
    }

    pub(super) fn spans(
        &mut self,
        frame: PageHostDebuggerLinkedFrame,
        expected_stack: PageHostDebuggerLinkedStackSnapshot,
        sources: [PageHostDebuggerLinkedSource; 2],
    ) -> Result<[page_host::PageHostDebuggerBlueTsSafePointSpan; 2], JavaScriptPageDebuggerError>
    {
        if !expected_stack.is_well_formed(frame)
            || sources
                .iter()
                .any(|source| !source.metadata.is_well_formed())
        {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        let reply = self
            .child
            .debugger_linked_stack_spans(frame, expected_stack.clone(), sources)
            .map_err(|_| JavaScriptPageDebuggerError::NoLiveRealm)?;
        let PageHostReply::DebuggerLinkedStackSpans {
            frame: echoed_frame,
            snapshot,
            spans,
        } = reply
        else {
            return Err(child_debugger_reply_error(&reply));
        };
        if echoed_frame != frame
            || *snapshot != expected_stack
            || spans.iter().enumerate().any(|(index, span)| {
                span.source_id != sources[index].source_id
                    || !span
                        .coordinates
                        .is_well_formed_for_range(span.start_byte, span.end_byte)
            })
        {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        Ok(*spans)
    }

    pub(super) fn resume(
        &mut self,
        frame: PageHostDebuggerLinkedFrame,
    ) -> Result<(), JavaScriptPageDebuggerError> {
        if !frame.is_well_formed() {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        let reply = self
            .child
            .resume_debugger_linked_nested_execution(frame)
            .map_err(|_| JavaScriptPageDebuggerError::NoLiveRealm)?;
        match reply {
            PageHostReply::DebuggerLinkedNestedResumeRequested { frame: echoed }
                if echoed == frame =>
            {
                Ok(())
            }
            _ => Err(child_debugger_reply_error(&reply)),
        }
    }
}
