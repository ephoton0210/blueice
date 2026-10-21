// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The reentrancy primitive shared by every cross-`Vm` boundary in this
//! crate: `ShadowRealm`'s `WrappedFunctionCreate` facades
//! ([`super::shadow_realm`]) and Test262's `$262.createRealm()` reverse
//! membrane ([`super::test262::reverse`]) both need two independently-owned
//! `Vm`s to call back into each other without violating Rust's aliasing
//! rules, including multi-hop chains and proper GC rooting. This module is a
//! shared primitive. Each registration owns its admitted metadata; preparation
//! precedes publication so the original VM-pointer validity window stays exact.

use super::native_context_guards::NativeContextGuards;
use super::*;
use std::cell::{Cell, OnceCell};

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/active_realm_storage.rs"]
mod storage_tests;

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/active_realm_publication.rs"]
mod publication_tests;

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/active_realm_resolution.rs"]
mod resolution_tests;

struct ActiveVm {
    tag: u64,
    ptr: Cell<*mut Vm>,
    scope: Option<SchedulingScope>,
    previous: OnceCell<Option<Rc<ActiveVm>>>,
    callers: Option<Rc<NativeContextGuards>>,
    _storage: Option<host_scheduling::SharedSchedulingCharge>,
}

fn active_allocation_bytes() -> usize {
    std::mem::size_of::<ActiveVm>() + 2 * std::mem::size_of::<usize>()
}

pub(super) struct ActiveRegistration {
    node: Rc<ActiveVm>,
}

impl ActiveRegistration {
    /// Publish only at the original cross-VM call boundary. Allocation and
    /// admission have already finished without capturing a VM pointer.
    pub(super) fn publish(self, vm: &mut Vm) -> Result<ActiveGuard, RuntimeError> {
        let same_policy = match (&self.node.scope, &vm.host_scheduling) {
            (Some(prepared), Some(current)) => prepared.same_document(current),
            (None, None) => true,
            _ => false,
        };
        if self.node.tag != vm.object_prototype.heap || !same_policy {
            return Err(RuntimeError::TypeError(
                "prepared realm registration no longer matches its VM".into(),
            ));
        }
        // Conversions between preparation and publication may cancel the
        // original account or a retained caller. Refuse before publishing a
        // pointer or taking the previous registration; dropping self refunds
        // the prepared metadata even when its owning account has failed.
        vm.check_scheduling_failure()?;
        vm.current_realm_root_owner()?;
        if let Some(owner) = &self.node._storage {
            owner.reserve_peer(0)?;
        }
        if let Some(callers) = &self.node.callers {
            callers.validate()?;
        }
        self.node.ptr.set(vm as *mut Vm);
        ACTIVE.with(|active| {
            let mut active = active.borrow_mut();
            let previous = active.take();
            assert!(
                self.node.previous.set(previous).is_ok(),
                "one prepared active registration publishes once"
            );
            *active = Some(self.node.clone());
        });
        Ok(ActiveGuard { node: self.node })
    }
}

#[cfg(any(test, coverage))]
fn active_backing_bytes() -> usize {
    ACTIVE.with(|active| {
        let active = active.borrow();
        let mut node = active.as_deref();
        let mut bytes = 0usize;
        while let Some(current) = node {
            bytes += active_allocation_bytes();
            node = current.previous.get().and_then(Option::as_deref);
        }
        bytes
    })
}

thread_local! {
    /// A stack of `Vm`s currently mid-call as part of one synchronous
    /// cross-realm call chain on this thread, most recently registered last.
    ///
    /// Safety model: [`register_active`] pushes a raw pointer to `self`
    /// immediately before `self` calls into another realm, and the returned
    /// [`ActiveGuard`] pops it the instant that nested call returns -- i.e.
    /// its validity window is exactly the dynamic extent of the `&mut self`
    /// call already in progress on the Rust stack when it was pushed. A
    /// `Vm` value can freely be moved by its owner *between* top-level
    /// calls (nothing here assumes a `Vm` is pinned in memory); what must
    /// not happen, and does not, is moving it *while* one of its own
    /// methods holds `&mut self` -- Rust already guarantees that on our
    /// behalf for the exact same reason a safe `&mut` borrow would.
    ///
    /// A facade that a realm retains and calls again long after the
    /// synchronous call that produced it has returned (so no entry remains
    /// for its home realm) simply fails [`resolve_active_entry`] and reports a
    /// catchable error, rather than dereferencing memory whose lifetime has
    /// already ended.
    static ACTIVE: RefCell<Option<Rc<ActiveVm>>> = const { RefCell::new(None) };
}

pub(super) struct ActiveGuard {
    node: Rc<ActiveVm>,
}

