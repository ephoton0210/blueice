// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Temporary realm-call roots own admitted IDs without copying guest values.

use super::host_scheduling::SharedSchedulingCharge;
use super::native_context_guards::NativeContextGuards;
use super::*;
use std::cell::Cell;

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/realm_root_affinity.rs"]
mod affinity_tests;

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/realm_root_writes.rs"]
mod write_tests;

pub(super) struct RealmCallRoots {
    values: RefCell<Vec<ObjectId>>,
    previous: RefCell<Option<Rc<Self>>>,
    active_heap: Cell<Option<u64>>,
    callers: RefCell<Option<Rc<NativeContextGuards>>>,
    // The backing allocation is destroyed before its native lease.
    _storage: RefCell<Option<SharedSchedulingCharge>>,
}

impl RealmCallRoots {
    fn allocation_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + 2 * std::mem::size_of::<usize>()
            + host_scheduling::slots(&self.values.borrow())
    }

    #[cfg(any(test, coverage))]
    pub(super) fn retained_bytes(&self) -> usize {
        self.allocation_bytes()
    }

    fn owner_view(&self) -> Result<Option<SharedSchedulingCharge>, RuntimeError> {
        self._storage
            .borrow()
            .as_ref()
            .map(|owner| owner.reserve_peer(0))
            .transpose()
    }

    fn adopt_owner(&self, owner: Option<&SharedSchedulingCharge>) -> Result<(), RuntimeError> {
        let mut storage = self._storage.borrow_mut();
        if storage.is_none() {
            if let Some(owner) = owner {
                *storage = Some(owner.reserve_peer(self.allocation_bytes())?);
            }
        }
        Ok(())
    }

    pub(super) fn new(
        count: usize,
        owner: Option<&SharedSchedulingCharge>,
    ) -> Result<Rc<Self>, RuntimeError> {
        let header = std::mem::size_of::<Self>() + 2 * std::mem::size_of::<usize>();
        let bytes = |capacity: usize| {
            header.saturating_add(capacity.saturating_mul(std::mem::size_of::<ObjectId>()))
        };
        let mut storage = owner
            .map(|owner| owner.reserve_peer(bytes(count)))
            .transpose()?;
        let mut values = Vec::new();
        values.try_reserve_exact(count).map_err(|_| {
            owner.map_or(RuntimeError::SchedulingLimit, SharedSchedulingCharge::fail)
        })?;
        if let Some(storage) = &mut storage {
            storage.resize(bytes(values.capacity()))?;
        }
        Ok(Rc::new(Self {
            values: RefCell::new(values),
            previous: RefCell::new(None),
            active_heap: Cell::new(None),
            callers: RefCell::new(None),
            _storage: RefCell::new(storage),
        }))
    }

    fn validate_write(&self, heap: &Heap, id: ObjectId) -> Result<(), RuntimeError> {
        self.owner_view()?;
        if let Some(callers) = self.callers.borrow().as_ref() {
            callers.validate()?;
        }
        if self
            .active_heap
            .get()
            .is_some_and(|active| active != id.heap)
        {
            return Err(RuntimeError::Heap(HeapError::InvalidObject(id)));
        }
        // This is a pure handle lookup: no guest trap, mutation or safepoint
        // precedes refusal, including when the admitted Vec already has room.
        heap.prototype(id)?;
        Ok(())
    }

    pub(super) fn push(&self, heap: &Heap, id: ObjectId) -> Result<(), RuntimeError> {
        self.validate_write(heap, id)?;
        let mut values = self.values.borrow_mut();
        assert!(
            values.len() < values.capacity(),
            "realm roots were prepared before guest entry"
        );
        values.push(id);
        Ok(())
    }

    /// Retire completed temporary edges while retaining their paid backing.
    pub(super) fn truncate(&self, length: usize) {
        self.values.borrow_mut().truncate(length);
    }

    /// Unknown getter results grow one prepared root buffer at a time. Admit
    /// the full replacement while the previous allocation is still alive.
    pub(super) fn push_admitted(&self, heap: &Heap, id: ObjectId) -> Result<(), RuntimeError> {
        self.validate_write(heap, id)?;
        let mut values = self.values.borrow_mut();
        if values.len() == values.capacity() {
            let count = values.capacity().saturating_mul(2).max(4);
            let mut storage = self._storage.borrow_mut();
            let mut replacement_charge = storage
                .as_ref()
                .map(|owner| {
                    owner.reserve_peer(count.saturating_mul(std::mem::size_of::<ObjectId>()))
                })
                .transpose()?;
            let mut replacement = Vec::new();
            replacement.try_reserve_exact(count).map_err(|_| {
                storage
                    .as_ref()
                    .map_or(RuntimeError::SchedulingLimit, SharedSchedulingCharge::fail)
            })?;
            if let Some(charge) = &mut replacement_charge {
                charge.resize(host_scheduling::slots(&replacement))?;
            }
            replacement.append(&mut values);
            *values = replacement;
            if let Some(storage) = storage.as_mut() {
                let header = std::mem::size_of::<Self>() + 2 * std::mem::size_of::<usize>();
                storage.release_to(header);
                storage.absorb(replacement_charge.unwrap())?;
            }
        }
        values.push(id);
        Ok(())
    }
}

