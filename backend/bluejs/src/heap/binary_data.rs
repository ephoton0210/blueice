// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

impl Heap {
    pub(crate) fn alloc_array_buffer(
        &mut self,
        byte_length: usize,
        prototype: Option<ObjectId>,
    ) -> Result<ObjectId, HeapError> {
        // Reject before asking Rust's allocator for an unbounded data block.
        // The heap limit is also the currently supported ArrayBuffer limit;
        // a future resizable-buffer implementation will have a separate
        // maximum-byte-length contract.
        if byte_length > self.config.max_heap_bytes.saturating_sub(OBJECT_BYTES) {
            return Err(HeapError::InvalidBufferRange);
        }
        self.alloc(
            ObjectKind::ArrayBuffer {
                bytes: vec![0; byte_length],
                detached: false,
            },
            prototype,
        )
    }

    pub(crate) fn alloc_data_view(
        &mut self,
        buffer: ObjectId,
        byte_offset: usize,
        byte_length: usize,
        prototype: Option<ObjectId>,
    ) -> Result<ObjectId, HeapError> {
        let length = self.validate_array_buffer(buffer)?;
        if byte_offset
            .checked_add(byte_length)
            .is_none_or(|end| end > length)
        {
            return Err(HeapError::InvalidBufferRange);
        }
        self.alloc(
            ObjectKind::DataView {
                buffer,
                byte_offset,
                byte_length,
            },
            prototype,
        )
    }

    pub(crate) fn alloc_typed_array(
        &mut self,
        buffer: ObjectId,
        byte_offset: usize,
        length: usize,
        kind: TypedArrayKind,
        prototype: Option<ObjectId>,
    ) -> Result<ObjectId, HeapError> {
        let byte_length = length
            .checked_mul(kind.byte_width())
            .ok_or(HeapError::InvalidBufferRange)?;
        let available = self.validate_array_buffer(buffer)?;
        if byte_offset
            .checked_add(byte_length)
            .is_none_or(|end| end > available)
        {
            return Err(HeapError::InvalidBufferRange);
        }
        self.alloc(
            ObjectKind::TypedArray {
                buffer,
                byte_offset,
                length,
                kind,
            },
            prototype,
        )
    }

    pub(crate) fn is_array_buffer(&self, object: ObjectId) -> Result<bool, HeapError> {
        Ok(matches!(
            self.object(object)?.kind,
            ObjectKind::ArrayBuffer { .. }
        ))
    }

    pub(crate) fn is_data_view(&self, object: ObjectId) -> Result<bool, HeapError> {
        Ok(matches!(
            self.object(object)?.kind,
            ObjectKind::DataView { .. }
        ))
    }

    pub(crate) fn is_typed_array(&self, object: ObjectId) -> Result<bool, HeapError> {
        Ok(matches!(
            self.object(object)?.kind,
            ObjectKind::TypedArray { .. }
        ))
    }

    pub(crate) fn max_array_buffer_byte_length(&self) -> usize {
        self.config.max_heap_bytes.saturating_sub(OBJECT_BYTES)
    }

    pub(crate) fn array_buffer_byte_length(&self, object: ObjectId) -> Result<usize, HeapError> {
        let ObjectKind::ArrayBuffer { bytes, .. } = &self.object(object)?.kind else {
            return Err(HeapError::InvalidInternalSlot(object));
        };
        Ok(bytes.len())
    }

    pub(crate) fn array_buffer_is_detached(&self, object: ObjectId) -> Result<bool, HeapError> {
        let ObjectKind::ArrayBuffer { detached, .. } = &self.object(object)?.kind else {
            return Err(HeapError::InvalidInternalSlot(object));
        };
        Ok(*detached)
    }

    fn validate_array_buffer(&self, object: ObjectId) -> Result<usize, HeapError> {
        if self.array_buffer_is_detached(object)? {
            return Err(HeapError::DetachedArrayBuffer);
        }
        self.array_buffer_byte_length(object)
    }

