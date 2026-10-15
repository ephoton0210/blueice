// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Promise records and suspended frames are traced through their live owners.

use super::*;

impl Vm {
    pub(super) fn await_continuation_references(&self, module: bool, serial: u64) -> Vec<ObjectId> {
        let mut references = Vec::new();
        if module {
            if let Some(frame) = self.module_continuations.get(&serial) {
                references.extend(Self::suspended_execution_references(&frame.execution));
                references.extend(frame.iterators.iter().filter_map(Value::object_id));
            }
        } else if let Some(frame) = self.async_continuations.get(&serial) {
            references.push(frame.target);
            references.extend(frame.generator);
            references.extend(Self::suspended_execution_references(&frame.execution));
            references.extend(frame.iterators.iter().filter_map(Value::object_id));
        }
        references
    }

    pub(super) fn promise_job_references(&self, job: &PromiseJob) -> Vec<ObjectId> {
        let mut references = Vec::new();
        match job {
            PromiseJob::Reaction {
                target,
                handler,
                value,
                ..
            } => {
                references.extend(handler.object_id());
                references.extend(value.object_id());
                match target {
                    ReactionTarget::Native(target) => references.push(*target),
                    ReactionTarget::Capability(capability) => {
                        for value in [&capability.promise, &capability.resolve, &capability.reject]
                        {
                            references.extend(value.object_id());
                        }
                    }
                }
            }
            PromiseJob::Thenable {
                target,
                thenable,
                then,
            } => {
                references.push(*target);
                references.extend(thenable.object_id());
                references.extend(then.object_id());
            }
            PromiseJob::DynamicImport { target, .. } => references.push(*target),
            PromiseJob::ModuleAwait {
                continuation,
                value,
                ..
            } => {
                references.extend(value.object_id());
                references.extend(self.await_continuation_references(true, *continuation));
            }
            PromiseJob::AsyncAwait {
                continuation,
                value,
                ..
            } => {
                references.extend(value.object_id());
                references.extend(self.await_continuation_references(false, *continuation));
            }
            PromiseJob::AsyncGeneratorYield {
                generator,
                target,
                result,
                value,
                ..
            } => {
                references.extend([*generator, *target, *result]);
                references.extend(value.object_id());
            }
            PromiseJob::AsyncGeneratorDelegate {
                generator,
                target,
                value,
                ..
            } => {
                references.extend([*generator, *target]);
                references.extend(value.object_id());
            }
            PromiseJob::FinalizationCleanup { callback, holdings } => {
                references.extend(callback.object_id());
                references.extend(holdings.object_id());
            }
            PromiseJob::HostMicrotask { callback } => references.extend(callback.object_id()),
        }
        references
    }

    pub(super) fn promise_owned_edges(&self) -> Vec<(ObjectId, Vec<ObjectId>)> {
        let mut edges = Vec::with_capacity(self.promises.len());
        for (&promise, record) in &self.promises {
            let mut references = Vec::new();
            match &record.status {
                PromiseStatus::Fulfilled(value) | PromiseStatus::Rejected(value) => {
                    references.extend(value.object_id())
                }
                PromiseStatus::Pending => {}
            }
            for reaction in &record.reactions {
                match reaction {
                    PromiseReaction::Then(reaction) => {
                        references.extend(reaction.on_fulfilled.object_id());
                        references.extend(reaction.on_rejected.object_id());
                        match &reaction.target {
                            ReactionTarget::Native(target) => references.push(*target),
                            ReactionTarget::Capability(capability) => {
                                for value in
                                    [&capability.promise, &capability.resolve, &capability.reject]
                                {
                                    references.extend(value.object_id());
                                }
                            }
                        }
                    }
                    PromiseReaction::ModuleAwait { continuation } => {
                        references.extend(self.await_continuation_references(true, *continuation))
                    }
                    PromiseReaction::AsyncAwait { continuation } => {
                        references.extend(self.await_continuation_references(false, *continuation));
                        if let Some(frame) = self.async_continuations.get(continuation) {
                            // A retained invocation/generator owns its suspended
                            // Await, while a wholly unreachable cycle can die.
                            edges.push((frame.target, vec![promise]));
                            if let Some(generator) = frame.generator {
                                edges.push((generator, vec![promise]));
                            }
                        }
                    }
                    PromiseReaction::AsyncGeneratorYield {
                        generator,
                        target,
                        result,
                    } => references.extend([*generator, *target, *result]),
                    PromiseReaction::AsyncGeneratorDelegate {
                        generator, target, ..
                    } => references.extend([*generator, *target]),
                }
            }
            edges.push((promise, references));
        }
        edges
    }

    pub(super) fn pending_module_and_import_roots(&self) -> Vec<ObjectId> {
        let mut roots = Vec::new();
        for (&promise, record) in &self.promises {
            if record
                .reactions
                .iter()
                .any(|reaction| matches!(reaction, PromiseReaction::ModuleAwait { .. }))
            {
                // Module evaluation is outstanding host work independently of
                // whether JavaScript retained a dynamic import's return value.
                roots.push(promise);
            }
        }
        roots.extend(self.module_import_waiters.values().flatten().copied());
        for waiter in &self.deferred_import_waiters {
            roots.extend([waiter.promise, waiter.namespace]);
        }
        roots
    }

    pub(super) fn prune_collected_promise_state(&mut self) {
        self.prune_module_error_origins();
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
    }
}
