// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Original event accounts, listener snapshots and paid callback storage.

use super::host_scheduling::SharedSchedulingCharge;
use super::native_context_guards::NativeContextGuards;
use super::native_value_storage::NativeValue;
use super::realm_call_storage::RealmCallRoots;
use super::*;
use crate::heap::{DragEventState, WebListener, WebTargetData};
use crate::vm::error_input_storage::utf16_from_utf8;

pub(super) struct EventContext {
    pub owner: Option<SharedSchedulingCharge>,
    caller: Option<SharedSchedulingCharge>,
    root: Option<SharedSchedulingCharge>,
    ambient: Option<SharedSchedulingCharge>,
    root_callers: Option<Rc<NativeContextGuards>>,
    ambient_callers: Option<Rc<NativeContextGuards>>,
}

impl EventContext {
    /// Cleanup can return existing credit after failure without admitting any
    /// new storage. It still cannot edit a different scoped document's heap.
    pub(super) fn check_affinity(vm: &Vm) -> Result<(), RuntimeError> {
        if let (Some(scope), Some(original)) =
            (&vm.host_scheduling, vm.heap.native_storage_budget())
        {
            if !scope.heap_native_budget().same_document(original) {
                return Err(scope.fail());
            }
        }
        Ok(())
    }

    pub(super) fn capture(vm: &Vm) -> Result<Self, RuntimeError> {
        let caller = vm.argument_list_owner()?;
        let original = vm
            .heap
            .native_storage_budget()
            .map(|budget| budget.reserve(0))
            .transpose()?;
        let owner = match original {
            Some(owner) => Some(owner),
            None => caller
                .as_ref()
                .map(|owner| owner.reserve_peer(0))
                .transpose()?,
        };
        if let (Some(scope), Some(owner)) = (&vm.host_scheduling, &owner) {
            if !owner.belongs_to(scope) {
                return Err(scope.fail());
            }
        }
        let root = vm.current_realm_root_owner()?;
        let root_callers = vm.current_realm_context_guards()?;
        // Owned documents retain their own policy. Neutral work also keeps
        // every required ambient/root identity, even when a different native
        // heap account pays for this event. These are existing immutable
        // snapshots and zero-byte views, not new backing allocations.
        let (ambient, ambient_callers) = if vm.host_scheduling.is_none() {
            (
                realm_reentrancy::reserve_active_native(0)?,
                realm_reentrancy::active_context_guards()?,
            )
        } else {
            (None, None)
        };
        Ok(Self {
            owner,
            caller,
            root,
            ambient,
            root_callers,
            ambient_callers,
        })
    }

    pub(super) fn validate(&self) -> Result<(), RuntimeError> {
        for owner in self
            .owner
            .iter()
            .chain(self.caller.iter())
            .chain(self.root.iter())
            .chain(self.ambient.iter())
        {
            owner.reserve_peer(0)?;
        }
        for callers in self.root_callers.iter().chain(self.ambient_callers.iter()) {
            callers.validate()?;
        }
        Ok(())
    }

    pub(super) fn with_roots<T>(
        &self,
        vm: &mut Vm,
        ids: impl Iterator<Item = ObjectId> + Clone,
        action: impl FnOnce(&mut Vm) -> Result<T, RuntimeError>,
    ) -> Result<T, RuntimeError> {
        let roots = self.prepare_roots(vm, ids, 0)?;
        let result = vm.with_realm_call_roots(roots, action);
        self.validate().and(result)
    }

    pub(super) fn prepare_roots(
        &self,
        vm: &Vm,
        ids: impl Iterator<Item = ObjectId> + Clone,
        extra: usize,
    ) -> Result<Rc<RealmCallRoots>, RuntimeError> {
        self.validate()?;
        let count = ids.clone().count().checked_add(extra).ok_or_else(|| {
            self.owner
                .as_ref()
                .map_or(RuntimeError::SchedulingLimit, SharedSchedulingCharge::fail)
        })?;
        let roots = RealmCallRoots::new(count, self.owner.as_ref())?;
        for id in ids {
            roots.push(&vm.heap, id)?;
        }
        Ok(roots)
    }
}