    pub(crate) fn detach_array_buffer(&mut self, object: ObjectId) -> Result<(), HeapError> {
        let obj = self
            .objects
            .get_mut(&object)
            .ok_or(HeapError::InvalidObject(object))?;
        let ObjectKind::ArrayBuffer { bytes, detached } = &mut obj.kind else {
            return Err(HeapError::InvalidInternalSlot(object));
        };
        if *detached {
            return Err(HeapError::DetachedArrayBuffer);
        }
        let released = bytes.len();
        bytes.clear();
        bytes.shrink_to_fit();
        *detached = true;
        obj.bytes -= released;
        self.managed_bytes -= released;
        Ok(())
    }

    pub(crate) fn array_buffer_copy(
        &self,
        object: ObjectId,
        byte_offset: usize,
        byte_length: usize,
    ) -> Result<Vec<u8>, HeapError> {
        let bytes = self.array_buffer_bytes(object)?;
        let end = byte_offset
            .checked_add(byte_length)
            .filter(|end| *end <= bytes.len())
            .ok_or(HeapError::InvalidBufferRange)?;
        Ok(bytes[byte_offset..end].to_vec())
    }

    pub(crate) fn array_buffer_write(
        &mut self,
        object: ObjectId,
        byte_offset: usize,
        values: &[u8],
    ) -> Result<(), HeapError> {
        let bytes = self.array_buffer_bytes_mut(object)?;
        let end = byte_offset
            .checked_add(values.len())
            .filter(|end| *end <= bytes.len())
            .ok_or(HeapError::InvalidBufferRange)?;
        bytes[byte_offset..end].copy_from_slice(values);
        Ok(())
    }

    /// Returns a DataView's internal slots without checking whether its
    /// backing buffer has detached. DataView element operations perform
    /// observable argument conversion before that validation.
    pub(crate) fn data_view_raw_info(
        &self,
        object: ObjectId,
    ) -> Result<(ObjectId, usize, usize), HeapError> {
        let ObjectKind::DataView {
            buffer,
            byte_offset,
            byte_length,
        } = self.object(object)?.kind
        else {
            return Err(HeapError::InvalidInternalSlot(object));
        };
        Ok((buffer, byte_offset, byte_length))
    }

    /// Returns the `[[ViewedArrayBuffer]]` slot without validating its current
    /// detach state. `DataView.prototype.buffer` exposes this slot even after
    /// detachment, while the byte-length, byte-offset, and element operations
    /// validate it at their specified observable step.
    pub(crate) fn data_view_buffer(&self, object: ObjectId) -> Result<ObjectId, HeapError> {
        self.data_view_raw_info(object).map(|(buffer, _, _)| buffer)
    }

    pub(crate) fn typed_array_info(
        &self,
        object: ObjectId,
    ) -> Result<(ObjectId, usize, usize, TypedArrayKind), HeapError> {
        let ObjectKind::TypedArray {
            buffer,
            byte_offset,
            length,
            kind,
        } = self.object(object)?.kind
        else {
            return Err(HeapError::InvalidInternalSlot(object));
        };
        Ok((buffer, byte_offset, length, kind))
    }

    pub(crate) fn typed_array_numeric_key(
        &self,
        object: ObjectId,
        key: &PropertyName,
    ) -> Result<Option<TypedArrayNumericKey>, HeapError> {
        if !matches!(self.object(object)?.kind, ObjectKind::TypedArray { .. }) {
            return Ok(None);
        }
        Ok(typed_array_numeric_key(key))
    }

    pub(crate) fn typed_array_index_value(
        &self,
        object: ObjectId,
        index: usize,
    ) -> Result<Option<Value>, HeapError> {
        if !matches!(self.object(object)?.kind, ObjectKind::TypedArray { .. }) {
            return Ok(None);
        }
        let (buffer, byte_offset, length, kind) = self.typed_array_info(object)?;
        if self.array_buffer_is_detached(buffer)? {
            return Ok(None);
        }
        if index >= length {
            return Ok(None);
        }
        let bytes = self.array_buffer_bytes(buffer)?;
        let start = byte_offset + index * kind.byte_width();
        Ok(Some(Value::Number(typed_read(kind, &bytes[start..]))))
    }

