// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Read results admit exact UTF16 output; metadata and decoder scratch stay owned.

use super::*;
use crate::vm::host_scheduling::SharedSchedulingCharge;
use crate::vm::web_file_api::storage::BlobBytes;
use base64::Engine;

pub(super) struct NativeUtf8 {
    pub data: String,
    _storage: Option<SharedSchedulingCharge>,
}

impl NativeUtf8 {
    pub(super) fn copied(
        text: &JsString,
        owner: Option<&SharedSchedulingCharge>,
    ) -> Result<Option<Self>, RuntimeError> {
        if let Some(owner) = owner {
            owner.reserve_peer(0)?;
        }
        let mut count = 0usize;
        for character in char::decode_utf16(text.as_code_units().iter().copied()) {
            let Ok(character) = character else {
                return Ok(None);
            };
            count = count.saturating_add(character.len_utf8());
        }
        let mut storage = owner.map(|owner| owner.reserve_peer(count)).transpose()?;
        let mut data = String::new();
        data.try_reserve_exact(count).map_err(|_| {
            owner.map_or(RuntimeError::SchedulingLimit, SharedSchedulingCharge::fail)
        })?;
        if let Some(storage) = &mut storage {
            storage.resize(data.capacity())?;
        }
        for character in char::decode_utf16(text.as_code_units().iter().copied()) {
            let character = character.expect("validated UTF16 metadata");
            assert!(data.len() + character.len_utf8() <= data.capacity());
            data.push(character);
        }
        Ok(Some(Self {
            data,
            _storage: storage,
        }))
    }
}

fn utf16(
    count: usize,
    limit: usize,
    owner: Option<&SharedSchedulingCharge>,
    fill: impl FnOnce(&mut Vec<u16>) -> Result<(), RuntimeError>,
) -> Result<NativeValue, RuntimeError> {
    let bytes = count
        .checked_mul(std::mem::size_of::<u16>())
        .filter(|bytes| *bytes <= limit)
        .ok_or(RuntimeError::StringLimit { limit })?;
    let mut storage = owner.map(|owner| owner.reserve_peer(bytes)).transpose()?;
    let mut units = Vec::new();
    units
        .try_reserve_exact(count)
        .map_err(|_| owner.map_or(RuntimeError::SchedulingLimit, SharedSchedulingCharge::fail))?;
    if let Some(storage) = &mut storage {
        storage.resize(host_scheduling::slots(&units))?;
    }
    fill(&mut units)?;
    assert_eq!(units.len(), count);
    Ok(NativeValue::from_retained_parts(
        Value::String(JsString::from_code_units(units)),
        storage,
    ))
}

fn walk_decoded(
    encoding: &'static encoding_rs::Encoding,
    bytes: &[u8],
    mut visit: impl FnMut(&[u16]) -> Result<(), RuntimeError>,
) -> Result<(), RuntimeError> {
    // A complete UTF16 character fits this fixed stack scratch; the decoder
    // and its output scratch allocate no heap storage.
    let mut decoder = encoding.new_decoder();
    let mut scratch = [0u16; 1024];
    let mut offset = 0;
    loop {
        let (result, read, written, _) =
            decoder.decode_to_utf16(&bytes[offset..], &mut scratch, true);
        visit(&scratch[..written])?;
        offset += read;
        if result == encoding_rs::CoderResult::InputEmpty {
            return Ok(());
        }
        if read == 0 && written == 0 {
            return Err(RuntimeError::Unsupported(
                "File text decoder made no progress",
            ));
        }
    }
}

