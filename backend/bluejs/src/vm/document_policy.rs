// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Activate one policy across an existing realm graph before resuming work.

use super::realm_collection::MetadataStorage;
use super::*;

const BUSY: &str = "document policy activation requires inactive member realms";

struct Node {
    anchor: Option<ShadowRealmRecord>,
    path: Vec<ObjectId>,
    heap: u64,
}

impl Node {
    fn handle(&self, root: &Vm) -> Result<Option<realm_cells::RealmHandle>, RuntimeError> {
        let mut handle = self.anchor.as_ref().map(|record| record.vm.clone());
        for id in &self.path {
            let next = if let Some(current) = &handle {
                let vm = current
                    .try_borrow()
                    .map_err(|_| RuntimeError::Unsupported(BUSY))?;
                vm.test262_realms
                    .get(id)
                    .expect("discovered owned realm")
                    .vm
                    .clone()
            } else {
                root.test262_realms
                    .get(id)
                    .expect("discovered owned realm")
                    .vm
                    .clone()
            };
            handle = Some(next);
        }
        Ok(handle)
    }
    fn read<T>(
        &self,
        root: &Vm,
        action: impl FnOnce(&Vm) -> Result<T, RuntimeError>,
    ) -> Result<T, RuntimeError> {
        if let Some(handle) = self.handle(root)? {
            let vm = handle
                .try_borrow_mut()
                .map_err(|_| RuntimeError::Unsupported(BUSY))?;
            action(&vm)
        } else {
            action(root)
        }
    }
    fn edit<T>(
        &self,
        root: &mut Vm,
        action: impl FnOnce(&mut Vm) -> Result<T, RuntimeError>,
    ) -> Result<T, RuntimeError> {
        if let Some(handle) = self.handle(root)? {
            let mut vm = handle
                .try_borrow_mut()
                .map_err(|_| RuntimeError::Unsupported(BUSY))?;
            action(&mut vm)
        } else {
            action(root)
        }
    }
}

#[derive(Clone, Copy)]
enum TaskKind {
    Timer,
    Browser,
    Finalization,
}

struct Task {
    node: usize,
    index: usize,
    kind: TaskKind,
    order: (u64, u64, u8, usize),
}

struct Plan {
    queue: Vec<Node>,
    members: Vec<Node>,
    seen: HashSet<u64>,
    tasks: Vec<Task>,
    storage: MetadataStorage,
    managed_bytes: usize,
    clock_ms: u64,
}

impl Plan {
    fn new(root: &Vm, scope: &SchedulingScope) -> Result<Self, RuntimeError> {
        let mut plan = Self {
            queue: Vec::new(),
            members: Vec::new(),
            seen: HashSet::new(),
            tasks: Vec::new(),
            storage: MetadataStorage::new(Some(scope))?,
            managed_bytes: 0,
            clock_ms: 0,
        };
        plan.storage.reserve_vec(&mut plan.queue, 1)?;
        plan.queue.push(Node {
            anchor: None,
            path: Vec::new(),
            heap: root.object_prototype.heap,
        });
        Ok(plan)
    }