    pub(crate) fn typed_array_set_index(
        &mut self,
        object: ObjectId,
        index: usize,
        value: f64,
    ) -> Result<bool, HeapError> {
        let (buffer, byte_offset, length, kind) = self.typed_array_info(object)?;
        if index >= length {
            return Ok(false);
        }
        let start = byte_offset + index * kind.byte_width();
        let bytes = self.array_buffer_bytes_mut(buffer)?;
        typed_write(kind, &mut bytes[start..], value);
        Ok(true)
    }

    fn array_buffer_bytes(&self, object: ObjectId) -> Result<&[u8], HeapError> {
        let ObjectKind::ArrayBuffer { bytes, detached } = &self.object(object)?.kind else {
            return Err(HeapError::InvalidInternalSlot(object));
        };
        if *detached {
            return Err(HeapError::DetachedArrayBuffer);
        }
        Ok(bytes)
    }

    fn array_buffer_bytes_mut(&mut self, object: ObjectId) -> Result<&mut [u8], HeapError> {
        let ObjectKind::ArrayBuffer { bytes, detached } = &mut self
            .objects
            .get_mut(&object)
            .ok_or(HeapError::InvalidObject(object))?
            .kind
        else {
            return Err(HeapError::InvalidObject(object));
        };
        if *detached {
            return Err(HeapError::DetachedArrayBuffer);
        }
        Ok(bytes)
    }
}

pub(super) fn typed_array_numeric_key(key: &PropertyName) -> Option<TypedArrayNumericKey> {
    let PropertyName::String(key) = key else {
        return None;
    };
    if let Some(index) = key.index() {
        return Some(TypedArrayNumericKey::Index(index));
    }
    let key = key.to_utf8().ok()?;
    if matches!(key.as_str(), "-0" | "NaN" | "Infinity" | "-Infinity") {
        return Some(TypedArrayNumericKey::Invalid);
    }
    let number = key.parse::<f64>().ok()?;
    if !number.is_finite() || !canonical_numeric_string_matches(number, &key) {
        return None;
    }
    if number < 0.0 || number.fract() != 0.0 || number > usize::MAX as f64 {
        return Some(TypedArrayNumericKey::Invalid);
    }
    Some(TypedArrayNumericKey::Index(number as usize))
}

pub(super) fn canonical_numeric_string_matches(number: f64, key: &str) -> bool {
    ecmascript_number_string(number) == key
}

/// The number formatting selection in `CanonicalNumericIndexString` follows
/// ECMAScript's Number::toString thresholds, which differ from Rust's display
/// formatter. In particular, 1e-7 is canonical as `"1e-7"`, never as
/// `"0.0000001"`; treating the latter as an integer-indexed key would prevent
/// a TypedArray from defining an ordinary property with that name.
pub(super) fn ecmascript_number_string(number: f64) -> String {
    let rendered = number.to_string();
    let magnitude = number.abs();
    if (1e-6..1e21).contains(&magnitude) {
        return scientific_to_fixed(&rendered).unwrap_or(rendered);
    }
    if magnitude != 0.0 && !rendered.contains('e') {
        return fixed_to_scientific(&rendered).unwrap_or(rendered);
    }
    let Some((mantissa, exponent)) = rendered.split_once('e') else {
        return rendered;
    };
    let exponent = exponent
        .parse::<i32>()
        .expect("Rust formats a decimal exponent");
    format!(
        "{mantissa}e{}{exponent}",
        if exponent >= 0 { "+" } else { "" }
    )
}

pub(super) fn scientific_to_fixed(number: &str) -> Option<String> {
    let (mantissa, exponent) = number.split_once('e')?;
    let exponent = exponent.parse::<i32>().ok()?;
    let (sign, mantissa) = mantissa
        .strip_prefix('-')
        .map_or(("", mantissa), |value| ("-", value));
    let decimal = mantissa.find('.').unwrap_or(mantissa.len()) as i32;
    let digits = mantissa.replace('.', "");
    let position = decimal.checked_add(exponent)?;
    let body = if position <= 0 {
        format!(
            "0.{}{}",
            "0".repeat(position.unsigned_abs() as usize),
            digits
        )
    } else if position as usize >= digits.len() {
        format!("{}{}", digits, "0".repeat(position as usize - digits.len()))
    } else {
        format!(
            "{}.{}",
            &digits[..position as usize],
            &digits[position as usize..]
        )
    };
    Some(format!("{sign}{body}"))
}

