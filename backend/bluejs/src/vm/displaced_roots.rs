// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Displaced execution roots retain admitted storage and unwind partial work.

use super::*;

impl Vm {
    /// Borrow every saved heap edge directly; tracing creates no intermediate
    /// object vector and can be shared by registered and scoped-root callers.
    pub(super) fn suspended_execution_references(
        execution: &SuspendedModuleExecution,
    ) -> impl Iterator<Item = ObjectId> + Clone + '_ {
        execution
            .stack
            .iter()
            .chain(execution.bindings.iter().flatten())
            .chain(std::iter::once(&execution.completion))
            .chain(execution.with_objects.iter())
            .chain(std::iter::once(&execution.this))
            .chain(execution.arguments.iter())
            .chain(std::iter::once(&execution.callee))
            .chain(std::iter::once(&execution.new_target))
            .chain(
                execution
                    .disposables
                    .iter()
                    .flat_map(DisposableResource::values),
            )
            .filter_map(Value::object_id)
            .chain(execution_payload::pending_references(
                &execution.pending_completions,
                &execution.completion_saves,
            ))
            .chain(execution.cells.values().copied())
            .chain(
                execution
                    .dynamic_eval_bindings
                    .values()
                    .chain(
                        execution
                            .dynamic_eval_outer_bindings
                            .iter()
                            .flat_map(|bindings| bindings.values()),
                    )
                    .flat_map(|binding| {
                        std::iter::once(binding.cell).chain(binding.shadowed_cells.iter().copied())
                    }),
            )
            .chain(execution.home_object)
    }

    /// Append transactionally. A failed append releases its own registrations
    /// and segment storage, leaving a caller's previously rooted prefix intact.
    pub(super) fn append_registered_frame_roots(
        roots: &mut ModuleRoots,
        heap: &mut Heap,
        scope: Option<&SchedulingScope>,
        references: impl IntoIterator<Item = ObjectId>,
    ) -> Result<(), RuntimeError> {
        let checkpoint = roots.len();
        for object in references {
            if let Err(error) = roots.root_object(heap, scope, object) {
                while roots.len() > checkpoint {
                    heap.unroot(roots.pop().expect("registered frame-root suffix"))
                        .expect("frame-root registration still belongs to this transaction");
                }
                return Err(error);
            }
        }
        Ok(())
    }

    pub(super) fn root_suspended_module_execution(
        &mut self,
        execution: &SuspendedModuleExecution,
    ) -> Result<ModuleRoots, RuntimeError> {
        let mut roots = ModuleRoots::default();
        Self::append_registered_frame_roots(
            &mut roots,
            &mut self.heap,
            self.host_scheduling.as_ref(),
            Self::suspended_execution_references(execution),
        )?;
        Ok(roots)
    }
}

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/displaced_root_ownership.rs"]
mod ownership_tests;
