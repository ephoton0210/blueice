// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Regression coverage for `Temporal.ZonedDateTime.prototype.until`/`since` and
//! `Temporal.Duration.prototype.round`/`total` rounding and balancing against a
//! `ZonedDateTime`, where a calendar day is not always 24 hours long (a named
//! IANA zone's day is 23 or 25 hours across a DST transition, and Samoa once
//! skipped a whole day).
//!
//! Before this fix the three call sites each carried their own, subtly
//! different, hand-rolled subset of the specification's
//! `DifferenceZonedDateTimeWithRounding`/`DifferenceZonedDateTimeWithTotal`
//! pipeline:
//!
//! * `ZonedDateTime.prototype.until`/`since` rounded a sub-day `smallestUnit`
//!   as a plain nanosecond remainder, so a remainder that rounded up to (or past)
//!   the receiver's own real day length never carried into `days` — the
//!   specification's `NudgeToZonedTime` step — and never bubbled up into the
//!   coarser units `largestUnit` allows (`RoundRelativeDuration` step 9).
//! * Its `NudgeToCalendarUnit` port skipped the "destination lies outside the
//!   first bracket, shift the window one increment" step and never checked that
//!   the bracket's ending instant is representable.
//! * `Duration.prototype.round`/`total` re-derived the same computation in a
//!   second, structurally different implementation (an "unbalance to the unit,
//!   then bracket" shape) that disagreed with it near a DST transition.
//!
//! Every expectation below is taken from a real Test262 fixture (named at each
//! test), not derived from the implementation.

use blueice_bluejs::{compile, parse, Value, Vm};

/// `TemporalHelpers.assertDuration`, reduced to the ten component getters.
const PRELUDE: &str = r#"
function assertDuration(actual, y, mo, w, d, h, mi, s, ms, us, ns, message) {
  const got = [actual.years, actual.months, actual.weeks, actual.days, actual.hours,
    actual.minutes, actual.seconds, actual.milliseconds, actual.microseconds, actual.nanoseconds];
  const want = [y, mo, w, d, h, mi, s, ms, us, ns];
  for (let i = 0; i < 10; i++) {
    if (got[i] !== want[i]) {
      throw new Error(message + ": got " + got.join(",") + " want " + want.join(","));
    }
  }
}
function assertSame(actual, expected, message) {
  if (!Object.is(actual, expected)) {
    throw new Error(message + ": got " + String(actual) + " want " + String(expected));
  }
}
function assertRangeError(fn, message) {
  try { fn(); } catch (e) {
    if (e instanceof RangeError) return;
    throw new Error(message + ": expected RangeError, got " + e);
  }
  throw new Error(message + ": expected RangeError, nothing thrown");
}
"#;

/// Runs `body` after [`PRELUDE`], failing with the JS-side message on a throw.
fn check(body: &str) {
    let source =
        format!("{PRELUDE}\ntry {{\n{body}\n\"ok\"\n}} catch (e) {{ \"FAILED: \" + e.message }}");
    let value = Vm::default()
        .execute(&compile(&parse(&source).unwrap()).unwrap())
        .unwrap_or_else(|error| panic!("{body}\n  -> {error:?}"));
    match value {
        Value::String(text) => {
            let text = text.to_utf8().unwrap();
            assert_eq!(text, "ok", "{body}");
        }
        other => panic!("unexpected completion value {other:?}"),
    }
}

/// `intl402/Temporal/ZonedDateTime/prototype/until/dst-rounding-result.js`:
/// "Rounding up to hours causes one more day of overflow" — `2 days 23:59`
/// rounds to `24` hours, which is the whole (24-hour) day, so it must carry.
#[test]
fn until_rounding_hours_up_to_the_day_length_carries_one_more_day() {
    check(
        r#"
        const start = Temporal.ZonedDateTime.from("2020-01-01T00:00-08:00[-08:00]");
        const end = Temporal.ZonedDateTime.from("2020-01-03T23:59-08:00[-08:00]");
        const options = { largestUnit: "days", smallestUnit: "hours", roundingMode: "halfExpand" };
        assertDuration(start.until(end, options), 0, 0, 0, 3, 0, 0, 0, 0, 0, 0, "until");
        assertDuration(end.until(start, options), 0, 0, 0, -3, 0, 0, 0, 0, 0, 0, "until (negative)");
        assertDuration(end.since(start, options), 0, 0, 0, 3, 0, 0, 0, 0, 0, 0, "since");
        assertDuration(start.since(end, options), 0, 0, 0, -3, 0, 0, 0, 0, 0, 0, "since (negative)");
    "#,
    );
}

