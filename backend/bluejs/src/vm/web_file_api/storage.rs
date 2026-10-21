// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Blob construction and reads keep copied bytes and UTF16 buffers paid.

use super::*;
use crate::vm::host_scheduling::SharedSchedulingCharge;
use crate::vm::web_drag_drop::storage::NativeDragFile;

pub(super) fn blob_usv(value: NativeValue) -> NativeValue {
    let (Value::String(text), storage) = value.into_retained_parts() else {
        unreachable!("converted Blob string")
    };
    NativeValue::from_retained_parts(Value::String(text.replace_unpaired_surrogates()), storage)
}

pub(super) fn blob_media_type(value: NativeValue) -> Result<NativeValue, RuntimeError> {
    let Value::String(text) = value.value() else {
        unreachable!()
    };
    if text
        .as_code_units()
        .iter()
        .any(|unit| !(0x20..=0x7e).contains(unit))
    {
        let owner = value.owner_view()?;
        return NativeValue::adopt(Value::String(JsString::default()), owner.as_ref());
    }
    let (Value::String(mut text), storage) = value.into_retained_parts() else {
        unreachable!()
    };
    text.make_ascii_lowercase();
    Ok(NativeValue::from_retained_parts(
        Value::String(text),
        storage,
    ))
}

#[derive(Clone, Copy)]
enum Part {
    Text(usize),
    Blob(ObjectId, usize),
    Buffer(ObjectId, usize, usize),
}

impl Part {
    fn snapshot(vm: &Vm, value: &Value, native_endings: bool) -> Result<Self, RuntimeError> {
        if let Some(id) = value.object_id() {
            if let Ok(blob) = vm.heap.web_blob(id) {
                return Ok(Self::Blob(id, blob.bytes.len()));
            }
            if vm.heap.is_buffer(id)? {
                return Ok(Self::Buffer(id, 0, vm.heap.buffer_byte_length(id)?));
            }
            if vm.heap.is_data_view(id)? {
                let (buffer, offset, length) = vm.heap.data_view_current_info(id)?;
                return Ok(Self::Buffer(buffer, offset, length));
            }
            let (buffer, offset, length, kind) = vm.heap.typed_array_info(id)?;
            if vm.heap.buffer_is_detached(buffer)? || vm.heap.typed_array_is_out_of_bounds(id)? {
                return Err(RuntimeError::TypeError(
                    "Blob part view is detached or out of bounds".into(),
                ));
            }
            return Ok(Self::Buffer(buffer, offset, length * kind.byte_width()));
        }
        let Value::String(text) = value else {
            unreachable!("converted Blob part")
        };
        let mut length = 0usize;
        visit_text_bytes(text, native_endings, |bytes| {
            length = length.saturating_add(bytes.len())
        });
        Ok(Self::Text(length))
    }

    fn length(self) -> usize {
        match self {
            Self::Text(length) | Self::Blob(_, length) | Self::Buffer(_, _, length) => length,
        }
    }

    fn append(
        self,
        vm: &Vm,
        value: &Value,
        native_endings: bool,
        bytes: &mut Vec<u8>,
    ) -> Result<(), RuntimeError> {
        let end = bytes
            .len()
            .checked_add(self.length())
            .expect("admitted Blob length");
        assert!(end <= bytes.capacity());
        match self {
            Self::Text(_) => {
                let Value::String(text) = value else {
                    unreachable!()
                };
                visit_text_bytes(text, native_endings, |part| bytes.extend_from_slice(part));
            }
            Self::Blob(id, _) => bytes.extend_from_slice(&vm.heap.web_blob(id)?.bytes),
            Self::Buffer(buffer, offset, _) => {
                let start = bytes.len();
                bytes.resize(end, 0);
                vm.heap
                    .array_buffer_read_into(buffer, offset, &mut bytes[start..])?;
            }
        }
        assert_eq!(bytes.len(), end);
        Ok(())
    }
}

/// Count and fill with the same scalar conversion, without an intermediate
/// UTF8 String or a second newline-normalization Vec. Only strings normalize.
fn visit_text_bytes(text: &JsString, native_endings: bool, mut visit: impl FnMut(&[u8])) {
    let mut chars = char::decode_utf16(text.as_code_units().iter().copied())
        .map(|character| character.unwrap_or(char::REPLACEMENT_CHARACTER))
        .peekable();
    while let Some(character) = chars.next() {
        if native_endings && matches!(character, '\r' | '\n') {
            if character == '\r' && chars.peek() == Some(&'\n') {
                chars.next();
            }
            visit(if cfg!(windows) { b"\r\n" } else { b"\n" });
        } else {
            let mut encoded = [0; 4];
            visit(character.encode_utf8(&mut encoded).as_bytes());
        }
    }
}

pub(in crate::vm) struct BlobBytes {
    bytes: Vec<u8>,
    storage: Option<SharedSchedulingCharge>,
}

