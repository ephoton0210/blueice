// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

fn normalized_exponential(number: f64, fraction_digits: Option<usize>) -> String {
    let rendered = if let Some(fraction_digits) = fraction_digits {
        format!("{number:.fraction_digits$e}")
    } else {
        format!("{number:e}")
    };
    let (mantissa, exponent) = rendered
        .split_once('e')
        .expect("Rust lower-exponential formatting includes an exponent");
    let exponent = exponent
        .parse::<i32>()
        .expect("Rust lower-exponential formatting has an integer exponent");
    format!(
        "{mantissa}e{}{exponent}",
        if exponent >= 0 { "+" } else { "" }
    )
}

/// The exact decimal expansion of a finite, positive `value`: its digits (no
/// leading zeros; trailing zeros are possible) and the decimal exponent of the
/// first digit, i.e. `value == d0.d1d2... x 10^exponent`. A binary64 has a
/// finite decimal expansion, so the number-formatting methods can round it
/// exactly, with the tie rule ECMA-262 states ("pick the larger n"), instead
/// of inheriting Rust's round-half-to-even formatting.
fn exact_decimal(value: f64) -> (Vec<u8>, i32) {
    let bits = value.to_bits();
    let biased = ((bits >> 52) & 0x7ff) as i32;
    let fraction = bits & ((1_u64 << 52) - 1);
    // value == mantissa * 2^scale
    let (mantissa, scale) = if biased == 0 {
        (fraction, -1074)
    } else {
        ((1_u64 << 52) | fraction, biased - 1075)
    };
    let (digits, exponent) = if scale >= 0 {
        let text = (BigUint::from(mantissa) << scale as usize).to_string();
        let exponent = text.len() as i32 - 1;
        (text, exponent)
    } else {
        // mantissa / 2^k == mantissa * 5^k / 10^k
        let text =
            (BigUint::from(mantissa) * BigUint::from(5_u32).pow((-scale) as u32)).to_string();
        let exponent = text.len() as i32 - 1 + scale;
        (text, exponent)
    };
    (digits.into_bytes(), exponent)
}

/// `value` (finite, positive) rounded half up to `count` significant digits:
/// exactly `count` ASCII digits and the decimal exponent of the first one
/// after any carry (`9.99` at two digits is `10` with exponent 1).
fn significant_digits(value: f64, count: usize) -> (Vec<u8>, i32) {
    let (mut digits, mut exponent) = exact_decimal(value);
    if digits.len() > count {
        let round_up = digits[count] >= b'5';
        digits.truncate(count);
        if round_up && !increment_digits(&mut digits) {
            digits.insert(0, b'1');
            digits.truncate(count);
            exponent += 1;
        }
    } else {
        digits.resize(count, b'0');
    }
    (digits, exponent)
}

/// Adds one to a string of ASCII decimal digits; false when the carry
/// overflowed (the digits are then all zeros).
fn increment_digits(digits: &mut [u8]) -> bool {
    for digit in digits.iter_mut().rev() {
        if *digit == b'9' {
            *digit = b'0';
        } else {
            *digit += 1;
            return true;
        }
    }
    false
}

/// `Number.prototype.toFixed`'s digit string for a finite `value` with
/// `|value| < 10^21`: the sign, integer part and exactly `fraction_digits`
/// fraction digits, rounding half up in magnitude.
fn fixed_string(value: f64, fraction_digits: usize) -> String {
    let magnitude = value.abs();
    let mut scaled: Vec<u8> = if magnitude == 0.0 {
        Vec::new()
    } else {
        let (digits, exponent) = exact_decimal(magnitude);
        // Digits to keep so the last kept one is the fraction_digits-th
        // after the decimal point.
        let keep = exponent as i64 + 1 + fraction_digits as i64;
        if keep < 0 {
            Vec::new()
        } else {
            let keep = keep as usize;
            let mut kept: Vec<u8> = digits.iter().copied().take(keep).collect();
            kept.resize(keep, b'0');
            if digits.get(keep).is_some_and(|&next| next >= b'5') {
                if kept.is_empty() {
                    kept.push(b'1');
                } else if !increment_digits(&mut kept) {
                    kept.insert(0, b'1');
                }
            }
            kept
        }
    };
    // Pad so there is at least one integer digit, then place the point.
    while scaled.len() < fraction_digits + 1 {
        scaled.insert(0, b'0');
    }
    let point = scaled.len() - fraction_digits;
    let mut text = String::new();
    if value < 0.0 {
        text.push('-');
    }
    text.push_str(std::str::from_utf8(&scaled[..point]).expect("ASCII digits"));
    if fraction_digits > 0 {
        text.push('.');
        text.push_str(std::str::from_utf8(&scaled[point..]).expect("ASCII digits"));
    }
    text
}

/// `Number.prototype.toExponential ( f )` for a finite value.
fn exponential_string(value: f64, fraction_digits: usize) -> String {
    let magnitude = value.abs();
    let (digits, exponent) = if magnitude == 0.0 {
        (vec![b'0'; fraction_digits + 1], 0)
    } else {
        significant_digits(magnitude, fraction_digits + 1)
    };
    let mut text = String::new();
    if value < 0.0 {
        text.push('-');
    }
    text.push(digits[0] as char);
    if fraction_digits > 0 {
        text.push('.');
        text.push_str(std::str::from_utf8(&digits[1..]).expect("ASCII digits"));
    }
    text.push_str(&format!(
        "e{}{}",
        if exponent >= 0 { "+" } else { "-" },
        exponent.abs()
    ));
    text
}

