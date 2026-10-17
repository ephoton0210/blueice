// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Observe one finite unit-test operation; allocation always delegates to System.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

thread_local! {
    static ENABLED: Cell<bool> = const { Cell::new(false) };
    static CALLS: Cell<usize> = const { Cell::new(0) };
}
struct ObservedSystem;
#[global_allocator]
static ALLOCATOR: ObservedSystem = ObservedSystem;

fn record() {
    if ENABLED.try_with(Cell::get).unwrap_or(false) {
        let _ = CALLS.try_with(|calls| calls.set(calls.get().saturating_add(1)));
    }
}
// SAFETY: Every operation forwards its exact pointer/layout to System. The
// observer only updates const-initialized thread-local scalar cells; it never
// allocates, refuses an allocation, changes its size or affects another thread.
unsafe impl GlobalAlloc for ObservedSystem {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        record();
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        record();
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        record();
        unsafe { System.realloc(pointer, layout, size) }
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) }
    }
}
struct Observation;
impl Drop for Observation {
    fn drop(&mut self) {
        ENABLED.with(|enabled| enabled.set(false));
    }
}
pub(super) fn count<T>(operation: impl FnOnce() -> T) -> (T, usize) {
    ENABLED.with(|enabled| assert!(!enabled.get(), "observations cannot nest"));
    CALLS.with(|calls| calls.set(0));
    ENABLED.with(|enabled| enabled.set(true));
    let observation = Observation;
    let result = operation();
    drop(observation);
    (result, CALLS.with(Cell::get))
}
