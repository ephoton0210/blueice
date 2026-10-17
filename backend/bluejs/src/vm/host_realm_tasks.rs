// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Admitted identity paths do not retain realms across document task callbacks.

use super::host_task_storage::TaskVector;
use super::realm_collection::MetadataStorage;
use super::*;

#[derive(Clone, Copy)]
enum Kind {
    Owned,
    Shared,
}

#[derive(Clone, Copy)]
struct Step {
    kind: Kind,
    instance: ObjectId,
    heap: u64,
}

pub(super) struct HostRealm {
    pub(super) heap: u64,
    path: TaskVector<Step>,
}

fn same_document(parent: &Vm, child: &Vm) -> bool {
    parent
        .host_scheduling
        .as_ref()
        .zip(child.host_scheduling.as_ref())
        .is_some_and(|(parent, child)| parent.same_document(child))
}

fn check_stack(vm: &Vm) -> Result<(), RuntimeError> {
    if call_stack_exhausted(native_stack::remaining_stack(), vm.call_depth) {
        Err(RuntimeError::Unsupported(
            "host realm path exceeds native stack reserve",
        ))
    } else {
        Ok(())
    }
}

fn read_path<T>(
    vm: &Vm,
    path: &[Step],
    blocked: &mut bool,
    action: impl FnOnce(&Vm) -> Result<T, RuntimeError>,
) -> Result<Option<T>, RuntimeError> {
    check_stack(vm)?;
    let Some((step, rest)) = path.split_first() else {
        return action(vm).map(Some);
    };
    match step.kind {
        Kind::Owned => {
            let Some(child) = vm.test262_realms.get(&step.instance) else {
                return Ok(None);
            };
            if child.vm.object_prototype.heap != step.heap || !same_document(vm, &child.vm) {
                return Ok(None);
            }
            if realm_reentrancy::resolve_active(step.heap).is_some() {
                *blocked = true;
                return Ok(None);
            }
            read_path(&child.vm, rest, blocked, action)
        }
        Kind::Shared => {
            let Some(record) = vm
                .shadow_realms
                .get(&step.instance)
                .filter(|record| record.heap == step.heap)
            else {
                return Ok(None);
            };
            // The next phase needs exclusive access too. Skip a busy route now.
            let Ok(child) = record.vm.try_borrow_mut() else {
                let foreign =
                    realm_reentrancy::active_scheduling_scope(step.heap).is_some_and(|active| {
                        vm.host_scheduling
                            .as_ref()
                            .is_some_and(|owner| !owner.same_document(&active))
                    });
                *blocked |= !foreign;
                return Ok(None);
            };
            if !same_document(vm, &child) {
                return Ok(None);
            }
            read_path(&child, rest, blocked, action)
        }
    }
}

fn enter_path<T>(
    vm: &mut Vm,
    path: &[Step],
    action: impl FnOnce(&mut Vm) -> Result<T, RuntimeError>,
) -> Result<Option<T>, RuntimeError> {
    check_stack(vm)?;
    let Some((step, rest)) = path.split_first() else {
        return action(vm).map(Some);
    };
    match step.kind {
        Kind::Owned => {
            let Some(child) = vm.test262_realms.get(&step.instance) else {
                return Ok(None);
            };
            if child.vm.object_prototype.heap != step.heap
                || !same_document(vm, &child.vm)
                || realm_reentrancy::resolve_active(step.heap).is_some()
            {
                return Ok(None);
            }
            // Reuse the existing reverse-membrane entry; every ancestor stays
            // registered and no map borrow is held across guest execution.
            vm.test262_with_host_realm(step.instance, |child| enter_path(child, rest, action))
        }
        Kind::Shared => {
            let Some(record) = vm
                .shadow_realms
                .get(&step.instance)
                .filter(|record| record.heap == step.heap)
            else {
                return Ok(None);
            };
            let realm = record.vm.clone();
            let Ok(mut child) = realm.try_borrow_mut() else {
                return Ok(None);
            };
            if !same_document(vm, &child) {
                return Ok(None);
            }
            let registration =
                realm_reentrancy::prepare_active_registration(vm, child.host_scheduling.as_ref())?;
            let guard = registration.publish(vm);
            let result = enter_path(&mut child, rest, action);
            drop(guard);
            result
        }
    }
}

