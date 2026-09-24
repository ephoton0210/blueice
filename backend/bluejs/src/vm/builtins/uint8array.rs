// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The Uint8Array Base64/Hex proposal (ECMA-262 §23.2.3).  Decoding stays
//! here, separate from the generic TypedArray algorithms: these methods have
//! a Uint8Array-only receiver and intentionally expose partial writes.

use super::*;

/// The heap accesses below run on a Uint8Array the calling method has just
/// validated (or on a value user code handed in, which is always a live
/// object), so the only failure the heap can report, a handle it does not own,
/// cannot happen.
const LIVE: &str = "a validated Uint8Array is a live heap object";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Base64Alphabet {
    Standard,
    Url,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum LastChunkHandling {
    Loose,
    Strict,
    StopBeforePartial,
}

struct DecodedBytes {
    read: usize,
    bytes: Vec<u8>,
    syntax_error: bool,
}

impl DecodedBytes {
    fn empty() -> Self {
        Self {
            read: 0,
            bytes: Vec::new(),
            syntax_error: false,
        }
    }

    fn error(read: usize, bytes: Vec<u8>) -> Self {
        Self {
            read,
            bytes,
            syntax_error: true,
        }
    }
}

// The native dispatcher already refuses `new` on these non-constructor
// functions, so every `_construct` below is false.
impl Vm {
    pub(super) fn uint8_array_from_base64(
        &mut self,
        args: &[Value],
        _construct: bool,
    ) -> Result<Value, RuntimeError> {
        let input = self.uint8_array_string_argument(native::argument(args, 0))?;
        let (alphabet, handling) = self.uint8_array_decode_options(native::argument(args, 1))?;
        let decoded = decode_base64(input, alphabet, handling, usize::MAX);
        if decoded.syntax_error {
            return Err(RuntimeError::SyntaxError("invalid base64 string".into()));
        }
        self.uint8_array_from_bytes(decoded.bytes)
    }

    pub(super) fn uint8_array_from_hex(
        &mut self,
        args: &[Value],
        _construct: bool,
    ) -> Result<Value, RuntimeError> {
        let input = self.uint8_array_string_argument(native::argument(args, 0))?;
        let decoded = decode_hex(input, usize::MAX);
        if decoded.syntax_error {
            return Err(RuntimeError::SyntaxError(
                "invalid hexadecimal string".into(),
            ));
        }
        self.uint8_array_from_bytes(decoded.bytes)
    }

    pub(super) fn uint8_array_method(
        &mut self,
        receiver: &Value,
        args: &[Value],
        _construct: bool,
        method: Uint8ArrayMethod,
    ) -> Result<Value, RuntimeError> {
        match method {
            Uint8ArrayMethod::SetFromBase64 => self.uint8_array_set_from_base64(receiver, args),
            Uint8ArrayMethod::SetFromHex => self.uint8_array_set_from_hex(receiver, args),
            Uint8ArrayMethod::ToBase64 => self.uint8_array_to_base64(receiver, args),
            Uint8ArrayMethod::ToHex => self.uint8_array_to_hex(receiver),
        }
    }

    fn uint8_array_set_from_base64(
        &mut self,
        receiver: &Value,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        // Validate the Uint8Array brand before any option lookup, but defer
        // detached/out-of-bounds validation until after observable options.
        self.uint8_array_object(receiver)?;
        self.reject_immutable_typed_array(receiver)?;
        let input = self.uint8_array_string_argument(native::argument(args, 0))?;
        let (alphabet, handling) = self.uint8_array_decode_options(native::argument(args, 1))?;
        let (target, length) = self.uint8_array_validated_receiver(receiver)?;
        let decoded = decode_base64(input, alphabet, handling, length);
        self.uint8_array_write_bytes(target, &decoded.bytes);
        if decoded.syntax_error {
            return Err(RuntimeError::SyntaxError("invalid base64 string".into()));
        }
        self.uint8_array_progress_record(decoded.read, decoded.bytes.len())
    }

    fn uint8_array_set_from_hex(
        &mut self,
        receiver: &Value,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        self.uint8_array_object(receiver)?;
        self.reject_immutable_typed_array(receiver)?;
        let input = self.uint8_array_string_argument(native::argument(args, 0))?;
        let (target, length) = self.uint8_array_validated_receiver(receiver)?;
        let decoded = decode_hex(input, length);
        self.uint8_array_write_bytes(target, &decoded.bytes);
        if decoded.syntax_error {
            return Err(RuntimeError::SyntaxError(
                "invalid hexadecimal string".into(),
            ));
        }
        self.uint8_array_progress_record(decoded.read, decoded.bytes.len())
    }

    fn uint8_array_to_base64(
        &mut self,
        receiver: &Value,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        self.uint8_array_object(receiver)?;
        let (alphabet, omit_padding) =
            self.uint8_array_encode_options(native::argument(args, 0))?;
        let (target, length) = self.uint8_array_validated_receiver(receiver)?;
        let bytes = self.uint8_array_bytes(target, length);
        Ok(Value::String(
            encode_base64(&bytes, alphabet, omit_padding).into(),
        ))
    }

    fn uint8_array_to_hex(&mut self, receiver: &Value) -> Result<Value, RuntimeError> {
        let (target, length) = self.uint8_array_validated_receiver(receiver)?;
        let bytes = self.uint8_array_bytes(target, length);
        let mut result = String::with_capacity(bytes.len() * 2);
        for byte in bytes {
            use std::fmt::Write;
            write!(&mut result, "{byte:02x}").expect("writing to String cannot fail");
        }
        Ok(Value::String(result.into()))
    }

    fn uint8_array_string_argument<'a>(
        &self,
        value: &'a Value,
    ) -> Result<&'a JsString, RuntimeError> {
        match value {
            Value::String(input) => Ok(input),
            _ => Err(RuntimeError::TypeError(
                "Uint8Array encoding input must be a string".into(),
            )),
        }
    }

    fn uint8_array_object(&self, receiver: &Value) -> Result<ObjectId, RuntimeError> {
        let object = receiver.object_id().ok_or_else(|| {
            RuntimeError::TypeError("Uint8Array method requires a Uint8Array receiver".into())
        })?;
        if !self.heap.is_typed_array(object).expect(LIVE) {
            return Err(RuntimeError::TypeError(
                "Uint8Array method requires a Uint8Array receiver".into(),
            ));
        }
        let (_, _, _, kind) = self.heap.typed_array_info(object).expect(LIVE);
        if kind != TypedArrayKind::Uint8 {
            return Err(RuntimeError::TypeError(
                "Uint8Array method requires a Uint8Array receiver".into(),
            ));
        }
        Ok(object)
    }

    fn uint8_array_validated_receiver(
        &self,
        receiver: &Value,
    ) -> Result<(ObjectId, usize), RuntimeError> {
        let object = self.uint8_array_object(receiver)?;
        let (_, _, length, _) = self.typed_array_receiver(receiver)?;
        Ok((object, length))
    }

    fn uint8_array_bytes(&self, target: ObjectId, length: usize) -> Vec<u8> {
        let (buffer, byte_offset, _, _) = self.heap.typed_array_info(target).expect(LIVE);
        self.heap
            .array_buffer_copy(buffer, byte_offset, length)
            .expect(LIVE)
    }

    fn uint8_array_write_bytes(&mut self, target: ObjectId, bytes: &[u8]) {
        for (index, byte) in bytes.iter().enumerate() {
            self.with_roots(|heap| {
                heap.typed_array_set_index(target, index, &Value::Number(f64::from(*byte)))
            })
            .expect(LIVE);
        }
    }

    fn uint8_array_from_bytes(&mut self, bytes: Vec<u8>) -> Result<Value, RuntimeError> {
        let buffer = self.new_typed_array_buffer(bytes.len(), TypedArrayKind::Uint8)?;
        let prototype = self.buffer_prototype("Uint8Array").expect(LIVE);
        let target = self.with_roots(|heap| {
            heap.alloc_typed_array(
                buffer,
                0,
                bytes.len(),
                false,
                TypedArrayKind::Uint8,
                Some(prototype),
            )
        })?;
        self.uint8_array_write_bytes(target, &bytes);
        Ok(Value::Object(target))
    }

    fn uint8_array_progress_record(
        &mut self,
        read: usize,
        written: usize,
    ) -> Result<Value, RuntimeError> {
        let prototype = self.object_prototype;
        let record = self.with_roots(|heap| heap.alloc_object(Some(prototype)))?;
        self.define_data(record, "read", Value::Number(read as f64), true, true, true)?;
        self.define_data(
            record,
            "written",
            Value::Number(written as f64),
            true,
            true,
            true,
        )?;
        Ok(Value::Object(record))
    }

    fn uint8_array_decode_options(
        &mut self,
        options: &Value,
    ) -> Result<(Base64Alphabet, LastChunkHandling), RuntimeError> {
        if *options == Value::Undefined {
            return Ok((Base64Alphabet::Standard, LastChunkHandling::Loose));
        }
        let alphabet = self.get_property(options, &"alphabet".into())?;
        let alphabet = match alphabet {
            Value::Undefined => Base64Alphabet::Standard,
            Value::String(ref value) if value == "base64" => Base64Alphabet::Standard,
            Value::String(ref value) if value == "base64url" => Base64Alphabet::Url,
            _ => {
                return Err(RuntimeError::TypeError(
                    "invalid Uint8Array base64 alphabet".into(),
                ));
            }
        };
        let handling = self.get_property(options, &"lastChunkHandling".into())?;
        let handling = match handling {
            Value::Undefined => LastChunkHandling::Loose,
            Value::String(ref value) if value == "loose" => LastChunkHandling::Loose,
            Value::String(ref value) if value == "strict" => LastChunkHandling::Strict,
            Value::String(ref value) if value == "stop-before-partial" => {
                LastChunkHandling::StopBeforePartial
            }
            _ => {
                return Err(RuntimeError::TypeError(
                    "invalid Uint8Array base64 lastChunkHandling".into(),
                ));
            }
        };
        Ok((alphabet, handling))
    }

    fn uint8_array_encode_options(
        &mut self,
        options: &Value,
    ) -> Result<(Base64Alphabet, bool), RuntimeError> {
        if *options == Value::Undefined {
            return Ok((Base64Alphabet::Standard, false));
        }
        let alphabet = self.get_property(options, &"alphabet".into())?;
        let alphabet = match alphabet {
            Value::Undefined => Base64Alphabet::Standard,
            Value::String(ref value) if value == "base64" => Base64Alphabet::Standard,
            Value::String(ref value) if value == "base64url" => Base64Alphabet::Url,
            _ => {
                return Err(RuntimeError::TypeError(
                    "invalid Uint8Array base64 alphabet".into(),
                ));
            }
        };
        let omit_padding = self.get_property(options, &"omitPadding".into())?;
        Ok((alphabet, self.to_boolean(&omit_padding).expect(LIVE)))
    }
}

