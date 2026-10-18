// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Temporary realm-call roots own admitted IDs without copying guest values.

use super::host_scheduling::SharedSchedulingCharge;
use super::*;

pub(super) struct RealmCallRoots {
    values: RefCell<Vec<ObjectId>>,
    previous: RefCell<Option<Rc<Self>>>,
    // The backing allocation is destroyed before its native lease.
    _storage: Option<SharedSchedulingCharge>,
}

impl RealmCallRoots {
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
            _storage: storage,
        }))
    }

    pub(super) fn push(&self, id: ObjectId) {
        let mut values = self.values.borrow_mut();
        assert!(
            values.len() < values.capacity(),
            "realm roots were prepared before guest entry"
        );
        values.push(id);
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

    pub(super) fn invoke(mut self, vm: &mut Vm, target: ObjectId) -> Result<Value, RuntimeError> {
        self.entry.retarget_native(vm.host_scheduling.as_ref())?;
        let result = vm.enter_call_owned(
            Value::Object(target),
            self.receiver,
            self.arguments,
            false,
            Value::Undefined,
            self.entry,
        );
        vm.reconcile_scheduling_ownership();
        result
    }
}

impl Vm {
    pub(super) fn realm_call_native_owner(
        &self,
        peer: &Vm,
    ) -> Result<Option<SharedSchedulingCharge>, RuntimeError> {
        match self
            .host_scheduling
            .as_ref()
            .or(peer.host_scheduling.as_ref())
        {
            Some(scope) => scope.shared_charge(0).map(Some),
            None => realm_reentrancy::reserve_active_native(0),
        }
    }

    /// An explicit dynamic root frame, restored on both ordinary success and
    /// error. Nested/reentrant calls link their own independently owned frames.
    pub(super) fn with_realm_call_roots<T>(
        &mut self,
        frame: Rc<RealmCallRoots>,
        operation: impl FnOnce(&mut Self) -> Result<T, RuntimeError>,
    ) -> Result<T, RuntimeError> {
        assert!(
            frame.previous.borrow().is_none(),
            "publish one root frame at a time"
        );
        *frame.previous.borrow_mut() = self.realm_call_roots.take();
        self.realm_call_roots = Some(frame.clone());
        let result = operation(self);
        let current = self.realm_call_roots.take().expect("published realm roots");
        assert!(
            Rc::ptr_eq(&current, &frame),
            "realm roots nest with their calls"
        );
        self.realm_call_roots = frame.previous.borrow_mut().take();
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