/// The remainder of `dst-rounding-result.js` that already passed before the
/// rewrite: day- and hour-granularity rounding around Vancouver's spring
/// forward must keep agreeing with the specification afterwards.
#[test]
fn until_rounding_around_a_spring_forward_day_is_dst_aware() {
    check(
        r#"
        const start = Temporal.PlainDateTime.from("2000-04-04T02:30").toZonedDateTime("America/Vancouver");
        const end = Temporal.PlainDateTime.from("2000-04-01T14:15").toZonedDateTime("America/Vancouver");
        const day = (roundingMode) => start.until(end, { smallestUnit: "days", roundingMode });
        assertDuration(day("halfExpand"), 0, 0, 0, -3, 0, 0, 0, 0, 0, 0, "nearest day");
        assertDuration(day("ceil"), 0, 0, 0, -2, 0, 0, 0, 0, 0, 0, "ceil day");
        assertDuration(day("trunc"), 0, 0, 0, -2, 0, 0, 0, 0, 0, 0, "trunc day");
        assertDuration(day("floor"), 0, 0, 0, -3, 0, 0, 0, 0, 0, 0, "floor day");
        const hour = (roundingMode) =>
          start.until(end, { largestUnit: "days", smallestUnit: "hours", roundingMode });
        assertDuration(hour("halfExpand"), 0, 0, 0, -2, -12, 0, 0, 0, 0, 0, "nearest hour");
        assertDuration(hour("ceil"), 0, 0, 0, -2, -12, 0, 0, 0, 0, 0, "ceil hour");
        assertDuration(hour("trunc"), 0, 0, 0, -2, -12, 0, 0, 0, 0, 0, "trunc hour");
        assertDuration(hour("floor"), 0, 0, 0, -2, -13, 0, 0, 0, 0, 0, "floor hour");
    "#,
    );
}

/// `built-ins/Temporal/ZonedDateTime/prototype/until/round-cross-unit-boundary.js`:
/// rounding a sub-day remainder up must keep bubbling into every coarser unit
/// `largestUnit` allows (`RoundRelativeDuration` step 9, with `NudgeToZonedTime`
/// reporting that it rounded past the end of the day).
#[test]
fn until_rounding_up_a_sub_day_remainder_bubbles_up_to_largest_unit() {
    check(
        r#"
        {
          const earlier = new Temporal.ZonedDateTime(1640995200_000_000_000n, "UTC");
          const later = new Temporal.ZonedDateTime(1703462400_000_000_000n, "UTC");
          const duration = earlier.until(later, { largestUnit: "years", smallestUnit: "months", roundingMode: "expand" });
          assertDuration(duration, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, "1 year 11 months balances to 2 years");
        }
        {
          const earlier = new Temporal.ZonedDateTime(0n, "UTC");
          const later = new Temporal.ZonedDateTime(7199_000_000_000n, "UTC");
          const duration = earlier.until(later, { largestUnit: "hours", smallestUnit: "minutes", roundingMode: "expand" });
          assertDuration(duration, 0, 0, 0, 0, 2, 0, 0, 0, 0, 0, "1:59 balances to 2 hours");
        }
        {
          const earlier = new Temporal.ZonedDateTime(0n, "UTC");
          const later = new Temporal.ZonedDateTime(63071999_999_999_999n, "UTC");
          const duration = earlier.until(later, { largestUnit: "years", smallestUnit: "microseconds", roundingMode: "expand" });
          assertDuration(duration, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, "rounding up 1 ns balances to 2 years");
        }
    "#,
    );
}