impl BlobBytes {
    pub(in crate::vm) fn copied(
        source: &[u8],
        owner: Option<&SharedSchedulingCharge>,
    ) -> Result<Self, RuntimeError> {
        let mut result = Self::new(source.len(), owner)?;
        result.bytes.extend_from_slice(source);
        Ok(result)
    }

    pub(in crate::vm) fn len(&self) -> usize {
        self.bytes.len()
    }

    pub(in crate::vm) fn copy_into_buffer(
        self,
        vm: &mut Vm,
        buffer: ObjectId,
    ) -> Result<(), RuntimeError> {
        if let Some(storage) = &self.storage {
            storage.reserve_peer(0)?;
        }
        vm.heap.array_buffer_write(buffer, 0, &self.bytes)?;
        Ok(())
    }

    fn new(length: usize, owner: Option<&SharedSchedulingCharge>) -> Result<Self, RuntimeError> {
        let mut storage = owner.map(|owner| owner.reserve_peer(length)).transpose()?;
        let mut bytes = Vec::new();
        bytes.try_reserve_exact(length).map_err(|_| {
            owner.map_or(RuntimeError::SchedulingLimit, SharedSchedulingCharge::fail)
        })?;
        if let Some(storage) = &mut storage {
            storage.resize(bytes.capacity())?;
        }
        Ok(Self { bytes, storage })
    }

    pub(super) fn into_candidate(
        self,
        media_type: NativeValue,
        name: Option<(NativeValue, i64)>,
    ) -> Result<NativeDragFile, RuntimeError> {
        NativeDragFile::from_blob_parts(self.bytes, self.storage, media_type, name)
    }
}

/// UTF8 replacement decoding writes directly into its independently admitted
/// UTF16 result. The two passes borrow immutable bytes and allocate no UTF8
/// copy; one leading BOM is consumed, as in the existing Blob.text contract.
pub(super) fn decode_text(
    bytes: &[u8],
    limit: usize,
    owner: Option<&SharedSchedulingCharge>,
) -> Result<NativeValue, RuntimeError> {
    let bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes);
    let mut count = 0usize;
    visit_lossy_utf16(bytes, |_| count = count.saturating_add(1));
    let payload = count
        .checked_mul(std::mem::size_of::<u16>())
        .filter(|bytes| *bytes <= limit)
        .ok_or(RuntimeError::StringLimit { limit })?;
    let mut storage = owner.map(|owner| owner.reserve_peer(payload)).transpose()?;
    let mut units = Vec::new();
    units
        .try_reserve_exact(count)
        .map_err(|_| owner.map_or(RuntimeError::SchedulingLimit, SharedSchedulingCharge::fail))?;
    if let Some(storage) = &mut storage {
        storage.resize(host_scheduling::slots(&units))?;
    }
    visit_lossy_utf16(bytes, |unit| {
        assert!(units.len() < units.capacity());
        units.push(unit);
    });
    Ok(NativeValue::from_retained_parts(
        Value::String(JsString::from_code_units(units)),
        storage,
    ))
}

fn visit_lossy_utf16(mut bytes: &[u8], mut visit: impl FnMut(u16)) {
    while !bytes.is_empty() {
        match std::str::from_utf8(bytes) {
            Ok(text) => {
                text.encode_utf16().for_each(&mut visit);
                break;
            }
            Err(error) => {
                let valid = error.valid_up_to();
                let prefix = std::str::from_utf8(&bytes[..valid]).expect("validated UTF8 prefix");
                prefix.encode_utf16().for_each(&mut visit);
                visit(0xfffd);
                if let Some(length) = error.error_len() {
                    bytes = &bytes[valid + length..];
                } else {
                    break;
                }
            }
        }
    }
}

pub(super) fn pack_parts(
    vm: &mut Vm,
    parts: TransportVector<Value>,
    native_endings: bool,
    context: &EventContext,
) -> Result<BlobBytes, RuntimeError> {
    context.validate()?;
    let mut plans = TransportVector::with_capacity(parts.len(), context.owner.as_ref())?;
    let mut length = 0usize;
    // Snapshot buffer/view metadata only after every dictionary conversion.
    // No guest call separates this pass from filling the exact output buffer.
    for value in parts.iter() {
        vm.charge_step()?;
        let plan = Part::snapshot(vm, value, native_endings)?;
        length = length.saturating_add(plan.length());
        if length > vm.heap.max_array_buffer_byte_length() {
            return Err(RuntimeError::RangeError(
                "Blob data exceeds heap byte limit".into(),
            ));
        }
        plans.push(plan)?;
    }
    let mut result = BlobBytes::new(length, context.owner.as_ref())?;
    let mut parts = parts.into_owned_iter()?;
    for plan in plans.iter().copied() {
        let part = parts.next_owned()?.expect("prepared Blob parts");
        plan.append(vm, part.value(), native_endings, &mut result.bytes)?;
    }
    Ok(result)
}