pub(super) fn fixed_to_scientific(number: &str) -> Option<String> {
    let (sign, number) = number
        .strip_prefix('-')
        .map_or(("", number), |value| ("-", value));
    let decimal = number.find('.').unwrap_or(number.len());
    let digits = number.replace('.', "");
    let first = digits.find(|character| character != '0')?;
    let significant = digits[first..].trim_end_matches('0');
    let exponent = decimal as i32 - first as i32 - 1;
    let (head, tail) = significant.split_at(1);
    let fraction = if tail.is_empty() {
        String::new()
    } else {
        format!(".{tail}")
    };
    Some(format!(
        "{sign}{head}{}e{}{exponent}",
        fraction,
        if exponent >= 0 { "+" } else { "" },
    ))
}

pub(super) fn integer_for_typed_array(value: f64) -> f64 {
    if !value.is_finite() || value == 0.0 {
        0.0
    } else {
        value.trunc()
    }
}

pub(super) fn typed_read(kind: TypedArrayKind, bytes: &[u8]) -> f64 {
    match kind {
        TypedArrayKind::Int8 => i8::from_le_bytes([bytes[0]]) as f64,
        TypedArrayKind::Uint8 | TypedArrayKind::Uint8Clamped => bytes[0] as f64,
        TypedArrayKind::Int16 => i16::from_le_bytes(bytes[..2].try_into().unwrap()) as f64,
        TypedArrayKind::Uint16 => u16::from_le_bytes(bytes[..2].try_into().unwrap()) as f64,
        TypedArrayKind::Int32 => i32::from_le_bytes(bytes[..4].try_into().unwrap()) as f64,
        TypedArrayKind::Uint32 => u32::from_le_bytes(bytes[..4].try_into().unwrap()) as f64,
        TypedArrayKind::Float32 => f32::from_le_bytes(bytes[..4].try_into().unwrap()) as f64,
        TypedArrayKind::Float64 => f64::from_le_bytes(bytes[..8].try_into().unwrap()),
    }
}

pub(super) fn typed_write(kind: TypedArrayKind, bytes: &mut [u8], value: f64) {
    let integer = integer_for_typed_array(value);
    match kind {
        TypedArrayKind::Int8 => bytes[..1].copy_from_slice(&(integer as i64 as i8).to_le_bytes()),
        TypedArrayKind::Uint8 => bytes[..1].copy_from_slice(&(integer as i64 as u8).to_le_bytes()),
        TypedArrayKind::Uint8Clamped => {
            let clamped = if value.is_nan() || value <= 0.0 {
                0
            } else if value >= 255.0 {
                255
            } else {
                let floor = value.floor();
                let fraction = value - floor;
                if fraction > 0.5 || (fraction == 0.5 && (floor as u8 & 1) == 1) {
                    floor as u8 + 1
                } else {
                    floor as u8
                }
            };
            bytes[0] = clamped;
        }
        TypedArrayKind::Int16 => bytes[..2].copy_from_slice(&(integer as i64 as i16).to_le_bytes()),
        TypedArrayKind::Uint16 => {
            bytes[..2].copy_from_slice(&(integer as i64 as u16).to_le_bytes())
        }
        TypedArrayKind::Int32 => bytes[..4].copy_from_slice(&(integer as i64 as i32).to_le_bytes()),
        TypedArrayKind::Uint32 => {
            bytes[..4].copy_from_slice(&(integer as i64 as u32).to_le_bytes())
        }
        TypedArrayKind::Float32 => bytes[..4].copy_from_slice(&(value as f32).to_le_bytes()),
        TypedArrayKind::Float64 => bytes[..8].copy_from_slice(&value.to_le_bytes()),
    }
}
