// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Stub extension-host process.
//!
//! Extensions run in their own OS process, isolated from `core` by a
//! process boundary and IPC (`BROWSER_CORE_PLAN.md` §1, Process
//! architecture) -- not in-process sandboxing (settled decision: an
//! extension crashing or corrupting its own memory must be physically
//! incapable of reaching `core`'s address space). No real extension API
//! ships in MVP (plan §4); this binary exists so the process/IPC seam is
//! in place from Phase 3 onward rather than retrofitted later.

fn main() {
    todo!("extension host process -- see BROWSER_CORE_PLAN.md §1 (Process architecture)")
}