/// `Number.prototype.toPrecision ( p )` for a finite value and `1 <= p <= 100`.
fn precision_string(value: f64, precision: usize) -> String {
    let magnitude = value.abs();
    let (digits, exponent) = if magnitude == 0.0 {
        (vec![b'0'; precision], 0)
    } else {
        significant_digits(magnitude, precision)
    };
    let digits = std::str::from_utf8(&digits).expect("ASCII digits");
    let mut text = String::new();
    if value < 0.0 {
        text.push('-');
    }
    if exponent < -6 || exponent >= precision as i32 {
        text.push_str(&digits[..1]);
        if precision > 1 {
            text.push('.');
            text.push_str(&digits[1..]);
        }
        text.push_str(&format!(
            "e{}{}",
            if exponent >= 0 { "+" } else { "-" },
            exponent.abs()
        ));
    } else if exponent == precision as i32 - 1 {
        text.push_str(digits);
    } else if exponent >= 0 {
        let split = exponent as usize + 1;
        text.push_str(&digits[..split]);
        text.push('.');
        text.push_str(&digits[split..]);
    } else {
        text.push_str("0.");
        text.push_str(&"0".repeat((-(exponent + 1)) as usize));
        text.push_str(digits);
    }
    text
}

impl Vm {
    pub(in super::super) fn number_receiver(
        &mut self,
        receiver: &Value,
    ) -> Result<f64, RuntimeError> {
        let value = if let Value::Object(object) = receiver {
            self.heap
                .boxed_primitive(*object)?
                .unwrap_or(Value::Undefined)
        } else {
            receiver.clone()
        };
        let Value::Number(number) = value else {
            return Err(RuntimeError::TypeError(
                "Number method requires a Number receiver".into(),
            ));
        };
        Ok(number)
    }

    /// ToIntegerOrInfinity of a digits/precision argument (the observable
    /// coercion); the callers apply their own range checks afterwards.
    pub(in super::super) fn number_digits_argument(
        &mut self,
        value: &Value,
    ) -> Result<f64, RuntimeError> {
        let value = self.coerce_number(value)?;
        Ok(if value.is_nan() { 0.0 } else { value.trunc() })
    }

    /// `low..=100` range check shared by the three formatting methods.
    fn number_digits_in_range(digits: f64, low: f64, method: &str) -> Result<usize, RuntimeError> {
        if !(low..=100.0).contains(&digits) {
            return Err(RuntimeError::RangeError(format!(
                "{method} argument must be between {low} and 100"
            )));
        }
        Ok(digits as usize)
    }

    pub(in super::super) fn number_method(
        &mut self,
        receiver: &Value,
        args: &[Value],
        method: NumberMethod,
    ) -> Result<Value, RuntimeError> {
        let number = self.number_receiver(receiver)?;
        if method == NumberMethod::LocaleString {
            let formatter =
                self.resolve_number_format(native::argument(args, 0), native::argument(args, 1))?;
            return formatter
                .format_f64(number)
                .map(|formatted| Value::String(formatted.into()))
                .map_err(|error| RuntimeError::RangeError(error.to_string()));
        }
        if method == NumberMethod::ToString {
            let radix = native::argument(args, 0);
            if radix == &Value::Undefined {
                return primitive::string(&Value::Number(number)).map(Value::String);
            }
            let radix = self.coerce_number(radix)?;
            let radix = if radix.is_nan() { 0.0 } else { radix.trunc() };
            if !radix.is_finite() || !(2.0..=36.0).contains(&radix) {
                return Err(RuntimeError::RangeError(
                    "Number.prototype.toString radix must be between 2 and 36".into(),
                ));
            }
            if !number.is_finite() || number == 0.0 {
                return primitive::string(&Value::Number(number)).map(Value::String);
            }
            return Ok(Value::String(
                number_radix_string(number, radix as u32).into(),
            ));
        }
        // Number formatting canonicalizes -0 before producing a string.
        let mut number = number;
        if number == 0.0 {
            number = 0.0;
        }
        let source_string = || primitive::string(&Value::Number(number));
        match method {
            NumberMethod::ToString => unreachable!("handled before numeric string methods"),
            NumberMethod::LocaleString => unreachable!("handled before numeric string methods"),
            NumberMethod::Fixed => {
                // toFixed range-checks the digits before it looks at the
                // receiver (unlike toExponential and toPrecision).
                let digits = self.number_digits_argument(native::argument(args, 0))?;
                let digits = Self::number_digits_in_range(digits, 0.0, "toFixed")?;
                if !number.is_finite() || number.abs() >= 1e21 {
                    return Ok(Value::String(source_string()?));
                }
                Ok(Value::String(fixed_string(number, digits).into()))
            }
            NumberMethod::Exponential => {
                let requested = native::argument(args, 0);
                let digits = if requested == &Value::Undefined {
                    None
                } else {
                    Some(self.number_digits_argument(requested)?)
                };
                if !number.is_finite() {
                    return Ok(Value::String(source_string()?));
                }
                match digits {
                    None => Ok(Value::String(normalized_exponential(number, None).into())),
                    Some(digits) => {
                        let digits = Self::number_digits_in_range(digits, 0.0, "toExponential")?;
                        Ok(Value::String(exponential_string(number, digits).into()))
                    }
                }
            }
            NumberMethod::Precision => {
                if native::argument(args, 0) == &Value::Undefined {
                    return Ok(Value::String(source_string()?));
                }
                let precision = self.number_digits_argument(native::argument(args, 0))?;
                if !number.is_finite() {
                    return Ok(Value::String(source_string()?));
                }
                let precision = Self::number_digits_in_range(precision, 1.0, "toPrecision")?;
                Ok(Value::String(precision_string(number, precision).into()))
            }
        }
    }
}