fn decode_base64(
    input: &JsString,
    alphabet: Base64Alphabet,
    handling: LastChunkHandling,
    maximum: usize,
) -> DecodedBytes {
    if maximum == 0 {
        return DecodedBytes::empty();
    }
    let units = input
        .as_code_units()
        .iter()
        .copied()
        .enumerate()
        .filter(|(_, unit)| !matches!(*unit, 0x09 | 0x0a | 0x0c | 0x0d | 0x20))
        .collect::<Vec<_>>();
    let mut bytes = Vec::new();
    let mut position = 0;
    let mut read = 0;
    while position + 4 <= units.len() {
        let group = &units[position..position + 4];
        let decoded =
            match decode_base64_group(group, alphabet, handling == LastChunkHandling::Strict) {
                Some(decoded) => decoded,
                None => return DecodedBytes::error(read, bytes),
            };
        if bytes.len().saturating_add(decoded.len()) > maximum {
            return DecodedBytes {
                read,
                bytes,
                syntax_error: false,
            };
        }
        let padded = group[2].1 == u16::from(b'=') || group[3].1 == u16::from(b'=');
        // A padded chunk is only complete when it is the final chunk. Check
        // that condition before appending it so `setFromBase64` preserves the
        // already-written prefix when it later reports a SyntaxError.
        if padded
            && position + 4 < units.len()
            && bytes.len().saturating_add(decoded.len()) != maximum
        {
            return DecodedBytes::error(read, bytes);
        }
        bytes.extend(decoded);
        position += 4;
        read = group[3].0 + 1;
        // Reaching the requested target length deliberately stops decoding
        // before looking at later input, including trailing garbage.
        if bytes.len() == maximum {
            return DecodedBytes {
                read,
                bytes,
                syntax_error: false,
            };
        }
        // A padded chunk that is not the last one was rejected above unless
        // it filled the destination, which returned already, so reaching this
        // point means the padded chunk ended the input.
        if padded {
            return DecodedBytes {
                read,
                bytes,
                syntax_error: false,
            };
        }
    }
    let remaining = &units[position..];
    if remaining.is_empty() {
        return DecodedBytes {
            read,
            bytes,
            syntax_error: false,
        };
    }
    let first_padding = remaining
        .iter()
        .position(|(_, unit)| *unit == u16::from(b'='));
    if let Some(first_padding) = first_padding {
        // A partial chunk is at most three units, so the only padding it can
        // validly hold is the single `=` that ends a three-unit chunk.
        if first_padding < 2 {
            return DecodedBytes::error(read, bytes);
        }
        return if handling == LastChunkHandling::StopBeforePartial {
            DecodedBytes {
                read,
                bytes,
                syntax_error: false,
            }
        } else {
            DecodedBytes::error(read, bytes)
        };
    }
    if handling == LastChunkHandling::StopBeforePartial {
        // Stopping before a valid partial chunk does not make an illegal
        // character valid. It only avoids decoding a syntactically valid
        // final group whose byte count is incomplete.
        if remaining
            .iter()
            .any(|(_, unit)| base64_value(*unit, alphabet).is_none())
        {
            return DecodedBytes::error(read, bytes);
        }
        return DecodedBytes {
            read,
            bytes,
            syntax_error: false,
        };
    }
    if remaining.len() == 1 || handling == LastChunkHandling::Strict {
        return DecodedBytes::error(read, bytes);
    }
    let values = remaining
        .iter()
        .map(|(_, unit)| base64_value(*unit, alphabet))
        .collect::<Option<Vec<_>>>();
    let Some(values) = values else {
        return DecodedBytes::error(read, bytes);
    };
    // The partial chunk holds two or three sextets here: one was rejected
    // above and four would have formed a full group.
    let (first, second) = (values[0], values[1]);
    let mut decoded = vec![(first << 2) | (second >> 4)];
    if let Some(third) = values.get(2) {
        decoded.push((second << 4) | (third >> 2));
    }
    if bytes.len().saturating_add(decoded.len()) > maximum {
        return DecodedBytes {
            read,
            bytes,
            syntax_error: false,
        };
    }
    bytes.extend(decoded);
    DecodedBytes {
        read: remaining.last().expect("non-empty partial chunk").0 + 1,
        bytes,
        syntax_error: false,
    }
}

