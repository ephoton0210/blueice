// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! ImportCall inputs and returned key snapshots retain native ownership.

use super::host_scheduling::SharedSchedulingCharge;
use super::*;

pub(super) struct ImportAttributeKeys {
    keys: Vec<PropertyName>,
    _charge: Option<SharedSchedulingCharge>,
}
impl std::ops::Deref for ImportAttributeKeys {
    type Target = [PropertyName];
    fn deref(&self) -> &Self::Target {
        &self.keys
    }
}
impl ImportAttributeKeys {
    pub(super) fn new(
        keys: Vec<PropertyName>,
        scope: Option<&SchedulingScope>,
    ) -> Result<Self, RuntimeError> {
        let bytes = Self::buffer_bytes(&keys);
        let charge = scope.map(|scope| scope.shared_charge(bytes)).transpose()?;
        Ok(Self {
            keys,
            _charge: charge,
        })
    }
    fn buffer_bytes(keys: &Vec<PropertyName>) -> usize {
        keys.iter()
            .fold(host_scheduling::slots(keys), |bytes, key| {
                bytes.saturating_add(match key {
                    PropertyName::String(text) => {
                        text.owned_heap_payload_bytes().unwrap_or(usize::MAX)
                    }
                    PropertyName::Symbol(symbol) => symbol.description.as_ref().map_or(0, |text| {
                        text.owned_heap_payload_bytes().unwrap_or(usize::MAX)
                    }),
                })
            })
    }
    #[cfg(any(test, coverage))]
    pub(super) fn bytes(&self) -> usize {
        Self::buffer_bytes(&self.keys)
    }
}

impl Vm {
    pub(super) fn prepare_import_call_inputs(
        &mut self,
        specifier: &Value,
        options: &Value,
    ) -> Result<Option<SharedSchedulingCharge>, RuntimeError> {
        self.synchronize_scheduling_usage()?;
        let bytes = host_scheduling::value_payload_bytes(specifier)
            .saturating_add(host_scheduling::value_payload_bytes(options));
        let charge = self
            .host_scheduling
            .as_ref()
            .map(|scope| scope.shared_charge(bytes))
            .transpose()?;
        // The two arguments, capability and optional attributes object can
        // coexist. Prepare their complete root backing before Promise creation.
        self.prepare_async_frame_root_slots(4)?;
        Ok(charge)
    }
    pub(super) fn import_specifier_name(
        &mut self,
        value: &Value,
    ) -> Result<ModuleName, RuntimeError> {
        if let Value::String(text) = value {
            return ModuleName::from_utf16(text, self.host_scheduling.as_ref());
        }
        let text = self.coerce_string(value)?;
        // Generic coercion still owns its producer allocations. Attach the
        // returned buffer before another allocation, then admit UTF-8 output.
        let _text_charge = self
            .host_scheduling
            .as_ref()
            .map(|scope| scope.shared_charge(text.owned_heap_payload_bytes().unwrap_or(usize::MAX)))
            .transpose()?;
        ModuleName::from_utf16(&text, self.host_scheduling.as_ref())
    }
}
