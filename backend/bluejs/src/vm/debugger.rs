// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A bounded, root-code-unit continuation for the private native debugger.
//!
//! BlueJS ordinarily runs a page script synchronously on the embedding
//! session thread.  A debugger cannot truthfully pause an arbitrary bytecode
//! instruction merely by remembering its offset: the operand stack, handler
//! stack, active iterator records and execution-context fields are also live.
//! This module retains those states for one classic-script root frame. Nested
//! function frames still use the Rust call stack and are deliberately outside
//! this capability.

use super::*;

/// Source-free outcome of a root-code-unit debugger run.
///
/// `Paused` means no instruction at `bytecode_offset` has executed yet. The
/// VM owns the complete root interpreter frame until
/// [`Vm::resume_debugger_execution`] finishes it. A root-instruction step may
/// instead suspend the same continuation again; stack inspection and nested
/// function control remain outside this surface.
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

enum DebuggerRunOutcome {
    Paused(Box<DebuggerContinuation>),
    Completed(Result<Value, RuntimeError>),
}

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
        self.debugger_module_pause_request = Some(ModuleDebuggerPauseRequest {
            entry: entry.to_string(),
            target,
        });
        let result =
            self.execute_module_graph_inner(entry, modules, false, false, ImportPhase::Evaluation);
        self.debugger_module_pause_request = None;
        result?;
        if self.debugger_module_continuation.is_some() {
            Ok(VmDebuggerExecutionState::Paused { bytecode_offset })
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
        self.resume_debugger_module_inner()
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

        match self.run_until_debugger_pause(code, target) {
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
        } else {
            Ok(())
        }
    }

    fn run_until_debugger_pause(&mut self, code: &Bytecode, target: usize) -> DebuggerRunOutcome {
        let pending_base = self.pending_completions.len();
        let save_base = self.completion_saves.len();
        let mut iterators = Vec::new();
        match self.interpret(
            code,
            &mut iterators,
            0,
            None,
            Some(InterpreterSuspensionPoint::Offset(target)),
            None,
        ) {
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
        references
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{compile, compile_module, parse, parse_module, Value};
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