impl HostRealm {
    #[cfg(any(test, coverage))]
    pub(super) fn retained_bytes(&self) -> usize {
        self.path.retained_bytes()
    }

    pub(super) fn read<T>(
        &self,
        root: &Vm,
        action: impl FnOnce(&Vm) -> Result<T, RuntimeError>,
    ) -> Result<Option<T>, RuntimeError> {
        read_path(root, &self.path, &mut false, action)
    }
    fn read_checkpoint<T>(
        &self,
        root: &Vm,
        blocked: &mut bool,
        action: impl FnOnce(&Vm) -> Result<T, RuntimeError>,
    ) -> Result<Option<T>, RuntimeError> {
        read_path(root, &self.path, blocked, action)
    }
    pub(super) fn enter<T>(
        &self,
        root: &mut Vm,
        action: impl FnOnce(&mut Vm) -> Result<T, RuntimeError>,
    ) -> Result<Option<T>, RuntimeError> {
        enter_path(root, &self.path, action)
    }
    fn child(&self, step: Step, owner: &SchedulingScope) -> Result<Self, RuntimeError> {
        let mut path = TaskVector::new(Some(owner))?;
        path.reserve(self.path.len().saturating_add(1))?;
        for &part in self.path.iter() {
            path.push(part)?;
        }
        path.push(step)?;
        Ok(Self {
            heap: step.heap,
            path,
        })
    }
    fn enqueue_children(
        &self,
        vm: &Vm,
        owner: &SchedulingScope,
        queue: &mut TaskVector<Self>,
    ) -> Result<(), RuntimeError> {
        queue.reserve(
            vm.test262_realms
                .len()
                .saturating_add(vm.shadow_realms.len()),
        )?;
        for (&instance, child) in &vm.test262_realms {
            queue.push(self.child(
                Step {
                    kind: Kind::Owned,
                    instance,
                    heap: child.vm.object_prototype.heap,
                },
                owner,
            )?)?;
        }
        for (&instance, child) in &vm.shadow_realms {
            queue.push(self.child(
                Step {
                    kind: Kind::Shared,
                    instance,
                    heap: child.heap,
                },
                owner,
            )?)?;
        }
        Ok(())
    }
}

impl Vm {
    pub(super) fn host_task_realms(&self) -> Result<TaskVector<HostRealm>, RuntimeError> {
        self.host_checkpoint_realms().map(|(members, _)| members)
    }
    pub(super) fn host_checkpoint_realms(
        &self,
    ) -> Result<(TaskVector<HostRealm>, bool), RuntimeError> {
        let mut blocked = false;
        let mut members = TaskVector::new(self.host_scheduling.as_ref())?;
        let Some(owner) = &self.host_scheduling else {
            return Ok((members, blocked));
        };
        if self.test262_realms.is_empty() && self.shadow_realms.is_empty() {
            return Ok((members, blocked));
        }
        let mut storage = MetadataStorage::new(Some(owner))?;
        let mut seen = HashSet::new();
        storage.reserve_set(&mut seen, 1)?;
        seen.insert(self.object_prototype.heap);
        let root = HostRealm {
            heap: self.object_prototype.heap,
            path: TaskVector::new(Some(owner))?,
        };
        let mut queue = TaskVector::new(Some(owner))?;
        root.enqueue_children(self, owner, &mut queue)?;
        while let Some(node) = queue.pop() {
            if seen.contains(&node.heap) {
                continue;
            }
            let found = node
                .read_checkpoint(self, &mut blocked, |vm| {
                    node.enqueue_children(vm, owner, &mut queue)
                })?
                .is_some();
            if found {
                storage.reserve_set(&mut seen, 1)?;
                seen.insert(node.heap);
                members.push(node)?;
            }
        }
        members.sort_unstable_by_key(|member| member.heap);
        Ok((members, blocked))
    }
}
