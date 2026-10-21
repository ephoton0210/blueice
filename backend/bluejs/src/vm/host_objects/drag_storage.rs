// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Host drag copies retain native credit until managed or owner publication.

use super::*;
use crate::heap::{DragItemData, WebBlobData};
use crate::vm::error_input_storage::utf16_from_utf8;
use crate::vm::host_scheduling::SharedSchedulingCharge;
use crate::vm::web_drag_drop::storage::lowercase;
use crate::vm::web_event_storage::push_slot;

pub(super) struct NativeHostDragText {
    pub format: NativeValue,
    pub data: NativeValue,
}
impl NativeHostDragText {
    pub(super) fn from_host(
        format: &str,
        data: &str,
        owner: Option<&SharedSchedulingCharge>,
    ) -> Result<Self, RuntimeError> {
        let format = lowercase(utf16_from_utf8(format, owner)?);
        let data = utf16_from_utf8(data, owner)?;
        Ok(Self { format, data })
    }
}

struct NativeHostString {
    text: String,
    storage: Option<SharedSchedulingCharge>,
}
impl NativeHostString {
    fn copied(
        value: &JsString,
        invalid: &'static str,
        owner: Option<&SharedSchedulingCharge>,
    ) -> Result<Self, RuntimeError> {
        if let Some(owner) = owner {
            owner.reserve_peer(0)?;
        }
        let fail = || owner.map_or(RuntimeError::SchedulingLimit, SharedSchedulingCharge::fail);
        let length = char::decode_utf16(value.as_code_units().iter().copied()).try_fold(
            0usize,
            |bytes, ch| {
                let ch = ch.map_err(|_| RuntimeError::TypeError(invalid.into()))?;
                bytes.checked_add(ch.len_utf8()).ok_or_else(fail)
            },
        )?;
        let mut storage = owner.map(|owner| owner.reserve_peer(length)).transpose()?;
        let mut text = String::new();
        text.try_reserve_exact(length).map_err(|_| fail())?;
        if let Some(storage) = &mut storage {
            storage.resize(text.capacity())?;
        }
        for ch in char::decode_utf16(value.as_code_units().iter().copied()) {
            text.push(ch.expect("validated UTF-16"));
        }
        Ok(Self { text, storage })
    }
    fn media_type(
        value: &JsString,
        owner: Option<&SharedSchedulingCharge>,
    ) -> Result<Self, RuntimeError> {
        // Preserve the existing export's empty result for an invalid file type.
        if char::decode_utf16(value.as_code_units().iter().copied()).any(|ch| ch.is_err()) {
            let storage = owner.map(|owner| owner.reserve_peer(0)).transpose()?;
            Ok(Self {
                text: String::new(),
                storage,
            })
        } else {
            Self::copied(value, "Non-UTF-8 drag media type", owner)
        }
    }
}

