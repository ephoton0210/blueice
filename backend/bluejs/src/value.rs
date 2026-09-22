// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The values a BlueJS program can put on its operand stack.
//!
//! Objects deliberately cross all subsystem boundaries as stable handles,
//! never as `Rc<RefCell<_>>`.  The owning [`crate::heap::Heap`] can therefore
//! move a young object to its tenured generation without invalidating a value
//! held by bytecode, an environment, or a host binding.

use crate::heap::ObjectId;

/// A JavaScript value in the Phase 2 language subset.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Undefined,
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Object(ObjectId),
}

impl Value {
    pub const fn is_nullish(&self) -> bool {
        matches!(self, Self::Undefined | Self::Null)
    }

    /// JavaScript's subset of `ToBoolean` needed for control-flow and logical
    /// bytecode. Objects are always truthy, including an empty array/object.
    pub fn is_truthy(&self) -> bool {
        match self {
            Self::Undefined | Self::Null => false,
            Self::Bool(value) => *value,
            Self::Number(value) => *value != 0.0 && !value.is_nan(),
            Self::String(value) => !value.is_empty(),
            Self::Object(_) => true,
        }
    }

    pub const fn type_of(&self) -> &'static str {
        match self {
            Self::Undefined => "undefined",
            Self::Null => "object",
            Self::Bool(_) => "boolean",
            Self::Number(_) => "number",
            Self::String(_) => "string",
            Self::Object(_) => "object",
        }
    }
}
