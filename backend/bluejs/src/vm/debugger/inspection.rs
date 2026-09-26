// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

impl Vm {
    /// Reads one currently active lexical slot without resuming the VM,
    /// looking up a name, or invoking an accessor. Heap-owned data are copied
    /// under fixed output budgets; unsupported shapes refuse in full.
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
        self.heap
            .debugger_value_preview(&value, self.object_prototype, self.array_prototype)
            .map_err(RuntimeError::Unsupported)
    }

    /// Inspects only an exact paused root or nested invocation. No bytecode is
    /// executed, no heap getter is read, and no binding value is copied.
    pub fn debugger_stack_snapshot(
        &self,
        frame_serial: Option<u64>,
        max_frames: u32,
        max_scope_entries: u32,
    ) -> Result<VmDebuggerStackSnapshot, RuntimeError> {
        self.debugger_stack_snapshot_inner(frame_serial, max_frames, max_scope_entries, false)
    }

    /// A separate native-only read for one linked-module child and its entry
    /// caller. The existing same-program snapshot must continue to refuse a
    /// child from another installed generation.
    pub fn debugger_linked_stack_snapshot(
        &self,
        frame_serial: u64,
        max_frames: u32,
        max_scope_entries: u32,
    ) -> Result<VmDebuggerStackSnapshot, RuntimeError> {
        if self.debugger_module_continuation.is_none() || self.module_graph.is_none() {
            return Err(RuntimeError::Unsupported(
                "debugger linked stack has no retained module entry graph",
            ));
        }
        let snapshot = self.debugger_stack_snapshot_inner(
            Some(frame_serial),
            max_frames,
            max_scope_entries,
            true,
        )?;
        if snapshot.frames.len() != 2
            || snapshot.frames[0].program_generation == snapshot.program_generation
            || snapshot.frames[1].program_generation != snapshot.program_generation
        {
            return Err(RuntimeError::Unsupported(
                "debugger linked stack is not a dependency child and entry caller",
            ));
        }
        Ok(snapshot)
    }

    fn debugger_stack_snapshot_inner(
        &self,
        frame_serial: Option<u64>,
        max_frames: u32,
        max_scope_entries: u32,
        allow_linked: bool,
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
                let child_generation =
                    child
                        .code
                        .debugger_program_generation
                        .ok_or(RuntimeError::Unsupported(
                            "debugger-paused child has no installed program generation",
                        ))?;
                if child_generation != program_generation && !allow_linked {
                    return Err(RuntimeError::Unsupported(
                        "nested debugger frame belongs to another program generation",
                    ));
                }
                frames.push(debugger_stack_frame(
                    child_generation,
                    child.code_unit_ordinal,
                    child.pc,
                    &child.execution.active_scope_slots,
                    max_entries,
                ));
                if max_frames > 1 {
                    frames.push(debugger_stack_frame(
                        program_generation,
                        0,
                        root_pc,
                        root_scopes,
                        max_entries,
                    ));
                }
                Ok(VmDebuggerStackSnapshot {
                    program_generation,
                    frames,
                    stack_truncated: max_frames == 1,
                })
            }
            (None, None) => {
                frames.push(debugger_stack_frame(
                    program_generation,
                    0,
                    root_pc,
                    root_scopes,
                    max_entries,
                ));
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
}
