// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

impl Vm {
    pub(in super::super) fn math_global(&mut self) -> Result<Value, RuntimeError> {
        if let Some(&id) = self.globals.get("Math") {
            return Ok(Value::Object(id));
        }
        let function_prototype = self.function_prototype()?;
        let object_prototype = self.object_prototype;
        let math = self.with_roots(|heap| heap.alloc_object(Some(object_prototype)))?;
        let root = self.heap.root(math)?;
        let result = (|| {
            for (name, value) in [
                ("E", std::f64::consts::E),
                ("LN10", std::f64::consts::LN_10),
                ("LN2", std::f64::consts::LN_2),
                ("LOG10E", std::f64::consts::LOG10_E),
                ("LOG2E", std::f64::consts::LOG2_E),
                ("PI", std::f64::consts::PI),
                ("SQRT1_2", std::f64::consts::FRAC_1_SQRT_2),
                ("SQRT2", std::f64::consts::SQRT_2),
            ] {
                self.define_data(math, name, Value::Number(value), false, false, false)?;
            }
            for (name, length, method) in [
                ("abs", 1, MathMethod::Abs),
                ("acos", 1, MathMethod::Acos),
                ("acosh", 1, MathMethod::Acosh),
                ("asin", 1, MathMethod::Asin),
                ("asinh", 1, MathMethod::Asinh),
                ("atan", 1, MathMethod::Atan),
                ("atanh", 1, MathMethod::Atanh),
                ("atan2", 2, MathMethod::Atan2),
                ("ceil", 1, MathMethod::Ceil),
                ("cbrt", 1, MathMethod::Cbrt),
                ("clz32", 1, MathMethod::Clz32),
                ("cos", 1, MathMethod::Cos),
                ("cosh", 1, MathMethod::Cosh),
                ("exp", 1, MathMethod::Exp),
                ("expm1", 1, MathMethod::Expm1),
                ("f16round", 1, MathMethod::F16round),
                ("floor", 1, MathMethod::Floor),
                ("fround", 1, MathMethod::Fround),
                ("hypot", 2, MathMethod::Hypot),
                ("imul", 2, MathMethod::Imul),
                ("log", 1, MathMethod::Log),
                ("log1p", 1, MathMethod::Log1p),
                ("log2", 1, MathMethod::Log2),
                ("log10", 1, MathMethod::Log10),
                ("max", 2, MathMethod::Max),
                ("min", 2, MathMethod::Min),
                ("pow", 2, MathMethod::Pow),
                ("random", 0, MathMethod::Random),
                ("round", 1, MathMethod::Round),
                ("sign", 1, MathMethod::Sign),
                ("sin", 1, MathMethod::Sin),
                ("sinh", 1, MathMethod::Sinh),
                ("sqrt", 1, MathMethod::Sqrt),
                ("tan", 1, MathMethod::Tan),
                ("tanh", 1, MathMethod::Tanh),
                ("trunc", 1, MathMethod::Trunc),
            ] {
                self.install_native(
                    math,
                    function_prototype,
                    name,
                    length,
                    NativeFunction::Math(method),
                )?;
            }
            self.define_data(
                math,
                JsSymbol::well_known("toStringTag"),
                Value::String("Math".into()),
                false,
                false,
                true,
            )?;
            Ok(Value::Object(math))
        })();
        match result {
            Ok(value) => {
                self.globals.insert("Math".into(), math);
                if let Some(&global) = self.globals.get("globalThis") {
                    self.define_data(global, "Math", Value::Object(math), true, false, true)?;
                }
                Ok(value)
            }
            Err(error) => {
                self.heap.unroot(root)?;
                Err(error)
            }
        }
    }