fn decoded_text(
    encoding: &'static encoding_rs::Encoding,
    bytes: &[u8],
    limit: usize,
    owner: Option<&SharedSchedulingCharge>,
) -> Result<NativeValue, RuntimeError> {
    let mut count = 0usize;
    walk_decoded(encoding, bytes, |units| {
        count = count
            .checked_add(units.len())
            .filter(|count| count.checked_mul(2).is_some_and(|bytes| bytes <= limit))
            .ok_or(RuntimeError::StringLimit { limit })?;
        Ok(())
    })?;
    utf16(count, limit, owner, |output| {
        walk_decoded(encoding, bytes, |units| {
            assert!(output.len() + units.len() <= output.capacity());
            output.extend_from_slice(units);
            Ok(())
        })
    })
}

pub(super) fn package(
    vm: &mut Vm,
    blob: ObjectId,
    kind: FileReadKind,
    label: Option<&JsString>,
    context: &EventContext,
    roots: &RealmCallRoots,
) -> Result<NativeValue, RuntimeError> {
    let owner = context.owner.as_ref();
    let limit = vm.config.max_string_bytes;
    let result = match kind {
        FileReadKind::ArrayBuffer => {
            let bytes = BlobBytes::copied(&vm.heap.web_blob(blob)?.bytes, owner)?;
            let length = bytes.len();
            let prototype = vm.buffer_prototype("ArrayBuffer");
            let prototype = context.validate().and(prototype)?;
            let buffer = vm.with_roots(|heap| heap.alloc_array_buffer(length, Some(prototype)))?;
            roots.push(&vm.heap, buffer)?;
            bytes.copy_into_buffer(vm, buffer)?;
            NativeValue::adopt(Value::Object(buffer), owner)?
        }
        FileReadKind::BinaryString => {
            let bytes = &vm.heap.web_blob(blob)?.bytes;
            utf16(bytes.len(), limit, owner, |output| {
                output.extend(bytes.iter().copied().map(u16::from));
                Ok(())
            })?
        }
        FileReadKind::DataUrl => {
            let data = vm.heap.web_blob(blob)?;
            let mime = &data.media_type;
            let valid_mime = !mime.is_empty()
                && char::decode_utf16(mime.as_code_units().iter().copied())
                    .all(|character| character.is_ok());
            let fallback = "application/octet-stream";
            let mime_length = if valid_mime {
                mime.len()
            } else {
                fallback.len()
            };
            let encoded = data
                .bytes
                .len()
                .checked_add(2)
                .and_then(|length| (length / 3).checked_mul(4))
                .ok_or(RuntimeError::StringLimit { limit })?;
            let count = 13usize
                .checked_add(mime_length)
                .and_then(|count| count.checked_add(encoded))
                .ok_or(RuntimeError::StringLimit { limit })?;
            utf16(count, limit, owner, |output| {
                output.extend("data:".encode_utf16());
                if valid_mime {
                    output.extend_from_slice(mime.as_code_units());
                } else {
                    output.extend(fallback.encode_utf16());
                }
                output.extend(";base64,".encode_utf16());
                for bytes in data.bytes.chunks(3) {
                    let mut ascii = [0; 4];
                    let count = base64::engine::general_purpose::STANDARD
                        .encode_slice(bytes, &mut ascii)
                        .expect("one base64 group fits");
                    output.extend(ascii[..count].iter().copied().map(u16::from));
                }
                Ok(())
            })?
        }
        FileReadKind::Text => {
            let label = label
                .map(|label| NativeUtf8::copied(label, owner))
                .transpose()?
                .flatten();
            let explicit = label
                .as_ref()
                .and_then(|label| encoding_rs::Encoding::for_label(label.data.as_bytes()));
            let mime = NativeUtf8::copied(&vm.heap.web_blob(blob)?.media_type, owner)?;
            let charset = if let Some(mime) = &mime {
                reader_mime_encoding(&mime.data, owner)?
            } else {
                None
            };
            let encoding = explicit.or(charset).unwrap_or(encoding_rs::UTF_8);
            decoded_text(encoding, &vm.heap.web_blob(blob)?.bytes, limit, owner)?
        }
    };
    context.validate()?;
    vm.check_string(result.value())?;
    Ok(result)
}
