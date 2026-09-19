// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Regression coverage for a real bug in `Temporal.PlainDateTime.prototype.round`
//! (Phase 26 Stage 2, second slice —
//! `development/browser_core/phase-26-ecma262-temporal/PLAN.md`).
//!
//! `Vm::temporal_date_time_round` (`backend/bluejs/src/vm/temporal.rs`)
//! validated `smallestUnit` against `Self::temporal_validated_time_unit`,
//! which only accepts `"hour"`..`"nanosecond"` — the vocabulary a bare
//! `Temporal.PlainTime` needs. `PlainDateTime.prototype.round`'s own
//! `smallestUnit` is one unit wider (`RoundISODateTime`'s range is
//! `"day"`..`"nanosecond"`), so every `{ smallestUnit: "day" }` call threw
//! `RangeError: invalid smallestUnit option` — confirmed against every
//! `built-ins/Temporal/PlainDateTime/prototype/round/roundingmode-*.js`,
//! `round/balance.js`, `round/roundingincrement-one-day.js` and
//! `round/limits.js` fixture, all of which use `"day"`.
//!
//! Fixed by handling `"day"`/`"days"` as its own case: `roundingIncrement`
//! must be exactly `1` for day granularity (`ValidateTemporalRoundingIncrement(increment,
//! 1, true)` — day has no finer subdivision to increment by, unlike
//! `Temporal.Instant.round`'s own day rule, which allows any divisor of a
//! day), and the whole time-of-day is rounded to the nearest whole day via
//! [`rounding::round_to_increment`] rather than through
//! `TimeDuration::round`, which has no day-length variant.

use blueice_bluejs::{compile, parse, RuntimeError, Value, Vm};

fn evaluate(source: &str) -> Value {
    Vm::default()
        .execute(&compile(&parse(source).unwrap()).unwrap())
        .unwrap_or_else(|error| panic!("{source}\n  -> {error:?}"))
}

fn evaluate_err(source: &str) -> RuntimeError {
    Vm::default()
        .execute(&compile(&parse(source).unwrap()).unwrap())
        .expect_err(&format!("{source}\n  -> expected an error, got a value"))
}

fn assert_true(source: &str) {
    assert_eq!(evaluate(source), Value::Bool(true), "{source}");
}

/// `round/balance.js`'s own case: a time near the end of the day, rounded
/// to the nearest day (default `roundingMode: "halfExpand"`), balances
/// forward into the next calendar day with a zeroed time.
#[test]
fn smallest_unit_day_balances_to_the_next_day() {
    assert_true(
        r#"
        (function() {
          const dt = new Temporal.PlainDateTime(1976, 11, 18, 23, 59, 59, 999, 999, 999);
          const r = dt.round({ smallestUnit: "day" });
          return r.year === 1976 && r.month === 11 && r.day === 19
              && r.hour === 0 && r.minute === 0 && r.second === 0
              && r.millisecond === 0 && r.microsecond === 0 && r.nanosecond === 0;
        })()
        "#,
    );
}

/// `round/roundingincrement-one-day.js`: `roundingIncrement: 1` is
/// explicitly valid for `smallestUnit: "day"` (not just the default-omitted
/// case), and a time before noon rounds *down* to midnight under the
/// default `halfExpand` mode.
#[test]
fn smallest_unit_day_accepts_rounding_increment_one() {
    assert_true(
        r#"
        (function() {
          const dt = new Temporal.PlainDateTime(1976, 11, 18, 14, 23, 30, 123, 456, 789);
          const r = dt.round({ smallestUnit: "day", roundingIncrement: 1 });
          return r.year === 1976 && r.month === 11 && r.day === 19
              && r.hour === 0 && r.minute === 0 && r.second === 0;
        })()
        "#,
    );
}

/// A `roundingIncrement` other than `1` is invalid for day granularity —
/// day has no finer subdivision within this call to increment by.
#[test]
fn smallest_unit_day_rejects_a_rounding_increment_other_than_one() {
    let error = evaluate_err(
        r#"
        new Temporal.PlainDateTime(1976, 11, 18, 14, 23, 30).round({ smallestUnit: "day", roundingIncrement: 2 })
        "#,
    );
    assert!(matches!(error, RuntimeError::RangeError(_)), "{error:?}");
}

/// `round/limits.js`: rounding away from the representable range's edge
/// must throw `RangeError`, not silently clamp or wrap.
#[test]
fn smallest_unit_day_throws_at_the_representable_range_limit() {
    let error = evaluate_err(
        r#"
        new Temporal.PlainDateTime(-271821, 4, 19, 0, 0, 0, 0, 0, 1).round({ smallestUnit: "day", roundingMode: "floor" })
        "#,
    );
    assert!(matches!(error, RuntimeError::RangeError(_)), "{error:?}");
}