impl Drop for ActiveGuard {
    fn drop(&mut self) {
        ACTIVE.with(|active| {
            let mut active = active.borrow_mut();
            debug_assert!(
                active
                    .as_ref()
                    .is_some_and(|node| Rc::ptr_eq(node, &self.node)),
                "cross-realm active-Vm registrations must nest with the Rust call stack"
            );
            *active = self.node.previous.get().and_then(Option::as_ref).cloned();
        });
    }
}

/// Admit and allocate metadata before changing either realm's execution state.
/// This preparation captures its tag and policy, but no VM pointer; publish the
/// result only at the original cross-realm call boundary.
pub(super) fn prepare_active_registration(
    vm: &mut Vm,
    owner_hint: Option<&SchedulingScope>,
) -> Result<ActiveRegistration, RuntimeError> {
    vm.synchronize_scheduling_usage()?;
    let tag = vm.object_prototype.heap;
    // Host policy cannot change while this synchronous call owns the VM.
    // Keep a zero-owning account view for reentrant compilation without
    // dereferencing the active VM merely to inspect its scheduling policy.
    let scope = vm.host_scheduling.as_ref().map(SchedulingScope::child);
    let bytes = active_allocation_bytes();
    let root_owner = vm.current_realm_root_owner()?;
    let root_guards = vm.current_realm_context_guards()?;
    let borrow_ambient = scope.is_none() && (root_owner.is_some() || owner_hint.is_none());
    let ambient = if borrow_ambient {
        reserve_active_native(0)?
    } else {
        None
    };
    let ambient_guards = if borrow_ambient {
        active_context_guards()?
    } else {
        None
    };
    let storage = if let Some(owner) = scope.as_ref() {
        Some(owner.shared_charge(bytes)?)
    } else if let Some(owner) = &root_owner {
        reserve_active_native(0)?;
        Some(owner.reserve_peer(bytes)?)
    } else if let Some(owner) = owner_hint {
        Some(owner.shared_charge(bytes)?)
    } else {
        // A neutral membrane nested inside bounded work still allocates
        // metadata for that call chain. Borrow its nearest owner's budget
        // without changing the neutral VM's captured policy.
        reserve_active_native(bytes)?
    };
    let callers = NativeContextGuards::capture(
        storage.as_ref(),
        [root_guards.as_ref(), ambient_guards.as_ref()],
        [root_owner.as_ref(), ambient.as_ref(), None],
    )?;
    Ok(ActiveRegistration {
        node: Rc::new(ActiveVm {
            tag,
            ptr: Cell::new(std::ptr::null_mut()),
            scope,
            previous: OnceCell::new(),
            callers,
            _storage: storage,
        }),
    })
}

/// Borrow only the nearest native account; do not resolve or access a VM.
pub(super) fn reserve_active_native(
    bytes: usize,
) -> Result<Option<host_scheduling::SharedSchedulingCharge>, RuntimeError> {
    ACTIVE.with(|active| {
        let active = active.borrow();
        let mut node = active.as_deref();
        while let Some(current) = node {
            if let Some(callers) = &current.callers {
                callers.validate()?;
            }
            if let Some(owner) = &current._storage {
                return owner.reserve_peer(bytes).map(Some);
            }
            node = current.previous.get().and_then(Option::as_deref);
        }
        Ok(None)
    })
}

/// Only copy immutable guard metadata; never access the registered VM pointer.
pub(super) fn active_context_guards() -> Result<Option<Rc<NativeContextGuards>>, RuntimeError> {
    ACTIVE.with(|active| {
        let active = active.borrow();
        let mut node = active.as_deref();
        while let Some(current) = node {
            if let Some(callers) = &current.callers {
                callers.validate()?;
            }
            if let Some(owner) = &current._storage {
                owner.reserve_peer(0)?;
                return Ok(current.callers.clone());
            }
            node = current.previous.get().and_then(Option::as_deref);
        }
        Ok(None)
    })
}

pub(super) fn register_active(vm: &mut Vm) -> Result<ActiveGuard, RuntimeError> {
    let prepared = prepare_active_registration(vm, None)?;
    prepared.publish(vm)
}

/// Whether this call is nested inside a different Realm's suspended call.
/// Only inspect heap tags; no registered VM pointer is dereferenced.
pub(super) fn has_foreign_caller(tag: u64) -> bool {
    ACTIVE.with(|active| {
        let active = active.borrow();
        let mut node = active.as_deref();
        while let Some(current) = node {
            if current.tag != tag {
                return true;
            }
            node = current.previous.get().and_then(Option::as_deref);
        }
        false
    })
}

fn active_node(tag: u64) -> Option<Rc<ActiveVm>> {
    ACTIVE.with(|active| {
        let active = active.borrow();
        let mut node = active.as_ref();
        while let Some(current) = node {
            if current.tag == tag {
                return Some(Rc::clone(current));
            }
            node = current.previous.get().and_then(Option::as_ref);
        }
        None
    })
}

