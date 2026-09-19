// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Regression coverage for a real `roundingMode`-reflection bug in
//! `Vm::temporal_date_difference` (`PlainDate`/`PlainDateTime`) and
//! `Vm::temporal_zoned_date_time_difference` (`ZonedDateTime`)
//! (`development/browser_core/phase-26-ecma262-temporal/PLAN.md`).
//!
//! Both functions always compute the receiver-to-argument difference (the
//! same direction `until` uses) and negate every field of the finished
//! result for `since`, per `DifferenceTemporalPlainDate`/
//! `DifferenceTemporalPlainDateTime`/`DifferenceTemporalZonedDateTime`'s own
//! step 10 (that direction itself was already fixed in an earlier pass —
//! see `temporal_since_until_direction.rs`). What neither function did is
//! reflect an asymmetric `roundingMode` when negating: `round_calendar_duration`
//! (day/week/month/year granularity) and `TimeDuration::round`
//! (sub-day/sub-hour granularity) both round a *real*, direction-aware
//! signed quantity — `Ceil`/`Floor` round toward a fixed end of the real
//! number line (`ceil(-x) == -floor(x)`, not `-ceil(x)`), and `HalfCeil`/
//! `HalfFloor` are the half-mode analogue. Negating the already-rounded
//! result without swapping `Ceil`<->`Floor`/`HalfCeil`<->`HalfFloor` in the
//! `roundingMode` passed to the rounding step silently rounds the wrong way
//! whenever `since` negates a non-exact value — exactly the same bug class
//! `temporal_year_month_difference` (`PlainYearMonth`) already had fixed for
//! it (see this document's "genuinely closed" `since`/`until` leap-month
//! bullet, bug #4).
//!
//! Confirmed against the real, unmodified pinned Test262 corpus before any
//! fix (not assumed from the leap-month pass's own similarly-worded but
//! unverified note): `built-ins/Temporal/{PlainDate,PlainDateTime,
//! ZonedDateTime}/prototype/since/roundingmode-{ceil,floor}.js` (plus
//! `PlainDateTime`/`ZonedDateTime`'s own `halfCeil`/`halfFloor` files) fail
//! outright; the corresponding `until/roundingmode-*.js` files all pass,
//! since `until` never negates and so never needed the reflection. Cases
//! below are taken directly from those real fixture files' own `earlier`/
//! `later` values and expected results.
//!
//! Manually re-derived by hand against `round_month_or_year`'s real
//! algorithm before writing this file, to confirm the fix is the right one
//! and not just "whatever makes the assertion pass": for
//! `PlainDate/prototype/since/roundingmode-ceil.js`'s "years" case,
//! `later.since(earlier)` (`later` = 2021-09-07, `earlier` = 2019-01-08)
//! computes the *unreflected* receiver-to-argument value as `years = -2`
//! (`ceil` applied to the real, negative `later -> earlier` direction:
//! `ceil(-2.663) == -2`), which negates to `2` — not the fixture's expected
//! `3`. Reflecting `Ceil` to `Floor` before rounding (since `since` negates)
//! gives `years = -3` (`floor(-2.663) == -3`), which negates to the
//! expected `3`.

use blueice_bluejs::{compile, parse, Value, Vm};

fn evaluate(source: &str) -> Value {
    Vm::default()
        .execute(&compile(&parse(source).unwrap()).unwrap())
        .unwrap_or_else(|error| panic!("{source}\n  -> {error:?}"))
}

fn assert_true(source: &str) {
    assert_eq!(evaluate(source), Value::Bool(true), "{source}");
}

/// `built-ins/Temporal/PlainDate/prototype/since/roundingmode-ceil.js`'s
/// "years" case, both directions.
#[test]
fn plain_date_since_ceil_years_reflects_to_floor_for_the_negated_result() {
    assert_true(
        r#"
        (function() {
          const earlier = new Temporal.PlainDate(2019, 1, 8);
          const later = new Temporal.PlainDate(2021, 9, 7);
          const positive = later.since(earlier, { smallestUnit: "years", roundingMode: "ceil" });
          const negative = earlier.since(later, { smallestUnit: "years", roundingMode: "ceil" });
          return positive.years === 3 && negative.years === -2;
        })()
        "#,
    );
}

/// The same fixture's "months" case, both directions.
#[test]
fn plain_date_since_ceil_months_reflects_to_floor_for_the_negated_result() {
    assert_true(
        r#"
        (function() {
          const earlier = new Temporal.PlainDate(2019, 1, 8);
          const later = new Temporal.PlainDate(2021, 9, 7);
          const positive = later.since(earlier, { smallestUnit: "months", roundingMode: "ceil" });
          const negative = earlier.since(later, { smallestUnit: "months", roundingMode: "ceil" });
          return positive.months === 32 && negative.months === -31;
        })()
        "#,
    );
}

/// `built-ins/Temporal/PlainDate/prototype/since/roundingmode-floor.js`'s
/// "years" case (the mirror mode: `Floor` reflects to `Ceil`).
#[test]
fn plain_date_since_floor_years_reflects_to_ceil_for_the_negated_result() {
    assert_true(
        r#"
        (function() {
          const earlier = new Temporal.PlainDate(2019, 1, 8);
          const later = new Temporal.PlainDate(2021, 9, 7);
          const positive = later.since(earlier, { smallestUnit: "years", roundingMode: "floor" });
          const negative = earlier.since(later, { smallestUnit: "years", roundingMode: "floor" });
          return positive.years === 2 && negative.years === -3;
        })()
        "#,
    );
}

