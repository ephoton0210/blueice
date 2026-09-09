// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Primitive coercions used by bytecode dispatch. Object-to-primitive
//! conversion needs callable builtins, which this slice explicitly lacks.

use crate::{RuntimeError, Value};
use std::cmp::Ordering;

pub(crate) fn truthy(value: &Value) -> bool {
    match value {
        Value::Undefined | Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => *n != 0.0 && !n.is_nan(),
        Value::String(s) => !s.is_empty(),
        Value::Object(_) => true,
    }
}

pub(crate) fn type_name(value: &Value) -> &'static str {
    match value {
        Value::Undefined => "undefined",
        Value::Null | Value::Object(_) => "object",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
    }
}

pub(crate) fn number(value: &Value) -> Result<f64, RuntimeError> {
    Ok(match value {
        Value::Undefined => f64::NAN,
        Value::Null => 0.0,
        Value::Bool(b) => f64::from(u8::from(*b)),
        Value::Number(n) => *n,
        Value::String(s) => string_number(s),
        Value::Object(_) => return Err(RuntimeError::Unsupported("object-to-primitive coercion")),
    })
}

pub(crate) fn string(value: &Value) -> Result<String, RuntimeError> {
    Ok(match value {
        Value::Undefined => "undefined".into(),
        Value::Null => "null".into(),
        Value::Bool(b) => b.to_string(),
        Value::String(s) => s.clone(),
        Value::Number(n) if n.is_nan() => "NaN".into(),
        Value::Number(n) if n.is_infinite() => if n.is_sign_negative() { "-Infinity" } else { "Infinity" }.into(),
        Value::Number(n) if *n == 0.0 => "0".into(),
        Value::Number(n) => number_string(*n),
        Value::Object(_) => return Err(RuntimeError::Unsupported("object-to-primitive coercion")),
    })
}

fn number_string(n: f64) -> String {
    let shortest = format!("{:e}", n.abs());
    let (mantissa, exponent) = shortest.split_once('e').expect("scientific format includes exponent");
    let exponent: i32 = exponent.parse().expect("formatted exponent is an integer");
    let mut digits = mantissa.replace('.', "");
    let significand: u64 = digits.parse().expect("shortest f64 has at most 17 decimal digits");
    if lower_even_tie(n.abs(), significand, exponent + 1 - digits.len() as i32) {
        digits = (significand - 1).to_string();
    }
    let sign = if n.is_sign_negative() { "-" } else { "" };
    if !(-6..21).contains(&exponent) {
        let fraction = if digits.len() == 1 { String::new() } else { format!(".{}", &digits[1..]) };
        format!("{sign}{}{fraction}e{}{exponent}", &digits[..1], if exponent < 0 { "" } else { "+" })
    } else {
        let point = exponent + 1;
        if point <= 0 {
            format!("{sign}0.{}{digits}", "0".repeat(-point as usize))
        } else if point as usize >= digits.len() {
            format!("{sign}{digits}{}", "0".repeat(point as usize - digits.len()))
        } else {
            format!("{sign}{}.{}", &digits[..point as usize], &digits[point as usize..])
        }
    }
}

fn lower_even_tie(n: f64, significand: u64, decimal_exponent: i32) -> bool {
    // Rust's shortest formatter resolves decimal midpoints upward;
    // Number::toString recommends the even significand in ES2026 Note 2,
    // and requires it in the current ES2027 draft step 5:
    // https://tc39.es/ecma262/multipage/ecmascript-data-types-and-values.html#sec-numeric-types-number-tostring
    // Never compare re-parsed floats: both adjacent decimals round to n.
    // Compare exact rationals n = m*2^e and (2*s-1)*10^k/2 instead.
    // An integral decimal midpoint cannot round-trip from either neighbor;
    // for k <= -25, 5^(-k) exceeds (2*s-1) < 2*10^17.
    // Proof and versioned references: research/js-conformance-baseline.md.
    if significand & 1 == 0 || !(-24..0).contains(&decimal_exponent) {
        return false;
    }
    let bits = n.to_bits();
    let exponent_bits = ((bits >> 52) & 0x7ff) as i32;
    // The decimal-exponent bound above also rules out subnormals.
    let mut binary_significand = (bits & ((1u64 << 52) - 1)) | (1u64 << 52);
    let zeros = binary_significand.trailing_zeros();
    binary_significand >>= zeros;
    let binary_exponent = exponent_bits - 1075 + zeros as i32;
    if binary_exponent != decimal_exponent - 1 {
        return false;
    }
    let midpoint = 2 * significand - 1;
    let factor = 5u64.pow((-decimal_exponent) as u32); // At most 5^24; fits u64.
    midpoint.is_multiple_of(factor) && midpoint / factor == binary_significand
}

