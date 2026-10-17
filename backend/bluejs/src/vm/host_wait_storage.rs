// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Host waits prepare native owners before exposing completion capabilities.

use super::host_scheduling::SharedSchedulingCharge;
use super::*;

pub(super) struct PreparedHostArguments {
    pub(super) values: Vec<HostValue>,
    pub(super) error: Option<RuntimeError>,
    charge: Option<SharedSchedulingCharge>,
}
impl Vm {
    pub(super) fn prepare_host_wait_arguments(
        &mut self,
        arguments: &[Value],
    ) -> Result<PreparedHostArguments, RuntimeError> {
        let payload = host_scheduling::values_bytes(arguments);
        let bytes = arguments
            .len()
            .saturating_mul(std::mem::size_of::<HostValue>())
            .saturating_add(payload);
        let charge = self
            .host_scheduling
            .as_ref()
            .map(|scope| scope.shared_charge(bytes))
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
                self.host_scheduling
                    .as_ref()
                    .map_or(RuntimeError::SchedulingLimit, SchedulingScope::fail)
            })?;
        let slots = host_scheduling::slots(&prepared.values);
        if let Some(charge) = &mut prepared.charge {
            charge.resize(slots.saturating_add(payload))?;
        }
        let mut remaining = payload;
        let mut copied = 0usize;
        for value in arguments {
            match HostValue::try_from(value) {
                Ok(value_copy) => {
                    remaining =
                        remaining.saturating_sub(host_scheduling::value_payload_bytes(value));
                    if let HostValue::String(text) = &value_copy {
                        copied = copied
                            .saturating_add(text.owned_heap_payload_bytes().unwrap_or(usize::MAX));
                    }
                    prepared.values.push(value_copy);
                    if let Some(charge) = &mut prepared.charge {
                        charge.resize(slots.saturating_add(copied).saturating_add(remaining))?;
                    }
                }
                Err(error) => {
                    prepared.values = Vec::new();
                    let message = error.to_string();
                    drop(error);
                    if let Some(charge) = &mut prepared.charge {
                        charge.resize(message.capacity())?;
                    }
                    prepared.error = Some(RuntimeError::TypeError(message));
                    break;
                }
            }
        }
        Ok(prepared)
    }
}

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/host_wait_storage.rs"]
mod ownership_tests;
