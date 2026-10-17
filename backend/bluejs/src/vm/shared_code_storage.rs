// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Immutable child code carries its own header/buffer lease through every user.

use super::host_scheduling::{
    cloned_code_bytes, owned_code_bytes, SharedNativeHeader, SharedSchedulingCharge,
};
use super::*;

struct SharedCodeOwner {
    code: Bytecode,
    identity: Option<Rc<()>>,
    runtime: bool,
    charge: RefCell<Option<SharedSchedulingCharge>>,
    header: Option<Rc<SharedNativeHeader>>,
}

#[derive(Clone)]
pub(crate) struct SharedBytecode(Rc<SharedCodeOwner>);

pub(super) struct WeakCode {
    // Free the allocation before refunding its separately retained header.
    code: std::rc::Weak<SharedCodeOwner>,
    _header: Option<Rc<SharedNativeHeader>>,
}
impl WeakCode {
    pub(super) fn upgrade(&self) -> Option<SharedBytecode> {
        self.code.upgrade().map(SharedBytecode)
    }
    pub(super) fn live(&self) -> bool {
        self.code.strong_count() != 0
    }
}
impl std::ops::Deref for SharedBytecode {
    type Target = Bytecode;
    fn deref(&self) -> &Bytecode {
        &self.0.code
    }
}
impl AsRef<Bytecode> for SharedBytecode {
    fn as_ref(&self) -> &Bytecode {
        &self.0.code
    }
}
fn resize(charge: &mut Option<SharedSchedulingCharge>, bytes: usize) -> Result<(), RuntimeError> {
    if let Some(charge) = charge {
        if bytes <= charge.bytes() {
            charge.release_to(bytes);
        } else {
            charge.resize(bytes)?;
        }
    }
    Ok(())
}
fn header_bytes(separate: bool) -> usize {
    std::mem::size_of::<SharedCodeOwner>()
        + usize::from(separate) * std::mem::size_of::<SharedNativeHeader>()
}
impl SharedBytecode {
    pub(super) fn host_identity(&self) -> usize {
        Rc::as_ptr(
            self.0
                .identity
                .as_ref()
                .expect("only neutral host code has a cache identity"),
        ) as usize
    }
    pub(super) fn weak_identity(&self) -> std::rc::Weak<()> {
        Rc::downgrade(
            self.0
                .identity
                .as_ref()
                .expect("neutral host cache identity"),
        )
    }
    pub(super) fn weak(&self) -> WeakCode {
        WeakCode {
            code: Rc::downgrade(&self.0),
            _header: self.0.header.clone(),
        }
    }
    pub(crate) fn is_runtime(&self) -> bool {
        self.0.runtime
    }
    pub(super) fn copy_host(
        &self,
        budget: &HeapNativeBudget,
        prepare: impl FnOnce(&mut Bytecode) -> Result<(), RuntimeError>,
    ) -> Result<Self, RuntimeError> {
        let header = header_bytes(true);
        let mut charge = budget.reserve(header.saturating_add(cloned_code_bytes(self)))?;
        let mut code = self.0.code.clone();
        charge.resize(header.saturating_add(owned_code_bytes(&code)))?;
        prepare(&mut code)?;
        charge.resize(header.saturating_add(owned_code_bytes(&code)))?;
        let header = SharedNativeHeader::new(charge.split_off(header));
        Ok(Self(Rc::new(SharedCodeOwner {
            code,
            identity: None,
            runtime: true,
            charge: RefCell::new(Some(charge)),
            header: Some(header),
        })))
    }
    pub(crate) fn new(code: Bytecode) -> Self {
        Self(Rc::new(SharedCodeOwner {
            code,
            identity: Some(Rc::new(())),
            runtime: false,
            charge: RefCell::new(None),
            header: None,
        }))
    }
    pub(crate) fn retained_bytes(&self) -> usize {
        header_bytes(self.0.header.is_some()).saturating_add(owned_code_bytes(self))
    }
    /// Compiler-only access before runtime ownership or sharing is installed.
    pub(crate) fn get_mut(&mut self) -> Option<&mut Bytecode> {
        let owner = Rc::get_mut(&mut self.0).filter(|owner| !owner.runtime)?;
        Self::invalidate_identity(owner);
        Some(&mut owner.code)
    }
    fn invalidate_identity(owner: &mut SharedCodeOwner) {
        if owner
            .identity
            .as_ref()
            .is_some_and(|identity| Rc::weak_count(identity) != 0)
        {
            owner.identity = Some(Rc::new(()));
        }
    }
    pub(super) fn enable_runtime(
        &mut self,
        scope: Option<&SchedulingScope>,
    ) -> Result<(), RuntimeError> {
        let owner = Rc::get_mut(&mut self.0)
            .ok_or_else(|| scope.map_or(RuntimeError::SchedulingLimit, SchedulingScope::fail))?;
        owner.runtime = true;
        owner.identity = None;
        if let Some(scope) = scope {
            self.attach_runtime_sources(&scope.heap_native_budget())?;
        }
        Ok(())
    }
    pub(crate) fn attach_runtime_sources(
        &self,
        budget: &HeapNativeBudget,
    ) -> Result<(), RuntimeError> {
        if self.0.runtime {
            budget.reserve(0)?;
            let mut charge = self.0.charge.borrow_mut();
            if let Some(owner) = charge.as_ref() {
                if !budget.owns(owner) {
                    return Err(budget.fail());
                }
            } else {
                *charge = Some(budget.reserve(self.retained_bytes())?);
            }
        }
        self.0.code.attach_runtime_sources(budget)
    }
    /// Admit a full coexisting clone and reconcile buffers even after editor errors.
    pub(crate) fn edit<R>(
        &mut self,
        edit: impl FnOnce(&mut Bytecode) -> R,
    ) -> Result<R, RuntimeError> {
        if Rc::get_mut(&mut self.0).is_none() {
            let owner = &self.0;
            let separate = owner.header.is_some();
            let header_bytes = header_bytes(separate);
            let hint = header_bytes.saturating_add(cloned_code_bytes(&owner.code));
            let mut charge = owner
                .charge
                .borrow()
                .as_ref()
                .map(|charge| charge.reserve_peer(hint))
                .transpose()?;
            let code = owner.code.clone();
            resize(
                &mut charge,
                header_bytes.saturating_add(owned_code_bytes(&code)),
            )?;
            let header = if separate {
                Some(SharedNativeHeader::new(
                    charge
                        .as_mut()
                        .expect("a cached document copy has a charge")
                        .split_off(header_bytes),
                ))
            } else {
                None
            };
            let runtime = owner.runtime;
            self.0 = Rc::new(SharedCodeOwner {
                code,
                identity: (!runtime).then(|| Rc::new(())),
                runtime,
                charge: RefCell::new(charge),
                header,
            });
        }
        let owner = Rc::get_mut(&mut self.0).expect("the edited code owner is unique");
        Self::invalidate_identity(owner);
        let result = edit(&mut owner.code);
        let bytes = owned_code_bytes(&owner.code).saturating_add(if owner.header.is_none() {
            std::mem::size_of::<SharedCodeOwner>()
        } else {
            0
        });
        resize(owner.charge.get_mut(), bytes)?;
        Ok(result)
    }
}
