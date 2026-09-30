// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A bounded, root-code-unit continuation for the private native debugger.
//!
//! BlueJS ordinarily runs a page script synchronously on the embedding
//! session thread.  A debugger cannot truthfully pause an arbitrary bytecode
//! instruction merely by remembering its offset: the operand stack, handler
//! stack, active iterator records and execution-context fields are also live.
//! This module retains those states for a classic or module root and one
//! explicitly selected synchronous child invocation. The nested frame has a
//! distinct serial and continuation; root-only controls cannot resume it.

use super::*;

/// Exact source-free bytecode instruction that originated the last uncaught
/// catchable exception. The embedding host must bind this to its own live
/// BlueTS attachment before mapping it to an original source span.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VmDebuggerThrowSite {
    pub program_generation: u64,
    pub code_unit_ordinal: u32,
    pub bytecode_offset: u32,
}

impl Vm {
    /// Available only after an outer execution finished with an uncaught
    /// language exception. A new execution, catch, or success clears it.
    pub fn debugger_uncaught_throw_site(&self) -> Option<VmDebuggerThrowSite> {
        self.uncaught_throw_site
    }
}

/// Source-free outcome of a root-code-unit debugger run.
///
/// `Paused` means no instruction at `bytecode_offset` has executed yet. The
/// VM owns the complete root interpreter frame until
/// [`Vm::resume_debugger_execution`] finishes it. A root-instruction step may
/// instead suspend the same continuation again; bounded stack inspection is
/// a separate snapshot API, and nested control uses a separate frame serial.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VmDebuggerExecutionState {
    Paused { bytecode_offset: u32 },
    Completed,
}

/// Interpreter state that is not resident in ordinary `Vm` fields while the
/// root frame is paused. `handlers` retain stack depths relative to the root
/// frame's zero operand-stack base; `interpreter.rs` performs the inverse
/// conversion before returning this record.
pub(super) struct DebuggerContinuation {
    code: Bytecode,
    pc: usize,
    iterators: Vec<Value>,
    handlers: Vec<HandlerFrame>,
}

pub(super) struct ModuleDebuggerPauseRequest {
    pub(super) entry: String,
    pub(super) target: usize,
}

pub(super) struct ModuleDebuggerContinuation {
    pub(super) module: String,
    pub(super) code: Bytecode,
    pub(super) pc: usize,
    pub(super) execution: SuspendedModuleExecution,
    pub(super) iterators: Vec<Value>,
    pub(super) handlers: Vec<HandlerFrame>,
    pub(super) previous_module: Option<String>,
}

pub(super) struct NestedDebuggerPauseRequest {
    pub(super) program_generation: u64,
    pub(super) code_unit_ordinal: u32,
    pub(super) bytecode_offset: usize,
    pub(super) caller_program_generation: Option<u64>,
}

pub(super) struct NestedDebuggerContinuation {
    pub(super) frame_serial: u64,
    pub(super) code_unit_ordinal: u32,
    pub(super) code: Bytecode,
    pub(super) pc: usize,
    pub(super) execution: SuspendedModuleExecution,
    pub(super) iterators: Vec<Value>,
    pub(super) handlers: Vec<HandlerFrame>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VmDebuggerNestedExecutionState {
    Paused {
        frame_serial: u64,
        code_unit_ordinal: u32,
        bytecode_offset: u32,
    },
    FrameReturned {
        root_bytecode_offset: u32,
    },
    Completed,
}

/// Exact installed entry and dependency target for a native linked pause.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VmDebuggerLinkedPauseTarget {
    pub entry_generation: u64,
    pub dependency_generation: u64,
    pub code_unit_ordinal: u32,
    pub bytecode_offset: u32,
}

/// Hard native inspection budgets; a caller may request a smaller positive
/// limit but cannot raise these caps or use a zero-limit probe.
pub const VM_DEBUGGER_MAX_STACK_FRAMES: u32 = 64;
pub const VM_DEBUGGER_MAX_SCOPE_ENTRIES: u32 = 256;
pub const VM_DEBUGGER_MAX_VALUE_PAYLOAD_BYTES: usize = 4_096;

/// Lossless, handle-free data copied from one exact paused lexical slot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VmDebuggerValuePreview {
    Undefined,
    Null,
    Bool(bool),
    NumberBits(u64),
    BigIntBytes(Vec<u8>),
    StringUnits(Vec<u16>),
    /// `None` is an own-property hole, distinct from JavaScript `undefined`.
    Array(Vec<Option<VmDebuggerValuePreview>>),
    /// Keys are ordered, lossless JavaScript strings; no prototype is copied.
    Record(Vec<(JsString, VmDebuggerValuePreview)>),
}

/// One active lexical binding slot. No identifier, value, object handle, or
/// source location leaves the VM through this first inspection seam.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VmDebuggerScopeEntry {
    pub slot_ordinal: u32,
    /// Zero is the innermost currently active lexical scope.
    pub scope_depth: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VmDebuggerStackFrame {
    pub program_generation: u64,
    pub code_unit_ordinal: u32,
    pub bytecode_offset: u32,
    pub scope_entries: Vec<VmDebuggerScopeEntry>,
    pub scope_truncated: bool,
}

