// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Borrowable realm cells and weak callable homes retain allocation ownership.

use super::host_scheduling::SharedSchedulingCharge;
use super::*;
use std::cell::{BorrowError, BorrowMutError, Ref, RefMut};
use std::rc::Weak;

struct CellStorage {
    owner: RefCell<Option<SharedSchedulingCharge>>,
}
impl CellStorage {
    fn bytes() -> usize {
        std::mem::size_of::<RefCell<Vm>>()
            + std::mem::size_of::<Self>()
            + 4 * std::mem::size_of::<usize>()
    }
    fn attach(&self, scope: &SchedulingScope) -> Result<(), RuntimeError> {
        let mut owner = self.owner.borrow_mut();
        if let Some(owner) = owner.as_ref() {
            if !owner.belongs_to(scope) {
                return Err(scope.fail());
            }
        } else {
            *owner = Some(scope.shared_charge(Self::bytes())?);
        }
        Ok(())
    }
}

#[derive(Clone)]
pub(super) struct RealmHandle {
    value: Rc<RefCell<Vm>>,
    // Value/weak allocation drops before its retained allocation credit.
    storage: Rc<CellStorage>,
    heap: u64,
}
#[derive(Clone)]
pub(super) struct RealmWeak {
    value: Weak<RefCell<Vm>>,
    storage: Rc<CellStorage>,
    heap: u64,
}

impl RealmHandle {
    pub(super) fn new(vm: Vm) -> Result<Self, RuntimeError> {
        let owner = vm
            .host_scheduling
            .as_ref()
            .map(|scope| scope.shared_charge(CellStorage::bytes()))
            .transpose()?;
        let heap = vm.object_prototype.heap;
        let handle = Self {
            value: Rc::new(RefCell::new(vm)),
            storage: Rc::new(CellStorage {
                owner: RefCell::new(owner),
            }),
            heap,
        };
        handle.value.borrow_mut().realm_home = Some(handle.downgrade());
        Ok(handle)
    }
    pub(super) fn heap(&self) -> u64 {
        self.heap
    }
    pub(super) fn downgrade(&self) -> RealmWeak {
        RealmWeak {
            value: Rc::downgrade(&self.value),
            storage: self.storage.clone(),
            heap: self.heap,
        }
    }
    pub(super) fn borrow(&self) -> Ref<'_, Vm> {
        self.value.borrow()
    }
    pub(super) fn borrow_mut(&self) -> RefMut<'_, Vm> {
        self.value.borrow_mut()
    }
    pub(super) fn try_borrow(&self) -> Result<Ref<'_, Vm>, BorrowError> {
        self.value.try_borrow()
    }
    pub(super) fn try_borrow_mut(&self) -> Result<RefMut<'_, Vm>, BorrowMutError> {
        self.value.try_borrow_mut()
    }
    pub(super) fn read<T>(
        &self,
        caller: u64,
        operation: impl FnOnce(&Vm) -> Result<T, RuntimeError>,
    ) -> Result<T, RuntimeError> {
        if caller == self.heap {
            return Err(RuntimeError::TypeError(
                "a realm membrane cannot resolve to itself".into(),
            ));
        }
        match self.try_borrow() {
            Ok(vm) => operation(&vm),
            Err(_) => realm_reentrancy::with_active_vm(caller, self.heap, |vm| operation(vm)),
        }
    }
    pub(super) fn edit<T>(
        &self,
        caller: u64,
        operation: impl FnOnce(&mut Vm) -> Result<T, RuntimeError>,
    ) -> Result<T, RuntimeError> {
        if caller == self.heap {
            return Err(RuntimeError::TypeError(
                "a realm membrane cannot resolve to itself".into(),
            ));
        }
        match self.try_borrow_mut() {
            Ok(mut vm) => operation(&mut vm),
            Err(_) => realm_reentrancy::with_active_vm(caller, self.heap, operation),
        }
    }
    #[cfg(any(test, coverage))]
    pub(super) fn strong_count(&self) -> usize {
        Rc::strong_count(&self.value)
    }
}
impl RealmWeak {
    pub(super) fn upgrade(&self) -> Option<RealmHandle> {
        self.value.upgrade().map(|value| RealmHandle {
            value,
            storage: self.storage.clone(),
            heap: self.heap,
        })
    }
    pub(super) fn attach(&self, scope: &SchedulingScope) -> Result<(), RuntimeError> {
        self.storage.attach(scope)
    }
    #[cfg(any(test, coverage))]
    pub(super) fn strong_count(&self) -> usize {
        self.value.strong_count()
    }
}
