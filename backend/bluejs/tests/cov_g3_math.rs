// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Math edge cases: abrupt argument coercion, exact-rounding corners of
//! `Math.sumPrecise`, fdlibm ranges of `acosh`/`atanh`, and heap exhaustion
//! while the namespace is built.

mod cov_g3_support;
use cov_g3_support::{assert_true, heap_limit_sweep};

#[test]
fn every_method_propagates_an_abrupt_argument_conversion() {
    let unary = [
        "abs", "acos", "acosh", "asin", "asinh", "atan", "atanh", "ceil", "cbrt", "clz32", "cos",
        "cosh", "exp", "expm1", "f16round", "floor", "fround", "log", "log1p", "log2", "log10",
        "round", "sign", "sin", "sinh", "sqrt", "tan", "tanh", "trunc",
    ];
    for name in unary {
        assert_true(&format!(
            "try {{ Math.{name}({{valueOf() {{ throw 7 }}}}); false }} catch (e) {{ e === 7 }}"
        ));
    }
    for name in ["atan2", "pow", "imul", "hypot", "max", "min"] {
        for arguments in [
            "{valueOf() { throw 7 }}, 1",
            "1, {valueOf() { throw 7 }}",
            "{valueOf() { throw 7 }}",
        ] {
            assert_true(&format!(
                "try {{ Math.{name}({arguments}); false }} catch (e) {{ e === 7 }}"
            ));
        }
    }
}

#[test]
fn hypot_sign_and_fdlibm_ranges_agree_with_their_definitions() {
    for source in [
        "Math.hypot(0, 0) === 0 && Math.hypot() === 0 && Math.hypot(-0) === 0",
        "Math.sign(5) === 1 && Math.sign(-5) === -1 && Math.sign(Infinity) === 1",
        // acosh: exactly 1, (1, 2], (2, 2**28), and beyond.
        "Math.acosh(1) === 0 && Math.abs(Math.acosh(1.5) - 0.9624236501192069) < 1e-15",
        "Math.abs(Math.acosh(3) - 1.762747174039086) < 1e-15",
        "Math.abs(Math.acosh(1e10) - 23.718998110500402) < 1e-12 && Math.acosh(Infinity) === Infinity",
        "Math.acosh(0.5) !== Math.acosh(0.5) && Math.acosh(NaN) !== Math.acosh(NaN)",
        // atanh: |x| < 2**-28, < 0.5, and >= 0.5, with both signs, and +-1.
        "Math.atanh(1e-10) === 1e-10 && 1 / Math.atanh(-0) === -Infinity",
        "Math.abs(Math.atanh(0.25) - 0.25541281188299536) < 1e-15",
        "Math.abs(Math.atanh(-0.75) + 0.9729550745276566) < 1e-15",
        "Math.atanh(1) === Infinity && Math.atanh(-1) === -Infinity",
        "Math.atanh(2) !== Math.atanh(2) && Math.atanh(NaN) !== Math.atanh(NaN)",
    ] {
        assert_true(source);
    }
}

#[test]
fn sum_precise_rounds_exactly_once_to_nearest_even() {
    for source in [
        // No items and only negative zeros.
        "1 / Math.sumPrecise([]) === -Infinity && 1 / Math.sumPrecise([-0, -0]) === -Infinity",
        "Math.sumPrecise([1, 2, 3.5]) === 6.5",
        "Math.sumPrecise([1e308, 1e308]) === Infinity",
        "Math.sumPrecise([1.7976931348623157e308, 1.7976931348623157e308]) === Infinity",
        "Math.sumPrecise([-1.7976931348623157e308, -1.7976931348623157e308]) === -Infinity",
        // A carry out of the mantissa: 2**53 - 1 + 0.75 rounds up to 2**53.
        "Math.sumPrecise([9007199254740991, 0.75]) === 9007199254740992",
        // Ties: 2**53 + 1 stays on the even 2**53; 2**53 + 3 goes to 2**53 + 4.
        "Math.sumPrecise([9007199254740992, 1]) === 9007199254740992",
        "Math.sumPrecise([9007199254740994, 1]) === 9007199254740996",
        // Above and below the halfway point.
        "Math.sumPrecise([9007199254740992, 1, 2 ** -30]) === 9007199254740994",
        "Math.sumPrecise([9007199254740992, 0.5]) === 9007199254740992",
        // Subnormal results.
        "Math.sumPrecise([5e-324, 5e-324]) === 1e-323 && Math.sumPrecise([5e-324, -5e-324]) === 0",
        "Math.sumPrecise([-5e-324]) === -5e-324",
        // Infinities and NaN.
        "Math.sumPrecise([Infinity, 1]) === Infinity && Math.sumPrecise([-Infinity, 1]) === -Infinity",
        "Number.isNaN(Math.sumPrecise([Infinity, -Infinity])) && Number.isNaN(Math.sumPrecise([-Infinity, Infinity]))",
        "Number.isNaN(Math.sumPrecise([NaN, 1, Infinity, -Infinity]))",
    ] {
        assert_true(source);
    }
}

#[test]
fn sum_precise_rejects_bad_inputs_and_closes_its_iterator() {
    for source in [
        "try { Math.sumPrecise(); false } catch (e) { e instanceof TypeError }",
        "try { Math.sumPrecise(null); false } catch (e) { e instanceof TypeError }",
        "try { Math.sumPrecise([1, '2']); false } catch (e) { e instanceof TypeError }",
        "let closed = 0; const iterable = { [Symbol.iterator]() { return { next() { return { done: false, value: 'x' } }, return() { closed++; return {} } } } };
         try { Math.sumPrecise(iterable); false } catch (e) { e instanceof TypeError && closed === 1 }",
    ] {
        assert_true(source);
    }
}

#[test]
fn building_the_math_namespace_survives_every_allocation_failure() {
    assert!(heap_limit_sweep("0;", "Math.PI; Math.abs(-1); Math.sumPrecise([1, 2])") > 0);
}

#[test]
fn the_math_namespace_is_the_one_the_global_object_carries() {
    // The namespace built when the script first names it is the very object
    // the global object later carries.
    assert_true(
        "const before = Math; Object.getOwnPropertyDescriptor(globalThis, 'Math').value === before && Math.abs(-1) === 1",
    );
}
