// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Retained listener records and transient serial snapshots own native storage.

use super::*;
use crate::vm::host_scheduling::SharedSchedulingCharge;
use crate::vm::transport_buffer_storage::TransportVector;

#[derive(Default)]
pub(in crate::vm) struct HostListeners {
    values: Vec<HostClickListener>,
    // Records, including their name buffers, die before their native credit.
    storage: Option<SharedSchedulingCharge>,
}

impl std::ops::Deref for HostListeners {
    type Target = Vec<HostClickListener>;
    fn deref(&self) -> &Self::Target {
        &self.values
    }
}

pub(super) struct PreparedListenerCapacity {
    replacement: Option<Vec<HostClickListener>>,
    replacement_storage: Option<SharedSchedulingCharge>,
    adoption: Option<SharedSchedulingCharge>,
    owner: Option<SharedSchedulingCharge>,
}

impl PreparedListenerCapacity {
    #[cfg(any(test, coverage))]
    pub(super) fn retained_slots(&self) -> usize {
        self.replacement.as_ref().map_or(0, host_scheduling::slots)
    }

    pub(super) fn owner(&self) -> Option<&SharedSchedulingCharge> {
        self.owner.as_ref()
    }
}

pub(super) struct RemovedListener {
    value: HostClickListener,
    _storage: Option<SharedSchedulingCharge>,
}

impl std::ops::Deref for RemovedListener {
    type Target = HostClickListener;
    fn deref(&self) -> &Self::Target {
        &self.value
    }
}

fn name_bytes(listener: &HostClickListener) -> usize {
    listener.event_type.owned_heap_payload_bytes().unwrap()
}

impl HostListeners {
    fn bytes(&self) -> usize {
        host_scheduling::slots(&self.values)
            .saturating_add(self.values.iter().map(name_bytes).sum::<usize>())
    }

    fn can_join(&self, scope: &SchedulingScope) -> bool {
        self.storage
            .as_ref()
            .is_none_or(|owner| owner.belongs_to(scope))
    }

    fn attach(&mut self, scope: &SchedulingScope) -> Result<(), RuntimeError> {
        if !self.can_join(scope) {
            return Err(scope.fail());
        }
        if self.storage.is_none() && self.values.capacity() != 0 {
            self.storage = Some(scope.shared_charge(self.bytes())?);
        }
        Ok(())
    }

    pub(super) fn owner_view(&self) -> Result<Option<SharedSchedulingCharge>, RuntimeError> {
        self.storage
            .as_ref()
            .map(|owner| owner.reserve_peer(0))
            .transpose()
    }

    pub(super) fn prepare(
        &self,
        hint: Option<&SharedSchedulingCharge>,
    ) -> Result<PreparedListenerCapacity, RuntimeError> {
        let original = self.owner_view()?;
        let adoption = if self.storage.is_none() {
            hint.map(|owner| owner.reserve_peer(self.bytes()))
                .transpose()?
        } else {
            None
        };
        let owner = match original {
            Some(owner) => Some(owner),
            None => adoption
                .as_ref()
                .map(|owner| owner.reserve_peer(0))
                .transpose()?,
        };
        let mut replacement = None;
        let mut replacement_storage = None;
        if self.values.len() == self.values.capacity() {
            let count = self.values.capacity().saturating_mul(2).max(4);
            let bytes = count
                .checked_mul(std::mem::size_of::<HostClickListener>())
                .ok_or_else(|| {
                    owner
                        .as_ref()
                        .map_or(RuntimeError::SchedulingLimit, SharedSchedulingCharge::fail)
                })?;
            let mut charge = owner
                .as_ref()
                .map(|owner| owner.reserve_peer(bytes))
                .transpose()?;
            let mut values = Vec::new();
            values.try_reserve_exact(count).map_err(|_| {
                owner
                    .as_ref()
                    .map_or(RuntimeError::SchedulingLimit, SharedSchedulingCharge::fail)
            })?;
            if let Some(charge) = &mut charge {
                charge.resize(host_scheduling::slots(&values))?;
            }
            replacement = Some(values);
            replacement_storage = charge;
        }
        Ok(PreparedListenerCapacity {
            replacement,
            replacement_storage,
            adoption,
            owner,
        })
    }