    fn discover(
        &mut self,
        root: &Vm,
        scope: &SchedulingScope,
        managed: &crate::heap::DocumentHeapBudget,
    ) -> Result<(), RuntimeError> {
        while let Some(node) = self.queue.pop() {
            if self.seen.contains(&node.heap) {
                continue;
            }
            self.storage.reserve_set(&mut self.seen, 1)?;
            self.seen.insert(node.heap);
            let include = node.read(root, |vm| {
                if vm
                    .host_scheduling
                    .as_ref()
                    .is_some_and(|current| !current.same_document(scope))
                {
                    // Shared records may refer to another document. Owned children must join.
                    return if node.anchor.is_some() && node.path.is_empty() {
                        Ok(false)
                    } else {
                        Err(scope.fail())
                    };
                }
                if !vm.heap.can_join_document_heap_budget(managed) {
                    return Err(scope.fail());
                }
                if !vm.membrane_storage_can_join(scope)
                    || !vm
                        .heap
                        .async_generator_counts_can_join(&scope.heap_native_budget())
                    || !vm.active_native_storage_can_join(scope)
                    || !vm.intrinsic_storage_can_join(scope)
                    || !vm.host_listener_storage_can_join(scope)
                    || !vm.host_family_storage_can_join(scope)
                    || !vm.host_registration_storage_can_join(scope)
                    || !vm.promise_map_storage_can_join(scope)
                    || !vm.rejection_origin_storage_can_join(scope)
                    || !vm.module_import_storage_can_join(scope)
                    || !vm.deferred_import_waiters.can_join(scope)
                    || !vm.browser_tasks.can_join(scope)
                    || !vm.host_timers.can_join(scope)
                    || !vm.host_finalization_tasks.can_join(scope)
                    || vm
                        .host_console
                        .as_ref()
                        .is_some_and(|console| !console.can_join(scope))
                    || vm
                        .host_rejections
                        .as_ref()
                        .is_some_and(|state| !state.can_join(scope))
                {
                    return Err(scope.fail());
                }
                if node.path.is_empty()
                    && node
                        .anchor
                        .as_ref()
                        .is_some_and(|record| !record.owner_storage_can_join(scope))
                {
                    return Err(scope.fail());
                }
                if vm.document_policy_ready {
                    return Ok(false);
                }
                if vm.heap.document_heap_budget_handle().is_none() {
                    self.managed_bytes = self
                        .managed_bytes
                        .checked_add(vm.heap.stats().managed_bytes)
                        .ok_or_else(|| scope.fail())?;
                }
                self.clock_ms = self.clock_ms.max(vm.host_timer_clock_ms);
                if vm.host_scheduling.is_none() {
                    let count = vm
                        .host_timers
                        .len()
                        .checked_add(vm.browser_tasks.len())
                        .and_then(|n| n.checked_add(vm.host_finalization_tasks.len()))
                        .ok_or_else(|| scope.fail())?;
                    self.storage.reserve_vec(&mut self.tasks, count)?;
                    let member = self.members.len();
                    for (index, task) in vm.host_timers.iter().enumerate() {
                        self.tasks.push(Task {
                            node: member,
                            index,
                            kind: TaskKind::Timer,
                            order: (task.serial, node.heap, 0, index),
                        });
                    }
                    for (index, task) in vm.browser_tasks.iter().enumerate() {
                        self.tasks.push(Task {
                            node: member,
                            index,
                            kind: TaskKind::Browser,
                            order: (task.order().1, node.heap, 1, index),
                        });
                    }
                    for (index, task) in vm.host_finalization_tasks.iter().enumerate() {
                        self.tasks.push(Task {
                            node: member,
                            index,
                            kind: TaskKind::Finalization,
                            order: (task.serial, node.heap, 2, index),
                        });
                    }
                }
                for (&id, child) in &vm.test262_realms {
                    let mut path = Vec::new();
                    self.storage
                        .reserve_vec(&mut path, node.path.len().saturating_add(1))?;
                    path.extend_from_slice(&node.path);
                    path.push(id);
                    self.storage.reserve_vec(&mut self.queue, 1)?;
                    self.queue.push(Node {
                        anchor: node.anchor.clone(),
                        path,
                        heap: child.vm.heap(),
                    });
                }
                for record in vm.shadow_realms.values() {
                    if self.seen.contains(&record.heap) {
                        continue;
                    }
                    self.storage.reserve_vec(&mut self.queue, 1)?;
                    self.queue.push(Node {
                        anchor: Some(record.clone()),
                        path: Vec::new(),
                        heap: record.heap,
                    });
                }
                Ok(true)
            });
            let include = match include {
                Err(RuntimeError::Unsupported(BUSY))
                    if node.anchor.is_some()
                        && node.path.is_empty()
                        && realm_reentrancy::active_scheduling_scope(node.heap)
                            .is_some_and(|active| !active.same_document(scope)) =>
                {
                    false
                }
                result => result?,
            };
            if include {
                self.storage.reserve_vec(&mut self.members, 1)?;
                self.members.push(node);
            }
        }
        Ok(())
    }

