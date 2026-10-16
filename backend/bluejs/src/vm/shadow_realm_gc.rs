// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Agent collection at explicit execution/call/checkpoint safepoints. Ordinary
//! allocating heap operations can own Rust temporaries invisible to the VM;
//! they must not initiate a second collection of other realms.

use super::*;

#[derive(Default)]
struct RealmEdges {
    instances: Vec<(ObjectId, u64, usize)>,
    wrappers: Vec<(ObjectId, u64, ObjectId, RootId)>,
}

impl Vm {
    fn shadow_gc_edges(&self) -> RealmEdges {
        RealmEdges {
            instances: self
                .shadow_realms
                .iter()
                .map(|(id, r)| (*id, r.heap, Rc::as_ptr(&r.owner) as usize))
                .collect(),
            wrappers: self
                .shadow_wrapped_functions
                .iter()
                .map(|(id, w)| (*id, w.home_heap, w.target, w.target_root.id))
                .collect(),
        }
    }

    fn shadow_has_work(&self) -> bool {
        !self.promise_jobs.is_empty()
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
    ) -> Result<HashSet<ObjectId>, RuntimeError> {
        let prepared = self.prepare_gc_trace()?;
        self.heap.release_realm_roots();
        self.heap.push_scoped_roots(prepared.batch);
        let marked = self.heap.realm_reachable(ignored, extra);
        self.finish_gc_trace(prepared.charge);
        Ok(marked)
    }

    fn prune_shadow_owners(&mut self, active: &HashSet<u64>, live: &HashSet<ObjectId>) {
        self.shadow_wrapped_functions
            .retain(|id, _| live.contains(id));
        self.shadow_realms
            .retain(|_, record| active.contains(&record.heap));
        self.shadow_realm_by_heap
            .retain(|heap, id| active.contains(heap) && self.shadow_realms.contains_key(id));
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
        let Some(realms) = self.shadow_collection_realms() else {
            // A synchronous cross-realm call still owns a borrowed VM. Its
            // ordinary heap roots remain valid; defer agent reclamation until
            // the outer caller reaches a safepoint with the complete tree.
            return self.collect_local_realm();
        };
        if realms.is_empty() && self.shadow_wrapped_functions.is_empty() {
            return self.collect_local_realm();
        }
        let own_heap = self.object_prototype.heap;
        let mut edges = HashMap::from([(own_heap, self.shadow_gc_edges())]);
        let mut known_owners = HashSet::new();
        let mut active = HashSet::from([own_heap]);
        for record in &realms {
            let vm = record.vm.borrow();
            if vm.shadow_has_work() {
                active.insert(record.heap);
            }
            edges.insert(record.heap, vm.shadow_gc_edges());
        }
        let mut ignored = HashMap::<u64, HashSet<RootId>>::new();
        for realm in edges.values() {
            for (_, _, owner) in &realm.instances {
                known_owners.insert(*owner);
            }
            for (_, heap, _, root) in &realm.wrappers {
                ignored.entry(*heap).or_default().insert(*root);
            }
        }
        for record in &realms {
            // Test262 can share one child with another owner outside this
            // tree. Only actual owning table entries count; task traversal
            // snapshots must not keep every discarded realm alive.
            let mut owners = record.owners.borrow_mut();
            owners.retain(|owner| owner.strong_count() != 0);
            if owners
                .iter()
                .any(|owner| !known_owners.contains(&(owner.as_ptr() as usize)))
            {
                active.insert(record.heap);
            }
        }
        let mut extra = HashMap::<u64, HashSet<ObjectId>>::new();
        let mut marked = HashMap::new();
        loop {
            let mut changed = false;
            for heap in active.clone() {
                let pins = ignored.entry(heap).or_default();
                let targets: Vec<_> = extra.entry(heap).or_default().iter().copied().collect();
                let live = if heap == own_heap {
                    self.shadow_mark(pins, &targets)?
                } else {
                    let record = realms.iter().find(|r| r.heap == heap).expect("known realm");
                    record.vm.borrow_mut().shadow_mark(pins, &targets)?
                };
                let realm = &edges[&heap];
                for (instance, child, _) in &realm.instances {
                    if live.contains(instance) {
                        changed |= active.insert(*child);
                    }
                }
                for (wrapper, home, target, _) in &realm.wrappers {
                    if live.contains(wrapper) && edges.contains_key(home) {
                        changed |= active.insert(*home);
                        changed |= extra.entry(*home).or_default().insert(*target);
                    }
                }
                marked.insert(heap, live);
            }
            if !changed {
                break;
            }
        }
        self.prune_shadow_owners(&active, &marked[&own_heap]);
        for record in &realms {
            let mut vm = record.vm.borrow_mut();
            if let Some(live) = marked.get(&record.heap) {
                vm.prune_shadow_owners(&active, live);
            } else {
                // Clear both directions before dropping snapshots, including
                // shared-record cycles introduced by the Test262 membrane.
                vm.shadow_wrapped_functions.clear();
                vm.shadow_realms.clear();
                vm.shadow_realm_by_heap.clear();
            }
        }
        // Lease drops above cannot reborrow a suspended source. Drain them
        // only now, and sweep before any JS can observe WeakRef targets.
        for record in &realms {
            if active.contains(&record.heap) {
                let mut vm = record.vm.borrow_mut();
                vm.collect_local_realm()?;
                vm.enqueue_finalization_cleanup_jobs()?;
                vm.synchronize_scheduling_usage()?;
            }
        }
        drop(realms);
        self.collect_local_realm()
    }

    fn shadow_collection_realms(&self) -> Option<Vec<ShadowRealmRecord>> {
        let mut records = Vec::<ShadowRealmRecord>::new();
        let mut seen = HashSet::from([self.object_prototype.heap]);
        for record in self.shadow_realms.values() {
            if seen.insert(record.heap) {
                records.push(record.clone());
            }
        }
        let mut index = 0;
        while index < records.len() {
            let children = {
                let vm = records[index].vm.try_borrow_mut().ok()?;
                vm.shadow_realms.values().cloned().collect::<Vec<_>>()
            };
            for child in children {
                if seen.insert(child.heap) {
                    records.push(child);
                }
            }
            index += 1;
        }
        Some(records)
    }
}
