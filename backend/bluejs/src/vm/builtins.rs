// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

mod array_change_by_copy;
mod array_from_async;
mod arrays;
mod binary_data;
mod execution;
mod generators;
mod globals;
mod immutable_arraybuffer;
mod math;
mod native_dispatch;
mod object;
mod promises;
mod resource_management;
mod typed_arrays;
mod uint8array;
use crate::heap::{
    f16_bits_to_f64, f64_to_f16_bits, same_value, ArrayIteratorKind, AsyncGeneratorCompletion,
    AsyncGeneratorDelegate, AsyncGeneratorRequest, AsyncGeneratorStatus, GeneratorState,
    IteratorHelperKind, IteratorHelperState, TypedArrayKind, TypedArrayNumericKey,
};
use crate::native::{
    AtomicOp, MapMethod, MathMethod, NumberMethod, ObjectMethod, PatternMethod, SetMethod,
    StringMethod, TypedArrayMethod, Uint8ArrayMethod, WeakCollectionMethod,
};
use num_bigint::BigUint;
use num_traits::One;
use std::collections::HashMap;
use std::rc::Rc;

mod arguments;
mod collections;
mod dynamic;
mod general;
mod numbers;
pub(super) struct ClosureCall {
    pub code: Rc<Bytecode>,
    pub captures: Vec<ObjectId>,
    pub callee: Value,
    pub receiver: Value,
    pub args: Vec<Value>,
    pub construct: bool,
    pub home: Option<ObjectId>,
    pub class_base: Option<Value>,
}

fn array_index_below_length(key: &PropertyName, length: u64) -> Option<u32> {
    let PropertyName::String(name) = key else {
        return None;
    };
    let name = name.to_utf8().ok()?;
    let index = name.parse::<u32>().ok()?;
    (name == index.to_string() && u64::from(index) < length).then_some(index)
}

fn same_value_zero(left: &Value, right: &Value) -> bool {
    left == right
        || matches!((left, right), (Value::Number(left), Value::Number(right)) if left.is_nan() && right.is_nan())
}

/// Return the exact, non-negative binary64 value as an integer divided by
/// 2^1074. Keeping a single denominator lets `Number.prototype.toString`
/// choose the shortest radix representation which rounds back to the input
/// Number without accumulating floating-point conversion error.
fn number_numerator(value: f64) -> BigUint {
    debug_assert!(value.is_finite() && value >= 0.0);
    let bits = value.to_bits();
    let exponent = ((bits >> 52) & 0x7ff) as usize;
    let fraction = bits & ((1_u64 << 52) - 1);
    if exponent == 0 {
        BigUint::from(fraction)
    } else {
        BigUint::from((1_u64 << 52) | fraction) << (exponent - 1)
    }
}

fn number_radix_string(number: f64, radix: u32) -> String {
    debug_assert!(number.is_finite() && number != 0.0 && (2..=36).contains(&radix));

    let negative = number.is_sign_negative();
    let magnitude = number.abs();
    let numerator = number_numerator(magnitude);

    // Values at least 2^52 are integral binary64 values. Their exact integer
    // form also avoids needing an upper rounding boundary for MAX_VALUE.
    if magnitude.fract() == 0.0 {
        let output = (numerator >> 1074_usize).to_str_radix(radix);
        return if negative {
            format!("-{output}")
        } else {
            output
        };
    }

    // A base-radix literal may differ from the exact binary value so long as
    // parsing it rounds back to the same binary64. Search increasing numbers
    // of fractional radix digits and use the closest candidate at each step.
    // Every finite binary64 reaches a candidate within 1075 binary digits;
    // larger radices only reduce that bound.
    const DENOMINATOR_BITS: usize = 1074;
    let denominator = BigUint::one() << DENOMINATOR_BITS;
    let previous = number_numerator(f64::from_bits(magnitude.to_bits() - 1));
    let next = number_numerator(f64::from_bits(magnitude.to_bits() + 1));
    let lower = &numerator + previous;
    let upper = &numerator + next;
    let inclusive_boundary = magnitude.to_bits() & 1 == 0;
    let mut power = BigUint::one();

    for fraction_digits in 0..=DENOMINATOR_BITS + 1 {
        let scaled = &numerator * &power;
        let mut candidate = &scaled >> DENOMINATOR_BITS;
        let remainder = scaled - (&candidate << DENOMINATOR_BITS);
        let doubled_remainder = &remainder << 1;
        if doubled_remainder > denominator
            || (doubled_remainder == denominator && (&candidate & BigUint::one()) == BigUint::one())
        {
            candidate += BigUint::one();
        }

        // Compare candidate / radix^fraction_digits to the two exact
        // round-to-nearest-even midpoints around `number`.
        let candidate_scaled = &candidate << (DENOMINATOR_BITS + 1);
        let lower_order = candidate_scaled.cmp(&(&lower * &power));
        let upper_order = candidate_scaled.cmp(&(&upper * &power));
        let above_lower = lower_order.is_gt() || (inclusive_boundary && lower_order.is_eq());
        let below_upper = upper_order.is_lt() || (inclusive_boundary && upper_order.is_eq());
        if above_lower && below_upper {
            let digits = candidate.to_str_radix(radix);
            let output = if fraction_digits == 0 {
                digits
            } else if digits.len() <= fraction_digits {
                format!("0.{}{}", "0".repeat(fraction_digits - digits.len()), digits)
            } else {
                let split_at = digits.len() - fraction_digits;
                format!("{}.{}", &digits[..split_at], &digits[split_at..])
            };
            return if negative {
                format!("-{output}")
            } else {
                output
            };
        }
        power *= radix;
    }

    unreachable!("every finite Number has a shortest radix representation")
}