pub(super) struct NativeHostDragOutcome {
    pub outcome: HostDragEventOutcome,
    storage: Option<SharedSchedulingCharge>,
}
impl NativeHostDragOutcome {
    fn absorb(&mut self, incoming: Option<SharedSchedulingCharge>) -> Result<(), RuntimeError> {
        match (&mut self.storage, incoming) {
            (Some(storage), Some(incoming)) => storage.absorb(incoming),
            (None, None) => Ok(()),
            _ => Err(RuntimeError::SchedulingLimit),
        }
    }
    fn string(&mut self, input: NativeHostString) -> Result<String, RuntimeError> {
        self.absorb(input.storage)?;
        Ok(input.text)
    }
    fn file(&mut self, blob: &WebBlobData) -> Result<HostFileData, RuntimeError> {
        let (name, last_modified) = blob.file.as_ref().expect("drag File brand");
        let name =
            NativeHostString::copied(name, "Non-UTF-8 drag filename", self.storage.as_ref())?;
        let media_type = NativeHostString::media_type(&blob.media_type, self.storage.as_ref())?;
        let mut storage = self
            .storage
            .as_ref()
            .map(|owner| owner.reserve_peer(blob.bytes.len()))
            .transpose()?;
        let bytes = blob.bytes.clone();
        if let Some(storage) = &mut storage {
            storage.resize(bytes.capacity())?;
        }
        let name = self.string(name)?;
        let media_type = self.string(media_type)?;
        self.absorb(storage)?;
        Ok(HostFileData {
            name,
            media_type,
            bytes,
            last_modified: *last_modified,
        })
    }
    pub(super) fn from_store(
        vm: &Vm,
        family: HostObjectFamily,
        target: HostObjectKey,
        transfer: ObjectId,
        kind: HostDragEventKind,
        default_prevented: bool,
        owner: Option<&SharedSchedulingCharge>,
    ) -> Result<Self, RuntimeError> {
        let storage = owner.map(|owner| owner.reserve_peer(0)).transpose()?;
        let mut output = Self {
            outcome: HostDragEventOutcome {
                default_prevented,
                drop_effect: String::new(),
                effect_allowed: String::new(),
                items: Vec::new(),
                image: None,
            },
            storage,
        };
        let store = vm.heap.drag_store(transfer)?;
        if kind == HostDragEventKind::DragStart {
            let total = store.items.iter().try_fold(0usize, |bytes, item| {
                Ok::<_, RuntimeError>(bytes.saturating_add(match &item.data {
                    DragItemData::Text(data) => data.byte_len(),
                    DragItemData::File(file) => vm.heap.web_blob(*file)?.bytes.len(),
                    DragItemData::FileMarker => 0,
                }))
            })?;
            if store.items.len() > 64 || total > 1_048_576 {
                return Err(RuntimeError::RangeError(
                    "Native drag export limit exceeded".into(),
                ));
            }
            for item in &store.items {
                let format = NativeHostString::copied(
                    &item.format,
                    "Non-UTF-8 native drag format",
                    output.storage.as_ref(),
                )?;
                if !matches!(item.data, DragItemData::FileMarker)
                    && !format.text.is_empty()
                    && !super::drag_events::valid_drag_format(&format.text)
                {
                    return Err(RuntimeError::TypeError(
                        "Invalid native drag export format".into(),
                    ));
                }
                let item = match &item.data {
                    DragItemData::Text(data) => {
                        let data = NativeHostString::copied(
                            data,
                            "Non-UTF-8 native drag text",
                            output.storage.as_ref(),
                        )?;
                        HostDragItem::Text {
                            format: output.string(format)?,
                            data: output.string(data)?,
                        }
                    }
                    DragItemData::File(file) => {
                        HostDragItem::File(output.file(vm.heap.web_blob(*file)?)?)
                    }
                    DragItemData::FileMarker => continue,
                };
                push_slot(&mut output.outcome.items, &mut output.storage, item)?;
            }
        }
        let drop_effect = NativeHostString::copied(
            &store.drop_effect,
            "Non-UTF-8 drag operation",
            output.storage.as_ref(),
        )?;
        output.outcome.drop_effect = output.string(drop_effect)?;
        let effect_allowed = NativeHostString::copied(
            &store.effect_allowed,
            "Non-UTF-8 drag operation",
            output.storage.as_ref(),
        )?;
        output.outcome.effect_allowed = output.string(effect_allowed)?;
        if kind == HostDragEventKind::DragStart {
            output.outcome.image = store
                .image
                .map(|(element, x, y)| {
                    let element = vm.host_object_families[family.index as usize]
                        .keys_by_wrapper
                        .get(&element)
                        .copied()
                        .filter(|key| key.matches_owner(target.owner, target.generation))
                        .ok_or_else(|| {
                            RuntimeError::TypeError("Drag image belongs to another document".into())
                        })?;
                    Ok::<_, RuntimeError>(HostDragImage { element, x, y })
                })
                .transpose()?;
        }
        Ok(output)
    }
    /// This same allocation crosses the public owner API with no intervening
    /// guest call or safepoint after terminal cleanup and context validation.
    pub(super) fn into_outcome(self) -> Result<HostDragEventOutcome, RuntimeError> {
        if let Some(storage) = &self.storage {
            storage.reserve_peer(0)?;
        }
        Ok(self.outcome)
    }
}