/// `built-ins/Temporal/ZonedDateTime/prototype/until/roundingincrement-addition-out-of-range.js`:
/// the rounding window's *ending* bound must itself be a representable instant.
#[test]
fn until_rejects_a_rounding_window_whose_end_is_not_representable() {
    check(
        r#"
        const earlier = new Temporal.ZonedDateTime(0n, "UTC");
        const later = new Temporal.ZonedDateTime(5n, "UTC");
        assertRangeError(() => earlier.until(later, { smallestUnit: "days", roundingIncrement: 1e8 + 1 }), "positive");
        assertRangeError(() => later.until(earlier, { smallestUnit: "days", roundingIncrement: 1e8 + 1 }), "negative");
        assertDuration(earlier.until(later, { smallestUnit: "days", roundingIncrement: 1e8, roundingMode: "expand" }),
          0, 0, 0, 1e8, 0, 0, 0, 0, 0, 0, "1e8 days is the largest window");
        assertDuration(later.until(earlier, { smallestUnit: "days", roundingIncrement: 1e8, roundingMode: "expand" }),
          0, 0, 0, -1e8, 0, 0, 0, 0, 0, 0, "-1e8 days is the largest window");
    "#,
    );
}

/// `built-ins/Temporal/ZonedDateTime/prototype/{until,since}/invalid-increments.js`
/// (`GetDifferenceSettings` step: `ValidateTemporalRoundingIncrement` with
/// `MaximumTemporalDurationRoundingIncrement(smallestUnit)` exclusive).
#[test]
fn until_and_since_validate_the_increment_against_the_unit_maximum() {
    check(
        r#"
        const earlier = new Temporal.ZonedDateTime(0n, "UTC");
        const later = new Temporal.ZonedDateTime(987654321_123_456_789n, "UTC");
        for (const method of ["until", "since"]) {
          for (const [smallestUnit, roundingIncrement] of [
            ["hours", 11], ["hours", 24], ["minutes", 29], ["minutes", 60], ["seconds", 29], ["seconds", 60],
            ["milliseconds", 29], ["milliseconds", 1000], ["microseconds", 29], ["microseconds", 1000],
            ["nanoseconds", 29], ["nanoseconds", 1000],
          ]) {
            assertRangeError(() => earlier[method](later, { smallestUnit, roundingIncrement }),
              method + " " + smallestUnit + " " + roundingIncrement);
          }
          // Date units are unbounded, and dividing evenly is only required for time units.
          earlier[method](later, { smallestUnit: "days", roundingIncrement: 100 });
          earlier[method](later, { largestUnit: "years", smallestUnit: "months", roundingIncrement: 5 });
        }
    "#,
    );
}

/// `built-ins/Temporal/ZonedDateTime/prototype/until/roundingmode-half-boundary.js`:
/// `Duration.prototype.total` relative to a `ZonedDateTime` measures the
/// fraction inside the *calendar year* containing the destination.
#[test]
fn total_of_a_hours_duration_in_years_measures_progress_through_the_year() {
    check(
        r#"
        const earlier1 = Temporal.ZonedDateTime.from("2019-01-01T00:00+00:00[UTC]");
        const later = Temporal.ZonedDateTime.from("2020-07-02T00:00+00:00[UTC]");
        assertSame(earlier1.until(later).total({ unit: "years", relativeTo: earlier1 }), 1.5, "1.5 years");
        const earlier2 = Temporal.ZonedDateTime.from("2018-01-01T00:00+00:00[UTC]");
        assertSame(earlier2.until(later).total({ unit: "years", relativeTo: earlier2 }), 2.5, "2.5 years");
    "#,
    );
}

