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

    pub(in super::super) fn number_precision_argument(
        &mut self,
        value: &Value,
        method: &str,
    ) -> Result<usize, RuntimeError> {
        let value = self.coerce_number(value)?;
        let value = if value.is_nan() { 0.0 } else { value.trunc() };
        if !value.is_finite() || !(0.0..=100.0).contains(&value) {
            return Err(RuntimeError::RangeError(format!(
                "{method} precision must be between 0 and 100"
            )));
        }
        Ok(value as usize)
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
                let digits =
                    self.number_precision_argument(native::argument(args, 0), "toFixed")?;
                if !number.is_finite() || number.abs() >= 1e21 {
                    return Ok(Value::String(source_string()?));
                }
                Ok(Value::String(format!("{number:.digits$}").into()))
            }
            NumberMethod::Exponential => {
                if native::argument(args, 0) == &Value::Undefined {
                    if !number.is_finite() {
                        return Ok(Value::String(source_string()?));
                    }
                    return Ok(Value::String(normalized_exponential(number, None).into()));
                }
                let digits =
                    self.number_precision_argument(native::argument(args, 0), "toExponential")?;
                if !number.is_finite() {
                    return Ok(Value::String(source_string()?));
                }
                Ok(Value::String(
                    normalized_exponential(number, Some(digits)).into(),
                ))
            }
            NumberMethod::Precision => {
                if native::argument(args, 0) == &Value::Undefined {
                    return Ok(Value::String(source_string()?));
                }
                let precision =
                    self.number_precision_argument(native::argument(args, 0), "toPrecision")?;
                if !number.is_finite() {
                    return Ok(Value::String(source_string()?));
                }
                let exponent = if number == 0.0 {
                    0
                } else {
                    number.abs().log10().floor() as i32
                };
                if exponent >= precision as i32 || exponent < -6 {
                    Ok(Value::String(
                        normalized_exponential(number, Some(precision - 1)).into(),
                    ))
                } else {
                    let fraction_digits = (precision as i32 - exponent - 1).max(0) as usize;
                    Ok(Value::String(format!("{number:.fraction_digits$}").into()))
                }
            }
        }
    }
}