/// A bounded snapshot of only the currently retained debugger continuation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VmDebuggerStackSnapshot {
    pub program_generation: u64,
    pub frames: Vec<VmDebuggerStackFrame>,
    pub stack_truncated: bool,
}

fn debugger_scope_entries(
    scopes: &[Vec<u32>],
    max_entries: usize,
) -> (Vec<VmDebuggerScopeEntry>, bool) {
    let mut entries = Vec::new();
    for (scope_depth, scope) in scopes.iter().rev().enumerate() {
        for &slot_ordinal in scope {
            if entries.len() == max_entries {
                return (entries, true);
            }
            entries.push(VmDebuggerScopeEntry {
                slot_ordinal,
                scope_depth: u32::try_from(scope_depth)
                    .expect("bounded interpreter scope depth fits the debugger wire"),
            });
        }
    }
    (entries, false)
}

fn debugger_stack_frame(
    program_generation: u64,
    code_unit_ordinal: u32,
    pc: usize,
    scopes: &[Vec<u32>],
    max_entries: usize,
) -> VmDebuggerStackFrame {
    let (scope_entries, scope_truncated) = debugger_scope_entries(scopes, max_entries);
    VmDebuggerStackFrame {
        program_generation,
        code_unit_ordinal,
        bytecode_offset: u32::try_from(pc)
            .expect("verified bytecode offset fits the debugger wire"),
        scope_entries,
        scope_truncated,
    }
}

enum DebuggerRunOutcome {
    Paused(Box<DebuggerContinuation>),
    Completed(Result<Value, RuntimeError>),
}

mod inspection;

impl Vm {
    /// Links an authorized module graph and pauses before a verified entry
    /// evaluation-body instruction. Dependency effects occur once during the
    /// linking/evaluation pass and are retained with the graph on pause.
    pub fn execute_module_graph_until_debugger_pause(
        &mut self,
        entry: &str,
        modules: &HashMap<String, Bytecode>,
        bytecode_offset: u32,
    ) -> Result<VmDebuggerExecutionState, RuntimeError> {
        self.ensure_no_debugger_continuation()?;
        let code = modules.get(entry).ok_or(RuntimeError::Unsupported(
            "debugger module entry is absent from the graph",
        ))?;
        let target = usize::try_from(bytecode_offset)
            .map_err(|_| RuntimeError::Unsupported("debugger safe-point offset is too large"))?;
        let evaluate_entry = code.module_evaluate_entry.ok_or(RuntimeError::Unsupported(
            "debugger module entry has no evaluation body",
        ))? as usize;
        let first_evaluate_instruction = code
            .instructions()
            .map(|instruction| instruction.offset)
            .find(|offset| *offset >= evaluate_entry);
        if first_evaluate_instruction != Some(target) {
            return Err(RuntimeError::Unsupported(
                "debugger safe point is not the first module evaluate-body instruction",
            ));
        }
        self.pending_throw_site = None;
        self.uncaught_throw_site = None;
        self.throw_epoch = 0;
        self.debugger_module_pause_request = Some(ModuleDebuggerPauseRequest {
            entry: entry.to_string(),
            target,
        });
        let result = (|| {
            self.execute_module_graph_inner(entry, modules, false, false, ImportPhase::Evaluation)?;
            // An async dependency can delay the entry body itself. Advance
            // only the queued turns needed to reach its requested boundary;
            // never run another job after the debugger owns that frame.
            while self.debugger_module_continuation.is_none() && !self.promise_jobs.is_empty() {
                self.remaining_instructions = self.config.instruction_budget;
                self.run_next_promise_job()?;
            }
            Ok::<(), RuntimeError>(())
        })();
        self.debugger_module_pause_request = None;
        result?;
        if self.debugger_module_continuation.is_some() {
            Ok(VmDebuggerExecutionState::Paused { bytecode_offset })
        } else if let Some(error) = self
            .linked_record(entry)
            .and_then(|record| record.error.clone())
        {
            Err(RuntimeError::Thrown(error))
        } else {
            Err(RuntimeError::Unsupported(
                "debugger module entry did not reach the requested safe point",
            ))
        }
    }

    /// Resumes the linked entry module from its retained pre-instruction
    /// continuation. The entry and its dependencies are not evaluated again.
    pub fn resume_debugger_module_execution(
        &mut self,
    ) -> Result<VmDebuggerExecutionState, RuntimeError> {
        self.resume_debugger_module_inner(None)
    }

    /// Executes one instruction in the retained entry-module root frame and
    /// reports its actual successor, or terminal completion. Dependencies and
    /// nested frames remain outside this one-frame stepping capability.
    pub fn step_debugger_module_root_instruction(
        &mut self,
    ) -> Result<VmDebuggerExecutionState, RuntimeError> {
        self.resume_debugger_module_inner(Some(InterpreterSuspensionPoint::AfterRootInstruction))
    }