pub(super) fn push_slot<T>(
    values: &mut Vec<T>,
    storage: &mut Option<SharedSchedulingCharge>,
    value: T,
) -> Result<(), RuntimeError> {
    if let Some(owner) = storage {
        owner.reserve_peer(0)?;
    }
    if values.len() == values.capacity() {
        let capacity = values.capacity().saturating_mul(2).max(4);
        let mut replacement_storage = storage
            .as_ref()
            .map(|owner| owner.reserve_peer(capacity.saturating_mul(std::mem::size_of::<T>())))
            .transpose()?;
        let mut replacement = Vec::new();
        replacement.try_reserve_exact(capacity).map_err(|_| {
            storage
                .as_ref()
                .map_or(RuntimeError::SchedulingLimit, SharedSchedulingCharge::fail)
        })?;
        if let Some(storage) = &mut replacement_storage {
            storage.resize(host_scheduling::slots(&replacement))?;
        }
        replacement.append(values);
        let old = host_scheduling::slots(values);
        *values = replacement;
        if let Some(storage) = storage {
            storage.release_to(storage.bytes() - old);
            storage
                .absorb(replacement_storage.unwrap())
                .expect("same abort account");
        }
    }
    values.push(value);
    Ok(())
}

pub(super) struct NativeEventTarget {
    pub state: WebTargetData,
    storage: Option<SharedSchedulingCharge>,
}
impl NativeEventTarget {
    pub(super) fn copied(
        vm: &Vm,
        target: ObjectId,
        owner: Option<&SharedSchedulingCharge>,
    ) -> Result<Self, RuntimeError> {
        let source = vm.heap.web_event_target(target)?;
        let mut storage = owner
            .map(|owner| owner.reserve_peer(source.copied_buffer_bytes()))
            .transpose()?;
        let state = source.clone();
        if let Some(storage) = &mut storage {
            storage.resize(state.owned_buffer_bytes())?;
        }
        Ok(Self { state, storage })
    }

    pub(super) fn listener(
        &mut self,
        listener: WebListener,
        name_storage: Option<SharedSchedulingCharge>,
    ) -> Result<(), RuntimeError> {
        if let Some(storage) = &self.storage {
            storage.reserve_peer(0)?;
            let incoming = name_storage.as_ref().expect("owned listener name");
            incoming.reserve_peer(0)?;
            if !storage.same_owner(incoming) {
                return Err(storage.fail());
            }
        } else {
            assert!(name_storage.is_none());
        }
        push_slot(&mut self.state.listeners, &mut self.storage, listener)?;
        if let Some(storage) = &mut self.storage {
            storage.absorb(name_storage.unwrap())?;
        }
        Ok(())
    }

    pub(super) fn handler(&mut self, callback: ObjectId) -> Result<(), RuntimeError> {
        let name = utf16_from_utf8("abort", self.storage.as_ref())?;
        let (name, name_storage) = name.into_retained_parts();
        let Value::String(name) = name else {
            unreachable!()
        };
        push_slot(
            &mut self.state.listeners,
            &mut self.storage,
            WebListener {
                serial: self.state.next_serial,
                name,
                callback,
                capture: false,
                once: false,
                passive: false,
                handler: true,
                signal: None,
            },
        )?;
        if let Some(storage) = &mut self.storage {
            storage.absorb(name_storage.unwrap())?;
        }
        Ok(())
    }

    pub(super) fn into_heap(mut self, vm: &mut Vm, target: ObjectId) -> Result<(), RuntimeError> {
        if let Some(storage) = &mut self.storage {
            storage.reserve_peer(0)?;
            storage.release_to(self.state.owned_buffer_bytes());
        }
        vm.with_roots(|heap| {
            if let Some(storage) = &mut self.storage {
                storage.release_to(0);
            }
            heap.set_web_event_target(target, self.state)
        })
    }
}

pub(super) struct NativeEventState {
    pub state: DragEventState,
    storage: Option<SharedSchedulingCharge>,
}

impl NativeEventState {
    pub(super) fn empty(
        rank: u8,
        count: usize,
        owner: Option<&SharedSchedulingCharge>,
    ) -> Result<Self, RuntimeError> {
        let bytes = count
            .checked_mul(std::mem::size_of::<(&str, Value)>())
            .ok_or_else(|| {
                owner.map_or(RuntimeError::SchedulingLimit, SharedSchedulingCharge::fail)
            })?;
        let mut storage = owner.map(|owner| owner.reserve_peer(bytes)).transpose()?;
        let mut state = DragEventState::new(rank);
        state.attributes.try_reserve_exact(count).map_err(|_| {
            owner.map_or(RuntimeError::SchedulingLimit, SharedSchedulingCharge::fail)
        })?;
        if let Some(storage) = &mut storage {
            storage.resize(state.owned_buffer_bytes())?;
        }
        Ok(Self { state, storage })
    }

