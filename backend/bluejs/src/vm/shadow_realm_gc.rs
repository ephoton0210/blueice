// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Agent collection at explicit execution/call/checkpoint safepoints. Ordinary
//! allocating heap operations can own Rust temporaries invisible to the VM;
//! they must not initiate a second collection of other realms.

use super::*;

use super::realm_collection::RealmCollection;

impl Vm {
    pub(super) fn shadow_has_work(&self) -> bool {
        // ClearKeptObjects belongs to the document checkpoint. Its owning
        // VM must survive even after the last public realm facade is dropped.
        !self.kept_weak_objects.is_empty()
            || self
                .host_rejections
                .as_ref()
                .is_some_and(|state| !state.pending.is_empty() || state.active != 0)
            || !self.promise_jobs.is_empty()
            || !self.browser_tasks.is_empty()
            || !self.host_finalization_tasks.is_empty()
            || !self.host_timers.is_empty()
            || !self.host_animation_frames.is_empty()
            || !self.host_promises.pending.is_empty()
            || !self.module_continuations.is_empty()
            || !self.module_import_waiters.is_empty()
            || !self.deferred_import_waiters.is_empty()
            || self.debugger_continuation.is_some()
    }

    fn shadow_mark(
        &mut self,
        ignored: &HashSet<RootId>,
        extra: &[ObjectId],
        previous: Option<crate::heap::CollectorMarks>,
    ) -> Result<crate::heap::CollectorMarks, RuntimeError> {
        if let Some(marks) = previous {
            self.heap.recycle_collector_marks(marks)?;
        }
        let prepared = self.prepare_gc_trace()?;
        self.heap.push_scoped_roots(prepared.batch);
        let marked = self.heap.realm_reachable(ignored, extra);
        self.finish_gc_trace(prepared.charge);
        Ok(marked?)
    }

    fn prune_shadow_owners(&mut self, plan: &RealmCollection, live: &HashSet<ObjectId>) {
        let parent = plan
            .index_of(self.object_prototype.heap)
            .expect("collected parent");
        self.shadow_wrapped_functions
            .retain(|id, _| live.contains(id));
        self.shadow_realms.retain(|instance, record| {
            plan.is_active(record.heap)
                && (live.contains(instance)
                    || plan.keeps_shadow_instance(parent, *instance, record.heap))
        });
        // Retarget existing keys without allocating after marking. One pass
        // over survivors avoids rescanning the table for every discarded key.
        for (&instance, record) in &self.shadow_realms {
            if let Some(id) = self.shadow_realm_by_heap.get_mut(&record.heap) {
                *id = instance;
            }
        }
        self.shadow_realm_by_heap.retain(|heap, id| {
            self.shadow_realms
                .get(id)
                .is_some_and(|record| record.heap == *heap)
        });
        self.shadow_realms.release_empty();
        self.shadow_realm_by_heap.release_empty();
        self.shadow_wrapped_functions.release_empty();
    }

    fn collect_local_realm(&mut self) -> Result<(), RuntimeError> {
        self.with_roots(|heap| {
            heap.collect_major();
            Ok(())
        })
    }

    /// Trace realm ownership independently of foreign persistent pins. A live
    /// facade supplies an edge to its target; an inactive realm's globals do
    /// not bootstrap it into liveness. This collects cycles across heaps.
    pub(super) fn collect_realm_garbage(&mut self) -> Result<(), RuntimeError> {
        if self.shadow_realms.is_empty()
            && self.shadow_wrapped_functions.is_empty()
            && self.test262_realms.is_empty()
        {
            return self.collect_local_realm();
        }
        let Some(mut plan) = self.prepare_realm_collection()? else {
            // A synchronous cross-realm call still owns a borrowed VM.
            return self.collect_local_realm();
        };
        let terminal_cleanup = self.check_scheduling_failure().is_err();
        let result = self.collect_prepared_realms(&mut plan, terminal_cleanup);
        let recycled = self.recycle_realm_marks(&mut plan);
        self.recycle_realm_collection(plan);
        result?;
        recycled?;
        self.collect_local_realm()
    }

    fn recycle_realm_marks(&mut self, plan: &mut RealmCollection) -> Result<(), RuntimeError> {
        for node in &mut plan.nodes[..plan.len] {
            if let Some(marks) = node.live.take() {
                if let Some(handle) = node.handle() {
                    handle.borrow_mut().heap.recycle_collector_marks(marks)?;
                } else {
                    self.heap.recycle_collector_marks(marks)?;
                }
            }
        }
        Ok(())
    }