    pub(super) fn commit(
        &mut self,
        mut prepared: PreparedListenerCapacity,
        listener: HostClickListener,
        payload: Option<SharedSchedulingCharge>,
    ) {
        if self.storage.is_none() {
            self.storage = prepared.adoption.take();
        }
        if let Some(mut replacement) = prepared.replacement.take() {
            let old_slots = host_scheduling::slots(&self.values);
            replacement.append(&mut self.values);
            self.values = replacement;
            if let Some(storage) = &mut self.storage {
                storage.release_to(storage.bytes() - old_slots);
                storage
                    .absorb(prepared.replacement_storage.take().unwrap())
                    .expect("prepared replacement keeps the listener table owner");
            }
        }
        if let Some(payload) = payload {
            self.storage
                .as_mut()
                .expect("prepared listener owner")
                .absorb(payload)
                .expect("prepared name and table have one owner");
        }
        assert!(self.values.len() < self.values.capacity());
        self.values.push(listener);
    }

    pub(super) fn remove(&mut self, index: usize) -> RemovedListener {
        let value = self.values.remove(index);
        let storage = self
            .storage
            .as_mut()
            .map(|owner| owner.split_off(name_bytes(&value)));
        if self.values.is_empty() {
            self.values = Vec::new();
            self.storage = None;
        }
        RemovedListener {
            value,
            _storage: storage,
        }
    }
}

pub(super) struct ListenerContext {
    caller: Option<SharedSchedulingCharge>,
    original: Option<SharedSchedulingCharge>,
}

impl ListenerContext {
    pub(super) fn capture(vm: &Vm) -> Result<Self, RuntimeError> {
        Ok(Self {
            caller: vm.argument_list_owner()?,
            original: vm.host_click_listeners.owner_view()?,
        })
    }

    pub(super) fn owner(&self) -> Option<&SharedSchedulingCharge> {
        self.original.as_ref().or(self.caller.as_ref())
    }

    pub(super) fn validate(&self, vm: &Vm) -> Result<(), RuntimeError> {
        for owner in [self.caller.as_ref(), self.original.as_ref()]
            .into_iter()
            .flatten()
        {
            owner.reserve_peer(0)?;
        }
        vm.argument_list_owner()?;
        Ok(())
    }
}

pub(super) struct ListenerSnapshot {
    pub(super) serials: TransportVector<u64>,
    context: ListenerContext,
}

impl ListenerSnapshot {
    pub(super) fn validate(&self, vm: &Vm) -> Result<(), RuntimeError> {
        self.context.validate(vm)?;
        self.serials.validate_owner()
    }
}

impl Vm {
    pub(super) fn listener_snapshot(
        &self,
        predicate: impl Fn(&HostClickListener) -> bool + Clone,
        capture_first: bool,
    ) -> Result<ListenerSnapshot, RuntimeError> {
        let context = ListenerContext::capture(self)?;
        let accepted = self
            .host_click_listeners
            .iter()
            .filter(|listener| predicate(listener));
        let first = accepted
            .clone()
            .filter(|listener| !capture_first || listener.capture);
        let second = accepted.filter(|listener| capture_first && !listener.capture);
        let serials = TransportVector::capture(
            first.chain(second).map(|listener| listener.serial),
            context.owner(),
        )?;
        Ok(ListenerSnapshot { serials, context })
    }

    pub(in crate::vm) fn host_listener_storage_can_join(&self, scope: &SchedulingScope) -> bool {
        self.host_click_listeners.can_join(scope)
    }

    pub(in crate::vm) fn attach_host_listener_storage(
        &mut self,
        scope: &SchedulingScope,
    ) -> Result<(), RuntimeError> {
        self.host_click_listeners.attach(scope)
    }

    pub(in crate::vm) fn unowned_host_listener_storage_bytes(&self) -> usize {
        if self.host_click_listeners.storage.is_none() {
            self.host_click_listeners.bytes()
        } else {
            0
        }
    }
}