    fn nested_debugger_target_generation(
        code: &Bytecode,
        code_unit_ordinal: u32,
        bytecode_offset: u32,
    ) -> Result<u64, RuntimeError> {
        fn find_code_unit(code: &Bytecode, ordinal: u32) -> Option<&Bytecode> {
            if code.debugger_code_unit_ordinal == Some(ordinal) {
                return Some(code);
            }
            code.child_code_units()
                .find_map(|child| find_code_unit(child, ordinal))
        }
        let target = find_code_unit(code, code_unit_ordinal).ok_or(RuntimeError::Unsupported(
            "nested debugger code unit is not installed in this program",
        ))?;
        let generation = code
            .debugger_program_generation
            .ok_or(RuntimeError::Unsupported(
                "nested debugger root has no installed program generation",
            ))?;
        if target.debugger_program_generation != Some(generation) {
            return Err(RuntimeError::Unsupported(
                "nested debugger code unit belongs to another program generation",
            ));
        }
        if code_unit_ordinal == 0
            || !target
                .instructions()
                .any(|instruction| instruction.offset == bytecode_offset as usize)
        {
            return Err(RuntimeError::Unsupported(
                "nested debugger target is not an inner instruction boundary",
            ));
        }
        if target.instructions().any(|instruction| {
            matches!(
                instruction.opcode,
                Opcode::TailCall | Opcode::DirectEval | Opcode::DirectEvalSpread
            )
        }) {
            return Err(RuntimeError::Unsupported(
                "nested debugger target contains a tail call or direct eval",
            ));
        }
        Ok(generation)
    }

    /// Runs a classic root until one verified instruction of a directly
    /// called synchronous closure. The parent call site and child invocation
    /// remain VM-owned; this first native seam does not yet expose a public
    /// child debugger route or resume operation.
    pub fn execute_script_until_nested_debugger_pause(
        &mut self,
        code: &Bytecode,
        code_unit_ordinal: u32,
        bytecode_offset: u32,
    ) -> Result<VmDebuggerNestedExecutionState, RuntimeError> {
        let generation =
            Self::nested_debugger_target_generation(code, code_unit_ordinal, bytecode_offset)?;
        self.prepare_root_execution(code, false)?;
        if let Err(error) = self.prepare_global_declarations(code) {
            return self.finish_root_execution(Err(error)).map(|_| {
                unreachable!("an abrupt debugger setup cannot produce a completion value")
            });
        }
        self.debugger_nested_pause_request = Some(NestedDebuggerPauseRequest {
            program_generation: generation,
            code_unit_ordinal,
            bytecode_offset: bytecode_offset as usize,
            caller_program_generation: Some(generation),
        });
        let result = self.run_debugger_script(code, None);
        self.debugger_nested_pause_request = None;
        match result {
            DebuggerRunOutcome::Paused(continuation) => {
                self.debugger_continuation = Some(*continuation);
                let child = self
                    .debugger_nested_continuation
                    .as_ref()
                    .expect("a nested pause suspends its child before its caller");
                Ok(VmDebuggerNestedExecutionState::Paused {
                    frame_serial: child.frame_serial,
                    code_unit_ordinal: child.code_unit_ordinal,
                    bytecode_offset: u32::try_from(child.pc)
                        .expect("verified bytecode offsets fit the debugger wire range"),
                })
            }
            DebuggerRunOutcome::Completed(result) => self
                .finish_root_execution(result)
                .map(|_| VmDebuggerNestedExecutionState::Completed),
        }
    }

    /// Evaluates an authorized linked graph until its entry module calls the
    /// exact synchronous child code unit and pauses at the selected inner
    /// instruction. Dependency work and the graph remain attached to the
    /// ordinary module debugger continuation, not reconstructed on resume.
    pub fn execute_module_graph_until_nested_debugger_pause(
        &mut self,
        entry: &str,
        modules: &HashMap<String, Bytecode>,
        code_unit_ordinal: u32,
        bytecode_offset: u32,
    ) -> Result<VmDebuggerNestedExecutionState, RuntimeError> {
        self.ensure_no_debugger_continuation()?;
        let code = modules.get(entry).ok_or(RuntimeError::Unsupported(
            "nested debugger module entry is absent from the graph",
        ))?;
        if !code.module {
            return Err(RuntimeError::Unsupported(
                "nested debugger entry is not a module",
            ));
        }
        let generation =
            Self::nested_debugger_target_generation(code, code_unit_ordinal, bytecode_offset)?;
        self.run_module_graph_until_nested_debugger_pause(
            entry,
            modules,
            generation,
            code.debugger_program_generation
                .expect("verified module generation"),
            code_unit_ordinal,
            bytecode_offset,
        )
    }

