// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

impl JavaScriptPageExecutor {
    /// Registers the execution record only after BlueJS admission and opaque
    /// debugger-program registration both succeeded. The first scheduler turn
    /// after admission is deliberately deferred, giving the remote owner one
    /// session turn to inspect exact locations and arm root entry.
    pub(in super::super) fn defer_debugger_execution(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        ordinal: u32,
        kind: BlueJsPageScriptKind,
        execution: DeferredJavaScriptExecution,
    ) -> Result<(), &'static str> {
        debug_assert!(self.debugger_execution_control_available());
        let bluejs_handle = match &execution {
            DeferredJavaScriptExecution::Classic { handle }
            | DeferredJavaScriptExecution::ModuleGraph { entry: handle, .. } => *handle,
        };
        let record = self
            .debugger_program_record_for_bluejs(tab_id, bluejs_handle)
            .ok_or("native debugger entry scheduling lost its program identity")?;
        let program = DebuggerProgramKey {
            program_handle: record.program_handle,
            program_generation: record.program_generation,
        };
        let entry_breakpoint = self
            .debugger_entry_breakpoint(tab_id, program)
            .map_err(|_| "BlueJS program has no compiler-verified root entry boundary")?;
        let states = self.debugger_execution_states.entry(tab_id).or_default();
        if states
            .insert(program, DebuggerExecutionStatus::Pending)
            .is_some()
        {
            return Err("native debugger entry scheduling duplicated a program identity");
        }
        self.pending_debugger_executions
            .entry(tab_id)
            .or_default()
            .push_back(PendingDebuggerExecution {
                program,
                entry_breakpoint,
                root_safe_point_armed: false,
                root_continuation_started: false,
                document_generation,
                ordinal,
                kind,
                execution,
            });
        Ok(())
    }

    /// Advances at most the currently runnable declaration sequence for every
    /// tab. A paused declaration blocks later same-tab declarations, retaining
    /// classic-script order. This runs only on the core session thread and
    /// never waits for a debugger socket.
    pub(in super::super) fn drive_debugger_executions(&mut self) {
        let tab_ids: Vec<_> = self.pending_debugger_executions.keys().copied().collect();
        for tab_id in tab_ids {
            while let Some(next) = self
                .pending_debugger_executions
                .get(&tab_id)
                .and_then(|pending| pending.front())
                .cloned()
            {
                let status = self
                    .debugger_execution_states
                    .get(&tab_id)
                    .and_then(|states| states.get(&next.program))
                    .copied();
                let Some(status) = status else {
                    // This cannot happen without an internal lifecycle bug;
                    // fail closed by dropping the orphaned work rather than
                    // executing an untracked declaration.
                    let _ = self
                        .pending_debugger_executions
                        .get_mut(&tab_id)
                        .and_then(VecDeque::pop_front);
                    self.push_deferred_rejection(
                        tab_id,
                        &next,
                        "native debugger execution state was lost",
                    );
                    continue;
                };
                match status {
                    DebuggerExecutionStatus::Paused(_) => break,
                    DebuggerExecutionStatus::Pending
                        if self
                            .debugger_breakpoints
                            .get(&tab_id)
                            .is_some_and(|breakpoints| {
                                breakpoints.contains(&next.entry_breakpoint)
                            }) =>
                    {
                        if next.entry_breakpoint.bytecode_offset == 0 {
                            self.debugger_execution_states
                                .get_mut(&tab_id)
                                .expect("a queued debugger execution has state")
                                .insert(
                                    next.program,
                                    DebuggerExecutionStatus::Paused(next.entry_breakpoint),
                                );
                            break;
                        }
                        // A non-entry root safe point has to execute real
                        // bytecode before it can pause. The page runtime owns
                        // the preserved operand/handler/iterator state; this
                        // scheduler retains only the source-free identity.
                        let result = match &next.execution {
                            DeferredJavaScriptExecution::Classic { handle } => self
                                .runtime
                                .execute_program_until_debugger_pause_at_root_offset(
                                    tab_id.as_u64(),
                                    *handle,
                                    next.entry_breakpoint.bytecode_offset,
                                )
                                .map_err(page_runtime_category),
                            DeferredJavaScriptExecution::ModuleGraph { .. } => Err(
                                "native debugger root continuation supports classic scripts only",
                            ),
                        };
                        match result {
                            Ok(BlueJsPageDebuggerExecutionState::Paused { .. }) => {
                                self.pending_debugger_executions
                                    .get_mut(&tab_id)
                                    .and_then(VecDeque::front_mut)
                                    .expect("a queued debugger execution remains at the head")
                                    .root_continuation_started = true;
                                self.debugger_execution_states
                                    .get_mut(&tab_id)
                                    .expect("a queued debugger execution has state")
                                    .insert(
                                        next.program,
                                        DebuggerExecutionStatus::Paused(next.entry_breakpoint),
                                    );
                                break;
                            }
                            Ok(BlueJsPageDebuggerExecutionState::Completed) => {
                                // The exact compiler boundary was validated
                                // before start, so this defensive branch is a
                                // terminal completion rather than a silent
                                // claim that a pause occurred.
                                let pending = self
                                    .pending_debugger_executions
                                    .get_mut(&tab_id)
                                    .and_then(VecDeque::pop_front)
                                    .expect(
                                        "the inspected pending debugger execution remains queued",
                                    );
                                self.debugger_execution_states
                                    .get_mut(&tab_id)
                                    .expect("a queued debugger execution has state")
                                    .insert(pending.program, DebuggerExecutionStatus::Completed);
                                self.push_report(JavaScriptPageExecutionReport::Executed {
                                    tab_id: tab_id.as_u64(),
                                    document_generation: pending.document_generation,
                                    ordinal: pending.ordinal,
                                    kind: pending.kind,
                                });
                            }
                            Err(category) => {
                                let pending = self
                                    .pending_debugger_executions
                                    .get_mut(&tab_id)
                                    .and_then(VecDeque::pop_front)
                                    .expect(
                                        "the inspected pending debugger execution remains queued",
                                    );
                                self.debugger_execution_states
                                    .get_mut(&tab_id)
                                    .expect("a queued debugger execution has state")
                                    .insert(pending.program, DebuggerExecutionStatus::Completed);
                                self.push_deferred_rejection(tab_id, &pending, category);
                            }
                        }
                    }
                    DebuggerExecutionStatus::StepRequested => {
                        let result = match &next.execution {
                            DeferredJavaScriptExecution::Classic { handle } => {
                                let prepared = if next.root_continuation_started {
                                    Ok(())
                                } else {
                                    match self.runtime.execute_program_until_debugger_pause_at_root_offset(
                                        tab_id.as_u64(),
                                        *handle,
                                        0,
                                    ) {
                                        Ok(BlueJsPageDebuggerExecutionState::Paused {
                                            bytecode_offset: 0,
                                        }) => {
                                            self.pending_debugger_executions
                                                .get_mut(&tab_id)
                                                .and_then(VecDeque::front_mut)
                                                .expect("entry-pause work remains queued")
                                                .root_continuation_started = true;
                                            Ok(())
                                        }
                                        Ok(_) => Err("native debugger entry step did not create an entry continuation"),
                                        Err(error) => Err(page_runtime_category(error)),
                                    }
                                };
                                prepared.and_then(|()| {
                                    self.runtime
                                        .step_debugger_root_instruction(tab_id.as_u64())
                                        .map_err(page_runtime_category)
                                })
                            }
                            DeferredJavaScriptExecution::ModuleGraph { .. } => {
                                Err("native debugger root stepping supports classic scripts only")
                            }
                        };
                        match result {
                            Ok(BlueJsPageDebuggerExecutionState::Paused { bytecode_offset }) => {
                                self.debugger_execution_states
                                    .get_mut(&tab_id)
                                    .expect("a queued debugger execution has state")
                                    .insert(
                                        next.program,
                                        DebuggerExecutionStatus::Paused(DebuggerBreakpointRecord {
                                            bytecode_offset,
                                            ..next.entry_breakpoint
                                        }),
                                    );
                                break;
                            }
                            Ok(BlueJsPageDebuggerExecutionState::Completed) | Err(_) => {
                                let pending = self
                                    .pending_debugger_executions
                                    .get_mut(&tab_id)
                                    .and_then(VecDeque::pop_front)
                                    .expect("the inspected debugger execution remains queued");
                                self.debugger_execution_states
                                    .get_mut(&tab_id)
                                    .expect("a queued debugger execution has state")
                                    .insert(pending.program, DebuggerExecutionStatus::Completed);
                                match result {
                                    Ok(_) => {
                                        self.push_report(JavaScriptPageExecutionReport::Executed {
                                            tab_id: tab_id.as_u64(),
                                            document_generation: pending.document_generation,
                                            ordinal: pending.ordinal,
                                            kind: pending.kind,
                                        })
                                    }
                                    Err(category) => {
                                        self.push_deferred_rejection(tab_id, &pending, category)
                                    }
                                }
                            }
                        }
                    }
                    DebuggerExecutionStatus::ResumeRequested
                        if next.entry_breakpoint.bytecode_offset != 0 =>
                    {
                        let result = self
                            .runtime
                            .resume_debugger_execution(tab_id.as_u64())
                            .map(|_| ())
                            .map_err(page_runtime_category);
                        let pending = self
                            .pending_debugger_executions
                            .get_mut(&tab_id)
                            .and_then(VecDeque::pop_front)
                            .expect("the inspected pending debugger execution remains queued");
                        self.debugger_execution_states
                            .get_mut(&tab_id)
                            .expect("a queued debugger execution has state")
                            .insert(pending.program, DebuggerExecutionStatus::Completed);
                        match result {
                            Ok(()) => self.push_report(JavaScriptPageExecutionReport::Executed {
                                tab_id: tab_id.as_u64(),
                                document_generation: pending.document_generation,
                                ordinal: pending.ordinal,
                                kind: pending.kind,
                            }),
                            Err(category) => {
                                self.push_deferred_rejection(tab_id, &pending, category)
                            }
                        }
                    }
                    DebuggerExecutionStatus::Pending | DebuggerExecutionStatus::ResumeRequested => {
                        let pending = self
                            .pending_debugger_executions
                            .get_mut(&tab_id)
                            .and_then(VecDeque::pop_front)
                            .expect("the inspected pending debugger execution remains queued");
                        let result = match &pending.execution {
                            DeferredJavaScriptExecution::Classic { handle } => self
                                .runtime
                                .execute_program(tab_id.as_u64(), *handle)
                                .map(|_: Value| ())
                                .map_err(page_runtime_category),
                            DeferredJavaScriptExecution::ModuleGraph { entry, installed } => self
                                .runtime
                                .execute_module_graph(tab_id.as_u64(), *entry, installed.clone())
                                .map(|_: Value| ())
                                .map_err(page_runtime_category),
                        };
                        self.debugger_execution_states
                            .get_mut(&tab_id)
                            .expect("a queued debugger execution has state")
                            .insert(pending.program, DebuggerExecutionStatus::Completed);
                        match result {
                            Ok(()) => self.push_report(JavaScriptPageExecutionReport::Executed {
                                tab_id: tab_id.as_u64(),
                                document_generation: pending.document_generation,
                                ordinal: pending.ordinal,
                                kind: pending.kind,
                            }),
                            Err(category) => {
                                self.push_deferred_rejection(tab_id, &pending, category)
                            }
                        }
                    }
                    DebuggerExecutionStatus::Completed => {
                        let _ = self
                            .pending_debugger_executions
                            .get_mut(&tab_id)
                            .and_then(VecDeque::pop_front);
                        self.push_deferred_rejection(
                            tab_id,
                            &next,
                            "native debugger execution was already completed",
                        );
                    }
                }
            }
            if self
                .pending_debugger_executions
                .get(&tab_id)
                .is_some_and(VecDeque::is_empty)
            {
                self.pending_debugger_executions.remove(&tab_id);
            }
        }
    }
}