/// Data precedes its lease so every failure drops input storage first.
pub(super) struct RealmCallInput {
    pub(super) callee: Value,
    pub(super) receiver: Value,
    pub(super) arguments: Vec<Value>,
    pub(super) entry: execution_payload::CallEntryCharge,
}

impl RealmCallInput {
    pub(super) fn objects(&self) -> impl Iterator<Item = ObjectId> + '_ {
        [&self.callee, &self.receiver]
            .into_iter()
            .chain(self.arguments.iter())
            .filter_map(Value::object_id)
    }

    pub(super) fn invoke(
        mut self,
        vm: &mut Vm,
        target: ObjectId,
        construct: bool,
    ) -> Result<Value, RuntimeError> {
        self.entry.retarget_native(vm.host_scheduling.as_ref())?;
        let owner = self.entry.native_owner()?;
        let result = vm.call_with_target_owned(
            Value::Object(target),
            self.receiver,
            self.arguments,
            construct,
            if construct {
                Value::Object(target)
            } else {
                Value::Undefined
            },
            self.entry,
        );
        if let Some(owner) = owner {
            owner.reserve_peer(0)?;
        }
        result
    }
}

impl Vm {
    pub(super) fn current_realm_root_owner(
        &self,
    ) -> Result<Option<SharedSchedulingCharge>, RuntimeError> {
        let mut frame = self.realm_call_roots.clone();
        while let Some(current) = frame {
            let owner = current.owner_view()?;
            if let Some(callers) = current.callers.borrow().as_ref() {
                callers.validate()?;
            }
            if owner.is_some() {
                return Ok(owner);
            }
            frame = current.previous.borrow().clone();
        }
        Ok(None)
    }

    pub(super) fn current_realm_context_guards(
        &self,
    ) -> Result<Option<Rc<NativeContextGuards>>, RuntimeError> {
        let Some(frame) = &self.realm_call_roots else {
            return Ok(None);
        };
        frame.owner_view()?;
        let callers = frame.callers.borrow().clone();
        if let Some(callers) = &callers {
            callers.validate()?;
        }
        Ok(callers)
    }

    pub(super) fn realm_call_native_owner(
        &self,
        peer: &Vm,
    ) -> Result<Option<SharedSchedulingCharge>, RuntimeError> {
        let root = self.current_realm_root_owner()?;
        if let Some(scope) = &self.host_scheduling {
            scope.shared_charge(0).map(Some)
        } else if let Some(owner) = root {
            realm_reentrancy::reserve_active_native(0)?;
            Ok(Some(owner))
        } else if let Some(scope) = &peer.host_scheduling {
            scope.shared_charge(0).map(Some)
        } else {
            realm_reentrancy::reserve_active_native(0)
        }
    }

    /// An explicit dynamic root frame, restored on both ordinary success and
    /// error. Nested/reentrant calls link their own independently owned frames.
    pub(super) fn with_realm_call_roots<T>(
        &mut self,
        frame: Rc<RealmCallRoots>,
        operation: impl FnOnce(&mut Self) -> Result<T, RuntimeError>,
    ) -> Result<T, RuntimeError> {
        // An outermost active frame has no previous node too. Record active
        // publication independently so an alias cannot link the frame to itself.
        if frame.active_heap.get().is_some() || frame.previous.borrow().is_some() {
            return Err(RuntimeError::TypeError(
                "Realm root frame is already active".into(),
            ));
        }
        frame.owner_view()?;
        let parent = self.current_realm_root_owner()?;
        let parent_guards = self.current_realm_context_guards()?;
        let caller = self.argument_list_owner()?;
        let ambient = if self.host_scheduling.is_none() {
            realm_reentrancy::reserve_active_native(0)?
        } else {
            None
        };
        let ambient_guards = if self.host_scheduling.is_none() {
            realm_reentrancy::active_context_guards()?
        } else {
            None
        };
        for object in frame.values.borrow().iter().copied() {
            self.heap.prototype(object)?;
        }
        frame.adopt_owner(caller.as_ref())?;
        let owner = frame.owner_view()?;
        let required = NativeContextGuards::capture(
            owner.as_ref(),
            [parent_guards.as_ref(), ambient_guards.as_ref()],
            [parent.as_ref(), caller.as_ref(), ambient.as_ref()],
        )?;
        *frame.callers.borrow_mut() = required.clone();
        frame.active_heap.set(Some(self.object_prototype.heap));
        *frame.previous.borrow_mut() = self.realm_call_roots.take();
        self.realm_call_roots = Some(frame.clone());
        let result = operation(self);
        let current = self.realm_call_roots.take().expect("published realm roots");
        assert!(
            Rc::ptr_eq(&current, &frame),
            "realm roots nest with their calls"
        );
        self.realm_call_roots = frame.previous.borrow_mut().take();
        frame.active_heap.set(None);
        *frame.callers.borrow_mut() = None;
        // Check both successful and failing callbacks after restoring the
        // caller's context. A healthy input owner cannot revive its caller.
        if let Some(owner) = owner {
            owner.reserve_peer(0)?;
        }
        if let Some(required) = required {
            required.validate()?;
        }
        result
    }

    pub(super) fn visit_realm_call_roots(&self, mut visit: impl FnMut(ObjectId)) {
        let mut frame = self.realm_call_roots.clone();
        while let Some(current) = frame {
            current.values.borrow().iter().copied().for_each(&mut visit);
            frame = current.previous.borrow().clone();
        }
    }
}
