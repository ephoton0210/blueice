// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Runtime values, separate from the parser's expression/property-key
//! types. Objects carry handles into [`crate::Heap`], never Rust
//! references or reference-counted cycles. Strings retain the parser's
//! UTF-8 representation for now; JS UTF-16 indexing/lone surrogates are
//! a language-runtime follow-up, not implemented by this storage slice.

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

/// The Phase 2 primitive subset and an object handle. `PartialEq` is
/// Rust value comparison, not a JS coercion operation; the interpreter
/// still needs to implement JS abstract equality and conversions.
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
    pub(crate) fn object_id(&self) -> Option<ObjectId> {
        match self {
            Value::Object(id) => Some(*id),
            _ => None,
        }
    }

    pub(crate) fn payload_bytes(&self) -> usize {
        match self {
            Value::String(s) => s.len(),
            _ => 0,
        }
    }
}