/// `intl402/Temporal/Duration/prototype/round/adjust-rounded-duration-days.js`:
/// a single extra day is added when rounding relative to a non-24-hour day.
#[test]
fn round_hours_relative_to_a_non_24_hour_day_adds_a_single_day() {
    check(
        r#"
        let zdt = Temporal.ZonedDateTime.from("2024-03-10T00:00:00[America/New_York]"); // 23-hour day
        let d = new Temporal.Duration(0, 0, 0, 0, 13, 0, 0, 0, 0, 0);
        assertDuration(d.round({ relativeTo: zdt, largestUnit: "years", smallestUnit: "hours",
          roundingIncrement: 12, roundingMode: "ceil" }), 0, 0, 0, 1, 12, 0, 0, 0, 0, 0, "13h -> 1 day 12h");
        assertDuration(d.round({ relativeTo: zdt, largestUnit: "years", smallestUnit: "days",
          roundingIncrement: 1, roundingMode: "ceil" }), 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, "13h -> 1 day");

        zdt = Temporal.ZonedDateTime.from("2024-11-03T00:00:00[America/New_York]"); // 25-hour day
        d = new Temporal.Duration(0, 0, 0, 0, 25, 0, 0, 0, 0, 0);
        assertDuration(d.round({ relativeTo: zdt, largestUnit: "years", smallestUnit: "hours",
          roundingIncrement: 12, roundingMode: "ceil" }), 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, "25h == 1 day");
        d = new Temporal.Duration(0, 0, 0, 0, 24, 0, 0, 0, 0, 0);
        assertDuration(d.round({ relativeTo: zdt, largestUnit: "years", smallestUnit: "hours",
          roundingIncrement: 12, roundingMode: "ceil" }), 0, 0, 0, 0, 24, 0, 0, 0, 0, 0, "24h stays 24h");
        d = new Temporal.Duration(0, 0, 0, 1, 0, 0, 0, 0, 0, 0);
        assertDuration(d.round({ relativeTo: zdt, largestUnit: "hours", smallestUnit: "hours",
          roundingIncrement: 12, roundingMode: "ceil" }), 0, 0, 0, 0, 36, 0, 0, 0, 0, 0, "1 day (25h) -> 36h");
    "#,
    );
}

/// `intl402/Temporal/Duration/prototype/round/dst-balancing-result.js`.
#[test]
fn round_balances_with_the_zones_real_day_length() {
    check(
        r#"
        const timeZone = "America/Vancouver";
        const oneDay = new Temporal.Duration(0, 0, 0, 1);
        const hours25 = new Temporal.Duration(0, 0, 0, 0, 25);
        const inRepeatedHour = new Temporal.ZonedDateTime(972806400_000_000_000n, timeZone);
        assertDuration(hours25.round({ largestUnit: "days", relativeTo: inRepeatedHour }),
          0, 0, 0, 1, 0, 0, 0, 0, 0, 0, "25 hours in days");
        assertDuration(oneDay.round({ largestUnit: "hours", relativeTo: inRepeatedHour }),
          0, 0, 0, 0, 25, 0, 0, 0, 0, 0, "1 day in hours");
        assertDuration(Temporal.Duration.from({ days: 126, hours: 1 }).round({ largestUnit: "hours", relativeTo: inRepeatedHour }),
          0, 0, 0, 0, 3026, 0, 0, 0, 0, 0, "126 days 1 hour in hours");

        // Samoa skipped a whole day (2011-12-30).
        const beforeSkippedDay = Temporal.PlainDateTime.from("2011-12-29T12:00").toZonedDateTime("Pacific/Apia");
        assertDuration(hours25.round({ largestUnit: "days", relativeTo: beforeSkippedDay }),
          0, 0, 0, 2, 1, 0, 0, 0, 0, 0, "25 hours in days across Samoa's skipped day");
        assertDuration(Temporal.Duration.from({ hours: 48 }).round({ largestUnit: "days", relativeTo: beforeSkippedDay }),
          0, 0, 0, 3, 0, 0, 0, 0, 0, 0, "48 hours in days across Samoa's skipped day");
    "#,
    );
}