    pub(super) fn new(vm: &Vm, rank: u8, name: NativeValue) -> Result<Self, RuntimeError> {
        let owner = name.owner_view()?;
        let count = 6
            + if (1..=3).contains(&rank) { 2 } else { 0 }
            + if (2..=3).contains(&rank) { 21 } else { 0 };
        let mut result = Self::empty(rank, count, owner.as_ref())?;
        let (name, storage) = name.into_retained_parts();
        let Value::String(name) = name else {
            unreachable!("event type was converted")
        };
        vm.initialize_web_event_state(&mut result.state, name);
        assert_eq!(result.state.attributes.len(), count);
        if let Some(owner) = &mut result.storage {
            owner.absorb(storage.expect("same event account"))?;
        }
        Ok(result)
    }

    pub(super) fn copied(
        vm: &Vm,
        event: ObjectId,
        owner: Option<&SharedSchedulingCharge>,
    ) -> Result<Self, RuntimeError> {
        let source = vm.heap.drag_event(event)?;
        let mut storage = owner
            .map(|owner| owner.reserve_peer(source.copied_buffer_bytes()))
            .transpose()?;
        let state = source.clone();
        if let Some(storage) = &mut storage {
            storage.resize(state.owned_buffer_bytes())?;
        }
        Ok(Self { state, storage })
    }

    pub(super) fn set(
        &mut self,
        vm: &Vm,
        roots: &RealmCallRoots,
        name: &'static str,
        value: NativeValue,
    ) -> Result<(), RuntimeError> {
        value.validate_owner()?;
        if let Some(storage) = &self.storage {
            storage.reserve_peer(0)?;
            if value
                .owner_view()?
                .as_ref()
                .is_none_or(|owner| !storage.same_owner(owner))
            {
                return Err(storage.fail());
            }
        }
        if let Some(object) = value.value().object_id() {
            roots.push(&vm.heap, object)?;
        }
        let previous = self
            .state
            .get_ref(name)
            .map(host_scheduling::value_payload_bytes);
        let (value, incoming) = value.into_retained_parts();
        if let Some(previous) = previous {
            self.state.set(name, value);
            if let Some(storage) = &mut self.storage {
                storage.release_to(storage.bytes() - previous);
            }
        } else {
            push_slot(&mut self.state.attributes, &mut self.storage, (name, value))?;
        }
        if let Some(storage) = &mut self.storage {
            storage.absorb(incoming.expect("same event account"))?;
        }
        Ok(())
    }

    pub(super) fn scalar(
        &mut self,
        vm: &Vm,
        roots: &RealmCallRoots,
        name: &'static str,
        value: Value,
    ) -> Result<(), RuntimeError> {
        self.set(
            vm,
            roots,
            name,
            NativeValue::adopt(value, self.storage.as_ref())?,
        )
    }

    pub(super) fn merge(
        &mut self,
        vm: &Vm,
        roots: &RealmCallRoots,
        mut patch: Self,
    ) -> Result<(), RuntimeError> {
        while let Some((name, value)) = patch.state.attributes.pop() {
            let storage = patch
                .storage
                .as_mut()
                .map(|storage| storage.split_off(host_scheduling::value_payload_bytes(&value)));
            self.set(
                vm,
                roots,
                name,
                NativeValue::from_retained_parts(value, storage),
            )?;
        }
        Ok(())
    }

    pub(super) fn into_heap(mut self, vm: &mut Vm, event: ObjectId) -> Result<(), RuntimeError> {
        if let Some(storage) = &mut self.storage {
            storage.reserve_peer(0)?;
        }
        vm.with_roots(|heap| {
            if let Some(storage) = &mut self.storage {
                storage.release_to(0);
            }
            heap.set_drag_event_state(event, self.state)
        })
    }