    /// Pauses a direct synchronous dependency closure called by this exact
    /// entry module. Both installed generations are supplied by the owning
    /// page runtime; unrelated graph members and stale handles fail before
    /// the graph executes.
    pub fn execute_module_graph_until_linked_nested_debugger_pause(
        &mut self,
        entry: &str,
        dependency: &str,
        modules: &HashMap<String, Bytecode>,
        target: VmDebuggerLinkedPauseTarget,
    ) -> Result<VmDebuggerNestedExecutionState, RuntimeError> {
        self.ensure_no_debugger_continuation()?;
        Self::validate_linked_nested_debugger_target(entry, dependency, modules, target)?;
        self.run_module_graph_until_nested_debugger_pause(
            entry,
            modules,
            target.dependency_generation,
            target.entry_generation,
            target.code_unit_ordinal,
            target.bytecode_offset,
        )
    }

    /// Validates a closed linked target without executing or reserving a
    /// module. A page host uses this before acknowledging an arm request.
    pub fn validate_linked_nested_debugger_target(
        entry: &str,
        dependency: &str,
        modules: &HashMap<String, Bytecode>,
        target: VmDebuggerLinkedPauseTarget,
    ) -> Result<(), RuntimeError> {
        if entry == dependency {
            return Err(RuntimeError::Unsupported(
                "linked debugger child must belong to a separate dependency",
            ));
        }
        let entry_code = modules.get(entry).ok_or(RuntimeError::Unsupported(
            "linked debugger entry is absent from the graph",
        ))?;
        let dependency_code = modules.get(dependency).ok_or(RuntimeError::Unsupported(
            "linked debugger dependency is absent from the graph",
        ))?;
        if !entry_code.module
            || !dependency_code.module
            || entry_code.debugger_program_generation != Some(target.entry_generation)
            || dependency_code.debugger_program_generation != Some(target.dependency_generation)
            || !Self::module_reaches(entry, dependency, modules, &mut HashSet::new())?
        {
            return Err(RuntimeError::Unsupported(
                "linked debugger target is not an exact live entry dependency",
            ));
        }
        let generation = Self::nested_debugger_target_generation(
            dependency_code,
            target.code_unit_ordinal,
            target.bytecode_offset,
        )?;
        if generation != target.dependency_generation {
            return Err(RuntimeError::Unsupported(
                "linked debugger target belongs to another installed generation",
            ));
        }
        Ok(())
    }

    fn run_module_graph_until_nested_debugger_pause(
        &mut self,
        entry: &str,
        modules: &HashMap<String, Bytecode>,
        generation: u64,
        caller_generation: u64,
        code_unit_ordinal: u32,
        bytecode_offset: u32,
    ) -> Result<VmDebuggerNestedExecutionState, RuntimeError> {
        self.pending_throw_site = None;
        self.uncaught_throw_site = None;
        self.throw_epoch = 0;
        self.debugger_nested_pause_request = Some(NestedDebuggerPauseRequest {
            program_generation: generation,
            code_unit_ordinal,
            bytecode_offset: bytecode_offset as usize,
            caller_program_generation: Some(caller_generation),
        });
        let result = (|| {
            self.execute_module_graph_inner(entry, modules, false, false, ImportPhase::Evaluation)?;
            while self.debugger_nested_continuation.is_none() && !self.promise_jobs.is_empty() {
                self.remaining_instructions = self.config.instruction_budget;
                self.run_next_promise_job()?;
            }
            Ok::<(), RuntimeError>(())
        })();
        self.debugger_nested_pause_request = None;
        result?;
        if let Some(child) = &self.debugger_nested_continuation {
            if self.debugger_module_continuation.is_none() || self.module_graph.is_none() {
                return Err(RuntimeError::Unsupported(
                    "nested debugger module did not retain its entry graph",
                ));
            }
            return Ok(VmDebuggerNestedExecutionState::Paused {
                frame_serial: child.frame_serial,
                code_unit_ordinal: child.code_unit_ordinal,
                bytecode_offset: u32::try_from(child.pc)
                    .expect("verified bytecode offsets fit the debugger wire range"),
            });
        }
        if let Some(error) = self
            .linked_record(entry)
            .and_then(|record| record.error.clone())
        {
            return Err(RuntimeError::Thrown(error));
        }
        if self
            .linked_record(entry)
            .is_some_and(|record| record.evaluated)
        {
            Ok(VmDebuggerNestedExecutionState::Completed)
        } else {
            Err(RuntimeError::Unsupported(
                "nested debugger module target was not reached",
            ))
        }
    }

    /// Steps one instruction in the exact retained synchronous closure
    /// invocation. A returned child rejoins its waiting root at the next
    /// instruction after `Call`; the root remains paused for ordinary resume.
    pub fn step_debugger_nested_instruction(
        &mut self,
        frame_serial: u64,
    ) -> Result<VmDebuggerNestedExecutionState, RuntimeError> {
        self.continue_debugger_nested_instruction(
            frame_serial,
            Some(InterpreterSuspensionPoint::AfterRootInstruction),
        )
    }

    /// Finishes only the exact paused nested invocation, then parks its
    /// original caller at the instruction after the call. A returned serial
    /// cannot be reused to resume another invocation of the same code unit.
    pub fn resume_debugger_nested_execution(
        &mut self,
        frame_serial: u64,
    ) -> Result<VmDebuggerNestedExecutionState, RuntimeError> {
        self.continue_debugger_nested_instruction(frame_serial, None)
    }