/// `intl402/Temporal/Duration/prototype/total/dst-day-length.js` and
/// `.../total/dst-balancing-result.js`.
#[test]
fn total_uses_the_zones_real_day_length() {
    check(
        r#"
        const oneDay = new Temporal.Duration(0, 0, 0, 1);
        const hours25 = new Temporal.Duration(0, 0, 0, 0, 25);
        const hours12 = new Temporal.Duration(0, 0, 0, 0, 12);
        const hours48 = new Temporal.Duration(0, 0, 0, 0, 48);
        const timeZone = "America/Vancouver";
        const beforeSkippedHour = new Temporal.ZonedDateTime(954585000_000_000_000n, timeZone);
        const skippedHourDay = new Temporal.ZonedDateTime(954662400_000_000_000n, timeZone);
        const repeatedHourDay = new Temporal.ZonedDateTime(972802800_000_000_000n, timeZone);
        const beforeRepeatedHour = new Temporal.ZonedDateTime(972716400_000_000_000n, timeZone);
        assertSame(hours25.total({ unit: "days", relativeTo: beforeSkippedHour }), 24 / 23, "normal -> skipped hour");
        assertSame(oneDay.total({ unit: "hours", relativeTo: beforeSkippedHour }), 24, "1 day = 24 hours");
        assertSame(hours25.total({ unit: "days", relativeTo: skippedHourDay }), 13 / 12, "before skipped hour");
        assertSame(oneDay.total({ unit: "hours", relativeTo: skippedHourDay }), 23, "1 day = 23 hours");
        assertSame(hours12.total({ unit: "days", relativeTo: skippedHourDay }), 12 / 23, "12/23 days");
        assertSame(hours25.total({ unit: "days", relativeTo: repeatedHourDay }), 1, "25 hours = 1 day");
        assertSame(hours12.total({ unit: "days", relativeTo: repeatedHourDay }), 12 / 25, "12/25 days");
        assertSame(hours48.total({ unit: "days", relativeTo: beforeRepeatedHour }), 49 / 25, "1 24/25 days");

        const samoa = Temporal.PlainDateTime.from("2011-12-29T12:00").toZonedDateTime("Pacific/Apia");
        const within = Math.abs(hours25.total({ unit: "days", relativeTo: samoa }) - (2 + 1 / 24)) < Number.EPSILON;
        assertSame(within, true, "25 hours over Samoa's skipped day");
        assertSame(Temporal.Duration.from({ hours: 48 }).total({ unit: "days", relativeTo: samoa }), 3, "48 hours");
        assertSame(Temporal.Duration.from({ days: 2 }).total({ unit: "hours", relativeTo: samoa }), 24, "2 days");

        // A year-bearing duration measures its trailing hours against the real 25-hour day.
        const relativeTo = new Temporal.ZonedDateTime(941184000_000_000_000n, timeZone);
        assertSame(new Temporal.Duration(1, 0, 0, 0, 24).total({ unit: "days", relativeTo }), 366.96,
          "24 hours does not balance to 1 day in a 25-hour day");
    "#,
    );
}

/// `intl402/Temporal/Duration/prototype/round/dst-rounding-result.js` (already
/// passing before the rewrite; pinned so the unified pipeline keeps it).
#[test]
fn round_months_and_days_measure_progress_in_real_elapsed_time() {
    check(
        r#"
        {
          const duration = new Temporal.Duration(0, 1, 0, 15, 11, 30);
          const relativeTo = new Temporal.ZonedDateTime(950868000_000_000_000n, "America/Vancouver");
          assertDuration(duration.round({ smallestUnit: "months", relativeTo }), 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, "up");
          assertDuration(duration.round({ smallestUnit: "months", roundingMode: "halfTrunc", relativeTo }),
            0, 1, 0, 0, 0, 0, 0, 0, 0, 0, "down");
        }
        {
          // 23-hour day: 11.5 hours is exactly half.
          const duration = new Temporal.Duration(0, 0, 0, 0, 11, 30);
          const relativeTo = new Temporal.PlainDateTime(2000, 4, 2).toZonedDateTime("America/Vancouver");
          assertDuration(duration.round({ relativeTo, smallestUnit: "days" }), 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, "half up");
          assertDuration(duration.round({ relativeTo, smallestUnit: "days", roundingMode: "halfTrunc" }),
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, "half down");
        }
    "#,
    );
}