    pub(super) fn allocate(
        mut self,
        vm: &mut Vm,
        prototype: ObjectId,
    ) -> Result<ObjectId, RuntimeError> {
        if let Some(storage) = &mut self.storage {
            storage.reserve_peer(0)?;
        }
        vm.with_roots(|heap| {
            if let Some(storage) = &mut self.storage {
                storage.release_to(0);
            }
            heap.alloc_drag_event(self.state, prototype)
        })
    }
}

pub(super) struct EventPath {
    values: Vec<ObjectId>,
    storage: Option<SharedSchedulingCharge>,
}

impl EventPath {
    pub(super) fn single(
        target: ObjectId,
        owner: Option<&SharedSchedulingCharge>,
    ) -> Result<Self, RuntimeError> {
        let mut result = Self::with_capacity(1, owner)?;
        result.push(target)?;
        Ok(result)
    }

    pub(super) fn with_capacity(
        count: usize,
        owner: Option<&SharedSchedulingCharge>,
    ) -> Result<Self, RuntimeError> {
        let bytes = count
            .checked_mul(std::mem::size_of::<ObjectId>())
            .ok_or_else(|| {
                owner.map_or(RuntimeError::SchedulingLimit, SharedSchedulingCharge::fail)
            })?;
        let mut storage = owner.map(|owner| owner.reserve_peer(bytes)).transpose()?;
        let mut values = Vec::new();
        values.try_reserve_exact(count).map_err(|_| {
            owner.map_or(RuntimeError::SchedulingLimit, SharedSchedulingCharge::fail)
        })?;
        if let Some(storage) = &mut storage {
            storage.resize(host_scheduling::slots(&values))?;
        }
        Ok(Self { values, storage })
    }

    pub(super) fn push(&mut self, target: ObjectId) -> Result<(), RuntimeError> {
        if let Some(storage) = &self.storage {
            storage.reserve_peer(0)?;
        }
        assert!(
            self.values.len() < self.values.capacity(),
            "prepared event path capacity"
        );
        self.values.push(target);
        Ok(())
    }

    pub(super) fn publish(
        mut self,
        vm: &mut Vm,
        event: ObjectId,
        target: ObjectId,
        trusted: bool,
    ) -> Result<(), RuntimeError> {
        if let Some(storage) = &self.storage {
            storage.reserve_peer(0)?;
        }
        vm.with_roots(|heap| {
            if let Some(storage) = &mut self.storage {
                storage.release_to(0);
            }
            heap.begin_event_dispatch(event, target, trusted, self.values)
        })
    }

    pub(super) fn publish_nodes(
        mut self,
        vm: &mut Vm,
        event: ObjectId,
        target: ObjectId,
        trusted: bool,
    ) -> Result<(), RuntimeError> {
        if let Some(storage) = &self.storage {
            storage.reserve_peer(0)?;
        }
        vm.with_roots(|heap| {
            if let Some(storage) = &mut self.storage {
                storage.release_to(0);
            }
            heap.begin_node_event_dispatch(event, target, trusted, self.values)
        })
    }
}

pub(super) struct EventCompletion {
    result: Result<Value, RuntimeError>,
    storage: Option<SharedSchedulingCharge>,
}

impl EventCompletion {
    fn bytes(result: &Result<Value, RuntimeError>) -> usize {
        match result {
            Ok(value) => host_scheduling::value_payload_bytes(value),
            Err(error) => host_scheduling::runtime_error_payload_bytes(error),
        }
    }

    pub(super) fn adopt(
        result: Result<Value, RuntimeError>,
        owner: Option<&SharedSchedulingCharge>,
    ) -> Result<Self, RuntimeError> {
        let storage = owner
            .map(|owner| owner.reserve_peer(Self::bytes(&result)))
            .transpose()?;
        Ok(Self { result, storage })
    }

    pub(super) fn cleanup(mut self, vm: &mut Vm) -> Result<Self, RuntimeError> {
        let result = std::mem::replace(&mut self.result, Ok(Value::Undefined));
        self.result = vm.cleanup_host_event_callback_retained(result);
        let bytes = Self::bytes(&self.result);
        if let Some(storage) = &mut self.storage {
            if bytes <= storage.bytes() {
                storage.release_to(bytes);
            } else {
                storage.resize(bytes)?;
            }
        }
        Ok(self)
    }

    pub(super) fn result(&self) -> &Result<Value, RuntimeError> {
        &self.result
    }

    pub(super) fn into_result(self) -> Result<Value, RuntimeError> {
        drop(self.storage);
        self.result
    }
}
