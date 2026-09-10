// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Runtime values, separate from the parser's expression/property-key
//! types. Objects carry handles into [`crate::Heap`], never Rust
//! references or reference-counted cycles. Strings preserve UTF-16 code
//! units throughout parsing, execution and property storage.

use crate::{JsString, JsSymbol};
use num_bigint::BigInt;

/// An opaque object identity. The heap identity prevents a handle from
/// another (even already-dropped) heap aliasing one of this heap's
/// objects. The counter is never reused, including after collection.
/// Copying this value does **not** keep the object alive: use
/// [`crate::Heap::root`] across later allocating operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ObjectId {
    pub(crate) heap: u64,
    pub(crate) serial: u64,
}

/// Implemented primitives and an object handle (including native functions
/// and boxed Strings). `PartialEq` is non-coercing value comparison; JS
/// abstract equality and the remaining conversions are separate operations.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Undefined,
    Null,
    Bool(bool),
    Number(f64),
    BigInt(BigInt),
    String(JsString),
    Symbol(JsSymbol),
    Object(ObjectId),
}

impl Value {
    pub(crate) fn object_id(&self) -> Option<ObjectId> {
        match self {
            Value::Object(id) => Some(*id),
            _ => None,
        }
    }

    pub(crate) fn payload_bytes(&self) -> usize {
        match self {
            Value::String(s) => s.byte_len(),
            Value::Symbol(s) => s.description.as_ref().map_or(0, JsString::byte_len),
            Value::BigInt(n) => n.to_signed_bytes_le().len(),
            _ => 0,
        }
    }
}
