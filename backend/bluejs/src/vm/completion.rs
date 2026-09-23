// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Completion records and interpreter exit state.

use crate::heap::{GeneratorHandlerFrame, GeneratorHandlerState, GeneratorPendingCompletion};
use crate::Value;

use super::RuntimeError;

#[derive(Clone)]
pub(super) enum Completion {
    Throw(RuntimeError),
    Return(Value),
    TailRecur(Vec<Value>),
    /// `[callee, this, arguments...]` of a call that replaces this frame.
    TailCall(Vec<Value>),
    Yield(Value),
    Jump {
        cleanup: usize,
        target: usize,
    },
    Resume(usize),
    Halt(Value),
}

impl Completion {
    /// Convert a catchable completion to the heap-owned representation used
    /// by a suspended generator. Every other RuntimeError is a host abort
    /// and is rejected by the interpreter before it reaches this boundary.
    pub(super) fn into_generator_pending(self) -> Result<GeneratorPendingCompletion, RuntimeError> {
        match self {
            Self::Throw(RuntimeError::Thrown(value)) => {
                Ok(GeneratorPendingCompletion::Throw(value))
            }
            Self::Throw(RuntimeError::ReferenceError(message)) => {
                Ok(GeneratorPendingCompletion::ReferenceError(message))
            }
            Self::Throw(RuntimeError::TypeError(message)) => {
                Ok(GeneratorPendingCompletion::TypeError(message))
            }
            Self::Throw(RuntimeError::RangeError(message)) => {
                Ok(GeneratorPendingCompletion::RangeError(message))
            }
            Self::Throw(RuntimeError::SyntaxError(message)) => {
                Ok(GeneratorPendingCompletion::SyntaxError(message))
            }
            Self::Throw(RuntimeError::Test262(message)) => {
                Ok(GeneratorPendingCompletion::Test262(message))
            }
            Self::Return(value) => Ok(GeneratorPendingCompletion::Return(value)),
            Self::TailRecur(values) => Ok(GeneratorPendingCompletion::TailRecur(values)),
            Self::Jump { cleanup, target } => {
                Ok(GeneratorPendingCompletion::Jump { cleanup, target })
            }
            Self::Throw(error) => Err(error),
            Self::TailCall(_) | Self::Yield(_) | Self::Resume(_) | Self::Halt(_) => Err(
                RuntimeError::Unsupported("cannot suspend a generator with an internal completion"),
            ),
        }
    }

    pub(super) fn from_generator_pending(completion: GeneratorPendingCompletion) -> Self {
        match completion {
            GeneratorPendingCompletion::Throw(value) => Self::Throw(RuntimeError::Thrown(value)),
            GeneratorPendingCompletion::ReferenceError(message) => {
                Self::Throw(RuntimeError::ReferenceError(message))
            }
            GeneratorPendingCompletion::TypeError(message) => {
                Self::Throw(RuntimeError::TypeError(message))
            }
            GeneratorPendingCompletion::RangeError(message) => {
                Self::Throw(RuntimeError::RangeError(message))
            }
            GeneratorPendingCompletion::SyntaxError(message) => {
                Self::Throw(RuntimeError::SyntaxError(message))
            }
            GeneratorPendingCompletion::Test262(message) => {
                Self::Throw(RuntimeError::Test262(message))
            }
            GeneratorPendingCompletion::Return(value) => Self::Return(value),
            GeneratorPendingCompletion::TailRecur(values) => Self::TailRecur(values),
            GeneratorPendingCompletion::Jump { cleanup, target } => Self::Jump { cleanup, target },
        }
    }
}

pub(super) type HandlerState = GeneratorHandlerState;
pub(super) type HandlerFrame = GeneratorHandlerFrame;

pub(super) enum CompletionAction {
    Continue,
    Jump(usize),
    Return(Value),
    TailRecur(Vec<Value>),
    TailCall(Vec<Value>),
    Throw(RuntimeError),
}

pub(super) enum InterpreterExit {
    Return(Value),
    Yield {
        value: Value,
        pc: usize,
        iterators: Vec<Value>,
        handlers: Vec<HandlerFrame>,
    },
    Suspend {
        pc: usize,
        iterators: Vec<Value>,
        handlers: Vec<HandlerFrame>,
    },
    /// An async execution context reaches an Await expression. Its execution
    /// context is moved into a continuation before the next Promise job turn
    /// resumes it.
    Await {
        promise: crate::ObjectId,
        pc: usize,
        handlers: Vec<HandlerFrame>,
    },
}

// Ordinary calls nest the Rust interpreter, so how deep JavaScript may recurse
// is a question about the native stack, and running out of it is a process
// abort rather than a catchable error. `enter_call` therefore refuses to nest
// another call, with a catchable RangeError, once fewer than
// `CALL_STACK_RED_ZONE` bytes remain on the current thread's own stack
// (`native_stack::remaining_stack`, which asks the OS about the thread actually
// running the VM: a normal process's main thread, a smaller worker, or a
// deliberately tiny one all get the right budget without any per-platform
// constant here). Cheap and expensive frames are not distinguished: an
// interpreted call costs about the same native stack whatever the script
// does, so depth follows the stack the host provisioned.
//
// The guard deliberately errors instead of growing the stack onto a freshly
// allocated segment. The
// stack is the runaway-recursion boundary: growing it would leave only
// `instruction_budget` (time, not memory) between a hostile script and
// hundreds of megabytes of native stack.
//
// Sizing, measured 2026-09-23 on a debug build (the profile the Test262
// adapter and `cargo test` run): one interpreted call costs about 15.3 KB of
// native stack, and across ~30 re-entrant host paths (plain, arrow,
// `call`/`apply`/`bind`/`Reflect.*`, getters and coercions, Proxy traps,
// `map`/`sort`/`replace` callbacks, generators, `super()` chains, direct
// `eval`, ...) the most native stack between two consecutive `enter_call`
// checks was about 25 KB (direct `eval`). The margin has to cover that one
// segment, the leaf built-in that may then run at the deepest permitted level,
// and unwinding the error, so it is 256 KiB: about ten times the worst
// measured segment and well above the 100 KiB rustc itself reserves. The
// margin is tested, not just reasoned: with `tests/call_stack_budget.rs`'s
// runaway-recursion patterns, a margin of 24 KiB or less overflowed the real
// stack and aborted the process, and 32 KiB was the smallest margin that
// survived them all, so 256 KiB leaves eightfold headroom over the measured
// minimum. On a normal 8 MiB stack it allows roughly 500 nested calls.
pub(super) const CALL_STACK_RED_ZONE: usize = 256 * 1024;

// Where the host cannot report the thread's stack (`native_stack` has no backend
// for an unknown OS, and under `miri` it reports nothing), fall back to the
// conservative frame count the guard used before it measured bytes: 32 calls
// at the measured cost fit inside 512 KiB.
pub(super) const UNMEASURED_STACK_MAX_CALL_DEPTH: usize = 32;

/// Whether one more nested call must be refused. `remaining_stack` is the
/// byte count `native_stack::remaining_stack` reported for the current thread.
pub(super) fn call_stack_exhausted(remaining_stack: Option<usize>, call_depth: usize) -> bool {
    match remaining_stack {
        Some(remaining) => remaining < CALL_STACK_RED_ZONE,
        None => call_depth >= UNMEASURED_STACK_MAX_CALL_DEPTH,
    }
}
