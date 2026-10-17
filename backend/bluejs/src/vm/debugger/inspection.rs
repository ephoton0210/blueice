// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

type FrameSelection<'a> = (u64, u32, usize, &'a [Vec<u32>]);
struct StackPlan<'a> {
    program_generation: u64,
    selected: [Option<FrameSelection<'a>>; 3],
    stack_truncated: bool,
}
fn scope_entries(
    scopes: &[Vec<u32>],
    limit: usize,
) -> impl Iterator<Item = VmDebuggerScopeEntry> + '_ {
    scopes
        .iter()
        .rev()
        .enumerate()
        .flat_map(|(depth, scope)| {
            scope.iter().map(move |&slot_ordinal| VmDebuggerScopeEntry {
                slot_ordinal,
                scope_depth: u32::try_from(depth)
                    .expect("bounded interpreter scope depth fits the debugger wire"),
            })
        })
        .take(limit)
}
impl StackPlan<'_> {
    fn matches(&self, snapshot: &VmDebuggerStackSnapshot, max_entries: usize) -> bool {
        self.program_generation == snapshot.program_generation
            && self.stack_truncated == snapshot.stack_truncated
            && self.selected.iter().flatten().count() == snapshot.frames.len()
            && self.selected.iter().flatten().zip(&snapshot.frames).all(
                |(&(generation, ordinal, pc, scopes), frame)| {
                    generation == frame.program_generation
                        && ordinal == frame.code_unit_ordinal
                        && pc == frame.bytecode_offset as usize
                        && (scope_entries(scopes, max_entries + 1).count() > max_entries)
                            == frame.scope_truncated
                        && scope_entries(scopes, max_entries)
                            .eq(frame.scope_entries.iter().copied())
                },
            )
    }
}

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
    ) -> Result<VmDebuggerInspection<VmDebuggerValuePreview>, RuntimeError> {
        let stack = self.debugger_stack_plan(
            frame_serial,
            VM_DEBUGGER_MAX_STACK_FRAMES,
            VM_DEBUGGER_MAX_SCOPE_ENTRIES,
            false,
        )?;
        let frame = stack
            .selected
            .get(frame_index as usize)
            .copied()
            .flatten()
            .ok_or(RuntimeError::Unsupported(
                "debugger value target is not an active paused frame",
            ))?;
        if frame.1 != expected_code_unit_ordinal
            || frame.2 != expected_bytecode_offset as usize
            || !scope_entries(frame.3, VM_DEBUGGER_MAX_SCOPE_ENTRIES as usize)
                .any(|candidate| candidate == entry)
        {
            return Err(RuntimeError::Unsupported(
                "debugger value target is not an active paused slot",
            ));
        }
        let (bindings, cells) = match frame.1 {
            0 => {
                if let Some(root) = &self.debugger_module_continuation {
                    (&root.execution.bindings, &root.execution.cells)
                } else {
                    (&self.bindings, &self.cells)
                }
            }
            _ => {
                if let Some(child) = self
                    .debugger_nested_continuation
                    .as_ref()
                    .filter(|child| child.code_unit_ordinal == frame.1)
                {
                    (&child.execution.bindings, &child.execution.cells)
                } else if let Some(intermediate) = self
                    .debugger_nested_intermediate_continuation
                    .as_ref()
                    .filter(|intermediate| {
                        intermediate.code.debugger_code_unit_ordinal == Some(frame.1)
                    })
                {
                    (
                        &intermediate.execution.bindings,
                        &intermediate.execution.cells,
                    )
                } else {
                    return Err(RuntimeError::Unsupported(
                        "debugger nested value frame is unavailable",
                    ));
                }
            }
        };
        self.debugger_binding_preview(bindings, cells, entry)
    }

    /// Reads one retained module entry-root slot while a distinct dependency
    /// child is paused. The complete two-frame snapshot must still match;
    /// ordinary root value selectors cannot be used for this state.
    pub fn debugger_linked_value_preview(
        &self,
        frame_serial: u64,
        expected_stack: &VmDebuggerStackSnapshot,
        entry: VmDebuggerScopeEntry,
    ) -> Result<VmDebuggerInspection<VmDebuggerValuePreview>, RuntimeError> {
        let current = self.debugger_linked_stack_plan(
            frame_serial,
            VM_DEBUGGER_MAX_STACK_FRAMES,
            VM_DEBUGGER_MAX_SCOPE_ENTRIES,
        )?;
        let root_frame = current.selected[1].expect("linked plan has its entry caller");
        if !current.matches(expected_stack, VM_DEBUGGER_MAX_SCOPE_ENTRIES as usize)
            || current.stack_truncated
            || current.selected.iter().flatten().any(|frame| {
                scope_entries(frame.3, VM_DEBUGGER_MAX_SCOPE_ENTRIES as usize + 1).count()
                    > VM_DEBUGGER_MAX_SCOPE_ENTRIES as usize
            })
            || root_frame.1 != 0
            || !scope_entries(root_frame.3, VM_DEBUGGER_MAX_SCOPE_ENTRIES as usize)
                .any(|candidate| candidate == entry)
        {
            return Err(RuntimeError::Unsupported(
                "debugger linked value target is not an active entry-root slot",
            ));
        }
        let root = self
            .debugger_module_continuation
            .as_ref()
            .expect("the linked stack snapshot validated its retained module entry continuation");
        self.debugger_binding_preview(&root.execution.bindings, &root.execution.cells, entry)
    }

    fn debugger_binding_preview(
        &self,
        bindings: &[Option<Value>],
        cells: &HashMap<usize, ObjectId>,
        entry: VmDebuggerScopeEntry,
    ) -> Result<VmDebuggerInspection<VmDebuggerValuePreview>, RuntimeError> {
        let slot = entry.slot_ordinal as usize;
        if let Some(cell) = cells.get(&slot) {
            return self.heap.debugger_cell_value_preview(
                *cell,
                self.object_prototype,
                self.array_prototype,
            );
        }
        let value = bindings[slot].as_ref().ok_or(RuntimeError::Unsupported(
            "debugger value binding is uninitialized",
        ))?;
        self.heap
            .debugger_owned_value_preview(value, self.object_prototype, self.array_prototype)
    }

    /// Inspects only an exact paused root or nested invocation, including one
    /// retained intermediate classic caller when present. No bytecode is
    /// executed, no heap getter is read, and no binding value is copied.
    pub fn debugger_stack_snapshot(
        &self,
        frame_serial: Option<u64>,
        max_frames: u32,
        max_scope_entries: u32,
    ) -> Result<VmDebuggerInspection<VmDebuggerStackSnapshot>, RuntimeError> {
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
    ) -> Result<VmDebuggerInspection<VmDebuggerStackSnapshot>, RuntimeError> {
        let plan = self.debugger_linked_stack_plan(frame_serial, max_frames, max_scope_entries)?;
        self.copy_debugger_stack(plan, max_scope_entries)
    }
    fn debugger_linked_stack_plan(
        &self,
        frame_serial: u64,
        max_frames: u32,
        max_scope_entries: u32,
    ) -> Result<StackPlan<'_>, RuntimeError> {
        if self.debugger_module_continuation.is_none() || self.module_graph.is_none() {
            return Err(RuntimeError::Unsupported(
                "debugger linked stack has no retained module entry graph",
            ));
        }
        let plan =
            self.debugger_stack_plan(Some(frame_serial), max_frames, max_scope_entries, true)?;
        if plan.selected.iter().flatten().count() != 2
            || plan.selected[0].unwrap().0 == plan.program_generation
            || plan.selected[1].unwrap().0 != plan.program_generation
        {
            return Err(RuntimeError::Unsupported(
                "debugger linked stack is not a dependency child and entry caller",
            ));
        }
        Ok(plan)
    }

    /// Native-only exact three-frame read for a script child called through
    /// one separately installed synchronous loader closure. Unlike the
    /// classic read, this preserves each frame's actual program generation.
    pub fn debugger_foreign_intermediate_stack_snapshot(
        &self,
        frame_serial: u64,
        intermediate_generation: u64,
        max_frames: u32,
        max_scope_entries: u32,
    ) -> Result<VmDebuggerInspection<VmDebuggerStackSnapshot>, RuntimeError> {
        if self.debugger_continuation.is_none()
            || self.debugger_module_continuation.is_some()
            || max_frames < 3
        {
            return Err(RuntimeError::Unsupported(
                "foreign debugger stack requires a retained script caller chain",
            ));
        }
        let snapshot =
            self.debugger_stack_plan(Some(frame_serial), max_frames, max_scope_entries, true)?;
        if snapshot.selected.iter().flatten().count() != 3
            || snapshot.stack_truncated
            || snapshot.program_generation == intermediate_generation
            || snapshot.selected[0].unwrap().0 != snapshot.program_generation
            || snapshot.selected[1].unwrap().0 != intermediate_generation
            || snapshot.selected[2].unwrap().0 != snapshot.program_generation
        {
            return Err(RuntimeError::Unsupported(
                "foreign debugger stack is not the exact child, loader, and root chain",
            ));
        }
        self.copy_debugger_stack(snapshot, max_scope_entries)
    }

    fn debugger_stack_snapshot_inner(
        &self,
        frame_serial: Option<u64>,
        max_frames: u32,
        max_scope_entries: u32,
        allow_linked: bool,
    ) -> Result<VmDebuggerInspection<VmDebuggerStackSnapshot>, RuntimeError> {
        let plan =
            self.debugger_stack_plan(frame_serial, max_frames, max_scope_entries, allow_linked)?;
        self.copy_debugger_stack(plan, max_scope_entries)
    }

    fn debugger_stack_plan(
        &self,
        frame_serial: Option<u64>,
        max_frames: u32,
        max_scope_entries: u32,
        allow_linked: bool,
    ) -> Result<StackPlan<'_>, RuntimeError> {
        if !(1..=VM_DEBUGGER_MAX_STACK_FRAMES).contains(&max_frames)
            || !(1..=VM_DEBUGGER_MAX_SCOPE_ENTRIES).contains(&max_scope_entries)
        {
            return Err(RuntimeError::Unsupported(
                "debugger stack or scope limit exceeds its fixed positive budget",
            ));
        }
        let (root_code, root_pc, root_scopes) = if let Some(root) = &self.debugger_continuation {
            (&*root.code, root.pc, self.active_scope_slots.as_slice())
        } else if let Some(root) = &self.debugger_module_continuation {
            (
                &*root.code,
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
        self.check_scheduling_failure()?;
        let mut selected = [None; 3];
        let total;
        match (frame_serial, &self.debugger_nested_continuation) {
            (Some(serial), Some(child)) if child.frame_serial == serial => {
                let child_generation = child
                    .code
                    .debugger_program_generation
                    .expect("a nested pause targets stamped immutable installed bytecode");
                if child_generation != program_generation && !allow_linked {
                    return Err(RuntimeError::Unsupported(
                        "nested debugger frame belongs to another program generation",
                    ));
                }
                selected[0] = Some((
                    child_generation,
                    child.code_unit_ordinal,
                    child.pc,
                    child.execution.active_scope_slots.as_slice(),
                ));
                total = if self.debugger_nested_intermediate_continuation.is_some() {
                    3
                } else {
                    2
                };
                if max_frames > 1 {
                    if let Some(intermediate) = &self.debugger_nested_intermediate_continuation {
                        let generation =
                            intermediate
                                .code
                                .debugger_program_generation
                                .ok_or(RuntimeError::Unsupported(
                                "debugger-paused intermediate has no installed program generation",
                            ))?;
                        if generation != program_generation && !allow_linked {
                            return Err(RuntimeError::Unsupported("nested debugger intermediate frame belongs to another program generation"));
                        }
                        selected[1] = Some((
                            generation,
                            intermediate
                                .code
                                .debugger_code_unit_ordinal
                                .expect("installed intermediate code unit retains its ordinal"),
                            intermediate.pc,
                            intermediate.execution.active_scope_slots.as_slice(),
                        ));
                    } else {
                        selected[1] = Some((program_generation, 0, root_pc, root_scopes));
                    }
                }
                if max_frames > 2 && total == 3 {
                    selected[2] = Some((program_generation, 0, root_pc, root_scopes));
                }
            }
            (None, None) => {
                total = 1;
                selected[0] = Some((program_generation, 0, root_pc, root_scopes));
            }
            _ => {
                return Err(RuntimeError::Unsupported(
                    "debugger stack target is not the exact paused frame",
                ))
            }
        }
        Ok(StackPlan {
            program_generation,
            selected,
            stack_truncated: (max_frames as usize) < total,
        })
    }

    fn copy_debugger_stack(
        &self,
        plan: StackPlan<'_>,
        max_scope_entries: u32,
    ) -> Result<VmDebuggerInspection<VmDebuggerStackSnapshot>, RuntimeError> {
        let StackPlan {
            program_generation,
            selected,
            stack_truncated,
        } = plan;
        let max_entries = max_scope_entries as usize;
        let counts = selected.map(|frame| {
            frame.map_or(0, |(_, _, _, scopes)| {
                scopes
                    .iter()
                    .fold(0usize, |n, scope| n.saturating_add(scope.len()))
                    .min(max_entries)
            })
        });
        let frame_count = selected.iter().flatten().count();
        let buffers = frame_count * std::mem::size_of::<VmDebuggerStackFrame>()
            + counts.iter().sum::<usize>() * std::mem::size_of::<VmDebuggerScopeEntry>();
        let budget = self
            .host_scheduling
            .as_ref()
            .map(SchedulingScope::heap_native_budget);
        let mut storage =
            InspectionStorage::<VmDebuggerStackSnapshot>::new(budget.as_ref(), buffers)?;
        let mut frames = storage.vector(frame_count)?;
        for (index, (generation, ordinal, pc, scopes)) in selected.into_iter().flatten().enumerate()
        {
            let mut entries = storage.vector(counts[index])?;
            let truncated = scope_entries(scopes, max_entries + 1).count() > max_entries;
            entries.extend(scope_entries(scopes, max_entries));
            frames.push(VmDebuggerStackFrame {
                program_generation: generation,
                code_unit_ordinal: ordinal,
                bytecode_offset: u32::try_from(pc)
                    .expect("verified bytecode offset fits the debugger wire"),
                scope_entries: entries,
                scope_truncated: truncated,
            });
        }
        Ok(storage.finish(VmDebuggerStackSnapshot {
            program_generation,
            frames,
            stack_truncated,
        }))
    }
}
