// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A shared agent registry owns one native charge, rather than one per VM.

use super::host_scheduling::SharedSchedulingCharge;
use super::*;

#[derive(Default)]
pub(super) struct AgentSymbolRegistry {
    entries: HashMap<JsString, JsSymbol>,
    data_bytes: usize,
    charge: Option<SharedSchedulingCharge>,
}

impl AgentSymbolRegistry {
    fn payload_bytes(&self, capacity: usize, data: usize) -> usize {
        capacity
            .saturating_mul(std::mem::size_of::<(JsString, JsSymbol)>())
            .saturating_add(data)
    }

    pub(super) fn attach(&mut self, scope: &SchedulingScope) -> Result<(), RuntimeError> {
        let bytes = self.payload_bytes(self.entries.capacity(), self.data_bytes);
        if let Some(charge) = &mut self.charge {
            if !charge.belongs_to(scope) {
                return Err(RuntimeError::TypeError(
                    "agent registry already has a document owner".into(),
                ));
            }
            return charge.resize(bytes);
        }
        self.charge = Some(scope.shared_charge(bytes)?);
        Ok(())
    }

    pub(super) fn symbol_for(&mut self, key: JsString) -> Result<JsSymbol, RuntimeError> {
        let current = self.payload_bytes(self.entries.capacity(), self.data_bytes);
        if let Some(charge) = &mut self.charge {
            charge.resize(current)?;
        }
        if let Some(symbol) = self.entries.get(&key) {
            return Ok(symbol.clone());
        }

        // Prepare growth without changing the live table. Existing entries
        // move without cloning their key/description buffers after admission.
        let mut replacement = if self.entries.len() == self.entries.capacity() {
            let mut table = HashMap::new();
            if table
                .try_reserve(self.entries.len().saturating_add(1))
                .is_err()
            {
                return Err(self
                    .charge
                    .as_ref()
                    .map_or(RuntimeError::SchedulingLimit, |charge| charge.fail()));
            }
            Some(table)
        } else {
            None
        };
        let stored_key = key.clone();
        let data = self
            .data_bytes
            .saturating_add(key.owned_heap_payload_bytes().unwrap_or(usize::MAX))
            .saturating_add(stored_key.owned_heap_payload_bytes().unwrap_or(usize::MAX));
        let capacity = replacement
            .as_ref()
            .map_or(self.entries.capacity(), |table| table.capacity());
        let bytes = self.payload_bytes(capacity, data);
        if let Some(charge) = &mut self.charge {
            charge.resize(bytes)?;
        }

        let symbol = JsSymbol::new(Some(key));
        let result = symbol.clone();
        if let Some(mut table) = replacement.take() {
            table.extend(self.entries.drain());
            self.entries = table;
        }
        self.entries.insert(stored_key, symbol);
        self.data_bytes = data;
        Ok(result)
    }

    pub(super) fn key_for(&self, symbol: &JsSymbol) -> Option<JsString> {
        self.entries
            .iter()
            .find_map(|(key, candidate)| (candidate == symbol).then(|| key.clone()))
    }

    pub(super) fn is_registered(&self, symbol: &JsSymbol) -> bool {
        self.entries.values().any(|registered| registered == symbol)
    }
}
