// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Bounded label tables own their exact slots and original UTF16 key buffers.

use super::*;
use crate::vm::native_value_storage::NativeValue;
use crate::vm::promise_queue_storage::PreparedVec;

pub(super) struct ConsoleLabels {
    entries: Vec<(JsString, u64)>,
    storage: Option<SharedSchedulingCharge>,
}

pub(super) struct PreparedLabel {
    slots: Option<PreparedVec<(JsString, u64)>>,
    key: NativeValue,
}

pub(super) struct ConsoleLabel {
    pub key: NativeValue,
    pub display: Output,
}

impl ConsoleLabel {
    pub(super) fn units(&self) -> &[u16] {
        let Value::String(text) = self.key.value() else {
            unreachable!("console label was converted")
        };
        text.as_code_units()
    }
}

impl ConsoleLabels {
    pub(super) fn new(owner: Option<&SharedSchedulingCharge>) -> Result<Self, RuntimeError> {
        Ok(Self {
            entries: Vec::new(),
            storage: owner.map(|owner| owner.reserve_peer(0)).transpose()?,
        })
    }
    pub(super) fn len(&self) -> usize {
        self.entries.len()
    }
    #[cfg(any(test, coverage))]
    pub(super) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    pub(super) fn get(&self, key: &[u16]) -> Option<&u64> {
        self.entries
            .iter()
            .find(|(text, _)| text.as_code_units() == key)
            .map(|(_, value)| value)
    }
    pub(super) fn get_mut(&mut self, key: &[u16]) -> Option<&mut u64> {
        self.entries
            .iter_mut()
            .find(|(text, _)| text.as_code_units() == key)
            .map(|(_, value)| value)
    }
    pub(super) fn owned_bytes(&self) -> usize {
        self.entries
            .iter()
            .fold(host_scheduling::slots(&self.entries), |bytes, (key, _)| {
                bytes.saturating_add(key.owned_heap_payload_bytes().unwrap_or(usize::MAX))
            })
    }
    pub(super) fn unowned_bytes(&self) -> usize {
        if self.storage.is_none() {
            self.owned_bytes()
        } else {
            0
        }
    }
    pub(super) fn owner_view(&self) -> Result<Option<SharedSchedulingCharge>, RuntimeError> {
        self.storage
            .as_ref()
            .map(|owner| owner.reserve_peer(0))
            .transpose()
    }
    pub(super) fn can_join(&self, scope: &SchedulingScope) -> bool {
        self.storage
            .as_ref()
            .is_none_or(|owner| owner.belongs_to(scope))
    }
    pub(super) fn needs_owner(&self) -> bool {
        self.storage.is_none()
    }
    pub(super) fn install_owner(&mut self, storage: SharedSchedulingCharge) {
        assert!(self.storage.is_none());
        assert_eq!(storage.bytes(), self.owned_bytes());
        self.storage = Some(storage);
    }
    pub(super) fn prepare(&self, key: NativeValue) -> Result<PreparedLabel, RuntimeError> {
        key.validate_owner()?;
        if let Some(owner) = &self.storage {
            owner.reserve_peer(0)?;
            if key
                .owner_view()?
                .as_ref()
                .is_none_or(|incoming| !owner.same_owner(incoming))
            {
                return Err(owner.fail());
            }
        } else {
            assert!(key.owner_view()?.is_none());
        }
        let Value::String(text) = key.value() else {
            unreachable!()
        };
        let slots = if self.get(text.as_code_units()).is_none() {
            Some(PreparedVec::for_push_owned(
                &self.entries,
                self.storage.as_ref(),
            )?)
        } else {
            None
        };
        Ok(PreparedLabel { slots, key })
    }
    pub(super) fn remove(&mut self, key: &[u16]) {
        let Some(index) = self
            .entries
            .iter()
            .position(|(text, _)| text.as_code_units() == key)
        else {
            return;
        };
        self.entries.remove(index);
        if self.entries.is_empty() {
            self.entries = Vec::new();
        }
        let bytes = self.owned_bytes();
        if let Some(storage) = &mut self.storage {
            storage.release_to(bytes);
        }
    }
}

impl PreparedLabel {
    /// The last callback, output/root preparation and owner validation precede
    /// publication. Moving keys and replacing these admitted slots cannot fail.
    pub(super) fn commit(self, table: &mut ConsoleLabels, value: u64) {
        let Value::String(text) = self.key.value() else {
            unreachable!()
        };
        if let Some(current) = table.get_mut(text.as_code_units()) {
            assert!(self.slots.is_none());
            *current = value;
            return;
        }
        let old_slots = host_scheduling::slots(&table.entries);
        let incoming_slots = self
            .slots
            .expect("new label slots were prepared")
            .commit_owned(&mut table.entries);
        if let Some(incoming) = incoming_slots {
            let storage = table
                .storage
                .as_mut()
                .expect("owned replacement has its account");
            storage.release_to(storage.bytes() - old_slots);
            storage
                .absorb(incoming)
                .expect("same label backing account");
        }
        let (Value::String(text), incoming) = self.key.into_retained_parts() else {
            unreachable!()
        };
        if let Some(storage) = &mut table.storage {
            storage
                .absorb(incoming.expect("owned label key"))
                .expect("same label account");
        }
        assert!(table.entries.len() < table.entries.capacity());
        table.entries.push((text, value));
    }
}
