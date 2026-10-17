// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Immutable child code carries its own header/buffer lease through every user.

use super::host_scheduling::{cloned_code_bytes, owned_code_bytes, SharedSchedulingCharge};
use super::*;

struct SharedCodeOwner {
    // The code and its descendants drop before this allocation's lease.
    code: Bytecode,
    runtime: bool,
    charge: RefCell<Option<SharedSchedulingCharge>>,
}

#[derive(Clone)]
pub(crate) struct SharedBytecode(Rc<SharedCodeOwner>);

impl std::ops::Deref for SharedBytecode {
    type Target = Bytecode;
    fn deref(&self) -> &Self::Target {
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

impl SharedBytecode {
    pub(crate) fn new(code: Bytecode) -> Self {
        Self(Rc::new(SharedCodeOwner {
            code,
            runtime: false,
            charge: RefCell::new(None),
        }))
    }

    pub(crate) fn retained_bytes(&self) -> usize {
        std::mem::size_of::<SharedCodeOwner>().saturating_add(owned_code_bytes(self))
    }

    /// Compiler-only access before runtime ownership or sharing is installed.
    pub(crate) fn get_mut(&mut self) -> Option<&mut Bytecode> {
        Rc::get_mut(&mut self.0)
            .filter(|owner| !owner.runtime)
            .map(|owner| &mut owner.code)
    }

    pub(super) fn enable_runtime(
        &mut self,
        scope: Option<&SchedulingScope>,
    ) -> Result<(), RuntimeError> {
        Rc::get_mut(&mut self.0)
            .ok_or_else(|| scope.map_or(RuntimeError::SchedulingLimit, SchedulingScope::fail))?
            .runtime = true;
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

    /// Admit a full coexisting clone before copying shared runtime code. Always
    /// reconcile edited buffers, including when the editor returns its own error.
    pub(crate) fn edit<R>(
        &mut self,
        edit: impl FnOnce(&mut Bytecode) -> R,
    ) -> Result<R, RuntimeError> {
        if Rc::get_mut(&mut self.0).is_none() {
            let owner = &self.0;
            let hint = std::mem::size_of::<SharedCodeOwner>()
                .saturating_add(cloned_code_bytes(&owner.code));
            let mut charge = owner
                .charge
                .borrow()
                .as_ref()
                .map(|charge| charge.reserve_peer(hint))
                .transpose()?;
            let code = owner.code.clone();
            resize(
                &mut charge,
                std::mem::size_of::<SharedCodeOwner>().saturating_add(owned_code_bytes(&code)),
            )?;
            let runtime = owner.runtime;
            self.0 = Rc::new(SharedCodeOwner {
                code,
                runtime,
                charge: RefCell::new(charge),
            });
        }
        let owner = Rc::get_mut(&mut self.0).expect("the edited code owner is unique");
        let result = edit(&mut owner.code);
        resize(
            owner.charge.get_mut(),
            std::mem::size_of::<SharedCodeOwner>().saturating_add(owned_code_bytes(&owner.code)),
        )?;
        Ok(result)
    }
}
