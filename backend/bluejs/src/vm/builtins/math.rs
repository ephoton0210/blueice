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
        let root = self
            .heap
            .root(math)
            .expect("a freshly allocated object can be rooted");
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
                ("sumPrecise", 1, MathMethod::SumPrecise),
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
                self.heap
                    .unroot(root)
                    .expect("the root registered above is still registered");
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
            MathMethod::Acosh => acosh(number(first, self)?),
            MathMethod::Asin => number(first, self)?.asin(),
            MathMethod::Asinh => number(first, self)?.asinh(),
            MathMethod::Atan => number(first, self)?.atan(),
            MathMethod::Atanh => atanh(number(first, self)?),
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
            MathMethod::SumPrecise => return self.math_sum_precise(first),
        };
        Ok(Value::Number(result))
    }

    /// `Math.sumPrecise ( items )`: the exactly-rounded sum of an iterable of
    /// Numbers. The sum is a big integer scaled by 2**1074 (every finite
    /// binary64 is an integer multiple of 2**-1074), so it never overflows or
    /// loses a bit before the single final rounding.
    fn math_sum_precise(&mut self, items: &Value) -> Result<Value, RuntimeError> {
        #[derive(PartialEq)]
        enum State {
            MinusZero,
            Finite,
            PlusInfinity,
            MinusInfinity,
            NotANumber,
        }
        if matches!(items, Value::Undefined | Value::Null) {
            return Err(RuntimeError::TypeError(
                "Math.sumPrecise requires an iterable".into(),
            ));
        }
        let base = self.stack.len();
        self.stack.push(items.clone());
        let result = (|| {
            let record = self.get_iterator(items)?;
            self.stack.push(record.clone());
            let mut state = State::MinusZero;
            let mut sum = BigInt::from(0);
            let mut count: u64 = 0;
            while let Some(next) = self.iterator_step(&record, true)? {
                count += 1;
                let number = match sum_precise_item(count, &next) {
                    Ok(number) => number,
                    Err(error) => {
                        // IteratorClose with a throw completion keeps that
                        // completion, whatever `return` does.
                        let _ = self.iterator_close(&record);
                        return Err(error);
                    }
                };
                if state == State::NotANumber {
                    continue;
                }
                if number.is_nan() {
                    state = State::NotANumber;
                } else if number == f64::INFINITY {
                    state = if state == State::MinusInfinity {
                        State::NotANumber
                    } else {
                        State::PlusInfinity
                    };
                } else if number == f64::NEG_INFINITY {
                    state = if state == State::PlusInfinity {
                        State::NotANumber
                    } else {
                        State::MinusInfinity
                    };
                } else if !(number == 0.0 && number.is_sign_negative())
                    && matches!(state, State::MinusZero | State::Finite)
                {
                    state = State::Finite;
                    sum += scaled_integer(number);
                }
            }
            Ok(Value::Number(match state {
                State::NotANumber => f64::NAN,
                State::PlusInfinity => f64::INFINITY,
                State::MinusInfinity => f64::NEG_INFINITY,
                State::MinusZero => -0.0,
                State::Finite => scaled_integer_to_f64(&sum),
            }))
        })();
        self.stack.truncate(base);
        result
    }
}

/// Validates the `count`th (1-based) item of a `Math.sumPrecise` iteration:
/// it must be a Number, and the iteration may not reach 2**53 items.
fn sum_precise_item(count: u64, item: &Value) -> Result<f64, RuntimeError> {
    if count >= 1 << 53 {
        return Err(RuntimeError::RangeError(
            "Math.sumPrecise received too many values".into(),
        ));
    }
    match item {
        Value::Number(number) => Ok(*number),
        _ => Err(RuntimeError::TypeError(
            "Math.sumPrecise requires Number values".into(),
        )),
    }
}

/// A finite binary64 as the integer `value * 2**1074`.
fn scaled_integer(value: f64) -> BigInt {
    let bits = value.to_bits();
    let biased = ((bits >> 52) & 0x7ff) as i64;
    let fraction = bits & ((1_u64 << 52) - 1);
    // value == mantissa * 2**scale, with scale >= -1074.
    let (mantissa, scale) = if biased == 0 {
        (fraction, 0_i64)
    } else {
        ((1_u64 << 52) | fraction, biased - 1)
    };
    let magnitude = BigInt::from(mantissa) << (scale as usize);
    if value.is_sign_negative() {
        -magnitude
    } else {
        magnitude
    }
}