/// `built-ins/Temporal/ZonedDateTime/prototype/{until,since}/float64-representable-integer.js`:
/// every Duration field is a Number, so an exact difference too large for a
/// double is observably rounded when the Duration is created.
#[test]
fn until_and_since_round_each_field_to_a_float64() {
    check(
        r#"
        const z1 = new Temporal.ZonedDateTime(0n, "UTC");
        const z2 = new Temporal.ZonedDateTime(18446744073_709_551_616n, "UTC");
        const result = z1.until(z2, { largestUnit: "microseconds" });
        assertSame(result.microseconds, 18446744073709552, "microseconds lose precision");
        assertSame(result.toString(), "PT18446744073.709552616S", "toString uses the rounded field");
        assertSame(Temporal.Duration.compare(result.add({ microseconds: 1 }), result), 0, "later ops agree");
        assertSame(z2.since(z1, { largestUnit: "microseconds" }).microseconds, 18446744073709552, "since");
    "#,
    );
}

/// `built-ins/Temporal/Duration/{compare,prototype/round,prototype/total}/relativeto-string-limits.js`:
/// at the last representable instant a blank duration is fine and 5 minutes
/// is not (its target is out of range) for `round`/`total`, but `compare` of
/// two time-only durations never adds either to the anchor.
#[test]
fn relative_to_the_last_representable_instant_only_a_date_part_or_a_target_can_fail() {
    check(
        r#"
        const instance = new Temporal.Duration(0, 0, 0, 0, 0, 5);
        const blank = new Temporal.Duration();
        for (const relativeTo of ["+275760-09-13T00:00Z[UTC]", "+275760-09-13T01:00+01:00[+01:00]",
                                  "+275760-09-13T23:59+23:59[+23:59]"]) {
          blank.round({ smallestUnit: "minutes", relativeTo });
          blank.total({ unit: "minutes", relativeTo });
          assertRangeError(() => instance.round({ smallestUnit: "minutes", relativeTo }), "round " + relativeTo);
          assertRangeError(() => instance.total({ unit: "minutes", relativeTo }), "total " + relativeTo);
          assertSame(Temporal.Duration.compare(instance, blank, { relativeTo }), 1, "compare " + relativeTo);
          // A day is not a fixed length there, so a date part does need the target.
          assertRangeError(() => Temporal.Duration.compare({ days: 1 }, blank, { relativeTo }), "compare days " + relativeTo);
        }
        // A plain date string just before the first representable date-time:
        // `compare` only converts it once a years/months/weeks part needs the
        // calendar, unlike `round`/`total`, which always do.
        for (const relativeTo of ["-271821-04-19", "-271821-04-19T01:00"]) {
          assertSame(Temporal.Duration.compare(instance, blank, { relativeTo }), 1, "compare " + relativeTo);
          assertSame(Temporal.Duration.compare({ days: 1 }, { hours: 24 }, { relativeTo }), 0, "days " + relativeTo);
          assertRangeError(() => Temporal.Duration.compare({ weeks: 1 }, blank, { relativeTo }), "weeks " + relativeTo);
          assertRangeError(() => instance.round({ smallestUnit: "minutes", relativeTo }), "round " + relativeTo);
          blank.round({ smallestUnit: "minutes", relativeTo });
        }
    "#,
    );
}

/// `built-ins/Temporal/ZonedDateTime/prototype/round/{day-rounding-out-of-range,
/// get-start-of-day-throws,throws-on-invalid-increments}.js`.
#[test]
fn round_rejects_unrepresentable_day_bounds_and_out_of_range_increments() {
    check(
        r#"
        // At the last instant there is no next day to round toward.
        assertRangeError(() => new Temporal.ZonedDateTime(86400_0000_0000_000_000_000n, "UTC").round({ smallestUnit: "day" }),
          "upper bound");
        // `GetStartOfDay` of the edge dates is not a representable instant.
        for (const [epoch, zone] of [[-864n * 10n ** 19n, "-01"], [-864n * 10n ** 19n, "+01"],
                                     [864n * 10n ** 19n, "-01"], [864n * 10n ** 19n, "+00"], [864n * 10n ** 19n, "+01"]]) {
          assertRangeError(() => new Temporal.ZonedDateTime(epoch, zone).round({ smallestUnit: "days" }), epoch + " " + zone);
        }
        // The increment must stay below the count of the unit in the next larger one, and divide it.
        const zdt = new Temporal.ZonedDateTime(217175010123456789n, "+01:00");
        for (const [smallestUnit, roundingIncrement] of [["day", 29], ["hour", 29], ["hour", 24], ["minute", 60],
            ["second", 60], ["millisecond", 1000], ["microsecond", 1000], ["nanosecond", 1000], ["hour", 5]]) {
          assertRangeError(() => zdt.round({ smallestUnit, roundingIncrement }), smallestUnit + " " + roundingIncrement);
        }
        zdt.round({ smallestUnit: "hour", roundingIncrement: 12 });
        zdt.round({ smallestUnit: "nanosecond", roundingIncrement: 500 });
    "#,
    );
}