/// Topology queries must still see a suspended call after account failure.
pub(super) fn is_active(tag: u64) -> bool {
    active_node(tag).is_some()
}

/// Identity observations for internal fixtures; never dereference this pointer.
#[cfg(any(test, coverage))]
pub(super) fn resolve_active(tag: u64) -> Option<*mut Vm> {
    active_node(tag).map(|node| node.ptr.get())
}

/// Retains paid metadata, not VM ownership or a longer pointer validity window.
/// Only a live synchronous registration can lend its VM to `with`.
pub(super) struct ActiveVmEntry {
    node: Rc<ActiveVm>,
    caller: Option<Rc<ActiveVm>>,
}

impl ActiveVmEntry {
    fn unavailable() -> RuntimeError {
        RuntimeError::TypeError("the callable home realm is not available".into())
    }

    fn validate_node(node: &Rc<ActiveVm>) -> Result<(), RuntimeError> {
        let live = ACTIVE.with(|active| {
            let active = active.borrow();
            let mut current = active.as_ref();
            while let Some(candidate) = current {
                if Rc::ptr_eq(candidate, node) {
                    return true;
                }
                current = candidate.previous.get().and_then(Option::as_ref);
            }
            false
        });
        if !live || node.ptr.get().is_null() {
            return Err(Self::unavailable());
        }
        if let Some(owner) = &node._storage {
            owner.reserve_peer(0)?;
        }
        if let Some(callers) = &node.callers {
            callers.validate()?;
        }
        Ok(())
    }

    fn validate(&self) -> Result<(), RuntimeError> {
        Self::validate_node(&self.node)?;
        if let Some(caller) = &self.caller {
            Self::validate_node(caller)?;
        }
        Ok(())
    }

    /// Suspend caller access for the original synchronous ancestor operation.
    /// Both success and error validate the same metadata and current root state.
    pub(super) fn with<T>(
        self,
        operation: impl FnOnce(&mut Vm) -> Result<T, RuntimeError>,
    ) -> Result<T, RuntimeError> {
        self.validate()?;
        // SAFETY: the exact registration is still on ACTIVE, and resolution
        // rejected a self-reference. Its original ancestor call remains
        // suspended for this operation; no pointer escapes this reborrow.
        let vm = unsafe { &mut *self.node.ptr.get() };
        vm.check_scheduling_failure()?;
        vm.current_realm_root_owner()?;
        let result = operation(vm);
        vm.check_scheduling_failure()?;
        vm.current_realm_root_owner()?;
        self.validate()?;
        result
    }

    #[cfg(any(test, coverage))]
    pub(super) fn pointer(&self) -> *mut Vm {
        self.node.ptr.get()
    }
}

/// Resolve account identities without accessing any registered VM pointer.
pub(super) fn resolve_active_entry(
    caller: u64,
    target: u64,
) -> Result<Option<ActiveVmEntry>, RuntimeError> {
    let Some(node) = active_node(target) else {
        return Ok(None);
    };
    if caller == target {
        return Err(RuntimeError::TypeError(
            "a realm membrane cannot resolve to itself".into(),
        ));
    }
    let entry = ActiveVmEntry {
        node,
        caller: active_node(caller),
    };
    entry.validate()?;
    Ok(Some(entry))
}

/// A suspended same-document call still owns this synchronous job boundary.
pub(super) fn has_active_document(scope: &SchedulingScope) -> bool {
    ACTIVE.with(|active| {
        let active = active.borrow();
        let mut node = active.as_deref();
        while let Some(current) = node {
            if current
                .scope
                .as_ref()
                .is_some_and(|owner| owner.same_document(scope))
            {
                return true;
            }
            node = current.previous.get().and_then(Option::as_deref);
        }
        false
    })
}

/// Read only the account view captured by the active call's guard.
pub(super) fn active_scheduling_scope(tag: u64) -> Option<SchedulingScope> {
    ACTIVE.with(|active| {
        let active = active.borrow();
        let mut node = active.as_deref();
        while let Some(current) = node {
            if current.tag == tag {
                return current.scope.as_ref().map(SchedulingScope::child);
            }
            node = current.previous.get().and_then(Option::as_deref);
        }
        None
    })
}

/// Reborrow only a live suspended ancestor, with a distinct caller identity.
pub(super) fn with_active_vm<T>(
    caller: u64,
    target: u64,
    operation: impl FnOnce(&mut Vm) -> Result<T, RuntimeError>,
) -> Result<T, RuntimeError> {
    if caller == target {
        return Err(RuntimeError::TypeError(
            "a realm membrane cannot resolve to itself".into(),
        ));
    }
    let entry = resolve_active_entry(caller, target)?.ok_or_else(ActiveVmEntry::unavailable)?;
    entry.with(operation)
}