/// The exactly-rounded (ties to even) binary64 nearest `scaled * 2**-1074`,
/// or an infinity when that magnitude is not below 2**1024 - 2**970.
fn scaled_integer_to_f64(scaled: &BigInt) -> f64 {
    if scaled.sign() == Sign::NoSign {
        return 0.0;
    }
    let negative = scaled.sign() == Sign::Minus;
    let magnitude = scaled.magnitude();
    let bits = magnitude.bits() as i64;
    // 2**exponent (exponent >= -1074) as an exact binary64, or infinity above 2**1023.
    let power_of_two = |exponent: i64| -> f64 {
        if exponent > 1023 {
            f64::INFINITY
        } else if exponent >= -1022 {
            f64::from_bits(((exponent + 1023) as u64) << 52)
        } else {
            // Subnormal: `scaled` is a multiple of 2**-1074, so no exponent
            // below -1074 is ever requested.
            f64::from_bits(1_u64 << (exponent + 1074))
        }
    };
    let value = if bits <= 53 {
        let small = magnitude.iter_u64_digits().next().unwrap_or(0);
        small as f64 * power_of_two(-1074)
    } else {
        let shift = (bits - 53) as usize;
        let mut kept = magnitude >> shift;
        let remainder = magnitude - (&kept << shift);
        let half = BigUint::from(1_u8) << (shift - 1);
        let odd = kept.bit(0);
        if remainder > half || (remainder == half && odd) {
            kept += 1_u8;
        }
        let mut exponent = shift as i64 - 1074;
        let mut mantissa = kept.iter_u64_digits().next().unwrap_or(0);
        if mantissa == 1_u64 << 53 {
            mantissa >>= 1;
            exponent += 1;
        }
        mantissa as f64 * power_of_two(exponent)
    };
    if negative {
        -value
    } else {
        value
    }
}

/// `Math.acosh` after fdlibm's `e_acosh.c`. `f64::acosh` evaluates
/// `ln(x + sqrt(x*x - 1))`, which for `x` just above 1 adds a tiny square root
/// to 1 and keeps almost none of its digits; the fdlibm ranges below use
/// `ln_1p` there instead.
fn acosh(x: f64) -> f64 {
    if x.is_nan() || x < 1.0 {
        f64::NAN
    } else if x >= 268_435_456.0 {
        // 2**28: acosh(x) = ln(2x), and x + x can overflow.
        if x.is_infinite() {
            x
        } else {
            x.ln() + std::f64::consts::LN_2
        }
    } else if x == 1.0 {
        0.0
    } else if x > 2.0 {
        (2.0 * x - 1.0 / (x + (x * x - 1.0).sqrt())).ln()
    } else {
        let t = x - 1.0;
        (t + (2.0 * t + t * t).sqrt()).ln_1p()
    }
}

/// `Math.atanh` after fdlibm's `e_atanh.c`. It works on `|x|` and restores the
/// sign at the end: `ln_1p(2x / (1 - x))` for a negative `x` near -1 subtracts
/// nearly equal numbers inside `ln_1p` and loses thousands of ulps.
fn atanh(x: f64) -> f64 {
    let magnitude = x.abs();
    if x.is_nan() || magnitude > 1.0 {
        return f64::NAN;
    }
    if magnitude == 1.0 {
        return x / 0.0;
    }
    if magnitude < 3.725_290_298_461_914e-9 {
        // 2**-28: atanh(x) == x to double precision (and keeps -0).
        return x;
    }
    let half = if magnitude < 0.5 {
        let doubled = magnitude + magnitude;
        0.5 * (doubled + doubled * magnitude / (1.0 - magnitude)).ln_1p()
    } else {
        0.5 * ((magnitude + magnitude) / (1.0 - magnitude)).ln_1p()
    };
    half.copysign(x)
}

/// Fault injection for direct calls of internal operations: see
/// [`fault_injection::heap_limit_failures`].
#[cfg(test)]
pub(in crate::vm) mod fault_injection {
    use crate::{HeapConfig, RuntimeError, Vm, VmConfig};

    type WarmUp<'a> = &'a dyn Fn(&mut Vm) -> Result<(), RuntimeError>;
    type Operation<'a> = &'a mut dyn FnMut(&mut Vm) -> Result<(), RuntimeError>;

    /// A VM whose heap ceiling is `limit` bytes, after `warm_up`.
    fn build(limit: usize, nursery_capacity: usize, warm_up: WarmUp) -> Vm {
        let mut vm = Vm::new(VmConfig {
            heap: HeapConfig {
                nursery_capacity,
                major_threshold_bytes: limit,
                max_heap_bytes: limit,
            },
            ..VmConfig::default()
        })
        .expect("a heap ceiling of the live bytes builds an empty VM");
        warm_up(&mut vm).expect("a heap ceiling of the live bytes holds the warm-up");
        vm
    }