/// Validate the non-mutating part of ValidateAndApplyPropertyDescriptor.
/// Heap::define_own_property performs the corresponding mutation for ordinary
/// objects; Proxy trap invariants need the same answer before a trap result is
/// allowed to claim success.
fn compatible_property_descriptor(
    extensible: bool,
    current: Option<&PropertyDescriptor>,
    descriptor: &PropertyDescriptor,
) -> bool {
    let Some(current) = current else {
        return extensible;
    };
    if descriptor.value.is_none()
        && descriptor.writable.is_none()
        && descriptor.get.is_none()
        && descriptor.set.is_none()
        && descriptor.enumerable.is_none()
        && descriptor.configurable.is_none()
    {
        return true;
    }
    if current.configurable == Some(false) {
        if descriptor.configurable == Some(true)
            || descriptor
                .enumerable
                .is_some_and(|value| Some(value) != current.enumerable)
        {
            return false;
        }
        let descriptor_is_data = descriptor.value.is_some() || descriptor.writable.is_some();
        let descriptor_is_accessor = descriptor.accessor();
        if (descriptor_is_data && current.accessor())
            || (descriptor_is_accessor && !current.accessor())
        {
            return false;
        }
        if current.accessor() {
            if descriptor.get.as_ref().is_some_and(|value| {
                current
                    .get
                    .as_ref()
                    .is_none_or(|current| !same_value(value, current))
            }) || descriptor.set.as_ref().is_some_and(|value| {
                current
                    .set
                    .as_ref()
                    .is_none_or(|current| !same_value(value, current))
            }) {
                return false;
            }
        } else if current.writable == Some(false)
            && (descriptor.writable == Some(true)
                || descriptor.value.as_ref().is_some_and(|value| {
                    current
                        .value
                        .as_ref()
                        .is_none_or(|current| !same_value(value, current))
                }))
        {
            return false;
        }
    }
    true
}

/// Proxy [[GetOwnProperty]] completes a trap-provided descriptor before it
/// validates invariants or exposes it to reflection.  Missing data/accessor
/// fields are observable as `undefined`/`false`, never as absent own fields
/// on the descriptor object returned by Object.getOwnPropertyDescriptor.
fn complete_property_descriptor(mut descriptor: PropertyDescriptor) -> PropertyDescriptor {
    if descriptor.accessor() {
        descriptor.get.get_or_insert(Value::Undefined);
        descriptor.set.get_or_insert(Value::Undefined);
    } else {
        descriptor.value.get_or_insert(Value::Undefined);
        descriptor.writable.get_or_insert(false);
    }
    descriptor.enumerable.get_or_insert(false);
    descriptor.configurable.get_or_insert(false);
    descriptor
}

