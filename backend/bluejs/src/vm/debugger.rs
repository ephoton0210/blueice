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

#[derive(Clone, Copy)]
pub(super) struct NestedDebuggerPauseRequest {
    pub(super) program_generation: u64,
    pub(super) code_unit_ordinal: u32,
    pub(super) bytecode_offset: usize,
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

/// Hard native inspection budgets; a caller may request a smaller positive
/// limit but cannot raise these caps or use a zero-limit probe.
pub const VM_DEBUGGER_MAX_STACK_FRAMES: u32 = 64;
pub const VM_DEBUGGER_MAX_SCOPE_ENTRIES: u32 = 256;
pub const VM_DEBUGGER_MAX_VALUE_PAYLOAD_BYTES: usize = 4_096;

/// Lossless, handle-free data copied from one exact paused lexical slot.
/// Containers are added only after the side-effect-free heap walk is installed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VmDebuggerValuePreview {
    Undefined,
    Null,
    Bool(bool),
    NumberBits(u64),
    BigIntBytes(Vec<u8>),
    StringUnits(Vec<u16>),
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
    code_unit_ordinal: u32,
    pc: usize,
    scopes: &[Vec<u32>],
    max_entries: usize,
) -> VmDebuggerStackFrame {
    let (scope_entries, scope_truncated) = debugger_scope_entries(scopes, max_entries);
    VmDebuggerStackFrame {
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

impl Vm {
    /// Reads one currently active lexical slot without resuming the VM,
    /// looking up a name, or invoking an accessor. This first native seam
    /// copies primitives only; objects and symbols are refused in full.
    pub fn debugger_value_preview(
        &self,
        frame_serial: Option<u64>,
        frame_index: u32,
        expected_code_unit_ordinal: u32,
        expected_bytecode_offset: u32,
        entry: VmDebuggerScopeEntry,
    ) -> Result<VmDebuggerValuePreview, RuntimeError> {
        let stack = self.debugger_stack_snapshot(
            frame_serial,
            VM_DEBUGGER_MAX_STACK_FRAMES,
            VM_DEBUGGER_MAX_SCOPE_ENTRIES,
        )?;
        let frame = stack
            .frames
            .get(frame_index as usize)
            .ok_or(RuntimeError::Unsupported(
                "debugger value target is not an active paused frame",
            ))?;
        if frame.code_unit_ordinal != expected_code_unit_ordinal
            || frame.bytecode_offset != expected_bytecode_offset
            || !frame.scope_entries.contains(&entry)
        {
            return Err(RuntimeError::Unsupported(
                "debugger value target is not an active paused slot",
            ));
        }
        let (bindings, cells) =
            match frame.code_unit_ordinal {
                0 => {
                    if let Some(root) = &self.debugger_module_continuation {
                        (&root.execution.bindings, &root.execution.cells)
                    } else {
                        (&self.bindings, &self.cells)
                    }
                }
                _ => {
                    let child = self.debugger_nested_continuation.as_ref().ok_or(
                        RuntimeError::Unsupported("debugger nested value frame is unavailable"),
                    )?;
                    (&child.execution.bindings, &child.execution.cells)
                }
            };
        let slot = entry.slot_ordinal as usize;
        let value = if let Some(cell) = cells.get(&slot) {
            self.heap.get_own(*cell, "value")?
        } else {
            bindings.get(slot).cloned().flatten()
        }
        .ok_or(RuntimeError::Unsupported(
            "debugger value binding is uninitialized",
        ))?;
        match value {
            Value::Undefined => Ok(VmDebuggerValuePreview::Undefined),
            Value::Null => Ok(VmDebuggerValuePreview::Null),
            Value::Bool(value) => Ok(VmDebuggerValuePreview::Bool(value)),
            Value::Number(value) => Ok(VmDebuggerValuePreview::NumberBits(value.to_bits())),
            Value::BigInt(value) => {
                // At the boundary a positive sign byte may make the copy
                // one byte too long; the post-check rejects it atomically.
                if value.bits() > (VM_DEBUGGER_MAX_VALUE_PAYLOAD_BYTES as u64) * 8 {
                    return Err(RuntimeError::Unsupported(
                        "debugger value exceeds the payload byte budget",
                    ));
                }
                let bytes = value.to_signed_bytes_le();
                if bytes.len() > VM_DEBUGGER_MAX_VALUE_PAYLOAD_BYTES {
                    return Err(RuntimeError::Unsupported(
                        "debugger value exceeds the payload byte budget",
                    ));
                }
                Ok(VmDebuggerValuePreview::BigIntBytes(bytes))
            }
            Value::String(value) => {
                if value.byte_len() > VM_DEBUGGER_MAX_VALUE_PAYLOAD_BYTES {
                    return Err(RuntimeError::Unsupported(
                        "debugger value exceeds the payload byte budget",
                    ));
                }
                Ok(VmDebuggerValuePreview::StringUnits(
                    value.as_code_units().to_vec(),
                ))
            }
            Value::Symbol(_) | Value::Object(_) => Err(RuntimeError::Unsupported(
                "debugger value is not a supported primitive",
            )),
        }
    }

    /// Inspects only an exact paused root or nested invocation. No bytecode is
    /// executed, no heap getter is read, and no binding value is copied.
    pub fn debugger_stack_snapshot(
        &self,
        frame_serial: Option<u64>,
        max_frames: u32,
        max_scope_entries: u32,
    ) -> Result<VmDebuggerStackSnapshot, RuntimeError> {
        if !(1..=VM_DEBUGGER_MAX_STACK_FRAMES).contains(&max_frames)
            || !(1..=VM_DEBUGGER_MAX_SCOPE_ENTRIES).contains(&max_scope_entries)
        {
            return Err(RuntimeError::Unsupported(
                "debugger stack or scope limit exceeds its fixed positive budget",
            ));
        }
        let (root_code, root_pc, root_scopes) = if let Some(root) = &self.debugger_continuation {
            (&root.code, root.pc, self.active_scope_slots.as_slice())
        } else if let Some(root) = &self.debugger_module_continuation {
            (
                &root.code,
                root.pc,
                root.execution.active_scope_slots.as_slice(),
            )
        } else {
            return Err(RuntimeError::Unsupported(
                "no debugger-paused root frame is available",
            ));
        };
        let program_generation =
            root_code
                .debugger_program_generation
                .ok_or(RuntimeError::Unsupported(
                    "debugger-paused program has no installed generation",
                ))?;
        let max_entries = max_scope_entries as usize;
        let mut frames = Vec::new();
        match (frame_serial, &self.debugger_nested_continuation) {
            (Some(serial), Some(child)) if child.frame_serial == serial => {
                if child.code.debugger_program_generation != Some(program_generation) {
                    return Err(RuntimeError::Unsupported(
                        "nested debugger frame belongs to another program generation",
                    ));
                }
                frames.push(debugger_stack_frame(
                    child.code_unit_ordinal,
                    child.pc,
                    &child.execution.active_scope_slots,
                    max_entries,
                ));
                if max_frames > 1 {
                    frames.push(debugger_stack_frame(0, root_pc, root_scopes, max_entries));
                }
                Ok(VmDebuggerStackSnapshot {
                    program_generation,
                    frames,
                    stack_truncated: max_frames == 1,
                })
            }
            (None, None) => {
                frames.push(debugger_stack_frame(0, root_pc, root_scopes, max_entries));
                Ok(VmDebuggerStackSnapshot {
                    program_generation,
                    frames,
                    stack_truncated: false,
                })
            }
            _ => Err(RuntimeError::Unsupported(
                "debugger stack target is not the exact paused frame",
            )),
        }
    }

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
        self.debugger_nested_pause_request = Some(NestedDebuggerPauseRequest {
            program_generation: generation,
            code_unit_ordinal,
            bytecode_offset: bytecode_offset as usize,
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
mod tests {
    use super::*;
    use crate::{
        compile, compile_module, parse, parse_module, BlueJsProgramRegistry, BlueJsProgramV1,
        BlueJsSourceIdentity, Value,
    };
    use std::collections::HashMap;

    fn code(source: &str) -> Bytecode {
        compile(&parse(source).expect("test source parses")).expect("test source compiles")
    }

    fn non_entry_root_offset(code: &Bytecode) -> u32 {
        code.instructions()
            .map(|instruction| instruction.offset as u32)
            .find(|offset| *offset != 0)
            .expect("test source has a non-entry root instruction")
    }

    fn module_code(source: &str) -> Bytecode {
        compile_module(&parse_module(source).expect("test module parses"))
            .expect("test module compiles")
    }

    fn module_entry_offset(code: &Bytecode) -> u32 {
        code.instructions()
            .map(|instruction| instruction.offset as u32)
            .find(|offset| *offset >= code.module_evaluate_entry.unwrap())
            .expect("test module has an evaluation instruction")
    }

    fn installed_script(source: &str) -> Bytecode {
        let mut registry = BlueJsProgramRegistry::default();
        let handle = registry
            .install(
                BlueJsSourceIdentity::new("page:///nested.js", "sha256:nested").unwrap(),
                &BlueJsProgramV1::Script(parse(source).unwrap()),
            )
            .unwrap();
        registry.get(handle).unwrap().bytecode().clone()
    }

    #[test]
    fn stack_snapshot_excludes_inactive_scopes_and_never_evaluates_a_getter() {
        let program = installed_script(
            "var result = 0; function inner(a, b) { let first = a; let second = b; if (false) { let hidden = 9; } return first + second + watched.value; } result = inner(1, 2);",
        );
        let child_code = program.child_code_units().next().unwrap();
        let slot = |name: &str| {
            u32::try_from(
                child_code
                    .bindings
                    .iter()
                    .position(|binding| binding.name == name)
                    .unwrap(),
            )
            .unwrap()
        };
        let (first, second, hidden) = (slot("first"), slot("second"), slot("hidden"));
        let mut vm = Vm::default();
        vm.execute_script(&code(
            "var getterRuns = 0; var watched = { get value() { getterRuns++; return 7; } };",
        ))
        .unwrap();
        let VmDebuggerNestedExecutionState::Paused { frame_serial, .. } = vm
            .execute_script_until_nested_debugger_pause(&program, 1, 0)
            .unwrap()
        else {
            panic!("the direct child must pause");
        };
        for _ in 0..64 {
            let active = &vm
                .debugger_nested_continuation
                .as_ref()
                .unwrap()
                .execution
                .active_scope_slots;
            if active.iter().flatten().any(|slot| *slot == first)
                && active.iter().flatten().any(|slot| *slot == second)
            {
                break;
            }
            assert!(matches!(
                vm.step_debugger_nested_instruction(frame_serial).unwrap(),
                VmDebuggerNestedExecutionState::Paused { .. }
            ));
        }
        let full = vm
            .debugger_stack_snapshot(Some(frame_serial), 2, 256)
            .unwrap();
        assert_eq!(
            full.program_generation,
            program.debugger_program_generation.unwrap()
        );
        assert_eq!(full.frames.len(), 2);
        assert_eq!(full.frames[0].code_unit_ordinal, 1);
        assert_eq!(full.frames[1].code_unit_ordinal, 0);
        assert!(!full.stack_truncated);
        assert!(full.frames.iter().all(|frame| !frame.scope_truncated));
        let slots = &full.frames[0].scope_entries;
        assert!(slots.iter().any(|entry| entry.slot_ordinal == first));
        assert!(slots.iter().any(|entry| entry.slot_ordinal == second));
        assert!(!slots.iter().any(|entry| entry.slot_ordinal == hidden));
        let limited = vm
            .debugger_stack_snapshot(Some(frame_serial), 1, 1)
            .unwrap();
        assert_eq!(limited.frames.len(), 1);
        assert!(limited.stack_truncated);
        assert_eq!(limited.frames[0].scope_entries.len(), 1);
        assert!(limited.frames[0].scope_truncated);
        assert_eq!(
            vm.debugger_stack_snapshot(Some(frame_serial), 2, 256)
                .unwrap(),
            full
        );
        assert_eq!(
            vm.lookup_global_name("getterRuns").unwrap(),
            Some(Value::Number(0.0))
        );
        assert!(vm.debugger_stack_snapshot(None, 2, 256).is_err());
        assert!(vm
            .debugger_stack_snapshot(Some(frame_serial + 1), 2, 256)
            .is_err());
        for limits in [(0, 1), (65, 1), (1, 0), (1, 257)] {
            assert!(vm
                .debugger_stack_snapshot(Some(frame_serial), limits.0, limits.1)
                .is_err());
        }
        assert!(matches!(
            vm.resume_debugger_nested_execution(frame_serial).unwrap(),
            VmDebuggerNestedExecutionState::FrameReturned { .. }
        ));
        assert!(vm
            .debugger_stack_snapshot(Some(frame_serial), 2, 256)
            .is_err());
        let root = vm.debugger_stack_snapshot(None, 2, 256).unwrap();
        assert_eq!(root.frames.len(), 1);
        assert_eq!(root.frames[0].code_unit_ordinal, 0);
        assert!(!root.stack_truncated);
        assert_eq!(
            vm.lookup_global_name("getterRuns").unwrap(),
            Some(Value::Number(1.0))
        );
        vm.resume_debugger_execution().unwrap();
        assert!(vm.debugger_stack_snapshot(None, 2, 256).is_err());
    }

    fn active_entry(
        frame: &VmDebuggerStackFrame,
        code: &Bytecode,
        name: &str,
    ) -> VmDebuggerScopeEntry {
        let slot_ordinal = code
            .bindings
            .iter()
            .position(|binding| binding.name == name)
            .and_then(|slot| u32::try_from(slot).ok())
            .expect("binding has a wire-sized slot");
        *frame
            .scope_entries
            .iter()
            .find(|entry| entry.slot_ordinal == slot_ordinal)
            .expect("binding is active in the selected frame")
    }

    #[test]
    fn paused_root_previews_lossless_primitives_and_rejects_wrong_targets() {
        let program = installed_script(
            "let flag = true; let empty = null; let missing; let minus = -0; let not_a_number = NaN; let count = 123456789012345678901234567890n; let text = '\\ud800'; let too_long = 'x'.repeat(2049); let too_big = 1n << 32768n; let object = { a: 1 };",
        );
        let halt = program.instructions().last().unwrap().offset as u32;
        let mut vm = Vm::default();
        assert_eq!(
            vm.execute_script_until_debugger_pause(&program, halt)
                .unwrap(),
            VmDebuggerExecutionState::Paused {
                bytecode_offset: halt
            }
        );
        let frame = &vm.debugger_stack_snapshot(None, 1, 256).unwrap().frames[0];
        let read = |name| {
            vm.debugger_value_preview(
                None,
                0,
                frame.code_unit_ordinal,
                frame.bytecode_offset,
                active_entry(frame, &program, name),
            )
        };
        assert_eq!(read("flag").unwrap(), VmDebuggerValuePreview::Bool(true));
        assert_eq!(read("empty").unwrap(), VmDebuggerValuePreview::Null);
        assert_eq!(read("missing").unwrap(), VmDebuggerValuePreview::Undefined);
        assert_eq!(
            read("minus").unwrap(),
            VmDebuggerValuePreview::NumberBits((-0.0_f64).to_bits())
        );
        assert!(matches!(
            read("not_a_number").unwrap(),
            VmDebuggerValuePreview::NumberBits(bits) if f64::from_bits(bits).is_nan()
        ));
        assert_eq!(
            read("count").unwrap(),
            VmDebuggerValuePreview::BigIntBytes(
                num_bigint::BigInt::parse_bytes(b"123456789012345678901234567890", 10)
                    .unwrap()
                    .to_signed_bytes_le()
            )
        );
        assert_eq!(
            read("text").unwrap(),
            VmDebuggerValuePreview::StringUnits(vec![0xd800])
        );
        assert!(read("too_long").is_err());
        assert!(read("too_big").is_err());
        assert!(read("object").is_err());
        let flag = active_entry(frame, &program, "flag");
        for (serial, index, unit, offset, entry) in [
            (Some(1), 0, 0, halt, flag),
            (None, 1, 0, halt, flag),
            (None, 0, 1, halt, flag),
            (None, 0, 0, halt - 1, flag),
            (
                None,
                0,
                0,
                halt,
                VmDebuggerScopeEntry {
                    slot_ordinal: u32::MAX,
                    scope_depth: 0,
                },
            ),
        ] {
            assert!(vm
                .debugger_value_preview(serial, index, unit, offset, entry)
                .is_err());
        }
        vm.resume_debugger_execution().unwrap();
        assert!(vm.debugger_value_preview(None, 0, 0, halt, flag).is_err());
    }

    #[test]
    fn paused_nested_preview_reads_cell_backed_capture_without_running_child() {
        let program = installed_script(
            "function inner() { let captured = 17; const sink = () => captured; return captured; } inner();",
        );
        let child = program.child_code_units().next().unwrap();
        let mut vm = Vm::default();
        let VmDebuggerNestedExecutionState::Paused { frame_serial, .. } = vm
            .execute_script_until_nested_debugger_pause(&program, 1, 0)
            .unwrap()
        else {
            panic!("inner closure must pause");
        };
        let mut preview = None;
        for _ in 0..64 {
            let frame = &vm
                .debugger_stack_snapshot(Some(frame_serial), 2, 256)
                .unwrap()
                .frames[0];
            if let Some(entry) = child
                .bindings
                .iter()
                .position(|binding| binding.name == "captured")
                .and_then(|slot| {
                    if !vm
                        .debugger_nested_continuation
                        .as_ref()
                        .unwrap()
                        .execution
                        .cells
                        .contains_key(&slot)
                    {
                        return None;
                    }
                    frame
                        .scope_entries
                        .iter()
                        .find(|entry| entry.slot_ordinal == slot as u32)
                })
            {
                preview = Some(
                    vm.debugger_value_preview(
                        Some(frame_serial),
                        0,
                        frame.code_unit_ordinal,
                        frame.bytecode_offset,
                        *entry,
                    )
                    .unwrap(),
                );
                break;
            }
            vm.step_debugger_nested_instruction(frame_serial).unwrap();
        }
        assert_eq!(
            preview,
            Some(VmDebuggerValuePreview::NumberBits(17.0_f64.to_bits()))
        );
    }

    #[test]
    fn paused_module_root_preview_reads_retained_module_binding() {
        let entry = "preview/main.mjs";
        let mut registry = BlueJsProgramRegistry::default();
        let handle = registry
            .install_precompiled(
                BlueJsSourceIdentity::new(entry, "sha256:preview").unwrap(),
                module_code("let answer = 41; export const result = answer + 1;"),
            )
            .unwrap();
        let program = registry.get(handle).unwrap().bytecode().clone();
        let graph = HashMap::from([(entry.to_string(), program.clone())]);
        let mut vm = Vm::default();
        vm.execute_module_graph_until_debugger_pause(entry, &graph, module_entry_offset(&program))
            .unwrap();
        let mut preview = None;
        for _ in 0..64 {
            let frame = &vm.debugger_stack_snapshot(None, 1, 256).unwrap().frames[0];
            let slot = program
                .bindings
                .iter()
                .position(|binding| binding.name == "answer")
                .unwrap() as u32;
            if let Some(entry) = frame
                .scope_entries
                .iter()
                .find(|entry| entry.slot_ordinal == slot)
            {
                if let Ok(value) = vm.debugger_value_preview(
                    None,
                    0,
                    frame.code_unit_ordinal,
                    frame.bytecode_offset,
                    *entry,
                ) {
                    preview = Some(value);
                    break;
                }
            }
            assert!(matches!(
                vm.step_debugger_module_root_instruction().unwrap(),
                VmDebuggerExecutionState::Paused { .. }
            ));
        }
        assert_eq!(
            preview,
            Some(VmDebuggerValuePreview::NumberBits(41.0_f64.to_bits()))
        );
    }

    #[test]
    fn pauses_a_direct_inner_call_without_consuming_its_parent_call_site() {
        let code = installed_script(
            "var before = 0; var kept = {value: 7}; function inner(arg) { before = before + 1; return arg.value; } inner(kept);",
        );
        let mut vm = Vm::default();
        let state = vm
            .execute_script_until_nested_debugger_pause(&code, 1, 0)
            .unwrap();
        assert_eq!(
            state,
            VmDebuggerNestedExecutionState::Paused {
                frame_serial: 1,
                code_unit_ordinal: 1,
                bytecode_offset: 0,
            }
        );
        assert_eq!(
            vm.lookup_global_name("before").unwrap(),
            Some(Value::Number(0.0))
        );
        let kept = vm
            .lookup_global_name("kept")
            .unwrap()
            .unwrap()
            .object_id()
            .unwrap();
        let child = vm.debugger_nested_continuation.as_ref().unwrap();
        assert_eq!(child.execution.arguments[0].object_id(), Some(kept));
        assert!(vm.debugger_continuation_references().contains(&kept));
        let parent = vm.debugger_continuation.as_ref().unwrap();
        assert_eq!(
            parent.code.instruction(parent.pc).unwrap().opcode,
            Opcode::Call
        );
        assert!(
            vm.stack.len() >= 3,
            "call inputs remain on the parent stack"
        );
        assert_eq!(
            vm.execute_script(&code),
            Err(RuntimeError::Unsupported(
                "a debugger-paused root script must resume before another execution starts"
            ))
        );
    }

    #[test]
    fn nested_pause_rejects_a_deeper_call_before_the_target_executes() {
        let code = installed_script(
            "var reached = 0; function inner() { reached = reached + 1; } function outer() { inner(); } outer();",
        );
        let mut vm = Vm::default();
        assert_eq!(
            vm.execute_script_until_nested_debugger_pause(&code, 1, 0),
            Err(RuntimeError::Unsupported(
                "nested debugger pause requires a direct synchronous closure call"
            ))
        );
        assert_eq!(
            vm.lookup_global_name("reached").unwrap(),
            Some(Value::Number(0.0))
        );
        assert!(vm.debugger_nested_continuation.is_none());
        assert!(vm.debugger_continuation.is_none());
        assert!(vm.debugger_nested_pause_request.is_none());
    }

    #[test]
    fn nested_pause_rejects_constructor_entry_before_its_body() {
        let code =
            installed_script("var reached = 0; function Inner() { reached = 1; } new Inner();");
        let mut vm = Vm::default();
        assert_eq!(
            vm.execute_script_until_nested_debugger_pause(&code, 1, 0),
            Err(RuntimeError::Unsupported(
                "nested debugger pause requires a direct synchronous closure call"
            ))
        );
        assert_eq!(
            vm.lookup_global_name("reached").unwrap(),
            Some(Value::Number(0.0))
        );
        assert!(vm.debugger_nested_continuation.is_none());
        assert!(vm.debugger_continuation.is_none());
        assert!(vm.debugger_nested_pause_request.is_none());
    }

    #[test]
    fn nested_pause_does_not_capture_an_old_closure_with_the_same_ordinal() {
        let mut registry = BlueJsProgramRegistry::default();
        let first = registry
            .install(
                BlueJsSourceIdentity::new("page:///first.js", "sha256:first").unwrap(),
                &BlueJsProgramV1::Script(
                    parse("globalThis.foreignHit = 0; globalThis.foreign = function(){globalThis.foreignHit = 1;};")
                        .unwrap(),
                ),
            )
            .unwrap();
        let second = registry
            .install(
                BlueJsSourceIdentity::new("page:///second.js", "sha256:second").unwrap(),
                &BlueJsProgramV1::Script(
                    parse("function target(){globalThis.foreignHit = 99;} globalThis.foreign();")
                        .unwrap(),
                ),
            )
            .unwrap();
        let first_code = registry.get(first).unwrap().bytecode();
        let second_code = registry.get(second).unwrap().bytecode();
        assert_eq!(
            first_code
                .child_code_units()
                .next()
                .unwrap()
                .debugger_code_unit_ordinal,
            Some(1)
        );
        assert_eq!(
            second_code
                .child_code_units()
                .next()
                .unwrap()
                .debugger_code_unit_ordinal,
            Some(1)
        );
        assert_ne!(
            first_code.debugger_program_generation,
            second_code.debugger_program_generation
        );
        let mut vm = Vm::default();
        vm.execute_script(first_code).unwrap();
        assert_eq!(
            vm.execute_script_until_nested_debugger_pause(second_code, 1, 0)
                .unwrap(),
            VmDebuggerNestedExecutionState::Completed
        );
        assert_eq!(
            vm.lookup_global_name("foreignHit").unwrap(),
            Some(Value::Number(1.0))
        );
        assert!(vm.debugger_nested_continuation.is_none());
    }

    #[test]
    fn nested_instruction_steps_rejoin_the_original_call_once() {
        let code = installed_script(
            "var calls = 0; var result = 0; function inner(){var i = 0; while (i < 2) { i++; } calls++; return i + 1;} result = inner() + 1;",
        );
        let safe_offsets = code
            .child_code_units()
            .next()
            .unwrap()
            .instructions()
            .map(|instruction| instruction.offset as u32)
            .collect::<std::collections::HashSet<_>>();
        let mut vm = Vm::default();
        let VmDebuggerNestedExecutionState::Paused {
            frame_serial,
            bytecode_offset: mut previous,
            ..
        } = vm
            .execute_script_until_nested_debugger_pause(&code, 1, 0)
            .unwrap()
        else {
            panic!("the direct inner call must pause before its first instruction");
        };
        assert_eq!(
            vm.step_debugger_nested_instruction(frame_serial + 1),
            Err(RuntimeError::Unsupported(
                "debugger nested frame invocation is stale"
            ))
        );
        let mut saw_backward_successor = false;
        let mut returned = None;
        for _ in 0..128 {
            match vm.step_debugger_nested_instruction(frame_serial).unwrap() {
                VmDebuggerNestedExecutionState::Paused {
                    frame_serial: same_frame,
                    code_unit_ordinal: 1,
                    bytecode_offset,
                } => {
                    assert_eq!(same_frame, frame_serial);
                    assert!(safe_offsets.contains(&bytecode_offset));
                    saw_backward_successor |= bytecode_offset < previous;
                    previous = bytecode_offset;
                }
                VmDebuggerNestedExecutionState::FrameReturned {
                    root_bytecode_offset,
                } => {
                    returned = Some(root_bytecode_offset);
                    break;
                }
                other => panic!("unexpected nested step state: {other:?}"),
            }
        }
        assert!(
            saw_backward_successor,
            "loop steps must report their real PC"
        );
        let root_successor = returned.expect("the child must return within the step budget");
        assert_eq!(
            vm.debugger_continuation.as_ref().unwrap().pc,
            root_successor as usize
        );
        assert!(vm.debugger_nested_continuation.is_none());
        assert_eq!(
            vm.step_debugger_nested_instruction(frame_serial),
            Err(RuntimeError::Unsupported(
                "no debugger-paused nested frame is available"
            ))
        );
        assert_eq!(
            vm.resume_debugger_execution().unwrap(),
            VmDebuggerExecutionState::Completed
        );
        assert_eq!(
            vm.lookup_global_name("calls").unwrap(),
            Some(Value::Number(1.0))
        );
        assert_eq!(
            vm.lookup_global_name("result").unwrap(),
            Some(Value::Number(4.0))
        );
    }

    #[test]
    fn nested_resume_keeps_one_original_call_and_revokes_its_serial() {
        let code = installed_script(
            "var calls = 0; var result = 0; function inner(){ calls++; return 4; } result = inner() + 1;",
        );
        let mut vm = Vm::default();
        let VmDebuggerNestedExecutionState::Paused { frame_serial, .. } = vm
            .execute_script_until_nested_debugger_pause(&code, 1, 0)
            .unwrap()
        else {
            panic!("direct child must pause before its first instruction");
        };
        assert_eq!(
            vm.resume_debugger_nested_execution(frame_serial + 1),
            Err(RuntimeError::Unsupported(
                "debugger nested frame invocation is stale"
            ))
        );
        assert!(matches!(
            vm.step_debugger_nested_instruction(frame_serial),
            Ok(VmDebuggerNestedExecutionState::Paused { .. })
        ));
        let VmDebuggerNestedExecutionState::FrameReturned {
            root_bytecode_offset,
        } = vm.resume_debugger_nested_execution(frame_serial).unwrap()
        else {
            panic!("nested resume must rejoin the original root");
        };
        assert_eq!(
            vm.debugger_continuation.as_ref().unwrap().pc,
            root_bytecode_offset as usize
        );
        assert!(vm.debugger_nested_continuation.is_none());
        assert_eq!(
            vm.resume_debugger_nested_execution(frame_serial),
            Err(RuntimeError::Unsupported(
                "no debugger-paused nested frame is available"
            ))
        );
        assert_eq!(
            vm.resume_debugger_execution().unwrap(),
            VmDebuggerExecutionState::Completed
        );
        assert_eq!(
            vm.lookup_global_name("calls").unwrap(),
            Some(Value::Number(1.0))
        );
        assert_eq!(
            vm.lookup_global_name("result").unwrap(),
            Some(Value::Number(5.0))
        );
    }

    #[test]
    fn nested_throw_enters_the_original_caller_catch() {
        let code = installed_script(
            "var caught = 0; function inner(){throw 7;} try { inner(); } catch (error) { caught = error; }",
        );
        let mut vm = Vm::default();
        let VmDebuggerNestedExecutionState::Paused { frame_serial, .. } = vm
            .execute_script_until_nested_debugger_pause(&code, 1, 0)
            .unwrap()
        else {
            panic!("the inner throw must be intercepted before execution");
        };
        let mut returned = false;
        for _ in 0..32 {
            match vm.step_debugger_nested_instruction(frame_serial).unwrap() {
                VmDebuggerNestedExecutionState::Paused { .. } => {}
                VmDebuggerNestedExecutionState::FrameReturned { .. } => {
                    returned = true;
                    break;
                }
                other => panic!("unexpected throw step state: {other:?}"),
            }
        }
        assert!(returned, "the thrown value must reach the waiting caller");
        assert!(vm.debugger_nested_continuation.is_none());
        assert_eq!(
            vm.resume_debugger_execution().unwrap(),
            VmDebuggerExecutionState::Completed
        );
        assert_eq!(
            vm.lookup_global_name("caught").unwrap(),
            Some(Value::Number(7.0))
        );
    }

    #[test]
    fn nested_step_keeps_the_waiting_callers_operand_alive_through_gc() {
        let code = installed_script(
            "function inner(){var scratch = {x: 1}; return 0;} var result = ({0: 7})[inner()];",
        );
        let mut config = VmConfig::default();
        config.heap.nursery_capacity = 1;
        let mut vm = Vm::new(config).unwrap();
        let VmDebuggerNestedExecutionState::Paused { frame_serial, .. } = vm
            .execute_script_until_nested_debugger_pause(&code, 1, 0)
            .unwrap()
        else {
            panic!("the computed-key call must pause in its child");
        };
        let mut returned = false;
        for _ in 0..64 {
            match vm.step_debugger_nested_instruction(frame_serial).unwrap() {
                VmDebuggerNestedExecutionState::Paused { .. } => {}
                VmDebuggerNestedExecutionState::FrameReturned { .. } => {
                    returned = true;
                    break;
                }
                other => panic!("unexpected GC step state: {other:?}"),
            }
        }
        assert!(returned);
        assert!(vm.debugger_nested_parent_execution.is_none());
        assert_eq!(
            vm.resume_debugger_execution().unwrap(),
            VmDebuggerExecutionState::Completed
        );
        assert_eq!(
            vm.lookup_global_name("result").unwrap(),
            Some(Value::Number(7.0))
        );
    }

    #[test]
    fn nested_module_pause_retains_its_entry_graph_after_dependency_evaluation() {
        let entry = "pages/entry.mjs";
        let dependency = "pages/dep.mjs";
        let mut registry = BlueJsProgramRegistry::default();
        let dependency_handle = registry
            .install_precompiled(
                BlueJsSourceIdentity::new(dependency, "sha256:dependency").unwrap(),
                module_code("globalThis.dependencyRuns = (globalThis.dependencyRuns || 0) + 1; export const seed = 41;"),
            )
            .unwrap();
        let entry_handle = registry
            .install_precompiled(
                BlueJsSourceIdentity::new(entry, "sha256:entry").unwrap(),
                module_code("import { seed } from './dep.mjs'; globalThis.entryCalls = 0; function inner(){globalThis.entryCalls++; return seed + 1;} export const answer = inner();"),
            )
            .unwrap();
        let graph = HashMap::from([
            (
                dependency.to_string(),
                registry.get(dependency_handle).unwrap().bytecode().clone(),
            ),
            (
                entry.to_string(),
                registry.get(entry_handle).unwrap().bytecode().clone(),
            ),
        ]);
        let mut vm = Vm::default();
        assert_eq!(
            vm.execute_module_graph_until_nested_debugger_pause(entry, &graph, 1, 0)
                .unwrap(),
            VmDebuggerNestedExecutionState::Paused {
                frame_serial: 1,
                code_unit_ordinal: 1,
                bytecode_offset: 0,
            }
        );
        assert_eq!(
            vm.lookup_global_name("dependencyRuns").unwrap(),
            Some(Value::Number(1.0))
        );
        assert_eq!(
            vm.lookup_global_name("entryCalls").unwrap(),
            Some(Value::Number(0.0))
        );
        assert!(vm.module_graph.is_some());
        assert!(vm.linked_record(dependency).unwrap().evaluated);
        let parent = vm.debugger_module_continuation.as_ref().unwrap();
        assert_eq!(parent.module, entry);
        assert_eq!(
            parent.code.instruction(parent.pc).unwrap().opcode,
            Opcode::Call
        );
        assert!(vm.debugger_nested_continuation.is_some());
        assert!(matches!(
            vm.execute_script(&code("1 + 1")),
            Err(RuntimeError::Unsupported(_))
        ));
        let safe_offsets = graph[entry]
            .child_code_units()
            .next()
            .unwrap()
            .instructions()
            .map(|instruction| instruction.offset as u32)
            .collect::<std::collections::HashSet<_>>();
        let mut returned = None;
        for _ in 0..128 {
            match vm.step_debugger_nested_instruction(1).unwrap() {
                VmDebuggerNestedExecutionState::Paused {
                    frame_serial: 1,
                    code_unit_ordinal: 1,
                    bytecode_offset,
                } => assert!(safe_offsets.contains(&bytecode_offset)),
                VmDebuggerNestedExecutionState::FrameReturned {
                    root_bytecode_offset,
                } => {
                    returned = Some(root_bytecode_offset);
                    break;
                }
                other => panic!("unexpected module child step: {other:?}"),
            }
        }
        assert_eq!(
            vm.debugger_module_continuation.as_ref().unwrap().pc,
            returned.expect("module child must return within its step budget") as usize
        );
        assert!(vm.debugger_nested_continuation.is_none());
        assert!(vm.module_graph.is_some());
        assert_eq!(
            vm.resume_debugger_module_execution().unwrap(),
            VmDebuggerExecutionState::Completed
        );
        assert!(vm.linked_record(entry).unwrap().evaluated);
        assert_eq!(
            vm.lookup_global_name("dependencyRuns").unwrap(),
            Some(Value::Number(1.0))
        );
        assert_eq!(
            vm.lookup_global_name("entryCalls").unwrap(),
            Some(Value::Number(1.0))
        );
    }

    #[test]
    fn unhandled_nested_module_throw_releases_both_debugger_frames() {
        let entry = "pages/throwing.mjs";
        let mut registry = BlueJsProgramRegistry::default();
        let handle = registry
            .install_precompiled(
                BlueJsSourceIdentity::new(entry, "sha256:throwing").unwrap(),
                module_code("function inner(){throw 7;} export const answer = inner();"),
            )
            .unwrap();
        let graph = HashMap::from([(
            entry.to_string(),
            registry.get(handle).unwrap().bytecode().clone(),
        )]);
        let mut vm = Vm::default();
        let VmDebuggerNestedExecutionState::Paused { frame_serial, .. } = vm
            .execute_module_graph_until_nested_debugger_pause(entry, &graph, 1, 0)
            .unwrap()
        else {
            panic!("the throwing child must pause before its body");
        };
        let mut thrown = None;
        for _ in 0..32 {
            match vm.step_debugger_nested_instruction(frame_serial) {
                Ok(VmDebuggerNestedExecutionState::Paused { .. }) => {}
                Err(error) => {
                    thrown = Some(error);
                    break;
                }
                other => panic!("unexpected nested module throw result: {other:?}"),
            }
        }
        assert_eq!(thrown, Some(RuntimeError::Thrown(Value::Number(7.0))));
        assert!(vm.debugger_module_continuation.is_none());
        assert!(vm.debugger_nested_continuation.is_none());
        let record = vm.linked_record(entry).unwrap();
        assert!(!record.suspended);
        assert_eq!(record.error, Some(Value::Number(7.0)));
        assert_eq!(
            vm.execute_script(&code("1 + 1")).unwrap(),
            Value::Number(2.0)
        );
    }

    #[test]
    fn nested_module_throw_resumes_its_original_catch_and_finally() {
        let entry = "pages/catching.mjs";
        let mut registry = BlueJsProgramRegistry::default();
        let handle = registry
            .install_precompiled(
                BlueJsSourceIdentity::new(entry, "sha256:catching").unwrap(),
                module_code("function inner(){throw 7;} globalThis.caught = 0; globalThis.finallyRuns = 0; try { inner(); } catch (error) { globalThis.caught = error; } finally { globalThis.finallyRuns++; } export const answer = globalThis.caught;"),
            )
            .unwrap();
        let graph = HashMap::from([(
            entry.to_string(),
            registry.get(handle).unwrap().bytecode().clone(),
        )]);
        let mut vm = Vm::default();
        let VmDebuggerNestedExecutionState::Paused { frame_serial, .. } = vm
            .execute_module_graph_until_nested_debugger_pause(entry, &graph, 1, 0)
            .unwrap()
        else {
            panic!("the throwing module child must pause before execution");
        };
        let mut returned = false;
        for _ in 0..32 {
            match vm.step_debugger_nested_instruction(frame_serial).unwrap() {
                VmDebuggerNestedExecutionState::Paused { .. } => {}
                VmDebuggerNestedExecutionState::FrameReturned { .. } => {
                    returned = true;
                    break;
                }
                other => panic!("unexpected module handler state: {other:?}"),
            }
        }
        assert!(returned, "the throw must rejoin the saved module handler");
        assert!(vm.debugger_nested_continuation.is_none());
        assert!(vm.debugger_module_continuation.is_some());
        assert_eq!(
            vm.resume_debugger_module_execution().unwrap(),
            VmDebuggerExecutionState::Completed
        );
        let record = vm.linked_record(entry).unwrap();
        assert!(record.evaluated);
        assert!(record.error.is_none());
        assert_eq!(
            vm.lookup_global_name("caught").unwrap(),
            Some(Value::Number(7.0))
        );
        assert_eq!(
            vm.lookup_global_name("finallyRuns").unwrap(),
            Some(Value::Number(1.0))
        );
    }

    #[test]
    fn nested_module_step_preserves_async_dependency_and_entry_await() {
        let entry = "async-nested/entry.mjs";
        let dependency = "async-nested/dep.mjs";
        let mut registry = BlueJsProgramRegistry::default();
        let dependency_handle = registry
            .install_precompiled(
                BlueJsSourceIdentity::new(dependency, "sha256:async-dependency").unwrap(),
                module_code("globalThis.dependencyRuns = (globalThis.dependencyRuns || 0) + 1; await Promise.resolve(); export const seed = 41;"),
            )
            .unwrap();
        let entry_handle = registry
            .install_precompiled(
                BlueJsSourceIdentity::new(entry, "sha256:async-entry").unwrap(),
                module_code("import { seed } from './dep.mjs'; globalThis.entryRuns = (globalThis.entryRuns || 0) + 1; globalThis.innerRuns = 0; function inner(){globalThis.innerRuns++; return seed + 1;} export const answer = inner(); await Promise.resolve(); globalThis.afterAwait = 1;"),
            )
            .unwrap();
        let graph = HashMap::from([
            (
                dependency.to_string(),
                registry.get(dependency_handle).unwrap().bytecode().clone(),
            ),
            (
                entry.to_string(),
                registry.get(entry_handle).unwrap().bytecode().clone(),
            ),
        ]);
        let mut vm = Vm::default();
        let VmDebuggerNestedExecutionState::Paused { frame_serial, .. } = vm
            .execute_module_graph_until_nested_debugger_pause(entry, &graph, 1, 0)
            .unwrap()
        else {
            panic!("async dependency must settle before entry child pause");
        };
        assert_eq!(
            vm.lookup_global_name("dependencyRuns").unwrap(),
            Some(Value::Number(1.0))
        );
        assert_eq!(
            vm.lookup_global_name("entryRuns").unwrap(),
            Some(Value::Number(1.0))
        );
        assert_eq!(
            vm.lookup_global_name("innerRuns").unwrap(),
            Some(Value::Number(0.0))
        );
        let mut returned = false;
        for _ in 0..128 {
            match vm.step_debugger_nested_instruction(frame_serial).unwrap() {
                VmDebuggerNestedExecutionState::Paused { .. } => {}
                VmDebuggerNestedExecutionState::FrameReturned { .. } => {
                    returned = true;
                    break;
                }
                other => panic!("unexpected async module child step: {other:?}"),
            }
        }
        assert!(returned);
        assert_eq!(
            vm.resume_debugger_module_execution().unwrap(),
            VmDebuggerExecutionState::Completed
        );
        assert!(vm.linked_record(entry).unwrap().evaluated);
        assert!(vm.linked_record(dependency).unwrap().evaluated);
        assert!(vm.module_continuations.is_empty());
        assert!(vm.promise_jobs.is_empty());
        for (name, expected) in [
            ("dependencyRuns", 1.0),
            ("entryRuns", 1.0),
            ("innerRuns", 1.0),
            ("afterAwait", 1.0),
        ] {
            assert_eq!(
                vm.lookup_global_name(name).unwrap(),
                Some(Value::Number(expected)),
                "{name} must run exactly once"
            );
        }
    }

    #[test]
    fn nested_module_step_budget_failure_clears_frames_without_completion() {
        let entry = "budget/entry.mjs";
        let mut registry = BlueJsProgramRegistry::default();
        let handle = registry
            .install_precompiled(
                BlueJsSourceIdentity::new(entry, "sha256:budget").unwrap(),
                module_code("function inner(){while (true) {}} export const answer = inner();"),
            )
            .unwrap();
        let graph = HashMap::from([(
            entry.to_string(),
            registry.get(handle).unwrap().bytecode().clone(),
        )]);
        let config = VmConfig {
            instruction_budget: 256,
            ..VmConfig::default()
        };
        let mut vm = Vm::new(config).unwrap();
        let VmDebuggerNestedExecutionState::Paused { frame_serial, .. } = vm
            .execute_module_graph_until_nested_debugger_pause(entry, &graph, 1, 0)
            .unwrap()
        else {
            panic!("the loop must pause before its first instruction");
        };
        let mut failure = None;
        for _ in 0..512 {
            match vm.step_debugger_nested_instruction(frame_serial) {
                Ok(VmDebuggerNestedExecutionState::Paused { .. }) => {}
                Err(error) => {
                    failure = Some(error);
                    break;
                }
                other => panic!("loop cannot complete its nested frame: {other:?}"),
            }
        }
        assert_eq!(failure, Some(RuntimeError::InstructionLimit));
        assert!(vm.debugger_nested_continuation.is_none());
        assert!(vm.debugger_module_continuation.is_none());
        assert!(vm.debugger_nested_parent_execution.is_none());
        let record = vm.linked_record(entry).unwrap();
        assert!(!record.evaluated && !record.suspended && !record.evaluating);
        assert!(record.error.is_none());
        assert_eq!(
            vm.execute_script(&code("1 + 1")).unwrap(),
            Value::Number(2.0)
        );
    }

    #[test]
    fn root_safe_point_preserves_operand_and_global_state_until_resume() {
        let program = code("globalThis.before = 1; globalThis.after = 2;");
        let offset = non_entry_root_offset(&program);
        let mut vm = Vm::default();

        assert_eq!(
            vm.execute_script_until_debugger_pause(&program, offset)
                .unwrap(),
            VmDebuggerExecutionState::Paused {
                bytecode_offset: offset
            }
        );
        assert_eq!(
            vm.execute_script(&code("globalThis.before + globalThis.after")),
            Err(RuntimeError::Unsupported(
                "a debugger-paused root script must resume before another execution starts"
            ))
        );
        let mut modules = HashMap::new();
        modules.insert(
            "blocked.mjs".to_string(),
            compile_module(&parse_module("export const blocked = 1;").unwrap()).unwrap(),
        );
        assert_eq!(
            vm.execute_module_graph("blocked.mjs", &modules),
            Err(RuntimeError::Unsupported(
                "a debugger-paused root script must resume before another execution starts"
            ))
        );
        assert_eq!(
            vm.resume_debugger_execution().unwrap(),
            VmDebuggerExecutionState::Completed
        );
        assert_eq!(
            vm.execute_script(&code("globalThis.before + globalThis.after"))
                .unwrap(),
            Value::Number(3.0)
        );
    }

    #[test]
    fn rejects_non_boundary_and_double_resume_without_destroying_a_realm() {
        let code = code("globalThis.value = 1;");
        let mut vm = Vm::default();
        let non_boundary = (0..code.bytes().len())
            .find(|candidate| {
                !code
                    .instructions()
                    .any(|instruction| instruction.offset == *candidate)
            })
            .expect("fixture contains an instruction operand byte");
        assert!(matches!(
            vm.execute_script_until_debugger_pause(&code, non_boundary as u32),
            Err(RuntimeError::Unsupported(
                "debugger safe point is not a root bytecode instruction boundary"
            ))
        ));
        assert_eq!(
            vm.execute_script_until_debugger_pause(&code, 0).unwrap(),
            VmDebuggerExecutionState::Paused { bytecode_offset: 0 }
        );
        assert_eq!(
            vm.resume_debugger_execution().unwrap(),
            VmDebuggerExecutionState::Completed
        );
        assert!(matches!(
            vm.resume_debugger_execution(),
            Err(RuntimeError::Unsupported(
                "no debugger-paused root script is available"
            ))
        ));
    }

    #[test]
    fn root_step_preserves_one_continuation_across_branches_and_loop_hits() {
        let program =
            code("let index = 0; while (index < 2) { index++; } globalThis.steppedResult = index;");
        let mut vm = Vm::default();
        assert_eq!(
            vm.execute_script_until_debugger_pause(&program, 0).unwrap(),
            VmDebuggerExecutionState::Paused { bytecode_offset: 0 }
        );
        let instruction_offsets: Vec<_> = program
            .instructions()
            .map(|instruction| instruction.offset as u32)
            .collect();
        let first = vm.step_debugger_root_instruction().unwrap();
        assert_eq!(
            first,
            VmDebuggerExecutionState::Paused {
                bytecode_offset: instruction_offsets[1],
            }
        );
        assert!(matches!(
            vm.execute_script(&code("globalThis.forbidden = true;")),
            Err(RuntimeError::Unsupported(
                "a debugger-paused root script must resume before another execution starts"
            ))
        ));
        let mut offsets = vec![0, instruction_offsets[1]];
        let mut completed = false;
        for _ in 0..256 {
            match vm.step_debugger_root_instruction().unwrap() {
                VmDebuggerExecutionState::Paused { bytecode_offset } => {
                    assert!(instruction_offsets.contains(&bytecode_offset));
                    offsets.push(bytecode_offset);
                }
                VmDebuggerExecutionState::Completed => {
                    completed = true;
                    break;
                }
            }
        }
        assert!(
            completed,
            "bounded root steps must reach terminal completion"
        );
        assert!(
            offsets
                .iter()
                .collect::<std::collections::HashSet<_>>()
                .len()
                < offsets.len(),
            "a loop must revisit at least one real root instruction"
        );
        assert_eq!(
            vm.execute_script(&code("globalThis.steppedResult"))
                .unwrap(),
            Value::Number(2.0)
        );
        assert!(matches!(
            vm.step_debugger_root_instruction(),
            Err(RuntimeError::Unsupported(
                "no debugger-paused root script is available"
            ))
        ));
    }

    #[test]
    fn root_step_can_resume_to_completion_without_restarting_the_script() {
        let program = code("globalThis.once = (globalThis.once || 0) + 1;");
        let mut vm = Vm::default();
        assert_eq!(
            vm.execute_script_until_debugger_pause(&program, 0).unwrap(),
            VmDebuggerExecutionState::Paused { bytecode_offset: 0 }
        );
        assert!(matches!(
            vm.step_debugger_root_instruction().unwrap(),
            VmDebuggerExecutionState::Paused { .. }
        ));
        assert_eq!(
            vm.resume_debugger_execution().unwrap(),
            VmDebuggerExecutionState::Completed
        );
        assert_eq!(
            vm.execute_script(&code("globalThis.once")).unwrap(),
            Value::Number(1.0)
        );
    }

    #[test]
    fn paused_module_entry_resumes_a_linked_graph_without_replaying_dependencies() {
        let entry = "pages/entry.mjs";
        let dependency = "pages/dependency.mjs";
        let entry_code = compile_module(
            &parse_module("import { answer } from './dependency.mjs'; globalThis.entryRuns = (globalThis.entryRuns || 0) + 1; export const result = answer + 1;").unwrap(),
        )
        .unwrap();
        let dependency_code = compile_module(
            &parse_module("globalThis.dependencyRuns = (globalThis.dependencyRuns || 0) + 1; export const answer = 41;").unwrap(),
        )
        .unwrap();
        let target = entry_code
            .instructions()
            .map(|instruction| instruction.offset as u32)
            .find(|offset| *offset >= entry_code.module_evaluate_entry.unwrap())
            .unwrap();
        let modules = HashMap::from([
            (entry.to_string(), entry_code),
            (dependency.to_string(), dependency_code),
        ]);
        let mut vm = Vm::default();
        assert_eq!(
            vm.execute_module_graph_until_debugger_pause(entry, &modules, target)
                .unwrap(),
            VmDebuggerExecutionState::Paused {
                bytecode_offset: target
            }
        );
        let continuation = vm.debugger_module_continuation.as_ref().unwrap();
        assert_eq!(continuation.module, entry);
        assert_eq!(continuation.pc, target as usize);
        assert_eq!(
            continuation.execution.active_module_name.as_deref(),
            Some(entry)
        );
        let linked = &vm.module_graph.as_ref().unwrap().linked;
        assert!(linked.get(dependency).unwrap().evaluated);
        assert!(linked.get(entry).unwrap().suspended);
        vm.with_roots(|heap| {
            heap.collect_major();
            Ok(())
        })
        .unwrap();
        assert_eq!(
            vm.execute_script(&code("globalThis.entryRuns")),
            Err(RuntimeError::Unsupported(
                "a debugger-paused root module must resume before another execution starts"
            ))
        );
        assert_eq!(
            vm.resume_debugger_module_execution().unwrap(),
            VmDebuggerExecutionState::Completed
        );
        assert_eq!(
            vm.execute_script(&code(
                "globalThis.dependencyRuns * 10 + globalThis.entryRuns"
            ))
            .unwrap(),
            Value::Number(11.0)
        );
        assert!(vm.last_module_namespace.is_some());
        vm.execute_module_graph(entry, &modules).unwrap();
        assert_eq!(
            vm.execute_script(&code(
                "globalThis.dependencyRuns * 10 + globalThis.entryRuns"
            ))
            .unwrap(),
            Value::Number(11.0)
        );
    }

    #[test]
    fn paused_module_rejects_concurrent_entry_points_and_recovers_after_throw() {
        let entry = "throwing/main.mjs";
        let program = module_code(
            "globalThis.runCount = (globalThis.runCount || 0) + 1; throw new TypeError('expected');",
        );
        let offset = module_entry_offset(&program);
        let modules = HashMap::from([(entry.to_string(), program)]);
        let mut vm = Vm::default();
        vm.execute_module_graph_until_debugger_pause(entry, &modules, offset)
            .unwrap();
        let blocked = RuntimeError::Unsupported(
            "a debugger-paused root module must resume before another execution starts",
        );
        assert_eq!(
            vm.execute_module_graph(entry, &modules),
            Err(blocked.clone())
        );
        assert_eq!(vm.execute(&code("1")), Err(blocked.clone()));
        assert_eq!(vm.run_promise_jobs(), Err(blocked.clone()));
        assert_eq!(vm.run_promise_jobs_bounded(1), Err(blocked));
        let thrown = vm.resume_debugger_module_execution().unwrap_err();
        assert!(matches!(thrown, RuntimeError::Thrown(Value::Object(_))));
        assert!(vm.debugger_module_continuation.is_none());
        let record = vm.module_graph.as_ref().unwrap().linked.get(entry).unwrap();
        assert!(record.evaluated);
        assert!(!record.evaluating && !record.suspended);
        assert_eq!(
            record.error,
            Some(match &thrown {
                RuntimeError::Thrown(value) => value.clone(),
                _ => unreachable!("checked thrown completion"),
            })
        );
        assert_eq!(vm.execute_module_graph(entry, &modules), Err(thrown));
        assert_eq!(
            vm.execute_script(&code("globalThis.runCount")).unwrap(),
            Value::Number(1.0)
        );
    }

    #[test]
    fn paused_module_resumes_through_top_level_await_and_cleans_up() {
        let entry = "awaiting/main.mjs";
        let program = module_code("globalThis.beforeAwait = 1; await Promise.resolve(42); globalThis.afterAwait = 1; export const answer = 42;");
        let offset = module_entry_offset(&program);
        let modules = HashMap::from([(entry.to_string(), program)]);
        let mut vm = Vm::default();
        vm.execute_module_graph_until_debugger_pause(entry, &modules, offset)
            .unwrap();
        assert_eq!(
            vm.resume_debugger_module_execution(),
            Ok(VmDebuggerExecutionState::Completed)
        );
        let record = vm.module_graph.as_ref().unwrap().linked.get(entry).unwrap();
        assert!(record.evaluated);
        assert!(!record.evaluating && !record.suspended);
        assert!(vm.module_continuations.is_empty());
        assert!(vm.promise_jobs.is_empty());
        assert_eq!(
            vm.execute_script(&code("globalThis.beforeAwait + globalThis.afterAwait"))
                .unwrap(),
            Value::Number(2.0)
        );
    }

    #[test]
    fn paused_module_preserves_async_dependency_order_and_rejection() {
        let entry = "async/entry.mjs";
        let dependency = "async/dependency.mjs";
        let entry_code = module_code("import { answer } from './dependency.mjs'; globalThis.entryRuns = (globalThis.entryRuns || 0) + 1; export const result = answer + 1;");
        let offset = module_entry_offset(&entry_code);
        let modules = HashMap::from([
            (entry.to_string(), entry_code),
            (dependency.to_string(), module_code("globalThis.dependencyRuns = (globalThis.dependencyRuns || 0) + 1; await Promise.resolve(); export const answer = 41;")),
        ]);
        let mut vm = Vm::default();
        assert_eq!(
            vm.execute_module_graph_until_debugger_pause(entry, &modules, offset),
            Ok(VmDebuggerExecutionState::Paused {
                bytecode_offset: offset
            })
        );
        assert_eq!(
            vm.resume_debugger_module_execution(),
            Ok(VmDebuggerExecutionState::Completed)
        );
        assert_eq!(
            vm.execute_script(&code(
                "globalThis.dependencyRuns * 10 + globalThis.entryRuns"
            ))
            .unwrap(),
            Value::Number(11.0)
        );

        let rejection = "async/reject.mjs";
        let rejected_code =
            module_code("await Promise.resolve(); throw new TypeError('expected');");
        let rejected_offset = module_entry_offset(&rejected_code);
        let rejected_modules = HashMap::from([(rejection.to_string(), rejected_code)]);
        let mut rejected_vm = Vm::default();
        rejected_vm
            .execute_module_graph_until_debugger_pause(
                rejection,
                &rejected_modules,
                rejected_offset,
            )
            .unwrap();
        let error = rejected_vm.resume_debugger_module_execution().unwrap_err();
        assert!(matches!(error, RuntimeError::Thrown(Value::Object(_))));
        let record = rejected_vm
            .module_graph
            .as_ref()
            .unwrap()
            .linked
            .get(rejection)
            .unwrap();
        assert!(record.evaluated);
        assert!(!record.evaluating && !record.suspended);
        assert!(rejected_vm.module_continuations.is_empty());
        assert!(rejected_vm.promise_jobs.is_empty());
        assert_eq!(
            rejected_vm.execute_module_graph(rejection, &rejected_modules),
            Err(error)
        );
    }

    #[test]
    fn module_root_step_retains_the_frame_across_branches_until_completion() {
        let entry = "stepping/entry.mjs";
        let program = module_code("let index = 0; while (index < 2) { index++; } globalThis.moduleStepped = index; export const answer = index;");
        let entry_offset = module_entry_offset(&program);
        let instruction_offsets: Vec<_> = program
            .instructions()
            .map(|instruction| instruction.offset as u32)
            .filter(|offset| *offset >= entry_offset)
            .collect();
        let modules = HashMap::from([(entry.to_string(), program)]);
        let mut vm = Vm::default();
        assert_eq!(
            vm.execute_module_graph_until_debugger_pause(entry, &modules, entry_offset),
            Ok(VmDebuggerExecutionState::Paused {
                bytecode_offset: entry_offset
            })
        );
        let mut offsets = vec![entry_offset];
        let mut completed = false;
        for _ in 0..256 {
            match vm.step_debugger_module_root_instruction().unwrap() {
                VmDebuggerExecutionState::Paused { bytecode_offset } => {
                    assert!(instruction_offsets.contains(&bytecode_offset));
                    offsets.push(bytecode_offset);
                    assert!(vm.debugger_module_continuation.is_some());
                }
                VmDebuggerExecutionState::Completed => {
                    completed = true;
                    break;
                }
            }
        }
        assert!(completed, "bounded module steps must complete");
        assert!(
            offsets
                .iter()
                .collect::<std::collections::HashSet<_>>()
                .len()
                < offsets.len()
        );
        assert!(vm.debugger_module_continuation.is_none());
        assert_eq!(
            vm.execute_script(&code("globalThis.moduleStepped"))
                .unwrap(),
            Value::Number(2.0)
        );
        assert!(matches!(
            vm.step_debugger_module_root_instruction(),
            Err(RuntimeError::Unsupported(
                "no debugger-paused root module is available"
            ))
        ));
    }

    #[test]
    fn paused_continuation_iterator_edges_are_roots_at_gc_safepoints() {
        let mut vm = Vm::default();
        let iterator_record = vm.heap.alloc_object(None).unwrap();
        vm.debugger_continuation = Some(DebuggerContinuation {
            code: Bytecode::empty(),
            pc: 0,
            iterators: vec![Value::Object(iterator_record)],
            handlers: Vec::new(),
        });

        vm.with_roots(|heap| {
            heap.collect_major();
            Ok(())
        })
        .unwrap();
        assert!(
            vm.heap.contains(iterator_record),
            "a paused iterator record must remain alive across a VM GC safepoint"
        );
    }
}