/// `until` never negates, so it must be completely unaffected by this fix
/// in either direction — `later.until(earlier)`/`earlier.until(later)` with
/// `roundingMode: "ceil"` keep their own already-correct (real-direction)
/// values.
#[test]
fn plain_date_until_ceil_is_unaffected_by_the_since_reflection() {
    assert_true(
        r#"
        (function() {
          const earlier = new Temporal.PlainDate(2019, 1, 8);
          const later = new Temporal.PlainDate(2021, 9, 7);
          const positive = later.until(earlier, { smallestUnit: "years", roundingMode: "ceil" });
          const negative = earlier.until(later, { smallestUnit: "years", roundingMode: "ceil" });
          return positive.years === -2 && negative.years === 3;
        })()
        "#,
    );
}

/// `built-ins/Temporal/PlainDateTime/prototype/since/roundingmode-ceil.js`'s
/// "years" case -- confirms `PlainDateTime` (which shares
/// `Vm::temporal_date_difference` with `PlainDate`) needs the identical fix.
#[test]
fn plain_date_time_since_ceil_years_reflects_to_floor_for_the_negated_result() {
    assert_true(
        r#"
        (function() {
          const earlier = new Temporal.PlainDateTime(2019, 1, 8, 8, 22, 36, 123, 456, 789);
          const later = new Temporal.PlainDateTime(2021, 9, 7, 12, 39, 40, 987, 654, 289);
          const positive = later.since(earlier, { smallestUnit: "years", roundingMode: "ceil" });
          const negative = earlier.since(later, { smallestUnit: "years", roundingMode: "ceil" });
          return positive.years === 3 && negative.years === -2;
        })()
        "#,
    );
}

/// The same fixture's "hours" case -- exercises `temporal_date_difference`'s
/// *sub-day* branch (`TimeDuration::round`, not `round_calendar_duration`),
/// confirming the reflection is needed there too, not just the calendar
/// branch.
#[test]
fn plain_date_time_since_ceil_hours_reflects_to_floor_for_the_negated_result() {
    assert_true(
        r#"
        (function() {
          const earlier = new Temporal.PlainDateTime(2019, 1, 8, 8, 22, 36, 123, 456, 789);
          const later = new Temporal.PlainDateTime(2021, 9, 7, 12, 39, 40, 987, 654, 289);
          const positive = later.since(earlier, { smallestUnit: "hours", roundingMode: "ceil" });
          const negative = earlier.since(later, { smallestUnit: "hours", roundingMode: "ceil" });
          return positive.days === 973 && positive.hours === 5
              && negative.days === -973 && negative.hours === -4;
        })()
        "#,
    );
}

/// `built-ins/Temporal/ZonedDateTime/prototype/since/roundingmode-ceil.js`'s
/// "years" case -- confirms `Vm::temporal_zoned_date_time_difference` has
/// the identical bug in its own calendar-unit (`nudge_to_calendar_unit`)
/// branch.
#[test]
fn zoned_date_time_since_ceil_years_reflects_to_floor_for_the_negated_result() {
    assert_true(
        r#"
        (function() {
          const earlier = new Temporal.ZonedDateTime(1546935756_123_456_789n, "UTC");
          const later = new Temporal.ZonedDateTime(1631018380_987_654_289n, "UTC");
          const positive = later.since(earlier, { smallestUnit: "years", roundingMode: "ceil" });
          const negative = earlier.since(later, { smallestUnit: "years", roundingMode: "ceil" });
          return positive.years === 3 && negative.years === -2;
        })()
        "#,
    );
}

/// The same fixture's "hours" case -- exercises
/// `temporal_zoned_date_time_difference_fields`'s own sub-day branch (plain
/// epoch-nanosecond `TimeDuration::round`, no calendar/zone consulted),
/// confirming the reflection is needed there too.
#[test]
fn zoned_date_time_since_ceil_hours_reflects_to_floor_for_the_negated_result() {
    assert_true(
        r#"
        (function() {
          const earlier = new Temporal.ZonedDateTime(1546935756_123_456_789n, "UTC");
          const later = new Temporal.ZonedDateTime(1631018380_987_654_289n, "UTC");
          const positive = later.since(earlier, { smallestUnit: "hours", roundingMode: "ceil" });
          const negative = earlier.since(later, { smallestUnit: "hours", roundingMode: "ceil" });
          return positive.hours === 23357 && negative.hours === -23356;
        })()
        "#,
    );
}

/// `built-ins/Temporal/ZonedDateTime/prototype/since/roundingmode-halfCeil.js`'s
/// "years" case -- confirms the half-mode reflection (`HalfCeil`<->`HalfFloor`)
/// is needed for `ZonedDateTime` too, not just plain `Ceil`/`Floor`.
#[test]
fn zoned_date_time_since_half_ceil_years_reflects_to_half_floor_for_the_negated_result() {
    assert_true(
        r#"
        (function() {
          const earlier = new Temporal.ZonedDateTime(1546935756_123_456_789n, "UTC");
          const later = new Temporal.ZonedDateTime(1631018380_987_654_289n, "UTC");
          const positive = later.since(earlier, { smallestUnit: "years", roundingMode: "halfCeil" });
          const negative = earlier.since(later, { smallestUnit: "years", roundingMode: "halfCeil" });
          return positive.years === 3 && negative.years === -3;
        })()
        "#,
    );
}