fn typed_array_kind(name: &str) -> Option<TypedArrayKind> {
    Some(match name {
        "Int8Array" => TypedArrayKind::Int8,
        "Uint8Array" => TypedArrayKind::Uint8,
        "Uint8ClampedArray" => TypedArrayKind::Uint8Clamped,
        "Int16Array" => TypedArrayKind::Int16,
        "Uint16Array" => TypedArrayKind::Uint16,
        "Int32Array" => TypedArrayKind::Int32,
        "Uint32Array" => TypedArrayKind::Uint32,
        "Float16Array" => TypedArrayKind::Float16,
        "Float32Array" => TypedArrayKind::Float32,
        "Float64Array" => TypedArrayKind::Float64,
        "BigInt64Array" => TypedArrayKind::BigInt64,
        "BigUint64Array" => TypedArrayKind::BigUint64,
        _ => return None,
    })
}

fn data_view_number(bytes: &[u8], signed: bool, floating: bool, little_endian: bool) -> f64 {
    if floating {
        return match (bytes.len(), little_endian) {
            (2, true) => f16_bits_to_f64(u16::from_le_bytes(bytes.try_into().unwrap())),
            (2, false) => f16_bits_to_f64(u16::from_be_bytes(bytes.try_into().unwrap())),
            (4, true) => f32::from_le_bytes(bytes.try_into().unwrap()) as f64,
            (4, false) => f32::from_be_bytes(bytes.try_into().unwrap()) as f64,
            (8, true) => f64::from_le_bytes(bytes.try_into().unwrap()),
            (8, false) => f64::from_be_bytes(bytes.try_into().unwrap()),
            _ => unreachable!("DataView floating-point access is 16, 32, or 64 bits"),
        };
    }
    match (bytes.len(), signed, little_endian) {
        (1, true, _) => i8::from_ne_bytes([bytes[0]]) as f64,
        (1, false, _) => bytes[0] as f64,
        (2, true, true) => i16::from_le_bytes(bytes.try_into().unwrap()) as f64,
        (2, false, true) => u16::from_le_bytes(bytes.try_into().unwrap()) as f64,
        (2, true, false) => i16::from_be_bytes(bytes.try_into().unwrap()) as f64,
        (2, false, false) => u16::from_be_bytes(bytes.try_into().unwrap()) as f64,
        (4, true, true) => i32::from_le_bytes(bytes.try_into().unwrap()) as f64,
        (4, false, true) => u32::from_le_bytes(bytes.try_into().unwrap()) as f64,
        (4, true, false) => i32::from_be_bytes(bytes.try_into().unwrap()) as f64,
        (4, false, false) => u32::from_be_bytes(bytes.try_into().unwrap()) as f64,
        _ => unreachable!("DataView only installs fixed integer widths"),
    }
}

fn data_view_value(
    bytes: &[u8],
    signed: bool,
    floating: bool,
    little_endian: bool,
    bigint: bool,
) -> Value {
    if bigint {
        let bytes: [u8; 8] = bytes.try_into().expect("BigInt DataView access is 64 bits");
        return Value::BigInt(if signed {
            if little_endian {
                i64::from_le_bytes(bytes).into()
            } else {
                i64::from_be_bytes(bytes).into()
            }
        } else if little_endian {
            u64::from_le_bytes(bytes).into()
        } else {
            u64::from_be_bytes(bytes).into()
        });
    }
    Value::Number(data_view_number(bytes, signed, floating, little_endian))
}

fn data_view_bytes(
    value: &Value,
    width: usize,
    signed: bool,
    floating: bool,
    little_endian: bool,
    bigint: bool,
) -> Vec<u8> {
    if bigint {
        let Value::BigInt(value) = value else {
            unreachable!("BigInt DataView writes receive a BigInt value");
        };
        let source = value.to_signed_bytes_le();
        let fill = if value.sign() == num_bigint::Sign::Minus {
            0xff
        } else {
            0
        };
        let mut bytes = [fill; 8];
        let copied = source.len().min(bytes.len());
        bytes[..copied].copy_from_slice(&source[..copied]);
        return if little_endian {
            bytes.to_vec()
        } else {
            bytes.into_iter().rev().collect()
        };
    }
    let Value::Number(value) = value else {
        unreachable!("numeric DataView writes receive a Number value");
    };
    let value = *value;
    if floating {
        return match (width, little_endian) {
            (2, true) => f64_to_f16_bits(value).to_le_bytes().to_vec(),
            (2, false) => f64_to_f16_bits(value).to_be_bytes().to_vec(),
            (4, true) => (value as f32).to_le_bytes().to_vec(),
            (4, false) => (value as f32).to_be_bytes().to_vec(),
            (8, true) => value.to_le_bytes().to_vec(),
            (8, false) => value.to_be_bytes().to_vec(),
            _ => unreachable!("DataView floating-point access is 16, 32, or 64 bits"),
        };
    }
    let integer = (if value.is_finite() {
        value.trunc()
    } else {
        0.0
    }) as i64;
    match (width, signed, little_endian) {
        (1, true, _) => (integer as i8).to_ne_bytes().to_vec(),
        (1, false, _) => (integer as u8).to_ne_bytes().to_vec(),
        (2, true, true) => (integer as i16).to_le_bytes().to_vec(),
        (2, false, true) => (integer as u16).to_le_bytes().to_vec(),
        (2, true, false) => (integer as i16).to_be_bytes().to_vec(),
        (2, false, false) => (integer as u16).to_be_bytes().to_vec(),
        (4, true, true) => (integer as i32).to_le_bytes().to_vec(),
        (4, false, true) => (integer as u32).to_le_bytes().to_vec(),
        (4, true, false) => (integer as i32).to_be_bytes().to_vec(),
        (4, false, false) => (integer as u32).to_be_bytes().to_vec(),
        _ => unreachable!("DataView only installs fixed integer widths"),
    }
}

