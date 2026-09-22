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

// Ordinary calls still nest the Rust interpreter. The public 32-frame bound
// makes recursive JavaScript report a catchable RangeError on the normal
// process stack; an embedding that executes BlueJS on a deliberately smaller
// worker stack must provision enough host stack for that documented bound.
pub(super) const MAX_RECURSIVE_CALL_DEPTH: usize = 32;
