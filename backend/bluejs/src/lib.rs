// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! BlueJS: BlueIce's own JavaScript engine
//! (`development/browser_core/phase-13-bluejs-engine/PLAN.md`).
//!
//! The front end tokenizes/parses the Phase 2 language subset into an
//! AST. The runtime-storage slices add [`Value`] and [`Heap`]: ordinary
//! objects and sparse arrays with stable handles, explicit roots, prototype
//! lookup, and a two-generation collector. [`compile`] and [`Vm`] execute
//! the first subset via operand-stack bytecode: primitive expressions,
//! bindings, control flow, ordinary objects and array literals/indexing.
//! Arrays preserve holes and enforce length growth/truncation. Functions,
//! native methods, the event loop and browser integration remain future work.
//! The ECMAScript 2026 track adds lexical TDZ, radix/separator Number
//! literals and short-circuit/newline grammar corrections. It is not yet
//! a complete edition 17 implementation (notably strings still use UTF-8).
//!
//! ```
//! use blueice_bluejs::{compile, parse, Value, Vm};
//!
//! let program = parse("let a=[1,2,3,4,5]; let sum=0; for(let i=0;i<a.length;i++){sum+=a[i];} sum").unwrap();
//! let code = compile(&program)?;
//! assert_eq!(Vm::default().execute(&code)?, Value::Number(15.0));
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! Objects held across allocating calls must be rooted or reachable
//! from a root. The heap's public API includes collection so this
//! contract is testable independently of the interpreter or browser host:
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
mod bytecode;
mod compiler;
mod heap;
mod parser;
mod primitive;
mod token;
mod value;
mod vm;

pub use ast::*;
pub use bytecode::{Bytecode, Instruction, Opcode, MAY_USE_INLINE_CACHE};
pub use compiler::{compile, compile_with_limit, CompileError};
pub use heap::{Heap, HeapConfig, HeapError, HeapStats, RootId};
pub use parser::{parse, ParseError};
pub use value::{ObjectId, Value};
pub use vm::{RuntimeError, Vm, VmConfig};
