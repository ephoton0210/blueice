// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The reentrancy primitive shared by every cross-`Vm` boundary in this
//! crate: `ShadowRealm`'s `WrappedFunctionCreate` facades
//! ([`super::shadow_realm`]) and Test262's `$262.createRealm()` reverse
//! membrane ([`super::test262::reverse`]) both need two independently-owned
//! `Vm`s to call back into each other without violating Rust's aliasing
//! rules, including multi-hop chains and proper GC rooting. This module is a
//! pure extraction of what was originally `shadow_realm.rs`'s own
//! private machinery -- moved here, unchanged, so a second boundary
//! (Test262's reverse facades) can reuse it rather than reimplementing it.

use super::*;

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
    static ACTIVE: RefCell<Vec<(u64, *mut Vm)>> = const { RefCell::new(Vec::new()) };
}

pub(super) struct ActiveGuard {
    tag: u64,
}

impl Drop for ActiveGuard {
    fn drop(&mut self) {
        ACTIVE.with(|active| {
            let popped = active.borrow_mut().pop();
            debug_assert!(
                popped.is_some_and(|(tag, _)| tag == self.tag),
                "cross-realm active-Vm registrations must nest with the Rust call stack"
            );
        });
    }
}

/// Registers `vm` as reachable by its own heap tag for the dynamic extent of
/// the returned guard. Call this immediately before `vm` calls into another
/// realm, and keep the guard alive across exactly that call.
pub(super) fn register_active(vm: &mut Vm) -> ActiveGuard {
    let tag = vm.object_prototype.heap;
    ACTIVE.with(|active| active.borrow_mut().push((tag, vm as *mut Vm)));
    ActiveGuard { tag }
}

/// Finds the most recently registered `Vm` for `tag`, if one is currently
/// mid-call on this thread's synchronous cross-realm call chain. See
/// `ACTIVE`'s own documentation for the safety argument governing every
/// caller of this function.
pub(super) fn resolve_active(tag: u64) -> Option<*mut Vm> {
    ACTIVE.with(|active| {
        active
            .borrow()
            .iter()
            .rev()
            .find(|(t, _)| *t == tag)
            .map(|(_, ptr)| *ptr)
    })
}
