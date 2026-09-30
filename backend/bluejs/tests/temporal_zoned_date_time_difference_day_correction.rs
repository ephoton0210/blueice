// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Regression coverage for two real, cross-cutting `ZonedDateTime` `since`/
//! `until` bugs found and fixed together in the same pass:
//!
//! 1. `zoned_date_time::difference_zoned_date_time` used to derive its
//!    exact-nanosecond remainder from `date2` combined with `date1`'s own
//!    time-of-day, which is only ever consistent with the overall
//!    years/months/weeks/days direction in the common case. Ported Gecko's
//!    real `DifferenceZonedDateTime` day-correction loop
//!    (`ZonedDateTime.cpp`) instead, which finds the actual anchor date a
//!    consistent remainder can be measured from. Pinned directly from the
//!    real, previously-failing Test262 fixtures
//!    `built-ins/Temporal/ZonedDateTime/prototype/since/
//!    negative-epochnanoseconds.js` and `.../reversibility-of-differences.js`
//!    (both threw `RangeError: duration fields must have a common sign`
//!    before this fix).
//! 2. `Temporal.ZonedDateTime.prototype.since`/`until` had no fast path for
//!    `DifferenceTemporalZonedDateTime`'s own step 8 (equal epoch
//!    nanoseconds -> blank duration), so a fixture exercising many
//!    granularity combinations at an already-equal instant ran the full
//!    calendar-bracketing path every time and exhausted the Test262
//!    harness's own per-script instruction budget
//!    (`same-epoch-nanoseconds.js`).

use blueice_bluejs::{compile, parse, Value, Vm};

fn evaluate(source: &str) -> Value {
    Vm::default()
        .execute(&compile(&parse(source).unwrap()).unwrap())
        .unwrap_or_else(|error| panic!("{source}\n  -> {error:?}"))
}

fn assert_true(source: &str) {
    assert_eq!(evaluate(source), Value::Bool(true), "{source}");
}

#[test]
fn since_a_pre_epoch_instant_with_a_month_largest_unit_has_a_consistent_sign() {
    // `built-ins/Temporal/ZonedDateTime/prototype/since/
    // negative-epochnanoseconds.js`: a pre-epoch receiver, largestUnit
    // "month" -- previously threw a common-sign `RangeError` instead of
    // returning this exact negative duration.
    assert_true(
        r#"
        const datetime = new Temporal.ZonedDateTime(-13849764_999_999_999n, "UTC");
        const result = datetime.since(new Temporal.ZonedDateTime(0n, "UTC"), { largestUnit: "month" });
        result.toString() === "-P5M7DT7H9M24.999999999S"
    "#,
    );
}

#[test]
fn since_and_until_of_the_same_pair_stay_exact_negations_at_day_granularity() {
    // `built-ins/Temporal/ZonedDateTime/prototype/since/
    // reversibility-of-differences.js`'s own shape: `a.since(b)` and
    // `a.until(b).negated()` must agree once the common-sign bug that made
    // one of the two directions throw is fixed.
    assert_true(
        r#"
        const a = new Temporal.ZonedDateTime(-13849764_999_999_999n, "UTC");
        const b = new Temporal.ZonedDateTime(0n, "UTC");
        const since = a.since(b, { largestUnit: "days" });
        const until = a.until(b, { largestUnit: "days" });
        since.days === -until.days
        && since.hours === -until.hours
        && since.minutes === -until.minutes
        && since.seconds === -until.seconds
    "#,
    );
}

#[test]
fn since_returns_a_blank_duration_immediately_for_equal_instants_at_every_unit() {
    // The `DifferenceTemporalZonedDateTime` step 8 fast path
    // (`same-epoch-nanoseconds.js`'s own premise): once epoch nanoseconds
    // are equal, every largestUnit/smallestUnit combination is a blank
    // duration, including day-or-coarser units that would otherwise invoke
    // the full calendar-bracketing path.
    assert_true(
        r#"
        const a = new Temporal.ZonedDateTime(0n, "UTC");
        const b = new Temporal.ZonedDateTime(0n, "UTC");
        const d1 = a.since(b, { largestUnit: "years", smallestUnit: "years" });
        const d2 = a.since(b, { largestUnit: "months", smallestUnit: "days" });
        d1.years === 0 && d1.toString() === "PT0S"
        && d2.months === 0 && d2.days === 0 && d2.toString() === "PT0S"
    "#,
    );
}

#[test]
fn until_across_a_dst_shortened_day_still_produces_a_common_sign_duration() {
    // A same-local-time-of-day-but-different-real-offset pair spanning a
    // real DST spring-forward transition in `America/New_York`
    // (2019-03-10, clocks jump 02:00 -> 03:00): confirms the day-correction
    // loop's zone-aware candidate resolution (not just the pre-epoch UTC
    // case above) still produces a valid, common-sign duration rather than
    // a `RangeError`.
    assert_true(
        r#"
        const a = Temporal.ZonedDateTime.from("2019-03-09T12:00:00-05:00[America/New_York]");
        const b = Temporal.ZonedDateTime.from("2019-03-11T12:00:00-04:00[America/New_York]");
        const result = a.until(b, { largestUnit: "days" });
        result.days === 2 && result.hours === 0
    "#,
    );
}