/// `built-ins/Temporal/ZonedDateTime/from/argument-string-limits.js` (and the
/// `compare`/`equals`/`until`/`since`/`Duration` `relativeTo` fixtures that
/// share it): `prefer`/`reject` check the *wall-clock* date against
/// `CheckISODaysRange` (`InterpretISODateTimeOffset` step 7), a day narrower
/// at the start of the range than `use`/`ignore`, which only need the
/// resulting instant to be representable.
#[test]
fn prefer_and_reject_check_the_wall_clock_date_against_the_days_range() {
    check(
        r#"
        // Both instants are representable (the first is exactly the minimum), but
        // their wall-clock date is one day before the minimum date.
        const wallClockOutside = ["-271821-04-19T23:00-01:00[-01:00]", "-271821-04-19T00:01-23:59[-23:59]"];
        for (const offset of ["use", "ignore"]) {
          for (const arg of wallClockOutside) Temporal.ZonedDateTime.from(arg, { offset });
        }
        for (const offset of ["prefer", "reject"]) {
          for (const arg of wallClockOutside) {
            assertRangeError(() => Temporal.ZonedDateTime.from(arg, { offset }), arg + " " + offset);
          }
          for (const arg of ["-271821-04-20T00:00Z[UTC]", "+275760-09-13T00:00Z[UTC]", "+275760-09-13T23:59+23:59[+23:59]"]) {
            Temporal.ZonedDateTime.from(arg, { offset });
          }
        }
        // The same rule reaches every consumer of a ZonedDateTime string.
        const receiver = new Temporal.ZonedDateTime(0n, "UTC");
        assertRangeError(() => receiver.until("-271821-04-19T23:00-01:00[-01:00]"), "until");
        assertRangeError(() => receiver.since("-271821-04-19T23:00-01:00[-01:00]"), "since");
        assertRangeError(() => Temporal.Duration.compare({ minutes: 5 }, { minutes: 6 },
          { relativeTo: "-271821-04-19T23:00-01:00[-01:00]" }), "relativeTo");
    "#,
    );
}

/// `intl402/Temporal/ZonedDateTime/prototype/round/same-date-starts-twice.js`:
/// Antarctica/Casey turned its clocks back across `2010-03-05T00:00`, so that
/// midnight occurs twice. `round` to a day must always land on a start-of-day,
/// even for an instant on the *second* occurrence of the date (later than the
/// next day's own start).
#[test]
fn round_to_days_lands_on_a_start_of_day_even_when_midnight_occurs_twice() {
    check(
        r#"
        const zone = "Antarctica/Casey";
        const zdt1 = Temporal.ZonedDateTime.from("2010-03-04T23:10:00+11:00[" + zone + "]");
        const zdt2 = Temporal.ZonedDateTime.from("2010-03-05T00:45:00+11:00[" + zone + "]");
        const zdt3 = Temporal.ZonedDateTime.from("2010-03-04T23:10:00+08:00[" + zone + "]");
        const zdt4 = Temporal.ZonedDateTime.from("2010-03-05T00:45:00+08:00[" + zone + "]");
        const startOfMarch4 = Temporal.ZonedDateTime.from("2010-03-04T00:00:00+11:00[" + zone + "]");
        const startOfMarch5 = Temporal.ZonedDateTime.from("2010-03-05T00:00:00+11:00[" + zone + "]");
        const startOfMarch6 = Temporal.ZonedDateTime.from("2010-03-06T00:00:00+08:00[" + zone + "]");
        const expectations = {
          floor: [startOfMarch4, startOfMarch5, startOfMarch4, startOfMarch5],
          trunc: [startOfMarch4, startOfMarch5, startOfMarch4, startOfMarch5],
          halfExpand: [startOfMarch5, startOfMarch5, startOfMarch5, startOfMarch5],
          halfTrunc: [startOfMarch5, startOfMarch5, startOfMarch5, startOfMarch5],
          ceil: [startOfMarch5, startOfMarch6, startOfMarch5, startOfMarch6],
          expand: [startOfMarch5, startOfMarch6, startOfMarch5, startOfMarch6],
        };
        for (const roundingMode of Object.keys(expectations)) {
          [zdt1, zdt2, zdt3, zdt4].forEach((zdt, index) => {
            const rounded = zdt.round({ smallestUnit: "days", roundingMode });
            assertSame(rounded.equals(expectations[roundingMode][index]), true,
              zdt.toString() + " " + roundingMode + " -> " + rounded.toString());
          });
        }
    "#,
    );
}

