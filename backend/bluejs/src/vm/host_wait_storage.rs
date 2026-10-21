// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Host callbacks own converted input through invocation and result handoff.

use super::host_scheduling::SharedSchedulingCharge;
use super::*;

pub(super) struct PreparedHostArguments {
    pub(super) values: Vec<HostValue>,
    pub(super) error: Option<RuntimeError>,
    charge: Option<SharedSchedulingCharge>,
}

impl PreparedHostArguments {
    pub(super) fn owner_view(&self) -> Result<Option<SharedSchedulingCharge>, RuntimeError> {
        self.charge
            .as_ref()
            .map(|owner| owner.reserve_peer(0))
            .transpose()
    }
}

impl Vm {
    pub(super) fn prepare_host_wait_arguments(
        &mut self,
        arguments: &[Value],
    ) -> Result<PreparedHostArguments, RuntimeError> {
        self.prepare_host_arguments(arguments, None)
    }

    pub(super) fn prepare_host_arguments(
        &mut self,
        arguments: &[Value],
        hint: Option<&SharedSchedulingCharge>,
    ) -> Result<PreparedHostArguments, RuntimeError> {
        // Transient input belongs to the caller. A retained registration is
        // the fallback only when this neutral call has no current document.
        let owner = match self.argument_list_owner()? {
            Some(owner) => Some(owner),
            None => hint.map(|owner| owner.reserve_peer(0)).transpose()?,
        };
        // Primitive conversion has no guest side effects. Detect rejection
        // before copying any prefix or allocating a temporary host error.
        if arguments.iter().any(|value| {
            !matches!(
                value,
                Value::Undefined
                    | Value::Null
                    | Value::Bool(_)
                    | Value::Number(_)
                    | Value::String(_)
            )
        }) {
            let mut charge = owner
                .as_ref()
                .map(|owner| owner.reserve_peer(HOST_PRIMITIVE_ARGUMENT_ERROR.len()))
                .transpose()?;
            let message = HOST_PRIMITIVE_ARGUMENT_ERROR.to_owned();
            if let Some(charge) = &mut charge {
                charge.resize(message.capacity())?;
            }
            return Ok(PreparedHostArguments {
                values: Vec::new(),
                error: Some(RuntimeError::TypeError(message)),
                charge,
            });
        }
        let payload = host_scheduling::values_bytes(arguments);
        let bytes = arguments
            .len()
            .checked_mul(std::mem::size_of::<HostValue>())
            .and_then(|bytes| bytes.checked_add(payload))
            .ok_or_else(|| {
                owner
                    .as_ref()
                    .map_or(RuntimeError::SchedulingLimit, SharedSchedulingCharge::fail)
            })?;
        let charge = owner
            .as_ref()
            .map(|owner| owner.reserve_peer(bytes))
            .transpose()?;
        let mut prepared = PreparedHostArguments {
            values: Vec::new(),
            error: None,
            charge,
        };
        prepared
            .values
            .try_reserve_exact(arguments.len())
            .map_err(|_| {
                owner
                    .as_ref()
                    .map_or(RuntimeError::SchedulingLimit, SharedSchedulingCharge::fail)
            })?;
        let slots = host_scheduling::slots(&prepared.values);
        if let Some(charge) = &mut prepared.charge {
            charge.resize(slots.saturating_add(payload))?;
        }
        let mut remaining = payload;
        let mut copied = 0usize;
        for value in arguments {
            let value_copy = HostValue::try_from(value).expect("prevalidated host primitive");
            remaining = remaining.saturating_sub(host_scheduling::value_payload_bytes(value));
            if let HostValue::String(text) = &value_copy {
                copied =
                    copied.saturating_add(text.owned_heap_payload_bytes().unwrap_or(usize::MAX));
            }
            prepared.values.push(value_copy);
            if let Some(charge) = &mut prepared.charge {
                charge.resize(slots.saturating_add(copied).saturating_add(remaining))?;
            }
        }
        Ok(prepared)
    }

    /// The ABI transfers an already produced host buffer. Admit it before any
    /// VM consumer and move it without making a second formatted String.
    pub(super) fn finish_host_callback<T>(
        &self,
        result: Result<T, HostFunctionError>,
        owner: Option<&SharedSchedulingCharge>,
    ) -> Result<T, RuntimeError> {
        match result {
            Ok(value) => Ok(value),
            Err(HostFunctionError(message)) => {
                let _storage = owner
                    .map(|owner| owner.reserve_peer(message.capacity()))
                    .transpose()?;
                // Pure transfer of this same String to the native error
                // consumer: no guest call or collection separates the views.
                Err(RuntimeError::TypeError(message))
            }
        }
    }

    pub(super) fn finish_host_callback_value(
        &self,
        result: Result<HostValue, HostFunctionError>,
        owner: Option<&SharedSchedulingCharge>,
    ) -> Result<Value, RuntimeError> {
        let value = self.finish_host_callback(result, owner)?;
        let value = native_value_storage::NativeValue::adopt(value.into(), owner)?;
        self.check_string(value.value())?;
        Ok(value.into_value())
    }
}

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/host_wait_storage.rs"]
mod ownership_tests;
