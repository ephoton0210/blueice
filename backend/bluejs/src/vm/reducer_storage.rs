// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Shared argument assembly for Array and TypedArray reductions.

use super::native_value_storage::NativeValue;
use super::transport_buffer_storage::TransportVector;
use super::*;

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/reducer_storage.rs"]
mod ownership_tests;

impl Vm {
    pub(super) fn reducer_arguments(
        &self,
        accumulator: NativeValue,
        value: NativeValue,
        index: f64,
        object: ObjectId,
    ) -> Result<TransportVector<Value>, RuntimeError> {
        accumulator.validate_owner()?;
        value.validate_owner()?;
        // Neutral contexts can consume an already-owned input. Select that
        // loan before allocating the argument slots without rebinding policy.
        let owner = self
            .argument_list_owner()?
            .or(accumulator.owner_view()?)
            .or(value.owner_view()?);
        let mut arguments = TransportVector::with_capacity(4, owner.as_ref())?;
        arguments.push_owned(accumulator)?;
        arguments.push_owned(value)?;
        arguments.push_owned(NativeValue::adopt(Value::Number(index), owner.as_ref())?)?;
        arguments.push_owned(NativeValue::adopt(Value::Object(object), owner.as_ref())?)?;
        Ok(arguments)
    }

    pub(super) fn call_reducer(
        &mut self,
        callback: &Value,
        accumulator: NativeValue,
        value: NativeValue,
        index: f64,
        object: ObjectId,
    ) -> Result<NativeValue, RuntimeError> {
        let arguments = self.reducer_arguments(accumulator, value, index, object)?;
        let owner = arguments.owner_view()?;
        let value = arguments.with_arguments(self, |vm, arguments| {
            arguments.call_native_borrowed(vm, callback, &Value::Undefined)
        })?;
        NativeValue::adopt(value, owner.as_ref())
    }
}
