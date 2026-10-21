// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Native Error messages retain their source and independently admitted UTF-16 input.

use super::host_scheduling::{runtime_error_payload_bytes, SharedSchedulingCharge};
use super::native_value_storage::NativeValue;
use super::*;
use std::fmt::Write;

pub(super) struct ErrorMessage {
    message: String,
    // Destroy the UTF-8 allocation before returning its original credit.
    storage: Option<SharedSchedulingCharge>,
}

struct ErrorInput {
    error: RuntimeError,
    storage: Option<SharedSchedulingCharge>,
}

impl ErrorMessage {
    pub(super) fn adopt(
        message: String,
        owner: Option<&SharedSchedulingCharge>,
    ) -> Result<Self, RuntimeError> {
        let storage = owner
            .map(|owner| owner.reserve_peer(message.capacity()))
            .transpose()?;
        Ok(Self { message, storage })
    }

    fn string_limit(
        limit: usize,
        mut storage: Option<SharedSchedulingCharge>,
    ) -> Result<Self, RuntimeError> {
        let length = "BlueJS string exceeds ".len()
            + (limit.checked_ilog10().unwrap_or(0) as usize + 1)
            + " bytes".len();
        if let Some(storage) = &mut storage {
            storage.resize(length)?;
        }
        let mut message = String::new();
        message.try_reserve_exact(length).map_err(|_| {
            storage
                .as_ref()
                .map_or(RuntimeError::SchedulingLimit, SharedSchedulingCharge::fail)
        })?;
        if let Some(storage) = &mut storage {
            storage.resize(message.capacity())?;
        }
        write!(message, "BlueJS string exceeds {limit} bytes")
            .expect("writing to an admitted String cannot fail");
        Ok(Self { message, storage })
    }

    pub(super) fn into_utf16(self) -> Result<NativeValue, RuntimeError> {
        let value = utf16_from_utf8(&self.message, self.storage.as_ref())?;
        // Both allocations remain paid until the source is actually destroyed.
        drop(self);
        Ok(value)
    }

    pub(super) fn into_error(self, vm: &mut Vm, name: &str) -> Result<NativeValue, RuntimeError> {
        let owner = self
            .storage
            .as_ref()
            .map(|owner| owner.reserve_peer(0))
            .transpose()?;
        let context = NativeValue::from_retained_parts(Value::Undefined, owner);
        context.with_owned_context(vm, |vm, context| {
            let owner = context.owner_view()?;
            // Lazy intrinsic installation can allocate/collect with UTF-8 still live.
            let constructor = vm.error_global(name)?;
            let message = self.into_utf16()?;
            let arguments = argument_list_storage::ArgumentValues::single_retained(vm, message)?;
            let value = arguments.call_native(vm, constructor, Value::Undefined, false)?;
            NativeValue::adopt(value, owner.as_ref())
        })
    }
}

pub(super) fn utf16_from_utf8(
    text: &str,
    owner: Option<&SharedSchedulingCharge>,
) -> Result<NativeValue, RuntimeError> {
    let fail = || owner.map_or(RuntimeError::SchedulingLimit, SharedSchedulingCharge::fail);
    let count = text.encode_utf16().count();
    let bytes = count
        .checked_mul(std::mem::size_of::<u16>())
        .ok_or_else(fail)?;
    let mut storage = owner.map(|owner| owner.reserve_peer(bytes)).transpose()?;
    let mut units = Vec::new();
    units.try_reserve_exact(count).map_err(|_| fail())?;
    if let Some(storage) = &mut storage {
        storage.resize(host_scheduling::slots(&units))?;
    }
    units.extend(text.encode_utf16());
    Ok(NativeValue::from_retained_parts(
        Value::String(JsString::from_code_units(units)),
        storage,
    ))
}

impl Vm {
    /// Move already retained error input into conversion without a duplicate lease.
    pub(super) fn error_value_with_input(
        &mut self,
        error: RuntimeError,
        storage: Option<SharedSchedulingCharge>,
    ) -> Result<NativeValue, RuntimeError> {
        let mut input = ErrorInput { error, storage };
        // Dynamic import converts linking failures to SyntaxError; host aborts
        // pass through unchanged without entering a JavaScript constructor.
        if !input.error.is_catchable() && !matches!(input.error, RuntimeError::ModuleResolution(_))
        {
            return Err(input.error);
        }
        if input.storage.is_none() {
            input.storage = self
                .argument_list_owner()?
                .as_ref()
                .map(|owner| owner.reserve_peer(runtime_error_payload_bytes(&input.error)))
                .transpose()?;
        }
        if let Some(owner) = &input.storage {
            owner.reserve_peer(0)?;
            if let Some(scope) = &self.host_scheduling {
                if !owner.belongs_to(scope) {
                    return Err(scope.fail());
                }
            }
        }
        let ErrorInput { error, storage } = input;
        let (name, message) = match error {
            RuntimeError::Thrown(value) => {
                return NativeValue::from_retained_parts(value, storage)
                    .with_owned_context(self, |_, value| Ok(value));
            }
            RuntimeError::ReferenceError(message) => ("ReferenceError", message),
            RuntimeError::TypeError(message) => ("TypeError", message),
            RuntimeError::RangeError(message) => ("RangeError", message),
            RuntimeError::SyntaxError(message) | RuntimeError::ModuleResolution(message) => {
                ("SyntaxError", message)
            }
            RuntimeError::Test262(message) => ("Test262Error", message),
            RuntimeError::StringLimit { limit } => {
                return ErrorMessage::string_limit(limit, storage)?.into_error(self, "RangeError");
            }
            error => return Err(error),
        };
        ErrorMessage { message, storage }.into_error(self, name)
    }

    pub(super) fn copy_error_value(
        &mut self,
        error: &RuntimeError,
        hint: Option<&SharedSchedulingCharge>,
    ) -> Result<NativeValue, RuntimeError> {
        let owner = if let Some(owner) = hint {
            Some(owner.reserve_peer(0)?)
        } else {
            self.argument_list_owner()?
        };
        let mut storage = owner
            .as_ref()
            .map(|owner| owner.reserve_peer(host_scheduling::runtime_error_copy_bytes(error)))
            .transpose()?;
        let copy = error.clone();
        if let Some(storage) = &mut storage {
            storage.resize(runtime_error_payload_bytes(&copy))?;
        }
        self.error_value_with_input(copy, storage)
    }
}
