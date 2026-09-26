// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

impl BlueJsChildHost {
    pub(super) fn advance_debugger_execution(
        &mut self,
        tab_id: u64,
        document_generation: u64,
    ) -> PageHostReply {
        let origin = match self.exact_document(tab_id, document_generation) {
            Ok(document) if document.debugger_execution_control => document.origin.clone(),
            Ok(_) => return invalid_debugger_state(),
            Err(reply) => return reply,
        };
        let mut reports = Vec::new();
        while let Some(mut pending) = self
            .documents
            .get_mut(&tab_id)
            .expect("the exact child document remains live while advancing")
            .pending_debugger_executions
            .pop_front()
        {
            let mut paused = false;
            let outcome = match &mut pending.execution {
                DeferredChildExecution::JavaScriptClassic {
                    handle,
                    root_safe_point,
                }
                | DeferredChildExecution::BlueTsClassic {
                    handle,
                    root_safe_point,
                } => {
                    let program = pending
                        .program
                        .expect("every deferred classic has a private debugger program");
                    let status = self
                        .documents
                        .get(&tab_id)
                        .and_then(|document| document.debugger_execution_states.get(&program))
                        .copied()
                        .expect("every deferred classic has scheduler state");
                    match status {
                        ChildDebuggerExecutionStatus::Paused(_)
                        | ChildDebuggerExecutionStatus::SourceStepLimitReached(_)
                        | ChildDebuggerExecutionStatus::NestedPaused { .. } => {
                            paused = true;
                            PageHostScriptOutcome::Executed
                        }
                        ChildDebuggerExecutionStatus::NestedStepRequested { frame, safe_point } => {
                            let result = self.runtime.step_debugger_nested_instruction(frame);
                            match result {
                                Ok(BlueJsPageDebuggerNestedExecutionState::Paused {
                                    frame: same_frame,
                                    bytecode_offset,
                                }) if same_frame == frame => {
                                    let successor = PageHostDebuggerSafePoint {
                                        bytecode_offset,
                                        ..safe_point
                                    };
                                    if self
                                        .exact_debugger_safe_point(
                                            tab_id,
                                            document_generation,
                                            successor,
                                        )
                                        .is_ok()
                                    {
                                        self.documents
                                            .get_mut(&tab_id)
                                            .expect("the nested document remains live")
                                            .debugger_execution_states
                                            .insert(
                                                program,
                                                ChildDebuggerExecutionStatus::NestedPaused {
                                                    frame,
                                                    safe_point: successor,
                                                },
                                            );
                                        paused = true;
                                        PageHostScriptOutcome::Executed
                                    } else {
                                        self.documents
                                            .get_mut(&tab_id)
                                            .expect("the nested document remains live")
                                            .debugger_execution_states
                                            .insert(
                                                program,
                                                ChildDebuggerExecutionStatus::Completed,
                                            );
                                        rejected("BlueJS nested step lost its verified boundary")
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
                                        .exact_debugger_safe_point(
                                            tab_id,
                                            document_generation,
                                            successor,
                                        )
                                        .is_ok()
                                    {
                                        *root_safe_point = Some(successor);
                                        self.documents
                                            .get_mut(&tab_id)
                                            .expect("the nested document remains live")
                                            .debugger_execution_states
                                            .insert(
                                                program,
                                                ChildDebuggerExecutionStatus::Paused(successor),
                                            );
                                        paused = true;
                                        PageHostScriptOutcome::Executed
                                    } else {
                                        self.documents
                                            .get_mut(&tab_id)
                                            .expect("the nested document remains live")
                                            .debugger_execution_states
                                            .insert(
                                                program,
                                                ChildDebuggerExecutionStatus::Completed,
                                            );
                                        rejected("BlueJS nested return lost its root boundary")
                                    }
                                }
                                Ok(_) => {
                                    self.documents
                                        .get_mut(&tab_id)
                                        .expect("the nested document remains live")
                                        .debugger_execution_states
                                        .insert(program, ChildDebuggerExecutionStatus::Completed);
                                    rejected("BlueJS nested step returned an inconsistent frame")
                                }
                                Err(error) => {
                                    self.documents
                                        .get_mut(&tab_id)
                                        .expect("the nested document remains live")
                                        .debugger_execution_states
                                        .insert(program, ChildDebuggerExecutionStatus::Completed);
                                    rejected(page_runtime_category(error))
                                }
                            }
                        }
                        ChildDebuggerExecutionStatus::NestedResumeRequested {
                            frame,
                            safe_point,
                        } => {
                            let (still_paused, outcome, root_successor) = self
                                .advance_nested_frame(
                                    tab_id,
                                    document_generation,
                                    program,
                                    frame,
                                    safe_point,
                                    true,
                                );
                            *root_safe_point = root_successor;
                            paused = still_paused;
                            outcome
                        }
                        ChildDebuggerExecutionStatus::StepRequested => {
                            match self.runtime.step_debugger_root_instruction(tab_id) {
                                Ok(BlueJsPageDebuggerExecutionState::Paused {
                                    bytecode_offset,
                                }) => {
                                    let original = root_safe_point
                                        .expect("a child step requires an armed root safe point");
                                    let successor = PageHostDebuggerSafePoint {
                                        bytecode_offset,
                                        ..original
                                    };
                                    if self
                                        .exact_debugger_safe_point(
                                            tab_id,
                                            document_generation,
                                            successor,
                                        )
                                        .is_ok()
                                    {
                                        self.documents
                                            .get_mut(&tab_id)
                                            .expect("the stepping document remains live")
                                            .debugger_execution_states
                                            .insert(
                                                program,
                                                ChildDebuggerExecutionStatus::Paused(successor),
                                            );
                                        paused = true;
                                        PageHostScriptOutcome::Executed
                                    } else {
                                        self.documents
                                            .get_mut(&tab_id)
                                            .expect("the stepping document remains live")
                                            .debugger_execution_states
                                            .insert(
                                                program,
                                                ChildDebuggerExecutionStatus::Completed,
                                            );
                                        rejected("BlueJS debugger step returned an invalid root boundary")
                                    }
                                }
                                Ok(BlueJsPageDebuggerExecutionState::Completed) => {
                                    self.documents
                                        .get_mut(&tab_id)
                                        .expect("the stepping document remains live")
                                        .debugger_execution_states
                                        .insert(program, ChildDebuggerExecutionStatus::Completed);
                                    PageHostScriptOutcome::Executed
                                }
                                Err(error) => {
                                    self.documents
                                        .get_mut(&tab_id)
                                        .expect("the stepping document remains live")
                                        .debugger_execution_states
                                        .insert(program, ChildDebuggerExecutionStatus::Completed);
                                    rejected(page_runtime_category(error))
                                }
                            }
                        }
                        ChildDebuggerExecutionStatus::BlueTsSourceStepRequested {
                            origin,
                            remaining,
                        } => {
                            if pending.language != PageHostScriptLanguage::BlueTs {
                                self.documents
                                    .get_mut(&tab_id)
                                    .expect("the source-step document remains live")
                                    .debugger_execution_states
                                    .insert(program, ChildDebuggerExecutionStatus::Completed);
                                rejected("BlueTS source step state was inconsistent")
                            } else {
                                let original = root_safe_point.expect(
                                    "a BlueTS source step requires an armed root safe point",
                                );
                                let (still_paused, outcome) = self.advance_bluets_source_step(
                                    tab_id,
                                    document_generation,
                                    program,
                                    original,
                                    (origin, remaining),
                                    false,
                                );
                                paused = still_paused;
                                outcome
                            }
                        }
                        ChildDebuggerExecutionStatus::Pending
                        | ChildDebuggerExecutionStatus::ResumeRequested => {
                            if let (Some(target), ChildDebuggerExecutionStatus::Pending) =
                                (pending.nested_safe_point, status)
                            {
                                let point = self
                                    .runtime
                                    .safe_points(
                                        tab_id,
                                        *handle,
                                        usize::try_from(
                                            PAGE_HOST_DEBUGGER_MAX_SAFE_POINTS_PER_PROGRAM,
                                        )
                                        .expect("page-host safe-point cap fits usize"),
                                    )
                                    .ok()
                                    .and_then(|points| {
                                        points.into_iter().find(|point| {
                                            point.code_unit.ordinal() == target.code_unit_ordinal
                                                && point.bytecode_offset == target.bytecode_offset
                                        })
                                    });
                                let result = point.map(|point| {
                                    self.runtime.execute_program_until_nested_debugger_pause(
                                        tab_id, *handle, point,
                                    )
                                });
                                let outcome = match result {
                                    Some(Ok(BlueJsPageDebuggerNestedExecutionState::Paused {
                                        frame,
                                        bytecode_offset,
                                    })) if frame.tab_id() == tab_id
                                        && frame.program() == *handle
                                        && frame.code_unit_ordinal()
                                            == target.code_unit_ordinal =>
                                    {
                                        let successor = PageHostDebuggerSafePoint {
                                            bytecode_offset,
                                            ..target
                                        };
                                        if self
                                            .exact_debugger_safe_point(
                                                tab_id,
                                                document_generation,
                                                successor,
                                            )
                                            .is_ok()
                                        {
                                            self.documents
                                                .get_mut(&tab_id)
                                                .expect("the nested document remains live")
                                                .debugger_execution_states
                                                .insert(
                                                    program,
                                                    ChildDebuggerExecutionStatus::NestedPaused {
                                                        frame,
                                                        safe_point: successor,
                                                    },
                                                );
                                            paused = true;
                                            PageHostScriptOutcome::Executed
                                        } else {
                                            rejected(
                                                "BlueJS nested pause lost its verified boundary",
                                            )
                                        }
                                    }
                                    Some(Ok(BlueJsPageDebuggerNestedExecutionState::Completed)) => {
                                        PageHostScriptOutcome::Executed
                                    }
                                    Some(Err(error)) => rejected(page_runtime_category(error)),
                                    _ => rejected(
                                        "BlueJS nested pause returned an inconsistent frame",
                                    ),
                                };
                                if !paused {
                                    self.documents
                                        .get_mut(&tab_id)
                                        .expect("the nested document remains live")
                                        .debugger_execution_states
                                        .insert(program, ChildDebuggerExecutionStatus::Completed);
                                }
                                outcome
                            } else {
                                let result = match (*root_safe_point, status) {
                                    (Some(target), ChildDebuggerExecutionStatus::Pending) => self
                                        .runtime
                                        .execute_program_until_debugger_pause_at_root_offset(
                                            tab_id,
                                            *handle,
                                            target.bytecode_offset,
                                        )
                                        .map(|state| (state, true)),
                                    (Some(_), ChildDebuggerExecutionStatus::ResumeRequested) => {
                                        self.runtime
                                            .resume_debugger_execution(tab_id)
                                            .map(|state| (state, false))
                                    }
                                    (None, _) => {
                                        self.runtime.execute_program(tab_id, *handle).map(|_| {
                                            (BlueJsPageDebuggerExecutionState::Completed, false)
                                        })
                                    }
                                    _ => {
                                        unreachable!(
                                            "only pending or resuming states reach execution"
                                        )
                                    }
                                };
                                match result {
                                    Ok((BlueJsPageDebuggerExecutionState::Paused { .. }, true)) => {
                                        let target = root_safe_point.expect(
                                            "a child pause has an armed exact root safe point",
                                        );
                                        self.documents
                                            .get_mut(&tab_id)
                                            .expect("the advancing document remains live")
                                            .debugger_execution_states
                                            .insert(
                                                program,
                                                ChildDebuggerExecutionStatus::Paused(target),
                                            );
                                        paused = true;
                                        PageHostScriptOutcome::Executed
                                    }
                                    Ok((BlueJsPageDebuggerExecutionState::Completed, _)) => {
                                        self.documents
                                            .get_mut(&tab_id)
                                            .expect("the advancing document remains live")
                                            .debugger_execution_states
                                            .insert(
                                                program,
                                                ChildDebuggerExecutionStatus::Completed,
                                            );
                                        PageHostScriptOutcome::Executed
                                    }
                                    Ok((
                                        BlueJsPageDebuggerExecutionState::Paused { .. },
                                        false,
                                    )) => {
                                        self.documents
                                            .get_mut(&tab_id)
                                            .expect("the advancing document remains live")
                                            .debugger_execution_states
                                            .insert(
                                                program,
                                                ChildDebuggerExecutionStatus::Completed,
                                            );
                                        rejected("BlueJS debugger continuation did not complete after resume")
                                    }
                                    Err(error) => {
                                        self.documents
                                            .get_mut(&tab_id)
                                            .expect("the advancing document remains live")
                                            .debugger_execution_states
                                            .insert(
                                                program,
                                                ChildDebuggerExecutionStatus::Completed,
                                            );
                                        rejected(page_runtime_category(error))
                                    }
                                }
                            }
                        }
                        ChildDebuggerExecutionStatus::LinkedPaused { .. }
                        | ChildDebuggerExecutionStatus::LinkedResumeRequested { .. }
                        | ChildDebuggerExecutionStatus::Completed => {
                            rejected("child debugger execution state was inconsistent")
                        }
                    }
                }
                DeferredChildExecution::JavaScriptModule { graph, programs } => {
                    execute_module_graph(
                        &mut self.runtime,
                        tab_id,
                        &origin,
                        graph.clone(),
                        programs.clone(),
                    )
                }
                DeferredChildExecution::BlueTsModule {
                    attachment,
                    root_safe_point,
                } => {
                    let program = pending
                        .program
                        .expect("every deferred BlueTS module has an entry debugger program");
                    let status = self.documents[&tab_id].debugger_execution_states[&program];
                    match (root_safe_point, status) {
                        (
                            _,
                            ChildDebuggerExecutionStatus::Paused(_)
                            | ChildDebuggerExecutionStatus::SourceStepLimitReached(_)
                            | ChildDebuggerExecutionStatus::NestedPaused { .. }
                            | ChildDebuggerExecutionStatus::LinkedPaused { .. },
                        ) => {
                            paused = true;
                            PageHostScriptOutcome::Executed
                        }
                        (
                            root_slot,
                            ChildDebuggerExecutionStatus::NestedStepRequested { frame, safe_point },
                        ) => {
                            let (still_paused, outcome, root_successor) = self
                                .advance_nested_frame(
                                    tab_id,
                                    document_generation,
                                    program,
                                    frame,
                                    safe_point,
                                    false,
                                );
                            *root_slot = root_successor;
                            paused = still_paused;
                            outcome
                        }
                        (
                            root_slot,
                            ChildDebuggerExecutionStatus::NestedResumeRequested {
                                frame,
                                safe_point,
                            },
                        ) => {
                            let (still_paused, outcome, root_successor) = self
                                .advance_nested_frame(
                                    tab_id,
                                    document_generation,
                                    program,
                                    frame,
                                    safe_point,
                                    true,
                                );
                            *root_slot = root_successor;
                            paused = still_paused;
                            outcome
                        }
                        (
                            root_slot,
                            ChildDebuggerExecutionStatus::LinkedResumeRequested {
                                frame,
                                safe_point,
                            },
                        ) => {
                            let result =
                                self.runtime.resume_debugger_linked_nested_execution(frame);
                            let (status, outcome) = match result {
                                Ok(BlueJsPageDebuggerLinkedExecutionState::FrameReturned {
                                    root_bytecode_offset,
                                }) => {
                                    let successor = PageHostDebuggerSafePoint {
                                        program,
                                        code_unit_ordinal: 0,
                                        bytecode_offset: root_bytecode_offset,
                                    };
                                    if self
                                        .exact_debugger_safe_point(
                                            tab_id,
                                            document_generation,
                                            successor,
                                        )
                                        .is_ok()
                                        && safe_point.program != program
                                    {
                                        *root_slot = Some(successor);
                                        paused = true;
                                        (
                                            ChildDebuggerExecutionStatus::Paused(successor),
                                            PageHostScriptOutcome::Executed,
                                        )
                                    } else {
                                        (
                                            ChildDebuggerExecutionStatus::Completed,
                                            rejected(
                                                "BlueJS linked return lost its entry boundary",
                                            ),
                                        )
                                    }
                                }
                                Ok(_) => (
                                    ChildDebuggerExecutionStatus::Completed,
                                    rejected("BlueJS linked resume returned an inconsistent frame"),
                                ),
                                Err(error) => (
                                    ChildDebuggerExecutionStatus::Completed,
                                    rejected(page_runtime_category(error)),
                                ),
                            };
                            self.documents
                                .get_mut(&tab_id)
                                .expect("the linked document remains live")
                                .debugger_execution_states
                                .insert(program, status);
                            outcome
                        }
                        (Some(target), ChildDebuggerExecutionStatus::StepRequested) => {
                            match self.runtime.step_debugger_module_root_instruction(tab_id) {
                                Ok(BlueJsPageDebuggerExecutionState::Paused {
                                    bytecode_offset,
                                }) => {
                                    let successor = PageHostDebuggerSafePoint {
                                        bytecode_offset,
                                        ..*target
                                    };
                                    if self
                                        .exact_debugger_safe_point(
                                            tab_id,
                                            document_generation,
                                            successor,
                                        )
                                        .is_ok()
                                    {
                                        self.documents
                                            .get_mut(&tab_id)
                                            .expect("the stepping module document remains live")
                                            .debugger_execution_states
                                            .insert(
                                                program,
                                                ChildDebuggerExecutionStatus::Paused(successor),
                                            );
                                        paused = true;
                                        PageHostScriptOutcome::Executed
                                    } else {
                                        self.documents
                                            .get_mut(&tab_id)
                                            .expect("the stepping module document remains live")
                                            .debugger_execution_states
                                            .insert(
                                                program,
                                                ChildDebuggerExecutionStatus::Completed,
                                            );
                                        rejected(
                                            "BlueJS module step returned an invalid root boundary",
                                        )
                                    }
                                }
                                Ok(BlueJsPageDebuggerExecutionState::Completed) => {
                                    self.documents
                                        .get_mut(&tab_id)
                                        .expect("the stepping module document remains live")
                                        .debugger_execution_states
                                        .insert(program, ChildDebuggerExecutionStatus::Completed);
                                    PageHostScriptOutcome::Executed
                                }
                                Err(error) => {
                                    self.documents
                                        .get_mut(&tab_id)
                                        .expect("the stepping module document remains live")
                                        .debugger_execution_states
                                        .insert(program, ChildDebuggerExecutionStatus::Completed);
                                    rejected(page_runtime_category(error))
                                }
                            }
                        }
                        (
                            Some(target),
                            ChildDebuggerExecutionStatus::BlueTsSourceStepRequested {
                                origin,
                                remaining,
                            },
                        ) => {
                            let (still_paused, outcome) = self.advance_bluets_source_step(
                                tab_id,
                                document_generation,
                                program,
                                *target,
                                (origin, remaining),
                                true,
                            );
                            paused = still_paused;
                            outcome
                        }
                        (Some(target), ChildDebuggerExecutionStatus::Pending) => {
                            let result = self
                                .runtime
                                .module_evaluate_entry_safe_point(tab_id, attachment.entry.handle)
                                .and_then(|point| {
                                    if point.bytecode_offset != target.bytecode_offset {
                                        return Err(
                                            BlueJsPageRuntimeError::DebuggerModuleEntrySafePointOnly,
                                        );
                                    }
                                    self.runtime.execute_module_graph_until_debugger_pause(
                                        tab_id,
                                        attachment.entry.handle,
                                        attachment.modules.values().map(|module| module.handle),
                                        point,
                                    )
                                });
                            match result {
                                Ok(BlueJsPageDebuggerExecutionState::Paused { .. }) => {
                                    self.documents
                                        .get_mut(&tab_id)
                                        .expect("the advancing module document remains live")
                                        .debugger_execution_states
                                        .insert(
                                            program,
                                            ChildDebuggerExecutionStatus::Paused(*target),
                                        );
                                    paused = true;
                                    PageHostScriptOutcome::Executed
                                }
                                Ok(BlueJsPageDebuggerExecutionState::Completed) => {
                                    self.documents
                                        .get_mut(&tab_id)
                                        .expect("the advancing module document remains live")
                                        .debugger_execution_states
                                        .insert(program, ChildDebuggerExecutionStatus::Completed);
                                    PageHostScriptOutcome::Executed
                                }
                                Err(error) => {
                                    self.documents
                                        .get_mut(&tab_id)
                                        .expect("the advancing module document remains live")
                                        .debugger_execution_states
                                        .insert(program, ChildDebuggerExecutionStatus::Completed);
                                    rejected(page_runtime_category(error))
                                }
                            }
                        }
                        (Some(_), ChildDebuggerExecutionStatus::ResumeRequested) => {
                            let result = self.runtime.resume_debugger_module_execution(tab_id);
                            self.documents
                                .get_mut(&tab_id)
                                .expect("the advancing module document remains live")
                                .debugger_execution_states
                                .insert(program, ChildDebuggerExecutionStatus::Completed);
                            match result {
                                Ok(BlueJsPageDebuggerExecutionState::Completed) => {
                                    PageHostScriptOutcome::Executed
                                }
                                Ok(BlueJsPageDebuggerExecutionState::Paused { .. }) => {
                                    rejected("BlueJS module debugger continuation did not complete")
                                }
                                Err(error) => rejected(page_runtime_category(error)),
                            }
                        }
                        (None, ChildDebuggerExecutionStatus::Pending)
                            if pending.linked_safe_point.is_some() =>
                        {
                            let target = pending
                                .linked_safe_point
                                .expect("the guarded module has a linked target");
                            let dependency_handle = self.documents[&tab_id]
                                .debugger_programs
                                .get(&target.program.program_handle)
                                .filter(|record| {
                                    record.program_generation == target.program.program_generation
                                })
                                .map(|record| record.runtime_handle);
                            let point = dependency_handle.and_then(|handle| {
                                self.runtime
                                    .safe_points(
                                        tab_id,
                                        handle,
                                        usize::try_from(
                                            PAGE_HOST_DEBUGGER_MAX_SAFE_POINTS_PER_PROGRAM,
                                        )
                                        .expect("page-host safe-point cap fits usize"),
                                    )
                                    .ok()
                                    .and_then(|points| {
                                        points.into_iter().find(|point| {
                                            point.code_unit.ordinal() == target.code_unit_ordinal
                                                && point.bytecode_offset == target.bytecode_offset
                                        })
                                    })
                                    .map(|point| (handle, point))
                            });
                            let result = point.map(|(handle, point)| {
                                self.runtime
                                    .execute_module_graph_until_linked_nested_debugger_pause(
                                        tab_id,
                                        attachment.entry.handle,
                                        handle,
                                        attachment.modules.values().map(|module| module.handle),
                                        point,
                                    )
                            });
                            let status = match result {
                                Some(Ok(BlueJsPageDebuggerLinkedExecutionState::Paused {
                                    frame,
                                    bytecode_offset,
                                })) if frame.tab_id() == tab_id
                                    && frame.entry_program() == attachment.entry.handle
                                    && dependency_handle == Some(frame.dependency_program())
                                    && frame.code_unit_ordinal() == target.code_unit_ordinal =>
                                {
                                    let successor = PageHostDebuggerSafePoint {
                                        bytecode_offset,
                                        ..target
                                    };
                                    if self
                                        .exact_debugger_safe_point(
                                            tab_id,
                                            document_generation,
                                            successor,
                                        )
                                        .is_ok()
                                    {
                                        paused = true;
                                        ChildDebuggerExecutionStatus::LinkedPaused {
                                            frame,
                                            safe_point: successor,
                                        }
                                    } else {
                                        ChildDebuggerExecutionStatus::Completed
                                    }
                                }
                                _ => ChildDebuggerExecutionStatus::Completed,
                            };
                            self.documents
                                .get_mut(&tab_id)
                                .expect("the advancing linked module document remains live")
                                .debugger_execution_states
                                .insert(program, status);
                            if paused {
                                PageHostScriptOutcome::Executed
                            } else {
                                rejected("BlueJS linked pause rejected its exact dependency target")
                            }
                        }
                        (None, ChildDebuggerExecutionStatus::Pending)
                            if pending.nested_safe_point.is_some() =>
                        {
                            let target = pending
                                .nested_safe_point
                                .expect("the guarded module has a nested target");
                            let point = self
                                .runtime
                                .safe_points(
                                    tab_id,
                                    attachment.entry.handle,
                                    usize::try_from(PAGE_HOST_DEBUGGER_MAX_SAFE_POINTS_PER_PROGRAM)
                                        .expect("page-host safe-point cap fits usize"),
                                )
                                .ok()
                                .and_then(|points| {
                                    points.into_iter().find(|point| {
                                        point.code_unit.ordinal() == target.code_unit_ordinal
                                            && point.bytecode_offset == target.bytecode_offset
                                    })
                                });
                            let result = point.map(|point| {
                                self.runtime
                                    .execute_module_graph_until_nested_debugger_pause(
                                        tab_id,
                                        attachment.entry.handle,
                                        attachment.modules.values().map(|module| module.handle),
                                        point,
                                    )
                            });
                            let (still_paused, outcome) = self.record_nested_pause(
                                tab_id,
                                document_generation,
                                program,
                                attachment.entry.handle,
                                target,
                                result,
                            );
                            paused = still_paused;
                            outcome
                        }
                        (None, ChildDebuggerExecutionStatus::Pending) => {
                            let outcome =
                                match attachment.execute_in_page_realm(&mut self.runtime, tab_id) {
                                    Ok(_) => PageHostScriptOutcome::Executed,
                                    Err(error) => rejected(bluets_bridge_category(error)),
                                };
                            self.documents
                                .get_mut(&tab_id)
                                .expect("the advancing module document remains live")
                                .debugger_execution_states
                                .insert(program, ChildDebuggerExecutionStatus::Completed);
                            outcome
                        }
                        _ => rejected("child module debugger execution state was inconsistent"),
                    }
                }
            };
            if paused {
                self.documents
                    .get_mut(&tab_id)
                    .expect("the paused document remains live")
                    .pending_debugger_executions
                    .push_front(pending);
                break;
            }
            match &pending.execution {
                DeferredChildExecution::BlueTsClassic { handle, .. } => {
                    self.snapshot_bluets_uncaught_location(tab_id, document_generation, [*handle])
                }
                DeferredChildExecution::BlueTsModule { attachment, .. } => {
                    self.snapshot_bluets_uncaught_location(
                        tab_id,
                        document_generation,
                        attachment.modules.values().map(|module| module.handle),
                    );
                }
                DeferredChildExecution::JavaScriptClassic { .. }
                | DeferredChildExecution::JavaScriptModule { .. } => {}
            }
            reports.push(script_report(
                tab_id,
                document_generation,
                pending.ordinal,
                pending.language,
                pending.kind,
                outcome,
            ));
        }
        if self.refresh_debugger_programs(tab_id).is_err() {
            return self.fail_debugger_execution_document(tab_id);
        }
        PageHostReply::DebuggerExecutionAdvanced {
            tab_id,
            document_generation,
            reports,
        }
    }
}