fn decode_base64_group(
    group: &[(usize, u16)],
    alphabet: Base64Alphabet,
    strict: bool,
) -> Option<Vec<u8>> {
    let first = base64_value(group[0].1, alphabet)?;
    let second = base64_value(group[1].1, alphabet)?;
    match (group[2].1, group[3].1) {
        (equals, equals_again) if equals == u16::from(b'=') && equals_again == u16::from(b'=') => {
            if strict && second & 0x0f != 0 {
                return None;
            }
            Some(vec![(first << 2) | (second >> 4)])
        }
        (third, equals) if equals == u16::from(b'=') => {
            let third = base64_value(third, alphabet)?;
            if strict && third & 0x03 != 0 {
                return None;
            }
            Some(vec![
                (first << 2) | (second >> 4),
                (second << 4) | (third >> 2),
            ])
        }
        (third, fourth) => {
            let third = base64_value(third, alphabet)?;
            let fourth = base64_value(fourth, alphabet)?;
            Some(vec![
                (first << 2) | (second >> 4),
                (second << 4) | (third >> 2),
                (third << 6) | fourth,
            ])
        }
    }
}

fn base64_value(unit: u16, alphabet: Base64Alphabet) -> Option<u8> {
    match unit {
        0x41..=0x5a => Some((unit - u16::from(b'A')) as u8),
        0x61..=0x7a => Some((unit - u16::from(b'a') + 26) as u8),
        0x30..=0x39 => Some((unit - u16::from(b'0') + 52) as u8),
        value
            if value
                == u16::from(if alphabet == Base64Alphabet::Standard {
                    b'+'
                } else {
                    b'-'
                }) =>
        {
            Some(62)
        }
        value
            if value
                == u16::from(if alphabet == Base64Alphabet::Standard {
                    b'/'
                } else {
                    b'_'
                }) =>
        {
            Some(63)
        }
        _ => None,
    }
}

