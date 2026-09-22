// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `Math.max`/`min`/`hypot` argument coercion, `Math.pow`'s and `**`'s NaN
//! rules, and the exactness of `Math.round`.

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
fn max_min_and_hypot_coerce_every_argument_before_deciding() {
    assert_true(
        r#"(function() {
          let calls = 0;
          const counted = { valueOf() { calls++; return 1; } };
          Math.max(NaN, counted);
          if (calls !== 1) return "Math.max stopped at NaN";
          Math.min(NaN, counted);
          if (calls !== 2) return "Math.min stopped at NaN";
          Math.hypot(Infinity, counted);
          if (calls !== 3) return "Math.hypot stopped at Infinity";
          Math.hypot(NaN, counted);
          if (calls !== 4) return "Math.hypot stopped at NaN";
          // An abrupt coercion after an infinite argument still propagates, and stops later coercions.
          let later = 0;
          try {
            Math.hypot(Infinity, -Infinity, NaN, 0, -0,
              { valueOf() { throw new EvalError("x"); } }, { valueOf() { later++; return 0; } });
            return "hypot swallowed the abrupt completion";
          } catch (e) { if (!(e instanceof EvalError) || later !== 0) return "hypot abrupt completion"; }
          if (Math.hypot(Infinity, NaN) !== Infinity || Math.hypot(NaN, 1) === Math.hypot(NaN, 1)) return "values";
          if (Math.max() !== -Infinity || Math.min() !== Infinity) return "empty";
          return true;
        })()"#,
    );
}

#[test]
fn pow_follows_number_exponentiate() {
    assert_true(
        r#"(function() {
          const nan = (v) => v !== v;
          for (const base of [-Infinity, -1.7976931348623157e308, -1e-15, -0, 0, 1e-15, 1.7976931348623157e308, Infinity, NaN, 1, -1]) {
            if (!nan(Math.pow(base, NaN))) return "pow(" + base + ", NaN) is not NaN";
            if (!nan(base ** NaN)) return base + " ** NaN is not NaN";
          }
          for (const exponent of [Infinity, -Infinity]) {
            if (!nan(Math.pow(1, exponent)) || !nan(Math.pow(-1, exponent))) return "|base| = 1 to an infinite power";
            if (!nan(1 ** exponent) || !nan((-1) ** exponent)) return "operator |base| = 1 to an infinite power";
          }
          if (Math.pow(NaN, 0) !== 1 || Math.pow(0, 0) !== 1 || Math.pow(2, 10) !== 1024) return "ordinary values";
          if (Math.pow(0.5, Infinity) !== 0 || Math.pow(2, -Infinity) !== 0 || Math.pow(2, Infinity) !== Infinity) return "infinite exponents";
          return true;
        })()"#,
    );
}

#[test]
fn round_is_exact_near_the_edge_of_the_mantissa() {
    assert_true(
        r#"(function() {
          const E = Number.EPSILON;
          if (1 / Math.round(-0.5) !== -Infinity || 1 / Math.round(-0.25) !== -Infinity || 1 / Math.round(-0) !== -Infinity) return "negative zero results";
          if (1 / Math.round(0.5 - E / 4) !== Infinity) return "just below one half rounds to +0";
          if (Math.round(0.5) !== 1 || Math.round(2.5) !== 3 || Math.round(-2.5) !== -2 || Math.round(-0.5000001) !== -1) return "halves";
          for (const x of [-(2 / E - 1), -(1.5 / E - 1), -(1 / E + 1), 1 / E + 1, 1.5 / E - 1, 2 / E - 1, 4503599627370495.5, -4503599627370495.5, 9007199254740991]) {
            if (Math.round(x) !== x && !(x === 4503599627370495.5 && Math.round(x) === 4503599627370496) &&
                !(x === -4503599627370495.5 && Math.round(x) === -4503599627370495)) return "round(" + x + ") = " + Math.round(x);
          }
          if (Math.round(0.49999999999999994) !== 0) return "0.49999999999999994";
          return true;
        })()"#,
    );
}

/// `Math.acosh` and `Math.atanh` are fdlibm-accurate (within a couple of ulps)
/// even where the textbook `ln(x + sqrt(x*x - 1))` and `ln_1p(2x / (1 - x))`
/// formulas lose most of their digits: `acosh` just above 1 and `atanh` of a
/// negative argument close to -1. Expected values are correctly rounded
/// references.
#[test]
fn acosh_and_atanh_stay_accurate_near_their_singularities() {
    assert_true(
        r#"(function() {
          const f = new Float64Array(2), u = new BigInt64Array(f.buffer);
          const ulps = (a, b) => { f[0] = a; f[1] = b; const d = u[0] - u[1]; return d < 0n ? -d : d; };
          const near = (name, actual, expected, tolerance) => {
            const error = ulps(actual, expected);
            return error <= BigInt(tolerance) ? null : `${name}: got ${actual}, expected ${expected} (${error} ulps)`;
          };
          const failures = [
            near("acosh(1.0000014305114746)", Math.acosh(1.0000014305114746), 0.0016914556651292944, 2),
            near("acosh(1.000007152557373)", Math.acosh(1.000007152557373), 0.003782208044661295, 2),
            near("acosh(1.0000000001)", Math.acosh(1.0000000001), 0.000014142136208675862, 2),
            near("acosh(1e300)", Math.acosh(1e300), 691.4686750787737, 2),
            near("atanh(-0.9999983310699463)", Math.atanh(-0.9999983310699463), -6.998237084679027, 2),
            near("atanh(-0.999992847442627)", Math.atanh(-0.999992847442627), -6.2705920974657525, 2),
            near("atanh(0.999992847442627)", Math.atanh(0.999992847442627), 6.2705920974657525, 2),
            near("atanh(3 / 5)", Math.atanh(3 / 5), Math.log(2), 2),
          ].filter(Boolean);
          if (failures.length) return failures.join("; ");
          // Edge values.
          if (Math.acosh(1) !== 0 || !Object.is(Math.acosh(1), 0)) return "acosh(1)";
          if (Math.acosh(Infinity) !== Infinity) return "acosh(Infinity)";
          if (!Number.isNaN(Math.acosh(0.999)) || !Number.isNaN(Math.acosh(NaN)) || !Number.isNaN(Math.acosh(-Infinity))) return "acosh NaN cases";
          if (Math.atanh(1) !== Infinity || Math.atanh(-1) !== -Infinity) return "atanh(±1)";
          if (!Object.is(Math.atanh(-0), -0) || !Object.is(Math.atanh(0), 0)) return "atanh(±0)";
          if (!Object.is(Math.atanh(-1e-300), -1e-300)) return "atanh tiny";
          if (!Number.isNaN(Math.atanh(1.0000001)) || !Number.isNaN(Math.atanh(-2)) || !Number.isNaN(Math.atanh(NaN))) return "atanh NaN cases";
          return true;
        })()"#,
    );
}