    /// Runs `operation` on freshly built VMs whose heap ceiling rises in small
    /// steps from the least a VM that has run `warm_up` needs (its live
    /// bytes, not the garbage a collection would reclaim), so that each
    /// allocation the operation makes fails in turn. The run repeats for
    /// several nursery sizes and for a few throwaway allocations made just
    /// before the operation, which shift which of its allocation calls has to
    /// make room. The headroom is raised only up to `max_extra` bytes, which
    /// keeps a sweep that only needs the operation's first allocations short.
    /// The outcome must be `Ok` or the heap-limit error, never another error;
    /// returns how many runs hit the limit.
    pub(in crate::vm) fn heap_limit_failures(
        warm_up: WarmUp,
        max_extra: usize,
        operation: Operation,
    ) -> usize {
        let mut probe = Vm::default();
        warm_up(&mut probe).unwrap();
        // The ceiling only has to hold the live objects, since a full heap is
        // collected before an allocation fails.
        probe.heap.collect_major();
        let live = probe.heap().stats().managed_bytes;
        let mut failures = 0;
        for (nursery_capacity, throwaways) in [(1, 0), (2, 1)] {
            let mut successes_in_a_row = 0;
            for limit in (live..=live.saturating_add(max_extra)).step_by(8) {
                let mut vm = build(limit, nursery_capacity, warm_up);
                for _ in 0..throwaways {
                    let _ = vm.heap.alloc_object(None);
                }
                let outcome = operation(&mut vm);
                match outcome {
                    Err(RuntimeError::Heap(_)) => {
                        failures += 1;
                        successes_in_a_row = 0;
                    }
                    other => {
                        other.expect("only the heap limit may stop the operation");
                        successes_in_a_row += 1;
                    }
                }
                if successes_in_a_row == 16 {
                    break;
                }
            }
        }
        failures
    }
}

#[cfg(test)]
mod tests {
    use super::fault_injection::heap_limit_failures;
    use super::*;

    #[test]
    fn scaled_sums_beyond_the_largest_finite_double_are_infinite() {
        // No iteration can accumulate this many items, but the conversion
        // itself is defined for any integer.
        assert_eq!(
            scaled_integer_to_f64(&(BigInt::from(1) << 2200_usize)),
            f64::INFINITY
        );
        assert_eq!(
            scaled_integer_to_f64(&-(BigInt::from(1) << 2200_usize)),
            f64::NEG_INFINITY
        );
        assert_eq!(scaled_integer_to_f64(&BigInt::from(0)), 0.0);
        assert_eq!(scaled_integer_to_f64(&BigInt::from(1)), 5e-324);
    }

    #[test]
    fn building_the_math_object_fails_cleanly_at_every_allocation() {
        // Asking twice also finds the namespace already built.
        let mut math = |vm: &mut Vm| {
            let first = vm.math_global()?;
            assert_eq!(vm.math_global().unwrap(), first);
            Ok(())
        };
        // Cold: the function prototype (and the String intrinsics under it)
        // are built lazily, and running out of heap there fails the build.
        assert!(heap_limit_failures(&|_| Ok(()), 512, &mut math) > 0);
        // Warm: the namespace itself, before and after the global object
        // exists (when the new namespace also becomes one of its properties).
        let strings = |vm: &mut Vm| vm.function_prototype().map(|_| ());
        assert!(heap_limit_failures(&strings, usize::MAX, &mut math) > 0);
        let global = |vm: &mut Vm| vm.global("globalThis").map(|_| ());
        assert!(heap_limit_failures(&global, usize::MAX, &mut math) > 0);
    }

    #[test]
    fn a_large_scaled_sum_is_a_normal_double() {
        // 2**110 units of 2**-1074 is exactly 2**-964.
        assert_eq!(
            scaled_integer_to_f64(&(BigInt::from(1) << 110_usize)),
            2_f64.powi(-964)
        );
    }

    #[test]
    fn rounding_a_scaled_sum_carries_ties_to_even() {
        // Sums just above 2**53 (in units of 2**-1074) round to the nearest
        // representable value, ties to the even mantissa, and a carry out of
        // the mantissa moves the exponent.
        for scaled in [
            (1_u64 << 53) + 1,
            (1_u64 << 53) + 3,
            (1_u64 << 54) - 1,
            (1_u64 << 54) + 3,
            (1_u64 << 54) + 2,
        ] {
            assert_eq!(
                scaled_integer_to_f64(&BigInt::from(scaled)),
                scaled as f64 * 5e-324,
                "{scaled}"
            );
        }
    }

    #[test]
    fn sum_precise_items_must_be_numbers_and_stay_below_two_to_the_53() {
        assert_eq!(sum_precise_item(1, &Value::Number(2.5)), Ok(2.5));
        assert_eq!(
            sum_precise_item(1, &Value::Bool(true)),
            Err(RuntimeError::TypeError(
                "Math.sumPrecise requires Number values".into()
            ))
        );
        assert_eq!(
            sum_precise_item((1 << 53) - 1, &Value::Number(1.0)),
            Ok(1.0)
        );
        assert_eq!(
            sum_precise_item(1 << 53, &Value::Number(1.0)),
            Err(RuntimeError::RangeError(
                "Math.sumPrecise received too many values".into()
            ))
        );
    }
}