    fn continue_debugger_nested_instruction(
        &mut self,
        frame_serial: u64,
        suspension_point: Option<InterpreterSuspensionPoint>,
    ) -> Result<VmDebuggerNestedExecutionState, RuntimeError> {
        let continuation =
            self.debugger_nested_continuation
                .take()
                .ok_or(RuntimeError::Unsupported(
                    "no debugger-paused nested frame is available",
                ))?;
        if continuation.frame_serial != frame_serial {
            self.debugger_nested_continuation = Some(continuation);
            return Err(RuntimeError::Unsupported(
                "debugger nested frame invocation is stale",
            ));
        }
        if matches!(
            continuation.code.instruction(continuation.pc),
            Some(crate::bytecode::Instruction {
                opcode: Opcode::TailCall | Opcode::DirectEval | Opcode::DirectEvalSpread,
                ..
            })
        ) {
            self.debugger_nested_continuation = Some(continuation);
            return Err(RuntimeError::Unsupported(
                "nested debugger cannot step a tail call or direct eval",
            ));
        }
        let NestedDebuggerContinuation {
            frame_serial,
            code_unit_ordinal,
            code,
            pc,
            execution,
            mut iterators,
            handlers,
        } = continuation;
        debug_assert!(self.debugger_nested_parent_execution.is_none());
        self.debugger_nested_parent_execution = Some(self.suspend_module_execution());
        self.restore_module_execution(execution);
        let previous_call_depth = self.call_depth;
        let previous_call_stack_len = self.call_stack.len();
        self.call_depth += 1;
        self.call_stack.extend(self.callee.object_id());
        let result = self.interpret(
            &code,
            &mut iterators,
            pc,
            None,
            suspension_point,
            Some((handlers, 0)),
        );
        self.call_stack.truncate(previous_call_stack_len);
        self.call_depth = previous_call_depth;
        let child_remaining_instructions = self.remaining_instructions;
        let parent = self
            .debugger_nested_parent_execution
            .take()
            .expect("nested instruction step retains its caller execution");
        match result {
            Ok(InterpreterExit::Suspend {
                pc,
                iterators,
                handlers,
            }) => {
                let execution = self.suspend_module_execution();
                self.restore_module_execution(parent);
                self.debugger_nested_continuation = Some(NestedDebuggerContinuation {
                    frame_serial,
                    code_unit_ordinal,
                    code,
                    pc,
                    execution,
                    iterators,
                    handlers,
                });
                Ok(VmDebuggerNestedExecutionState::Paused {
                    frame_serial,
                    code_unit_ordinal,
                    bytecode_offset: u32::try_from(pc)
                        .expect("verified bytecode offsets fit the debugger wire range"),
                })
            }
            result => {
                let completion = match result {
                    Ok(InterpreterExit::Return(value)) => {
                        self.finish_debugger_interpret_result(Ok(value), &mut iterators, 0, 0)
                    }
                    Ok(InterpreterExit::Yield { .. } | InterpreterExit::Await { .. }) => self
                        .finish_debugger_interpret_result(
                            Err(RuntimeError::Unsupported(
                                "nested debugger frame yielded or awaited",
                            )),
                            &mut iterators,
                            0,
                            0,
                        ),
                    Err(error) => {
                        self.finish_debugger_interpret_result(Err(error), &mut iterators, 0, 0)
                    }
                    Ok(InterpreterExit::Suspend { .. }) => {
                        unreachable!("nested suspend is handled above")
                    }
                };
                self.restore_module_execution(parent);
                self.remaining_instructions = child_remaining_instructions;
                match completion {
                    Ok(value) => {
                        if let Err(error) = self.check_string(&value) {
                            if self.debugger_module_continuation.is_some() {
                                return self.abort_nested_module_execution(error);
                            }
                            self.debugger_continuation = None;
                            return self.finish_root_execution(Err(error)).map(|_| {
                                unreachable!("failed nested completion cannot finish the root")
                            });
                        }
                        if let Some(root) = self.debugger_module_continuation.as_mut() {
                            let call = root
                                .code
                                .instruction(root.pc)
                                .expect("waiting module root retains a verified Call instruction");
                            debug_assert_eq!(call.opcode, Opcode::Call);
                            let call_base =
                                root.execution.stack.len() - call.operand.unwrap_or(0) as usize - 2;
                            root.execution.stack.truncate(call_base);
                            root.execution.stack.push(value);
                            root.execution.remaining_instructions = child_remaining_instructions;
                            root.pc += call.opcode.width();
                            return Ok(VmDebuggerNestedExecutionState::FrameReturned {
                                root_bytecode_offset: u32::try_from(root.pc)
                                    .expect("verified module offsets fit the debugger wire range"),
                            });
                        }
                        let root = self
                            .debugger_continuation
                            .as_mut()
                            .expect("nested child retains a waiting root frame");
                        let call = root
                            .code
                            .instruction(root.pc)
                            .expect("waiting root retains a verified Call instruction");
                        debug_assert_eq!(call.opcode, Opcode::Call);
                        let call_base = self.stack.len() - call.operand.unwrap_or(0) as usize - 2;
                        self.stack.truncate(call_base);
                        self.stack.push(value);
                        root.pc += call.opcode.width();
                        Ok(VmDebuggerNestedExecutionState::FrameReturned {
                            root_bytecode_offset: u32::try_from(root.pc)
                                .expect("verified bytecode offsets fit the debugger wire range"),
                        })
                    }
                    Err(error) => {
                        if self.debugger_module_continuation.is_some() {
                            return self.resume_nested_module_throw(error);
                        }
                        if !error.is_catchable() {
                            self.debugger_continuation = None;
                            return self.finish_root_execution(Err(error)).map(|_| {
                                unreachable!("uncatchable nested failure cannot finish the root")
                            });
                        }
                        let mut root = self
                            .debugger_continuation
                            .take()
                            .expect("nested child retains a waiting root frame");
                        match self.resolve_completion(
                            &root.code,
                            &mut root.handlers,
                            &mut root.iterators,
                            Completion::Throw(error),
                        ) {
                            Ok(CompletionAction::Jump(pc)) => {
                                root.pc = pc;
                                self.debugger_continuation = Some(root);
                                Ok(VmDebuggerNestedExecutionState::FrameReturned {
                                    root_bytecode_offset: u32::try_from(pc).expect(
                                        "verified bytecode offsets fit the debugger wire range",
                                    ),
                                })
                            }
                            Ok(CompletionAction::Return(value)) => self
                                .finish_root_execution(Ok(value))
                                .map(|_| VmDebuggerNestedExecutionState::Completed),
                            Ok(CompletionAction::Throw(error)) | Err(error) => self
                                .finish_root_execution(Err(error))
                                .map(|_| unreachable!("failed nested step cannot finish the root")),
                            Ok(
                                CompletionAction::Continue
                                | CompletionAction::TailRecur(_)
                                | CompletionAction::TailCall(_),
                            ) => unreachable!("throw cannot continue or become a tail call"),
                        }
                    }
                }
            }
        }
    }