/// Annex B permits HTML-style single-line comments while parsing the formal
/// parameter text supplied to the dynamic Function constructors.  The parser
/// intentionally keeps the main grammar strict, so normalize only this
/// legacy, Script-goal input before compiling the generated wrapper.  Strings,
/// templates, and ordinary comments retain their source verbatim.
fn strip_dynamic_function_html_comments(source: &str) -> String {
    #[derive(Clone, Copy)]
    enum Mode {
        Code,
        SingleQuoted,
        DoubleQuoted,
        Template,
        LineComment,
        BlockComment,
        HtmlComment,
    }

    let source = source.chars().collect::<Vec<_>>();
    let mut result = String::new();
    let mut mode = Mode::Code;
    // Parameter text follows the opening parenthesis in the generated source,
    // so it is not initially at a line start. `-->` becomes an Annex B HTML
    // close comment only after a line terminator in that text.
    let mut line_start = false;
    let mut escaped = false;
    let mut index = 0;
    while index < source.len() {
        let character = source[index];
        let next = source.get(index + 1).copied();
        let follows = |text: &[char]| source[index..].starts_with(text);
        match mode {
            Mode::Code if follows(&['<', '!', '-', '-']) => {
                mode = Mode::HtmlComment;
                index += 4;
                continue;
            }
            Mode::Code if line_start && follows(&['-', '-', '>']) => {
                mode = Mode::HtmlComment;
                index += 3;
                continue;
            }
            Mode::Code if character == '/' && next == Some('/') => {
                result.push(character);
                result.push('/');
                mode = Mode::LineComment;
                index += 2;
                continue;
            }
            Mode::Code if character == '/' && next == Some('*') => {
                result.push(character);
                result.push('*');
                mode = Mode::BlockComment;
                index += 2;
                continue;
            }
            Mode::Code if character == '\'' => mode = Mode::SingleQuoted,
            Mode::Code if character == '"' => mode = Mode::DoubleQuoted,
            Mode::Code if character == '`' => mode = Mode::Template,
            Mode::SingleQuoted | Mode::DoubleQuoted | Mode::Template if escaped => {
                escaped = false;
            }
            Mode::SingleQuoted | Mode::DoubleQuoted | Mode::Template if character == '\\' => {
                escaped = true;
            }
            Mode::SingleQuoted if character == '\'' => mode = Mode::Code,
            Mode::DoubleQuoted if character == '"' => mode = Mode::Code,
            Mode::Template if character == '`' => mode = Mode::Code,
            Mode::LineComment if matches!(character, '\n' | '\r') => mode = Mode::Code,
            Mode::BlockComment if character == '*' && next == Some('/') => {
                result.push(character);
                result.push('/');
                mode = Mode::Code;
                index += 2;
                continue;
            }
            Mode::HtmlComment if matches!(character, '\n' | '\r') => {
                mode = Mode::Code;
                line_start = true;
                result.push(character);
                index += 1;
                continue;
            }
            Mode::HtmlComment => {
                index += 1;
                continue;
            }
            _ => {}
        }
        result.push(character);
        line_start = if matches!(character, '\n' | '\r') {
            true
        } else if matches!(mode, Mode::Code) && character.is_whitespace() {
            line_start
        } else {
            false
        };
        index += 1;
    }
    result
}

#[cfg(test)]
#[path = "builtins/tests.rs"]
mod tests;
