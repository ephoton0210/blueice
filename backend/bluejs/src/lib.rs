// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! BlueJS: BlueIce's own JavaScript engine
//! (`development/browser_core/phase-13-bluejs-engine/PLAN.md`).
//!
//! The front end tokenizes/parses the Phase 2 language subset into an
//! AST. The runtime-storage slice adds [`Value`] and [`Heap`]: ordinary
//! data-property objects with stable handles, explicit roots, prototype
//! lookup, and a two-generation collector. The bytecode compiler/VM,
//! arrays/functions, event loop, and browser integration remain separate
//! checklist items; a parsed program cannot be executed yet.
//!
//! Objects held across allocating calls must be rooted or reachable
//! from a root. The heap's public API includes collection so this
//! contract is testable before an interpreter or browser host exists:
//!
//! ```
//! use blueice_bluejs::{Heap, Value};
//!
//! let mut heap = Heap::default();
//! let global = heap.alloc_object(None)?;
//! let root = heap.root(global)?;
//! let object = heap.alloc_object(None)?;
//! heap.set(global, "widget", Value::Object(object))?;
//! heap.set(object, "label", Value::String("BlueIce".into()))?;
//! heap.collect_minor();
//! assert_eq!(heap.get(object, "label")?, Value::String("BlueIce".into()));
//! heap.unroot(root)?;
//! heap.collect_major();
//! assert!(!heap.contains(object));
//! # Ok::<(), blueice_bluejs::HeapError>(())
//! ```

mod ast;
mod heap;
mod parser;
mod token;
mod value;

pub use ast::*;
pub use heap::{Heap, HeapConfig, HeapError, HeapStats, RootId};
pub use parser::{parse, ParseError};
pub use value::{ObjectId, Value};