    fn resume_nested_module_throw(
        &mut self,
        error: RuntimeError,
    ) -> Result<VmDebuggerNestedExecutionState, RuntimeError> {
        if !error.is_catchable() {
            return self.abort_nested_module_execution(error);
        }
        let mut root = self
            .debugger_module_continuation
            .take()
            .expect("nested module throw retains its entry root");
        let mut graph = self
            .module_graph
            .take()
            .expect("nested module throw retains its linked graph");
        self.restore_module_execution(root.execution);
        self.evaluating_linked = Some(std::mem::take(&mut graph.linked));
        let action = self.resolve_completion(
            &root.code,
            &mut root.handlers,
            &mut root.iterators,
            Completion::Throw(error),
        );
        graph.linked = self
            .evaluating_linked
            .take()
            .expect("module records return after resolving the nested throw");
        root.execution = self.suspend_module_execution();
        let record = graph
            .linked
            .get_mut(&root.module)
            .expect("the paused entry remains in its linked graph");
        record.cells = root.execution.cells.clone();
        record.suspended = true;
        self.active_module_name = root.previous_module.clone();
        let outcome = match action {
            Ok(CompletionAction::Jump(pc)) => {
                root.pc = pc;
                Ok(pc)
            }
            Ok(CompletionAction::Throw(error)) | Err(error) => Err(error),
            Ok(
                CompletionAction::Continue
                | CompletionAction::Return(_)
                | CompletionAction::TailRecur(_)
                | CompletionAction::TailCall(_),
            ) => Err(RuntimeError::Unsupported(
                "nested module throw did not produce a handler successor",
            )),
        };
        self.debugger_module_continuation = Some(root);
        self.store_module_graph(graph, false);
        match outcome {
            Ok(pc) => Ok(VmDebuggerNestedExecutionState::FrameReturned {
                root_bytecode_offset: u32::try_from(pc)
                    .expect("verified module offsets fit the debugger wire range"),
            }),
            Err(error) => self.abort_nested_module_execution(error),
        }
    }

    fn abort_nested_module_execution(
        &mut self,
        error: RuntimeError,
    ) -> Result<VmDebuggerNestedExecutionState, RuntimeError> {
        let continuation = self
            .debugger_module_continuation
            .take()
            .expect("a nested module failure retains its entry root");
        let ModuleDebuggerContinuation {
            module,
            execution,
            previous_module,
            ..
        } = continuation;
        self.restore_module_execution(execution);
        self.active_module_name = previous_module;
        let result = if error.is_catchable() {
            self.error_value(error).map(RuntimeError::Thrown)
        } else {
            Ok(error)
        };
        let mut error_root = None;
        let result = match result {
            Ok(RuntimeError::Thrown(Value::Object(id))) => match self.heap.root(id) {
                Ok(root) => {
                    error_root = Some(root);
                    Ok(RuntimeError::Thrown(Value::Object(id)))
                }
                Err(error) => Err(RuntimeError::from(error)),
            },
            other => other,
        };
        let graph = self
            .module_graph
            .as_mut()
            .expect("a nested module failure retains its linked graph");
        if let Some(root) = error_root {
            graph.roots.push(root);
        }
        let record = graph
            .linked
            .get_mut(&module)
            .expect("the paused entry remains in its linked graph");
        record.cells = std::mem::take(&mut self.cells);
        record.suspended = false;
        record.evaluating = false;
        if let Ok(RuntimeError::Thrown(value)) = &result {
            record.evaluated = true;
            record.error = Some(value.clone());
        }
        let error = result.unwrap_or_else(|error| error);
        self.finish_module_graph_execution(Err(error))
            .map(|_| unreachable!("failed nested module cannot complete successfully"))
    }

