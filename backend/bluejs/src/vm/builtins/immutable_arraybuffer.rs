// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The Immutable ArrayBuffer proposal
//! (tc39/proposal-immutable-arraybuffer, Test262 feature
//! `immutable-arraybuffer`): the `ArrayBuffer.prototype` API that creates and
//! observes an immutable buffer, plus the write-path guards shared by
//! TypedArray, DataView, Atomics and `Uint8Array.prototype.setFrom*`.
//!
//! The `[[ArrayBufferIsImmutable]]` slot itself lives on the heap's ArrayBuffer
//! record (`Heap::alloc_immutable_array_buffer`); everything here is the
//! VM-side observable behaviour keyed off `Heap::buffer_is_immutable`.

use super::*;

impl Vm {
    /// `get ArrayBuffer.prototype.immutable`.
    pub(super) fn array_buffer_immutable(&self, receiver: &Value) -> Result<Value, RuntimeError> {
        // RequireInternalSlot([[ArrayBufferData]]) plus the SharedArrayBuffer
        // rejection is exactly the ordinary ArrayBuffer receiver check.
        let buffer = self.array_buffer_receiver(receiver)?;
        Ok(Value::Bool(self.heap.buffer_is_immutable(buffer)?))
    }

    /// Steps 1-7 of ArrayBufferCopyAndDetach, shared by `transfer`,
    /// `transferToFixedLength` and `transferToImmutable`: the receiver check,
    /// then `ToIndex(newLength)` (which may run user code and so detach or
    /// resize the source), and only afterwards the detached/immutable
    /// rejections. Returns the source, its byte length as of *after* the
    /// coercion, and the requested new byte length.
    pub(super) fn array_buffer_copy_and_detach_source(
        &mut self,
        receiver: &Value,
        args: &[Value],
    ) -> Result<(ObjectId, usize, usize), RuntimeError> {
        let source = self.array_buffer_receiver(receiver)?;
        let new_length = if args.is_empty() || args[0] == Value::Undefined {
            None
        } else {
            Some(self.buffer_index(&args[0])?)
        };
        if self.heap.buffer_is_detached(source)? {
            return Err(RuntimeError::TypeError("ArrayBuffer is detached".into()));
        }
        if self.heap.buffer_is_immutable(source)? {
            return Err(RuntimeError::TypeError(
                "ArrayBuffer is immutable and cannot be detached".into(),
            ));
        }
        let source_length = self.heap.buffer_byte_length(source)?;
        Ok((source, source_length, new_length.unwrap_or(source_length)))
    }

    /// `ArrayBuffer.prototype.transferToImmutable ( [ newLength ] )`.
    /// Like `transfer`, it never observes `constructor` or species: the
    /// result is always an intrinsic %ArrayBuffer%, zero-extended when
    /// `newLength` exceeds the source, and the source is detached only after
    /// every fallible step succeeded.
    pub(super) fn array_buffer_transfer_to_immutable(
        &mut self,
        receiver: &Value,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        let (source, source_length, length) =
            self.array_buffer_copy_and_detach_source(receiver, args)?;
        if length > self.heap.max_array_buffer_byte_length() {
            return Err(RuntimeError::RangeError(
                "immutable ArrayBuffer length is too large".into(),
            ));
        }
        let mut bytes = self
            .heap
            .array_buffer_copy(source, 0, source_length.min(length))?;
        bytes.resize(length, 0);
        let prototype = self.buffer_prototype("ArrayBuffer")?;
        let target =
            self.with_roots(|heap| heap.alloc_immutable_array_buffer(bytes, Some(prototype)))?;
        self.with_roots(|heap| heap.detach_array_buffer(source))?;
        Ok(Value::Object(target))
    }

    /// `ArrayBuffer.prototype.sliceToImmutable ( start, end )`. Bounds are
    /// resolved against the length at entry (ResolveBounds may run user code
    /// that resizes the source); the copy is then checked against the length
    /// *after* that code ran.
    pub(super) fn array_buffer_slice_to_immutable(
        &mut self,
        receiver: &Value,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        let buffer = self.array_buffer_receiver(receiver)?;
        if self.heap.buffer_is_detached(buffer)? {
            return Err(RuntimeError::TypeError("ArrayBuffer is detached".into()));
        }
        let length = self.heap.array_buffer_byte_length(buffer)?;
        let first = self.relative_buffer_index(native::argument(args, 0), length)?;
        let last = if args.get(1).is_some_and(|value| *value != Value::Undefined) {
            self.relative_buffer_index(native::argument(args, 1), length)?
        } else {
            length
        };
        let new_length = last.saturating_sub(first);
        // Bounds resolution may have detached or resized the source.
        if self.heap.buffer_is_detached(buffer)? {
            return Err(RuntimeError::TypeError("ArrayBuffer is detached".into()));
        }
        if self.heap.array_buffer_byte_length(buffer)? < last {
            return Err(RuntimeError::RangeError(
                "ArrayBuffer shrank below the requested slice".into(),
            ));
        }
        let bytes = self.heap.array_buffer_copy(buffer, first, new_length)?;
        let prototype = self.buffer_prototype("ArrayBuffer")?;
        let result =
            self.with_roots(|heap| heap.alloc_immutable_array_buffer(bytes, Some(prototype)))?;
        Ok(Value::Object(result))
    }
}
