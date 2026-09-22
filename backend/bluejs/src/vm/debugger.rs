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
/// [`Vm::resume_debugger_execution`] finishes it. This is intentionally not a
/// stepping, stack-inspection, or nested-function control surface.
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

enum DebuggerRunOutcome {
    Paused(Box<DebuggerContinuation>),
    Completed(Result<Value, RuntimeError>),
}

impl Vm {
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
    /// It does not accept another breakpoint target, which prevents callers
    /// from interpreting this one-shot continuation as a stepping interface
    /// or as a persistent loop breakpoint facility.
    pub fn resume_debugger_execution(&mut self) -> Result<VmDebuggerExecutionState, RuntimeError> {
        let continuation = self
            .debugger_continuation
            .take()
            .ok_or(RuntimeError::Unsupported(
                "no debugger-paused root script is available",
            ))?;
        let mut iterators = continuation.iterators;
        let result = match self.interpret(
            &continuation.code,
            &mut iterators,
            continuation.pc,
            None,
            None,
            Some((continuation.handlers, 0)),
        ) {
            Ok(InterpreterExit::Return(value)) => Ok(value),
            Ok(InterpreterExit::Suspend { .. }) => Err(RuntimeError::Unsupported(
                "debugger resume encountered an unexpected suspension boundary",
            )),
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
        } else {
            Ok(())
        }
    }

    fn run_until_debugger_pause(&mut self, code: &Bytecode, target: usize) -> DebuggerRunOutcome {
        let pending_base = self.pending_completions.len();
        let save_base = self.completion_saves.len();
        let mut iterators = Vec::new();
        match self.interpret(code, &mut iterators, 0, None, Some(target), None) {
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
    fn finish_debugger_interpret_result(
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
        self.debugger_continuation
            .iter()
            .flat_map(|continuation| continuation.iterators.iter())
            .filter_map(Value::object_id)
            .collect()
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