    /// Runs a classic script until the exact compiler-provided root code-unit
    /// instruction boundary. The only accepted locations are bytecode
    /// instruction starts in the root code unit; callers that want a page
    /// program identity must validate it in `BlueJsPageRuntime` first.
    ///
    /// A target of zero implements root-entry pause. A nonzero target is a
    /// genuine resumable non-root-entry safe point, but it remains limited to
    /// the one top-level interpreter frame. Module roots and child function
    /// code units are intentionally unsupported here.
    pub fn execute_script_until_debugger_pause(
        &mut self,
        code: &Bytecode,
        bytecode_offset: u32,
    ) -> Result<VmDebuggerExecutionState, RuntimeError> {
        let target = usize::try_from(bytecode_offset)
            .map_err(|_| RuntimeError::Unsupported("debugger safe-point offset is too large"))?;
        // `Bytecode::instruction` deliberately decodes at an arbitrary byte
        // offset for internal compiler diagnostics; an operand byte can have
        // the numeric shape of another opcode. A debugger target must instead
        // be one of the sequentially decoded instruction starts.
        if !code
            .instructions()
            .any(|instruction| instruction.offset == target)
        {
            return Err(RuntimeError::Unsupported(
                "debugger safe point is not a root bytecode instruction boundary",
            ));
        }

        self.prepare_root_execution(code, false)?;
        if let Err(error) = self.prepare_global_declarations(code) {
            return self.finish_root_execution(Err(error)).map(|_| {
                unreachable!("an abrupt debugger setup cannot produce a completion value")
            });
        }

        match self.run_debugger_script(code, Some(InterpreterSuspensionPoint::Offset(target))) {
            DebuggerRunOutcome::Paused(continuation) => {
                debug_assert!(self.debugger_continuation.is_none());
                self.debugger_continuation = Some(*continuation);
                Ok(VmDebuggerExecutionState::Paused { bytecode_offset })
            }
            DebuggerRunOutcome::Completed(result) => self
                .finish_root_execution(result)
                .map(|_| VmDebuggerExecutionState::Completed),
        }
    }

    /// Resumes the one debugger-paused root script to terminal completion.
    /// It does not accept another breakpoint target or preserve a persistent
    /// loop breakpoint.
    pub fn resume_debugger_execution(&mut self) -> Result<VmDebuggerExecutionState, RuntimeError> {
        self.continue_debugger_execution(None)
    }

    /// Executes exactly one root-code-unit instruction from the suspended
    /// frame, then pauses before its actual successor. Control flow may jump
    /// to an earlier offset, and a nested call runs to completion as one root
    /// instruction. A terminal instruction completes instead of fabricating
    /// another pause. The caller receives only the next source-free offset.
    pub fn step_debugger_root_instruction(
        &mut self,
    ) -> Result<VmDebuggerExecutionState, RuntimeError> {
        self.continue_debugger_execution(Some(InterpreterSuspensionPoint::AfterRootInstruction))
    }

    fn continue_debugger_execution(
        &mut self,
        suspension_point: Option<InterpreterSuspensionPoint>,
    ) -> Result<VmDebuggerExecutionState, RuntimeError> {
        let continuation = self
            .debugger_continuation
            .take()
            .ok_or(RuntimeError::Unsupported(
                "no debugger-paused root script is available",
            ))?;
        let DebuggerContinuation {
            code,
            pc,
            mut iterators,
            handlers,
        } = continuation;
        let result = match self.interpret(
            &code,
            &mut iterators,
            pc,
            None,
            suspension_point,
            Some((handlers, 0)),
        ) {
            Ok(InterpreterExit::Suspend {
                pc,
                iterators,
                handlers,
            }) => {
                self.debugger_continuation = Some(DebuggerContinuation {
                    code,
                    pc,
                    iterators,
                    handlers,
                });
                return Ok(VmDebuggerExecutionState::Paused {
                    bytecode_offset: u32::try_from(pc)
                        .expect("verified BlueJS bytecode offsets fit the debugger wire range"),
                });
            }
            Ok(InterpreterExit::Return(value)) => Ok(value),
            Ok(InterpreterExit::Yield { .. }) => Err(RuntimeError::TypeError(
                "yield requires a generator function".into(),
            )),
            Ok(InterpreterExit::Await { .. }) => Err(RuntimeError::Unsupported(
                "root debugger continuation cannot suspend for module await",
            )),
            Err(error) => Err(error),
        };
        let result = self.finish_debugger_interpret_result(result, &mut iterators, 0, 0);
        self.finish_root_execution(result)
            .map(|_| VmDebuggerExecutionState::Completed)
    }

