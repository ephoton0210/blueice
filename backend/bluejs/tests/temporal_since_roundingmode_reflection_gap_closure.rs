// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Regression coverage for a real, cross-cutting `roundingMode` reflection
//! gap: `Temporal.{PlainDate,PlainDateTime,ZonedDateTime}.prototype.since`
//! all compute their `years`..`nanoseconds` result in a fixed `existing`
//! (receiver) -> `other` (argument) direction and only negate the *finished*
//! result for `since` -- but `roundingMode`'s `"ceil"`/`"floor"`/
//! `"halfCeil"`/`"halfFloor"` variants are sensitive to the real number
//! line's direction (`ceil(-x) == -floor(x)`, not `-ceil(x)`), so negating
//! without also reflecting an asymmetric mode silently rounds the wrong way
//! whenever the difference isn't already an exact multiple of the rounding
//! unit. `Temporal.PlainYearMonth.prototype.since`/`until` already had this
//! exact reflection (its own leap-month gap-closure pass); `PlainDate`,
//! `PlainDateTime` and `ZonedDateTime` did not, confirmed via the real
//! pinned Test262 fixtures `built-ins/Temporal/{PlainDate,PlainDateTime,
//! ZonedDateTime}/prototype/since/roundingmode-{ceil,floor}.js`, all
//! previously failing (`until`'s own fixtures already passed, since `until`
//! never negates). Values below are taken directly from those fixtures.
//!
//! Every one of these previously produced the *opposite* mode's answer
//! (e.g. `since(..., { roundingMode: "ceil" })` silently returning what
//! `"floor"` should have produced) before the `effective_mode` reflection
//! this test pins was added to `temporal_date_difference` (`PlainDate`/
//! `PlainDateTime`) and `temporal_zoned_date_time_difference`
//! (`ZonedDateTime`).

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
fn plain_date_since_ceil_years_reflects_direction() {
    // `built-ins/Temporal/PlainDate/prototype/since/roundingmode-ceil.js`:
    // positive case (`later.since(earlier)`) expects 3 years, not 2.
    assert_true(
        r#"
        const earlier = new Temporal.PlainDate(2019, 1, 8);
        const later = new Temporal.PlainDate(2021, 9, 7);
        later.since(earlier, { smallestUnit: "years", roundingMode: "ceil" }).toString() === "P3Y"
    "#,
    );
}

#[test]
fn plain_date_since_ceil_years_negative_case_reflects_direction() {
    // Same fixture: negative case (`earlier.since(later)`) expects -2 years,
    // not -3 -- ceil rounds a negative fractional difference *toward* zero.
    assert_true(
        r#"
        const earlier = new Temporal.PlainDate(2019, 1, 8);
        const later = new Temporal.PlainDate(2021, 9, 7);
        earlier.since(later, { smallestUnit: "years", roundingMode: "ceil" }).toString() === "-P2Y"
    "#,
    );
}

#[test]
fn plain_date_since_floor_years_reflects_direction() {
    // `built-ins/Temporal/PlainDate/prototype/since/roundingmode-floor.js`:
    // positive case expects 2 years (opposite of ceil's 3).
    assert_true(
        r#"
        const earlier = new Temporal.PlainDate(2019, 1, 8);
        const later = new Temporal.PlainDate(2021, 9, 7);
        later.since(earlier, { smallestUnit: "years", roundingMode: "floor" }).toString() === "P2Y"
    "#,
    );
}

#[test]
fn plain_date_until_ceil_years_is_unaffected() {
    // `until` never negates, so its own ceil/floor behaviour was already
    // correct before this fix and must stay that way.
    assert_true(
        r#"
        const earlier = new Temporal.PlainDate(2019, 1, 8);
        const later = new Temporal.PlainDate(2021, 9, 7);
        later.until(earlier, { smallestUnit: "years", roundingMode: "ceil" }).toString() === "-P2Y"
    "#,
    );
}

#[test]
fn plain_date_time_since_ceil_hours_reflects_direction_in_the_sub_day_branch() {
    // `built-ins/Temporal/PlainDateTime/prototype/since/roundingmode-ceil.js`,
    // `smallestUnit: "hours"` row (default `largestUnit` is `"day"` for
    // `PlainDateTime`, unlike `ZonedDateTime`'s `"hour"`): positive case
    // expects `P973DT5H` -- confirms the sub-day (`TimeDuration::round`)
    // branch also needed the reflection, not just the calendar-unit branch.
    assert_true(
        r#"
        const earlier = new Temporal.PlainDateTime(2019, 1, 8, 8, 22, 36, 123, 456, 789);
        const later = new Temporal.PlainDateTime(2021, 9, 7, 12, 39, 40, 987, 654, 289);
        later.since(earlier, { smallestUnit: "hours", roundingMode: "ceil" }).toString() === "P973DT5H"
    "#,
    );
}

#[test]
fn zoned_date_time_since_ceil_years_reflects_direction() {
    // `built-ins/Temporal/ZonedDateTime/prototype/since/roundingmode-ceil.js`:
    // identical instants/expectation shape to the `PlainDate` case above.
    assert_true(
        r#"
        const earlier = new Temporal.ZonedDateTime(1546935756_123_456_789n, "UTC");
        const later = new Temporal.ZonedDateTime(1631018380_987_654_289n, "UTC");
        later.since(earlier, { smallestUnit: "years", roundingMode: "ceil" }).toString() === "P3Y"
        && earlier.since(later, { smallestUnit: "years", roundingMode: "ceil" }).toString() === "-P2Y"
    "#,
    );
}

#[test]
fn zoned_date_time_since_half_ceil_and_half_floor_months_match_real_fixture() {
    // `roundingmode-halfCeil.js`/`roundingmode-halfFloor.js`, `months` row:
    // both modes agree on 32 months for this specific fixture pair (neither
    // rounding tie actually lands exactly halfway), but before the
    // `effective_mode` reflection this test pins, the *unreflected* code
    // rounded the positive case toward 31 months instead (the sibling
    // `roundingmode-ceil.js`/`roundingmode-floor.js` swap this file's other
    // tests pin) for both of these half-modes too.
    assert_true(
        r#"
        const earlier = new Temporal.ZonedDateTime(1546935756_123_456_789n, "UTC");
        const later = new Temporal.ZonedDateTime(1631018380_987_654_289n, "UTC");
        const halfCeil = later.since(earlier, { smallestUnit: "months", roundingMode: "halfCeil" }).toString();
        const halfFloor = later.since(earlier, { smallestUnit: "months", roundingMode: "halfFloor" }).toString();
        halfCeil === "P32M" && halfFloor === "P32M"
    "#,
    );
}

#[test]
fn zoned_date_time_until_ceil_years_is_unaffected() {
    assert_true(
        r#"
        const earlier = new Temporal.ZonedDateTime(1546935756_123_456_789n, "UTC");
        const later = new Temporal.ZonedDateTime(1631018380_987_654_289n, "UTC");
        later.until(earlier, { smallestUnit: "years", roundingMode: "ceil" }).toString() === "-P2Y"
    "#,
    );
}