    pub(in super::super) fn math_method(
        &mut self,
        method: MathMethod,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        let first = native::argument(args, 0);
        let second = native::argument(args, 1);
        let number = |value: &Value, vm: &mut Self| vm.coerce_number(value);
        let result = match method {
            MathMethod::Max | MathMethod::Min => {
                // Every argument is coerced before any of them is compared,
                // so a NaN does not hide a later argument's valueOf.
                let mut values = Vec::with_capacity(args.len());
                for value in args {
                    values.push(number(value, self)?);
                }
                let mut result = if method == MathMethod::Max {
                    f64::NEG_INFINITY
                } else {
                    f64::INFINITY
                };
                for value in values {
                    if value.is_nan() {
                        return Ok(Value::Number(f64::NAN));
                    }
                    if value == 0.0 && result == 0.0 {
                        if (method == MathMethod::Max && value.is_sign_positive())
                            || (method == MathMethod::Min && value.is_sign_negative())
                        {
                            result = value;
                        }
                    } else if (method == MathMethod::Max && value > result)
                        || (method == MathMethod::Min && value < result)
                    {
                        result = value;
                    }
                }
                result
            }
            MathMethod::Hypot => {
                // Coerce every argument first: an infinity does not excuse
                // a later argument's abrupt conversion.
                let mut values = Vec::with_capacity(args.len());
                for value in args {
                    values.push(number(value, self)?.abs());
                }
                if values.iter().any(|value| value.is_infinite()) {
                    f64::INFINITY
                } else if values.iter().any(|value| value.is_nan()) {
                    f64::NAN
                } else {
                    let scale = values.iter().copied().fold(0.0_f64, f64::max);
                    if scale == 0.0 {
                        0.0
                    } else {
                        scale
                            * values
                                .iter()
                                .map(|value| (value / scale).powi(2))
                                .sum::<f64>()
                                .sqrt()
                    }
                }
            }
            MathMethod::Imul => {
                let left = primitive::to_uint32(number(first, self)?);
                let right = primitive::to_uint32(number(second, self)?);
                (left as i32).wrapping_mul(right as i32) as f64
            }
            MathMethod::Clz32 => primitive::to_uint32(number(first, self)?).leading_zeros() as f64,
            MathMethod::Atan2 => number(first, self)?.atan2(number(second, self)?),
            MathMethod::Pow => {
                let base = number(first, self)?;
                primitive::number_exponentiate(base, number(second, self)?)
            }
            MathMethod::Random => {
                let elapsed = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default();
                (elapsed.as_nanos() % 1_000_000_000) as f64 / 1_000_000_000.0
            }
            MathMethod::Round => {
                let value = number(first, self)?;
                if !value.is_finite() || value == 0.0 || value.abs() >= 4_503_599_627_370_496.0 {
                    // Zeroes, non-finite values and every |x| >= 2**52 (already
                    // an integer) are returned unchanged.
                    value
                } else {
                    // Round half toward +Infinity, computed without the
                    // `floor(x + 0.5)` overshoot near 0.5 and near 2**52.
                    let floor = value.floor();
                    let rounded = if value - floor >= 0.5 {
                        floor + 1.0
                    } else {
                        floor
                    };
                    if rounded == 0.0 && value < 0.0 {
                        -0.0
                    } else {
                        rounded
                    }
                }
            }
            MathMethod::Sign => {
                let value = number(first, self)?;
                if value.is_nan() || value == 0.0 {
                    value
                } else {
                    value.signum()
                }
            }
            MathMethod::Abs => {
                let value = number(first, self)?;
                value.abs()
            }
            MathMethod::Acos => number(first, self)?.acos(),
            MathMethod::Acosh => number(first, self)?.acosh(),
            MathMethod::Asin => number(first, self)?.asin(),
            MathMethod::Asinh => number(first, self)?.asinh(),
            MathMethod::Atan => number(first, self)?.atan(),
            MathMethod::Atanh => number(first, self)?.atanh(),
            MathMethod::Ceil => number(first, self)?.ceil(),
            MathMethod::Cbrt => number(first, self)?.cbrt(),
            MathMethod::Cos => number(first, self)?.cos(),
            MathMethod::Cosh => number(first, self)?.cosh(),
            MathMethod::Exp => number(first, self)?.exp(),
            MathMethod::Expm1 => number(first, self)?.exp_m1(),
            MathMethod::F16round => f16_bits_to_f64(f64_to_f16_bits(number(first, self)?)),
            MathMethod::Floor => number(first, self)?.floor(),
            MathMethod::Fround => (number(first, self)? as f32) as f64,
            MathMethod::Log => number(first, self)?.ln(),
            MathMethod::Log1p => number(first, self)?.ln_1p(),
            MathMethod::Log2 => number(first, self)?.log2(),
            MathMethod::Log10 => number(first, self)?.log10(),
            MathMethod::Sin => number(first, self)?.sin(),
            MathMethod::Sinh => number(first, self)?.sinh(),
            MathMethod::Sqrt => number(first, self)?.sqrt(),
            MathMethod::Tan => number(first, self)?.tan(),
            MathMethod::Tanh => number(first, self)?.tanh(),
            MathMethod::Trunc => number(first, self)?.trunc(),
        };
        Ok(Value::Number(result))
    }
}
