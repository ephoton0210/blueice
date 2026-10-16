// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Promise records and suspended frames are traced through their live owners.

use super::*;

impl Vm {
    pub(super) fn visit_await_continuation_references(
        &self,
        module: bool,
        serial: u64,
        visit: &mut impl FnMut(ObjectId),
    ) {
        if module {
            if let Some(frame) = self.module_continuations.get(&serial) {
                Self::suspended_execution_references(&frame.execution).for_each(&mut *visit);
                frame
                    .iterators
                    .iter()
                    .filter_map(Value::object_id)
                    .for_each(visit);
            }
        } else if let Some(frame) = self.async_continuations.get(&serial) {
            visit(frame.target);
            frame.generator.into_iter().for_each(&mut *visit);
            Self::suspended_execution_references(&frame.execution).for_each(&mut *visit);
            frame
                .iterators
                .iter()
                .filter_map(Value::object_id)
                .for_each(visit);
        }
    }

    fn visit_reaction_target(target: &ReactionTarget, visit: &mut impl FnMut(ObjectId)) {
        match target {
            ReactionTarget::Native(target) => visit(*target),
            ReactionTarget::Capability(capability) => {
                [&capability.promise, &capability.resolve, &capability.reject]
                    .into_iter()
                    .filter_map(Value::object_id)
                    .for_each(visit);
            }
        }
    }

    pub(super) fn visit_promise_job_references(
        &self,
        job: &PromiseJob,
        visit: &mut impl FnMut(ObjectId),
    ) {
        match job {
            PromiseJob::Reaction {
                target,
                handler,
                value,
                ..
            } => {
                handler.object_id().into_iter().for_each(&mut *visit);
                value.object_id().into_iter().for_each(&mut *visit);
                Self::visit_reaction_target(target, visit);
            }
            PromiseJob::Thenable {
                target,
                thenable,
                then,
            } => {
                visit(*target);
                thenable.object_id().into_iter().for_each(&mut *visit);
                then.object_id().into_iter().for_each(visit);
            }
            PromiseJob::DynamicImport { target, .. } => visit(*target),
            PromiseJob::ModuleAwait {
                continuation,
                value,
                ..
            } => {
                value.object_id().into_iter().for_each(&mut *visit);
                self.visit_await_continuation_references(true, *continuation, visit);
            }
            PromiseJob::AsyncAwait {
                continuation,
                value,
                ..
            } => {
                value.object_id().into_iter().for_each(&mut *visit);
                self.visit_await_continuation_references(false, *continuation, visit);
            }
            PromiseJob::AsyncGeneratorYield {
                generator,
                target,
                result,
                value,
                ..
            } => {
                [*generator, *target, *result]
                    .into_iter()
                    .for_each(&mut *visit);
                value.object_id().into_iter().for_each(visit);
            }
            PromiseJob::AsyncGeneratorDelegate {
                generator,
                target,
                value,
                ..
            } => {
                [*generator, *target].into_iter().for_each(&mut *visit);
                value.object_id().into_iter().for_each(visit);
            }
            PromiseJob::FinalizationCleanup { callback, holdings } => {
                callback.object_id().into_iter().for_each(&mut *visit);
                holdings.object_id().into_iter().for_each(visit);
            }
            PromiseJob::HostMicrotask { callback } => {
                callback.object_id().into_iter().for_each(visit)
            }
        }
    }

    pub(super) fn visit_promise_owned_edges(&self, mut visit: impl FnMut(ObjectId, ObjectId)) {
        self.disposal_owned_edges()
            .for_each(|(owner, reference)| visit(owner, reference));
        for (&promise, record) in &self.promises {
            match &record.status {
                PromiseStatus::Fulfilled(value) | PromiseStatus::Rejected(value) => {
                    if let Some(reference) = value.object_id() {
                        visit(promise, reference);
                    }
                }
                PromiseStatus::Pending => {}
            }
            for reaction in &record.reactions {
                match reaction {
                    PromiseReaction::Then(reaction) => {
                        let mut edge = |reference| visit(promise, reference);
                        reaction
                            .on_fulfilled
                            .object_id()
                            .into_iter()
                            .for_each(&mut edge);
                        reaction
                            .on_rejected
                            .object_id()
                            .into_iter()
                            .for_each(&mut edge);
                        Self::visit_reaction_target(&reaction.target, &mut edge);
                    }
                    PromiseReaction::ModuleAwait { continuation } => self
                        .visit_await_continuation_references(
                            true,
                            *continuation,
                            &mut |reference| visit(promise, reference),
                        ),
                    PromiseReaction::AsyncAwait { continuation } => {
                        self.visit_await_continuation_references(
                            false,
                            *continuation,
                            &mut |reference| visit(promise, reference),
                        );
                        if let Some(frame) = self.async_continuations.get(continuation) {
                            // Only a retained invocation/generator keeps its Await alive.
                            visit(frame.target, promise);
                            if let Some(generator) = frame.generator {
                                visit(generator, promise);
                            }
                        }
                    }
                    PromiseReaction::AsyncGeneratorYield {
                        generator,
                        target,
                        result,
                    } => {
                        [*generator, *target, *result]
                            .into_iter()
                            .for_each(|reference| visit(promise, reference));
                    }
                    PromiseReaction::AsyncGeneratorDelegate {
                        generator, target, ..
                    } => {
                        [*generator, *target]
                            .into_iter()
                            .for_each(|reference| visit(promise, reference));
                    }
                }
            }
        }
    }

    pub(super) fn visit_pending_module_and_import_roots(&self, visit: &mut impl FnMut(ObjectId)) {
        for (&promise, record) in &self.promises {
            if record
                .reactions
                .iter()
                .any(|reaction| matches!(reaction, PromiseReaction::ModuleAwait { .. }))
            {
                visit(promise);
            }
        }
        self.module_import_waiters
            .values()
            .flatten()
            .copied()
            .for_each(&mut *visit);
        for waiter in &self.deferred_import_waiters {
            visit(waiter.promise);
            visit(waiter.namespace);
        }
    }

    pub(super) fn prune_collected_promise_state(&mut self) {
        self.prune_module_closure_referrers();
        self.prune_module_error_origins();
        self.prune_disposal_state();
        self.promises
            .retain(|promise, _| self.heap.contains(*promise));
        if self.promises.len().saturating_mul(4) < self.promises.capacity() {
            self.promises.shrink_to_fit();
        }
        self.promise_thunk_origins
            .retain(|state, _| self.heap.contains(*state));
        if self.promise_thunk_origins.len().saturating_mul(4)
            < self.promise_thunk_origins.capacity()
        {
            self.promise_thunk_origins.shrink_to_fit();
        }
        let mut modules = HashSet::new();
        let mut functions = HashSet::new();
        for record in self.promises.values() {
            for reaction in &record.reactions {
                match reaction {
                    PromiseReaction::ModuleAwait { continuation } => {
                        modules.insert(*continuation);
                    }
                    PromiseReaction::AsyncAwait { continuation } => {
                        functions.insert(*continuation);
                    }
                    _ => {}
                }
            }
        }
        for job in &self.promise_jobs {
            match job {
                PromiseJob::ModuleAwait { continuation, .. } => {
                    modules.insert(*continuation);
                }
                PromiseJob::AsyncAwait { continuation, .. } => {
                    functions.insert(*continuation);
                }
                _ => {}
            }
        }
        self.module_continuations
            .retain(|serial, _| modules.contains(serial));
        self.async_continuations
            .retain(|serial, _| functions.contains(serial));
        self.reconcile_scheduling_ownership();
    }
}