    fn collect_prepared_realms(
        &mut self,
        plan: &mut RealmCollection,
        terminal_cleanup: bool,
    ) -> Result<(), RuntimeError> {
        // All native discovery/edge/pin/target capacity is admitted before
        // pruning owner metadata, tracing roots or collecting either heap.
        for node in &plan.nodes[..plan.len] {
            if let Some(record) = &node.record {
                record.owners.prune();
            }
        }
        loop {
            let mut changed = false;
            // A live descendant retains the discovered access route through
            // owned and shared ancestors. First discovery chooses one route;
            // another dead alias need not retain an otherwise dead creator.
            for index in (1..plan.len).rev() {
                if plan.nodes[index].active {
                    if let Some(parent) = plan.nodes[index].parent {
                        if !plan.nodes[parent].active {
                            plan.nodes[parent].active = true;
                            changed = true;
                        }
                    }
                }
            }
            for index in 0..plan.len {
                if !plan.nodes[index].active {
                    continue;
                }
                let previous = plan.nodes[index].live.take();
                let node = &plan.nodes[index];
                let live = if let Some(handle) = node.handle() {
                    handle
                        .borrow_mut()
                        .shadow_mark(&node.ignored, &node.extra, previous)?
                } else {
                    self.shadow_mark(&node.ignored, &node.extra, previous)?
                };
                for instance in 0..plan.nodes[index].instances.len() {
                    let instance = plan.nodes[index].instances[instance];
                    if live.contains(&instance.object) {
                        if let Some(child) = plan.index_of(instance.child) {
                            if !plan.nodes[child].active {
                                plan.nodes[child].active = true;
                                changed = true;
                            }
                        }
                    }
                }
                for wrapper in 0..plan.nodes[index].wrappers.len() {
                    let edge = plan.nodes[index].wrappers[wrapper];
                    if !edge.followed && live.contains(&edge.object) {
                        if let Some(home) = plan.index_of(edge.home) {
                            plan.nodes[index].wrappers[wrapper].followed = true;
                            plan.nodes[home].active = true;
                            // Each wrapper contributes at most one target;
                            // all incoming slots were reserved during preparation.
                            assert!(
                                plan.nodes[home].extra.len() < plan.nodes[home].extra.capacity()
                            );
                            plan.nodes[home].extra.push(edge.target);
                            changed = true;
                        }
                    }
                }
                for pin in 0..plan.pins.len() {
                    let edge = &plan.pins[pin];
                    if edge.tracked
                        && !edge.followed
                        && edge.owner.heap == plan.nodes[index].heap
                        && live.contains(&edge.owner)
                    {
                        let target = edge.target;
                        let home = plan.index_of(target.heap).expect("tracked membrane target");
                        plan.pins[pin].followed = true;
                        plan.nodes[home].active = true;
                        assert!(plan.nodes[home].extra.len() < plan.nodes[home].extra.capacity());
                        plan.nodes[home].extra.push(target);
                        changed = true;
                    }
                }
                plan.nodes[index].live = Some(live);
            }
            // A facade represents the same object in an already live realm.
            // Preserve WeakRef/identity observers there, without allowing a
            // transport cache to activate a discarded destination VM.
            for pin in 0..plan.pins.len() {
                let edge = &plan.pins[pin];
                if edge.tracked
                    && !edge.reverse_followed
                    && edge.owner != edge.target
                    && plan.object_is_live(edge.target)
                {
                    let owner = plan
                        .index_of(edge.owner.heap)
                        .expect("tracked identity owner");
                    if plan.nodes[owner].active {
                        let object = edge.owner;
                        plan.pins[pin].reverse_followed = true;
                        assert!(plan.nodes[owner].extra.len() < plan.nodes[owner].extra.capacity());
                        plan.nodes[owner].extra.push(object);
                        changed = true;
                    }
                }
            }
            if !changed {
                break;
            }
        }
        // No guest code runs between marking and these scalar root releases.
        // Snapshot handles keep inactive targets alive until every pair is gone.
        plan.select_shared_routes();
        for pin in &plan.pins {
            if pin.tracked && !plan.object_is_live(pin.owner) {
                let target = plan.index_of(pin.target.heap).expect("tracked root heap");
                if let Some(handle) = plan.nodes[target].handle() {
                    handle.borrow_mut().heap.unroot(pin.root)?;
                } else {
                    self.heap.unroot(pin.root)?;
                }
            }
        }
        self.prune_membrane_owners(plan);
        self.prune_shadow_owners(
            plan,
            plan.nodes[0]
                .live
                .as_ref()
                .expect("the root realm is active"),
        );
        for node in &plan.nodes[1..plan.len] {
            let mut vm = node.handle().expect("a discovered child").borrow_mut();
            vm.prune_membrane_owners(plan);
            if let Some(live) = &node.live {
                vm.prune_shadow_owners(plan, live);
            } else {
                // Break record cycles before dropping the traversal snapshot.
                vm.shadow_wrapped_functions.clear();
                vm.shadow_realms.clear();
                vm.shadow_realm_by_heap.clear();
                vm.test262_realms.clear();
            }
        }
        // Snapshot records do not own alias tokens. Release newly dead weak
        // identity nodes in this collection, while VM data remains pinned.
        for node in &plan.nodes[..plan.len] {
            if let Some(record) = &node.record {
                record.owners.prune();
            }
        }
        self.recycle_realm_marks(plan)?;
        for node in &plan.nodes[1..plan.len] {
            if node.active {
                let mut vm = node.handle().expect("a discovered child").borrow_mut();
                vm.collect_local_realm()?;
                if terminal_cleanup {
                    vm.reconcile_scheduling_ownership();
                } else {
                    vm.enqueue_finalization_cleanup_jobs()?;
                    vm.synchronize_scheduling_usage()?;
                }
            }
        }
        Ok(())
    }
}