fn decode_hex(input: &JsString, maximum: usize) -> DecodedBytes {
    let units = input.as_code_units();
    // Hex validates its all-or-nothing pair structure before it starts
    // writing, including for a zero-length destination.
    if !units.len().is_multiple_of(2) {
        return DecodedBytes::error(0, Vec::new());
    }
    if maximum == 0 {
        return DecodedBytes::empty();
    }
    let mut bytes = Vec::new();
    for (index, pair) in units.chunks_exact(2).enumerate() {
        if index == maximum {
            return DecodedBytes {
                read: index * 2,
                bytes,
                syntax_error: false,
            };
        }
        let Some(high) = hex_value(pair[0]) else {
            return DecodedBytes::error(index * 2, bytes);
        };
        let Some(low) = hex_value(pair[1]) else {
            return DecodedBytes::error(index * 2, bytes);
        };
        bytes.push((high << 4) | low);
    }
    DecodedBytes {
        read: units.len(),
        bytes,
        syntax_error: false,
    }
}

fn hex_value(unit: u16) -> Option<u8> {
    match unit {
        0x30..=0x39 => Some((unit - u16::from(b'0')) as u8),
        0x61..=0x66 => Some((unit - u16::from(b'a') + 10) as u8),
        0x41..=0x46 => Some((unit - u16::from(b'A') + 10) as u8),
        _ => None,
    }
}

