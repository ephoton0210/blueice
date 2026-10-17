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

use super::*;
use std::cell::{Cell, OnceCell};

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/active_realm_storage.rs"]
mod storage_tests;

struct ActiveVm {
    tag: u64,
    ptr: Cell<*mut Vm>,
    scope: Option<SchedulingScope>,
    previous: OnceCell<Option<Rc<ActiveVm>>>,
    _storage: Option<host_scheduling::SharedSchedulingCharge>,
}

pub(super) struct ActiveRegistration {
    node: Rc<ActiveVm>,
}

impl ActiveRegistration {
    /// Publish only at the original cross-VM call boundary. Allocation and
    /// admission have already finished without capturing a VM pointer.
    pub(super) fn publish(self, vm: &mut Vm) -> ActiveGuard {
        debug_assert_eq!(self.node.tag, vm.object_prototype.heap);
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
        ActiveGuard { node: self.node }
    }
}

#[cfg(any(test, coverage))]
fn active_backing_bytes() -> usize {
    ACTIVE.with(|active| {
        let active = active.borrow();
        let mut node = active.as_deref();
        let mut bytes = 0usize;
        while let Some(current) = node {
            bytes += std::mem::size_of::<ActiveVm>();
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
    /// for its home realm) simply fails [`resolve_active`] and reports a
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
    let bytes = std::mem::size_of::<ActiveVm>();
    let storage = if let Some(owner) = scope.as_ref().or(owner_hint) {
        Some(owner.shared_charge(bytes)?)
    } else {
        // A neutral membrane nested inside bounded work still allocates
        // metadata for that call chain. Borrow its nearest owner's budget
        // without changing the neutral VM's captured policy.
        ACTIVE.with(|active| {
            let active = active.borrow();
            let mut node = active.as_deref();
            while let Some(current) = node {
                if let Some(owner) = &current._storage {
                    return owner.reserve_peer(bytes).map(Some);
                }
                node = current.previous.get().and_then(Option::as_deref);
            }
            Ok(None)
        })?
    };
    Ok(ActiveRegistration {
        node: Rc::new(ActiveVm {
            tag,
            ptr: Cell::new(std::ptr::null_mut()),
            scope,
            previous: OnceCell::new(),
            _storage: storage,
        }),
    })
}

pub(super) fn register_active(vm: &mut Vm) -> Result<ActiveGuard, RuntimeError> {
    let prepared = prepare_active_registration(vm, None)?;
    Ok(prepared.publish(vm))
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

/// Finds the most recently registered `Vm` for `tag`, if one is currently
/// mid-call on this thread's synchronous cross-realm call chain. See
/// `ACTIVE`'s own documentation for the safety argument governing every
/// caller of this function.
pub(super) fn resolve_active(tag: u64) -> Option<*mut Vm> {
    ACTIVE.with(|active| {
        let active = active.borrow();
        let mut node = active.as_deref();
        while let Some(current) = node {
            if current.tag == tag {
                return Some(current.ptr.get());
            }
            node = current.previous.get().and_then(Option::as_deref);
        }
        None
    })
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
    let ptr = resolve_active(target).ok_or_else(|| {
        RuntimeError::TypeError("the callable home realm is not available".into())
    })?;
    // SAFETY: ACTIVE owns this tag only for a suspended call's dynamic extent.
    // Its existing guard keeps the VM alive, and caller != target excludes self.
    unsafe { operation(&mut *ptr) }
}
