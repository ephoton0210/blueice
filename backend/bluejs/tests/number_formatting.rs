// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `Number.prototype.toFixed`, `toExponential` and `toPrecision`: exact
//! round-half-up digit generation and the order of argument coercion, range
//! check and non-finite receiver handling.

use blueice_bluejs::{compile, parse, Value, Vm};

fn evaluate(source: &str) -> Value {
    Vm::default()
        .execute(&compile(&parse(source).unwrap()).unwrap())
        .unwrap_or_else(|error| panic!("{source}\n  -> {error:?}"))
}

/// Expects `true`; a script may return a string describing the first failure.
fn assert_true(source: &str) {
    match evaluate(source) {
        Value::Bool(true) => {}
        Value::String(reason) => panic!("{}", reason.to_utf8().unwrap()),
        other => panic!("{source}\n  -> {other:?}"),
    }
}

#[test]
fn to_fixed_rounds_a_tie_up_in_magnitude() {
    assert_true(
        r#"(function() {
          const cases = [
            [2.5, 0, "3"], [-2.5, 0, "-3"], [0.5, 0, "1"], [-0.5, 0, "-1"], [1.5, 0, "2"], [0.125, 2, "0.13"],
            [1.005, 2, "1.00"], [10.235, 2, "10.23"], [123.456, 2, "123.46"], [0, 2, "0.00"], [-0, 2, "0.00"],
            [0.000001, 7, "0.0000010"], [1e21, 2, "1e+21"], [999.995, 2, "1000.00"], [0.4, 0, "0"], [0.6, 0, "1"],
            [1234.5678, 0, "1235"], [5e-324, 3, "0.000"], [-1.5e-10, 2, "-0.00"],
          ];
          for (const [x, f, expected] of cases) {
            const actual = x.toFixed(f);
            if (actual !== expected) return x + ".toFixed(" + f + ") = " + actual + ", expected " + expected;
          }
          try { (1).toFixed(101); return "101"; } catch (e) { if (!(e instanceof RangeError)) return "wrong error"; }
          try { (1).toFixed(-1); return "-1"; } catch (e) { if (!(e instanceof RangeError)) return "wrong error"; }
          // toFixed validates the digits before it looks at the receiver, unlike its two siblings.
          try { NaN.toFixed(200); return "NaN.toFixed(200)"; } catch (e) { if (!(e instanceof RangeError)) return "wrong error"; }
          if (NaN.toFixed(2) !== "NaN") return "NaN.toFixed(2)";
          return true;
        })()"#,
    );
}

#[test]
fn to_exponential_rounds_a_tie_up_and_treats_non_finite_receivers_before_the_range() {
    assert_true(
        r#"(function() {
          const cases = [
            [123.456, 0, "1e+2"], [123.456, 3, "1.235e+2"], [123.456, 20, "1.23456000000000003070e+2"], [-123.456, 4, "-1.2346e+2"],
            [0.0001, 1, "1.0e-4"], [0.0001, 20, "1.00000000000000004792e-4"], [0.9999, 0, "1e+0"], [0.9999, 3, "9.999e-1"],
            [25, 0, "3e+1"], [12345, 3, "1.235e+4"], [-25, 0, "-3e+1"], [0, 0, "0e+0"], [-0, 2, "0.00e+0"], [1, 100, "1." + "0".repeat(100) + "e+0"],
            [5e-324, 3, "4.941e-324"], [1.7976931348623157e308, 5, "1.79769e+308"], [99.99, 1, "1.0e+2"],
          ];
          for (const [x, f, expected] of cases) {
            const actual = x.toExponential(f);
            if (actual !== expected) return x + ".toExponential(" + f + ") = " + actual + ", expected " + expected;
          }
          if ((123456).toExponential() !== "1.23456e+5" || (0).toExponential() !== "0e+0") return "no argument";
          // The receiver is tested for finiteness before the range check, after the argument is coerced.
          let coerced = 0;
          const digits = { valueOf() { coerced++; return 1000; } };
          for (const x of [NaN, Infinity, -Infinity]) {
            if (x.toExponential(1000) !== String(x)) return "non-finite receiver with a huge argument";
            if (x.toExponential(digits) !== String(x)) return "non-finite receiver, coerced argument";
          }
          if (coerced !== 3) return "argument coercion count " + coerced;
          try { (1).toExponential(101); return "101"; } catch (e) { if (!(e instanceof RangeError)) return "wrong error"; }
          try { (1).toExponential(-1); return "-1"; } catch (e) { if (!(e instanceof RangeError)) return "wrong error"; }
          return true;
        })()"#,
    );
}

#[test]
fn to_precision_rounds_a_tie_up_and_validates_the_precision() {
    assert_true(
        r#"(function() {
          const cases = [
            [25, 1, "3e+1"], [35, 1, "4e+1"], [0.125, 2, "0.13"], [123456, 2, "1.2e+5"], [99.99, 2, "1.0e+2"], [9.99, 2, "10"],
            [0, 1, "0"], [0, 3, "0.00"], [-0, 3, "0.00"], [1e21, 3, "1.00e+21"], [0.000001, 2, "0.0000010"], [0.0000001, 2, "1.0e-7"],
            [123.456, 4, "123.5"], [123.456, 6, "123.456"], [123.456, 7, "123.4560"], [1000, 4, "1000"], [1000, 3, "1.00e+3"],
            [-123.456, 2, "-1.2e+2"], [0.5, 1, "0.5"], [1.25, 2, "1.3"], [1e100, 100, "1.000000000000000015902891109759918046836080856394528138978132755774783877217038106081346998585681510e+100"],
          ];
          for (const [x, p, expected] of cases) {
            const actual = x.toPrecision(p);
            if (actual !== expected) return x + ".toPrecision(" + p + ") = " + actual + ", expected " + expected;
          }
          if ((123.456).toPrecision() !== "123.456" || (123.456).toPrecision(undefined) !== "123.456") return "undefined precision";
          for (const bad of [0, 101, -1, 1e30, -Infinity]) {
            try { (1).toPrecision(bad); return "precision " + bad + " accepted"; } catch (e) { if (!(e instanceof RangeError)) return "wrong error"; }
          }
          // Non-finite receivers are returned as strings before the range check.
          for (const x of [NaN, Infinity, -Infinity]) if (x.toPrecision(0) !== String(x) || x.toPrecision(1000) !== String(x)) return "non-finite receiver";
          // The precision argument is coerced (once), even for a non-finite receiver.
          let coerced = 0;
          NaN.toPrecision({ valueOf() { coerced++; return 5; } });
          if (coerced !== 1) return "coercion";
          return true;
        })()"#,
    );
}