    fn publish(
        &mut self,
        root: &mut Vm,
        scope: &SchedulingScope,
        managed: &crate::heap::DocumentHeapBudget,
    ) -> Result<(), RuntimeError> {
        scope.bind_document_heap_budget(managed)?;
        // Publish a common terminal owner before any member's local attachment.
        for node in &self.members {
            node.edit(root, |vm| {
                if vm.host_scheduling.is_none() {
                    vm.host_scheduling = Some(scope.child());
                }
                vm.bounded_host_microtask_checkpoints = true;
                Ok(())
            })?;
        }
        managed.check_room(self.managed_bytes)?;
        self.tasks.sort_unstable_by_key(|task| task.order);
        let first = scope.reserve_task_serials(self.tasks.len())?;
        for (offset, task) in self.tasks.iter().enumerate() {
            self.members[task.node].edit(root, |vm| {
                let serial = first + offset as u64;
                match task.kind {
                    TaskKind::Timer => vm.host_timers.set_serial(task.index, serial),
                    TaskKind::Browser => vm.browser_tasks.set_serial(task.index, serial),
                    TaskKind::Finalization => {
                        vm.host_finalization_tasks[task.index].serial = serial
                    }
                }
                Ok(())
            })?;
        }
        scope.advance_clock(self.clock_ms);
        for node in &self.members {
            node.edit(root, |vm| {
                vm.heap.join_document_heap_budget(managed)?;
                vm.attach_document_policy_storage()
            })?;
        }
        for node in &self.members {
            node.edit(root, |vm| {
                vm.document_policy_ready = true;
                Ok(())
            })?;
        }
        Ok(())
    }
}

impl Vm {
    fn attach_document_policy_storage(&mut self) -> Result<(), RuntimeError> {
        self.attach_linked_module_cells();
        let scope = self.host_scheduling.as_ref().unwrap().child();
        self.attach_active_native_storage(&scope)?;
        self.attach_intrinsic_storage(&scope)?;
        self.attach_host_listener_storage(&scope)?;
        self.attach_host_family_storage(&scope)?;
        self.attach_host_registration_storage(&scope)?;
        let owner = scope.shared_charge(0)?;
        self.console_owners(Some(&owner))?;
        self.retain_host_wait_counts(Some(&owner))?;
        self.browser_queue_owner(Some(&owner))?;
        self.timer_table_owner(Some(&owner))?;
        self.finalization_queue_owner(Some(&owner))?;
        self.pending_rejection_owner()?;
        self.module_import_owner()?;
        self.deferred_import_owner(Some(&owner))?;
        let scope = self
            .host_scheduling
            .as_ref()
            .expect("published document scope");
        self.attach_cached_module_values(scope)?;
        self.synchronize_scheduling_usage()?;
        let scope = self.host_scheduling.as_ref().unwrap();
        self.symbol_registry.borrow_mut().attach(scope)?;
        self.attach_module_sources(scope)
    }

    pub(super) fn inherit_document_policy(&mut self, owner: &Vm) -> Result<(), RuntimeError> {
        self.bounded_host_microtask_checkpoints = owner.bounded_host_microtask_checkpoints;
        let Some(scope) = &owner.host_scheduling else {
            return Ok(());
        };
        self.host_scheduling = Some(scope.child());
        self.bounded_host_microtask_checkpoints = true;
        let managed = scope
            .document_heap_budget()
            .or_else(|| owner.heap.document_heap_budget_handle())
            .ok_or_else(|| scope.fail())?;
        let result: Result<(), RuntimeError> = (|| {
            scope.bind_document_heap_budget(&managed)?;
            self.heap.join_document_heap_budget(&managed)?;
            self.attach_document_policy_storage()?;
            self.document_policy_ready = true;
            Ok(())
        })();
        result.map_err(|_| scope.fail())
    }

    pub(crate) fn use_bounded_host_microtask_checkpoints(&mut self) -> Result<(), RuntimeError> {
        if self.document_policy_ready {
            return self.attach_document_policy_storage();
        }
        if realm_reentrancy::is_active(self.object_prototype.heap) {
            return Err(RuntimeError::Unsupported(BUSY));
        }
        let scope = self
            .host_scheduling
            .as_ref()
            .map_or_else(SchedulingScope::new, SchedulingScope::child);
        let managed = scope
            .document_heap_budget()
            .or_else(|| self.heap.document_heap_budget_handle())
            .unwrap_or_else(|| {
                crate::heap::DocumentHeapBudget::empty(self.config.heap.max_heap_bytes)
            });
        let result: Result<(), RuntimeError> = (|| {
            let mut plan = Plan::new(self, &scope)?;
            plan.discover(self, &scope, &managed)?;
            plan.publish(self, &scope, &managed)
        })();
        match result {
            Err(RuntimeError::Unsupported(BUSY)) => Err(RuntimeError::Unsupported(BUSY)),
            Err(_) => {
                if self.host_scheduling.is_none() {
                    self.host_scheduling = Some(scope.child());
                }
                self.bounded_host_microtask_checkpoints = true;
                Err(scope.fail())
            }
            Ok(()) => Ok(()),
        }
    }
}