fn encode_base64(bytes: &[u8], alphabet: Base64Alphabet, omit_padding: bool) -> String {
    let alphabet = match alphabet {
        Base64Alphabet::Standard => {
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/".as_slice()
        }
        Base64Alphabet::Url => {
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_".as_slice()
        }
    };
    let mut output = String::with_capacity(bytes.len().div_ceil(3) * 4);
    let chunks = bytes.chunks_exact(3);
    let remainder = chunks.remainder();
    for chunk in chunks {
        let (first, second, third) = (chunk[0], chunk[1], chunk[2]);
        output.push(alphabet[(first >> 2) as usize] as char);
        output.push(alphabet[((first & 0x03) << 4 | (second >> 4)) as usize] as char);
        output.push(alphabet[((second & 0x0f) << 2 | (third >> 6)) as usize] as char);
        output.push(alphabet[(third & 0x3f) as usize] as char);
    }
    match remainder {
        [first, second] => {
            output.push(alphabet[(first >> 2) as usize] as char);
            output.push(alphabet[((first & 0x03) << 4 | (second >> 4)) as usize] as char);
            output.push(alphabet[((second & 0x0f) << 2) as usize] as char);
            if !omit_padding {
                output.push('=');
            }
        }
        [first] => {
            output.push(alphabet[(first >> 2) as usize] as char);
            output.push(alphabet[((first & 0x03) << 4) as usize] as char);
            if !omit_padding {
                output.push_str("==");
            }
        }
        _ => {}
    }
    output
}