    /// Whether an ordinary public execution entry would overwrite a paused
    /// debugger-owned root frame. This is intentionally private: no code
    /// outside the VM may obtain or operate on the saved state.
    pub(super) fn ensure_no_debugger_continuation(&self) -> Result<(), RuntimeError> {
        if self.debugger_continuation.is_some() {
            Err(RuntimeError::Unsupported(
                "a debugger-paused root script must resume before another execution starts",
            ))
        } else if self.debugger_module_continuation.is_some() {
            Err(RuntimeError::Unsupported(
                "a debugger-paused root module must resume before another execution starts",
            ))
        } else if self.debugger_nested_continuation.is_some() {
            Err(RuntimeError::Unsupported(
                "a debugger-paused nested frame must resume before another execution starts",
            ))
        } else {
            Ok(())
        }
    }

    fn run_debugger_script(
        &mut self,
        code: &Bytecode,
        suspension_point: Option<InterpreterSuspensionPoint>,
    ) -> DebuggerRunOutcome {
        let pending_base = self.pending_completions.len();
        let save_base = self.completion_saves.len();
        let mut iterators = Vec::new();
        match self.interpret(code, &mut iterators, 0, None, suspension_point, None) {
            Ok(InterpreterExit::Suspend {
                pc,
                iterators,
                handlers,
            }) => DebuggerRunOutcome::Paused(Box::new(DebuggerContinuation {
                code: code.clone(),
                pc,
                iterators,
                handlers,
            })),
            Ok(InterpreterExit::Return(value)) => {
                DebuggerRunOutcome::Completed(self.finish_debugger_interpret_result(
                    Ok(value),
                    &mut iterators,
                    pending_base,
                    save_base,
                ))
            }
            Ok(InterpreterExit::Yield { .. }) => {
                DebuggerRunOutcome::Completed(self.finish_debugger_interpret_result(
                    Err(RuntimeError::TypeError(
                        "yield requires a generator function".into(),
                    )),
                    &mut iterators,
                    pending_base,
                    save_base,
                ))
            }
            Ok(InterpreterExit::Await { .. }) => {
                DebuggerRunOutcome::Completed(self.finish_debugger_interpret_result(
                    Err(RuntimeError::Unsupported(
                        "root debugger continuation cannot suspend for module await",
                    )),
                    &mut iterators,
                    pending_base,
                    save_base,
                ))
            }
            Err(error) => DebuggerRunOutcome::Completed(self.finish_debugger_interpret_result(
                Err(error),
                &mut iterators,
                pending_base,
                save_base,
            )),
        }
    }

    /// Mirrors `Vm::run`'s terminal cleanup. The paused path intentionally
    /// bypasses it because its root frame and iterator records remain live.
    pub(super) fn finish_debugger_interpret_result(
        &mut self,
        result: Result<Value, RuntimeError>,
        iterators: &mut Vec<Value>,
        pending_base: usize,
        save_base: usize,
    ) -> Result<Value, RuntimeError> {
        if result.is_err() {
            if let Err(RuntimeError::Thrown(value)) = &result {
                self.stack.push(value.clone());
            }
            self.stack.extend(iterators.iter().cloned());
            for record in std::mem::take(iterators).into_iter().rev() {
                // IteratorClose preserves an existing throw even when return
                // throws too. Resource exhaustion retains the original limit.
                let _ = self.iterator_close(&record);
            }
        }
        self.pending_completions.truncate(pending_base);
        self.completion_saves.truncate(save_base);
        result
    }

    /// Returns heap edges held only by the Rust-owned debugger continuation.
    /// All remaining root-frame state stays in the VM's ordinary fields and
    /// is already gathered by `Vm::with_roots` before any allocating action.
    pub(super) fn debugger_continuation_references(&self) -> Vec<ObjectId> {
        let mut references: Vec<_> = self
            .debugger_continuation
            .iter()
            .flat_map(|continuation| continuation.iterators.iter())
            .filter_map(Value::object_id)
            .collect();
        if let Some(continuation) = &self.debugger_module_continuation {
            references.extend(Self::suspended_execution_references(
                &continuation.execution,
            ));
            references.extend(continuation.iterators.iter().filter_map(Value::object_id));
        }
        if let Some(continuation) = &self.debugger_nested_continuation {
            references.extend(Self::suspended_execution_references(
                &continuation.execution,
            ));
            references.extend(continuation.iterators.iter().filter_map(Value::object_id));
        }
        if let Some(execution) = &self.debugger_nested_parent_execution {
            references.extend(Self::suspended_execution_references(execution));
        }
        references
    }
}

#[cfg(test)]
#[path = "debugger/tests.rs"]
mod tests;