/// `built-ins/Temporal/ZonedDateTime/prototype/{startOfDay,withPlainTime}/
/// {throws-if-epoch-nanoseconds-outside-valid-limits,get-start-of-day-throws}.js`
/// and `intl402/.../withPlainTime/dst-skipped-cross-midnight.js`.
#[test]
fn start_of_day_and_with_plain_time_reject_unrepresentable_instants() {
    check(
        r#"
        const min = -864n * 10n ** 19n;
        // The start of the wall-clock day of an instant at the very edge is out of range...
        assertRangeError(() => new Temporal.ZonedDateTime(min, "-01").startOfDay(), "startOfDay -01");
        assertRangeError(() => new Temporal.ZonedDateTime(min, "+01").startOfDay(), "startOfDay +01");
        assertRangeError(() => new Temporal.ZonedDateTime(min, "-01").withPlainTime(), "withPlainTime() -01");
        assertRangeError(() => new Temporal.ZonedDateTime(min, "+01").withPlainTime(), "withPlainTime() +01");
        // ...unless the zone is UTC, where it is exactly the minimum.
        assertSame(new Temporal.ZonedDateTime(min, "+00").startOfDay().epochNanoseconds, min, "UTC start of day");
        // An explicit time is resolved through the zone and must itself be representable.
        assertRangeError(() => new Temporal.ZonedDateTime(min, "-01").withPlainTime("00:00"), "-01 00:00");
        assertRangeError(() => new Temporal.ZonedDateTime(min, "+01").withPlainTime("00:00"), "+01 00:00");
        assertRangeError(() => new Temporal.ZonedDateTime(864n * 10n ** 19n, "UTC").withPlainTime("01:00"), "max 01:00");
    "#,
    );
}

/// `intl402/Temporal/ZonedDateTime/prototype/withPlainTime/dst-skipped-cross-midnight.js`:
/// with no argument the result is the day's start, which is *not* the
/// `compatible` resolution of a midnight that does not exist.
#[test]
fn with_plain_time_without_an_argument_is_the_start_of_the_day() {
    check(
        r#"
        // Toronto's 1919-03-31 gap started at 00:30, so the day starts at 00:30
        // -- neither 00:00 nor the 01:00 that `compatible` moves midnight to.
        const instance = Temporal.ZonedDateTime.from({ year: 1919, month: 3, day: 31, hour: 12, timeZone: "America/Toronto" });
        const startOfDay = instance.withPlainTime();
        const midnightDisambiguated = instance.withPlainTime(new Temporal.PlainTime());
        assertSame(startOfDay.epochNanoseconds, instance.startOfDay().epochNanoseconds, "same instant as startOfDay");
        assertDuration(startOfDay.until(midnightDisambiguated), 0, 0, 0, 0, 0, 30, 0, 0, 0, 0,
          "start of day is 30 minutes earlier than the disambiguated midnight");
    "#,
    );
}
