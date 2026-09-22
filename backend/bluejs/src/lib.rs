// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! BlueJS: BlueIce's own JavaScript engine
//! (`development/browser_core/phase-13-bluejs-engine/PLAN.md`).
//!
//! This crate currently provides the tokenizer/parser plus the managed
//! value/object layer that the bytecode compiler and interpreter build on.
//! It is scoped throughout to exactly `phase-2-mvp-scope/PLAN.md`'s
//! "MVP JS scope (decided)" section, not the full ECMAScript grammar.

mod analysis;
mod ast;
mod batch;
mod bytecode;
mod compiler;
mod event_loop;
mod heap;
mod interpreter;
mod parser;
mod script_host;
mod script_process;
mod token;
mod value;

pub use analysis::{
    CapabilitySummary, CapabilityUse, ScriptCapability, ScriptGatekeeperHook, analyze,
    analyze_program,
};
pub use ast::*;
pub use batch::{BatchResult, run_batch};
pub use bytecode::{
    ArrayBindingElement, BindingKey, BindingPattern, BytecodeFunction, BytecodeModule,
    CompiledParameter, Constant, Instruction, ObjectBindingProperty, Opcode,
};
pub use compiler::{CompileError, compile};
pub use event_loop::EventLoop;
pub use heap::{
    EnvironmentId, Heap, HeapError, HeapLimits, HeapRoots, HeapStats, HostObjectKind, ObjectAccess,
    ObjectId, ObjectKind,
};
pub use interpreter::{ClassListOperation, DEFAULT_INSTRUCTION_BUDGET, DomHost, Vm, VmError};
pub use parser::{ParseError, parse};
pub use script_host::ScriptDomClient;
pub use script_process::run_script_process;
pub use value::Value;