pub(crate) fn compare(left: &Value, right: &Value) -> Result<Option<Ordering>, RuntimeError> {
    if let (Value::String(a), Value::String(b)) = (left, right) {
        // ECMAScript ordering is by UTF-16 code units, not UTF-8 bytes
        // or Unicode scalar values (notably astral vs. BMP characters).
        Ok(Some(a.encode_utf16().cmp(b.encode_utf16())))
    } else {
        Ok(number(left)?.partial_cmp(&number(right)?))
    }
}

pub(crate) fn whitespace(c: char) -> bool {
    matches!(c, '\u{0009}'..='\u{000d}' | ' ' | '\u{00a0}' | '\u{1680}' | '\u{2000}'..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{3000}' | '\u{feff}')
}

fn string_number(s: &str) -> f64 {
    let s = s.trim_matches(whitespace);
    if s.is_empty() {
        return 0.0;
    }
    match s {
        "Infinity" | "+Infinity" => return f64::INFINITY,
        "-Infinity" => return f64::NEG_INFINITY,
        _ => {}
    }
    for (prefixes, bits) in [(["0x", "0X"], 4), (["0o", "0O"], 3), (["0b", "0B"], 1)] {
        if prefixes.iter().any(|prefix| s.starts_with(prefix)) {
            return radix_number(&s[2..], bits);
        }
    }
    // Validate StringNumericLiteral before Rust's parser: Rust also
    // accepts forms like "inf" that JavaScript must turn into NaN.
    let bytes = s.as_bytes();
    let mut i = usize::from(matches!(bytes[0], b'+' | b'-'));
    let mut digits = 0;
    while bytes.get(i).is_some_and(u8::is_ascii_digit) {
        i += 1;
        digits += 1;
    }
    if bytes.get(i) == Some(&b'.') {
        i += 1;
        while bytes.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
            digits += 1;
        }
    }
    if digits == 0 {
        return f64::NAN;
    }
    if matches!(bytes.get(i), Some(b'e' | b'E')) {
        i += 1;
        if matches!(bytes.get(i), Some(b'+' | b'-')) {
            i += 1;
        }
        let start = i;
        while bytes.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
        if i == start {
            return f64::NAN;
        }
    }
    if i != bytes.len() {
        return f64::NAN;
    }
    s.parse().unwrap_or(f64::NAN)
}

pub(crate) fn radix_number(s: &str, digit_bits: u32) -> f64 {
    if s.is_empty() {
        return f64::NAN;
    }
    // Accumulate only the leading 53 bits, plus guard/sticky bits for a
    // single round-to-nearest-even. Repeated float multiply/add would
    // round intermediate values and misparse long binary/octal/hex strings.
    let mut count = 0usize;
    let mut significand = 0u64;
    let mut guard = false;
    let mut sticky = false;
    for c in s.chars() {
        let Some(digit) = c.to_digit(1 << digit_bits) else { return f64::NAN };
        for shift in (0..digit_bits).rev() {
            let bit = (digit >> shift) & 1;
            if count == 0 && bit == 0 {
                continue;
            }
            count += 1;
            if count <= 53 {
                significand = (significand << 1) | u64::from(bit);
            } else if count == 54 {
                guard = bit != 0;
            } else {
                sticky |= bit != 0;
            }
        }
    }
    if count <= 53 {
        return significand as f64;
    }
    if count > 1024 {
        return f64::INFINITY;
    }
    if guard && (sticky || significand & 1 != 0) {
        significand += 1;
    }
    significand as f64 * 2f64.powi((count - 53) as i32)
}
