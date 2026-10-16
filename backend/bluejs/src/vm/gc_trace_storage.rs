// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Reusable safepoint batches own admitted root and side-table edge capacity.

use super::*;

pub(super) struct PreparedGcTrace {
    pub(super) batch: crate::heap::GcTraceBatch,
    pub(super) charge: execution_payload::ExecutionPayloadCharge,
}

fn replacement_capacity<T>(values: &Vec<T>, required: usize) -> Option<usize> {
    (required > values.capacity()).then(|| required.max(values.capacity().saturating_mul(2)).max(4))
}

fn allocate_replacement<T>(
    capacity: Option<usize>,
    scope: Option<&SchedulingScope>,
) -> Result<Option<Vec<T>>, RuntimeError> {
    capacity
        .map(|capacity| {
            let mut values = Vec::new();
            values
                .try_reserve_exact(capacity)
                .map_err(|_| scope.map_or(RuntimeError::SchedulingLimit, SchedulingScope::fail))?;
            Ok(values)
        })
        .transpose()
}

impl Vm {
    pub(super) fn visit_execution_roots(&self, mut visit: impl FnMut(ObjectId)) {
        self.stack
            .iter()
            .chain(self.bindings.iter().flatten())
            .chain([&self.completion, &self.this, &self.callee, &self.new_target])
            .chain(self.arguments.iter())
            .chain(self.with_objects.iter())
            .chain(self.pending_tail_call.iter().flatten())
            .chain(self.disposables.iter().flat_map(DisposableResource::values))
            .chain(self.async_dispose_helper.iter())
            .filter_map(Value::object_id)
            .for_each(&mut visit);
        execution_payload::pending_references(&self.pending_completions, &self.completion_saves)
            .for_each(&mut visit);
        self.kept_weak_objects
            .iter()
            .chain(self.async_frame_roots.iter())
            .chain(self.home_object.iter())
            .chain(self.templates.values())
            .chain(self.joining.iter())
            .chain(self.call_stack.iter())
            .chain(self.cells.values())
            .chain(self.parameter_eval_env.iter())
            .copied()
            .for_each(&mut visit);
        [
            self.promise_prototype,
            self.map_prototype,
            self.set_prototype,
            self.weak_map_prototype,
            self.weak_set_prototype,
            self.weak_ref_prototype,
            self.finalization_registry_prototype,
            self.disposable_stack_prototype,
            self.async_disposable_stack_prototype,
        ]
        .into_iter()
        .flatten()
        .for_each(&mut visit);
        for binding in self.dynamic_eval_bindings.values().chain(
            self.dynamic_eval_outer_bindings
                .iter()
                .flat_map(|bindings| bindings.values()),
        ) {
            visit(binding.cell);
            binding.shadowed_cells.iter().copied().for_each(&mut visit);
        }
        self.visit_pending_module_and_import_roots(&mut visit);
        self.visit_debugger_continuation_references(&mut visit);
        for job in &self.promise_jobs {
            self.visit_promise_job_references(job, &mut visit);
        }
        if let Some(Err(value)) = &self.test262_done {
            value.object_id().into_iter().for_each(visit);
        }
    }

    /// Retained capacity is reusable even during terminal cleanup. Growth
    /// admits both full replacements while the old allocations coexist.
    pub(super) fn prepare_gc_trace(&mut self) -> Result<PreparedGcTrace, RuntimeError> {
        self.bind_collector_budget()?;
        let terminal_cleanup = self.check_scheduling_failure().is_err();
        let mut roots = 0usize;
        let mut edges = 0usize;
        self.visit_execution_roots(|_| roots = roots.saturating_add(1));
        self.visit_promise_owned_edges(|_, _| edges = edges.saturating_add(1));
        let mut batch = std::mem::take(&mut self.gc_trace_cache);
        let mut charge = self.charge_execution_control(batch.retained_bytes());
        let prepared = (|| {
            let root_capacity = replacement_capacity(&batch.roots, roots);
            let edge_capacity = replacement_capacity(&batch.edges, edges);
            if root_capacity.is_some() || edge_capacity.is_some() {
                let bytes = root_capacity
                    .unwrap_or(0)
                    .saturating_mul(std::mem::size_of::<ObjectId>())
                    .saturating_add(
                        edge_capacity
                            .unwrap_or(0)
                            .saturating_mul(std::mem::size_of::<(ObjectId, ObjectId)>()),
                    );
                self.admit_scheduling_work(HostSchedulingUsage {
                    payload_bytes: bytes,
                    ..Default::default()
                })?;
                let new_roots = allocate_replacement(root_capacity, self.host_scheduling.as_ref())?;
                let new_edges = allocate_replacement(edge_capacity, self.host_scheduling.as_ref())?;
                let actual = new_roots
                    .as_ref()
                    .map_or(0, host_scheduling::slots)
                    .saturating_add(new_edges.as_ref().map_or(0, host_scheduling::slots));
                let staging = self.charge_execution_control(actual);
                if let Err(error) = self.synchronize_scheduling_usage() {
                    drop(new_roots);
                    drop(new_edges);
                    drop(staging);
                    return Err(error);
                }
                if let Some(values) = new_roots {
                    batch.roots = values;
                }
                if let Some(values) = new_edges {
                    batch.edges = values;
                }
                charge.resize(if charge.enabled() {
                    batch.retained_bytes()
                } else {
                    0
                });
                drop(staging);
            } else if !terminal_cleanup {
                // Reuse needs no allocation, but normal entry must publish
                // any newly changed owners before the heap operation starts.
                self.synchronize_scheduling_usage()?;
            }
            self.visit_execution_roots(|id| batch.roots.push(id));
            self.visit_promise_owned_edges(|owner, reference| batch.edges.push((owner, reference)));
            Ok(())
        })();
        if let Err(error) = prepared {
            self.gc_trace_cache = batch;
            drop(charge);
            self.reconcile_scheduling_ownership();
            return Err(error);
        }
        self.reconcile_scheduling_ownership();
        Ok(PreparedGcTrace { batch, charge })
    }

    pub(super) fn finish_gc_trace(&mut self, charge: execution_payload::ExecutionPayloadCharge) {
        self.gc_trace_cache = self.heap.pop_scoped_roots();
        self.gc_trace_cache.roots.clear();
        self.gc_trace_cache.edges.clear();
        drop(charge);
        self.reconcile_scheduling_ownership();
    }
}

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/gc_trace_storage.rs"]
mod ownership_tests;
