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
