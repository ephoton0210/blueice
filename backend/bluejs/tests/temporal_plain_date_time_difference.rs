// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Regression coverage for `Temporal.PlainDateTime.prototype.until`/`since`
//! (Phase 26 Stage 3, `development/browser_core/phase-26-ecma262-temporal/PLAN.md`).
//!
//! `Vm::temporal_date_difference` (`backend/bluejs/src/vm/temporal/dates.rs`)
//! used to treat a `PlainDateTime` as a `PlainDate` difference with a
//! separately-handled time-of-day, which is not what
//! `DifferenceISODateTime`/`DifferencePlainDateTimeWithRounding` define:
//!
//! - `largestUnit` a time unit (`"hours"` .. `"nanoseconds"`) must fold every
//!   whole day into the time fields (`P2DT5H` is `PT53H`), not report `days`.
//! - Rounding to `day`/`week`/`month`/`year` must measure the fraction from
//!   the argument's exact position, time-of-day included, so `1 day 12 hours`
//!   rounded up (`ceil`, `expand`) is two days, not one.
//! - A time-of-day that runs against the date direction borrows one day.
//! - Rounding a time remainder up can reach the next calendar unit's
//!   boundary and must bubble up to `largestUnit`.
//! - A time-unit `roundingIncrement` must be smaller than, and divide, the
//!   next larger unit (`ValidateTemporalRoundingIncrement`); a rounded
//!   calendar bracket outside the representable range is a `RangeError`.
//! - Duration fields are Numbers: totals past 2^53 lose precision instead of
//!   wrapping (`nanoseconds` used to be truncated through an `i64`).
//!
//! `Temporal.PlainDate.prototype.until`/`since` is the same algorithm at
//! midnight (`DifferenceTemporalPlainDate` builds two midnight date-times and
//! calls `RoundRelativeDuration`), so it shares the module and these tests:
//! a rounded calendar bracket outside the valid range must throw
//! (`PlainDate/prototype/{until,since}/throws-if-rounded-date-outside-valid-iso-range.js`),
//! and a months/years increment rounds the *months remainder* of the
//! duration (`ComputeNudgeWindow`), never a flattened `years * 12 + months`.

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

/// Expects `true`; a script may instead return a string describing the first
/// case that failed, which is reported as text rather than as UTF-16 units.
fn assert_true(source: &str) {
    match evaluate(source) {
        Value::Bool(true) => {}
        Value::String(reason) => panic!("{}", reason.to_utf8().unwrap()),
        other => panic!("{source}\n  -> {other:?}"),
    }
}

fn assert_range_error(source: &str) {
    assert!(
        matches!(evaluate_err(source), RuntimeError::RangeError(_)),
        "{source}"
    );
}

/// The ten fields of the Duration `expr` evaluates to, comma-joined in
/// `years..nanoseconds` order, so a failing case prints both sides.
fn fields(expr: &str) -> Value {
    evaluate(&format!(
        r#"(function() {{
          const r = {expr};
          return [r.years, r.months, r.weeks, r.days, r.hours, r.minutes,
                  r.seconds, r.milliseconds, r.microseconds, r.nanoseconds].join(",");
        }})()"#
    ))
}

fn assert_fields(expr: &str, expected: [i64; 10]) {
    let Value::String(actual) = fields(expr) else {
        panic!("{expr}\n  -> expected a string of fields");
    };
    let list = expected.map(|field| field.to_string()).join(",");
    assert_eq!(actual.to_utf8().unwrap(), list, "{expr}");
}

const EARLY: &str = "new Temporal.PlainDateTime(2020, 1, 1)";
const LATE: &str = "new Temporal.PlainDateTime(2020, 1, 3, 5)";

/// `largestUnit` a time unit folds the two days into the time fields; there is
/// no `days` field to report them in. `until/units-changed.js` pins the same
/// shape over a whole leap year.
#[test]
fn a_time_unit_largest_unit_folds_whole_days_into_the_time_fields() {
    let until = |unit: &str| format!(r#"{EARLY}.until({LATE}, {{ largestUnit: "{unit}" }})"#);
    assert_fields(&until("hours"), [0, 0, 0, 0, 53, 0, 0, 0, 0, 0]);
    assert_fields(&until("minutes"), [0, 0, 0, 0, 0, 3180, 0, 0, 0, 0]);
    assert_fields(&until("seconds"), [0, 0, 0, 0, 0, 0, 190_800, 0, 0, 0]);
    assert_fields(
        &until("milliseconds"),
        [0, 0, 0, 0, 0, 0, 0, 190_800_000, 0, 0],
    );
    assert_fields(
        &until("microseconds"),
        [0, 0, 0, 0, 0, 0, 0, 0, 190_800_000_000, 0],
    );
    assert_fields(
        &until("nanoseconds"),
        [0, 0, 0, 0, 0, 0, 0, 0, 0, 190_800_000_000_000],
    );
    // `since` is the same difference, negated.
    assert_fields(
        &format!(r#"{EARLY}.since({LATE}, {{ largestUnit: "hours" }})"#),
        [0, 0, 0, 0, -53, 0, 0, 0, 0, 0],
    );
}

/// A whole leap year is 8784 hours (`until/units-changed.js`).
#[test]
fn a_year_of_hours_and_a_year_of_nanoseconds() {
    let until = |unit: &str| {
        format!(
            r#"new Temporal.PlainDateTime(2020, 2, 1).until(new Temporal.PlainDateTime(2021, 2, 1), {{ largestUnit: "{unit}" }})"#
        )
    };
    assert_fields(&until("hours"), [0, 0, 0, 0, 8784, 0, 0, 0, 0, 0]);
    assert_fields(
        &until("nanoseconds"),
        [0, 0, 0, 0, 0, 0, 0, 0, 0, 31_622_400_000_000_000],
    );
}

/// With no `largestUnit` (or `"auto"`, or `"days"`) the difference reports
/// days and a time remainder; a `"weeks"` largest unit holds no whole week
/// here.
#[test]
fn the_default_largest_unit_is_days() {
    for options in [
        "",
        r#", { largestUnit: "auto" }"#,
        r#", { largestUnit: "days" }"#,
    ] {
        assert_fields(
            &format!("{EARLY}.until({LATE}{options})"),
            [0, 0, 0, 2, 5, 0, 0, 0, 0, 0],
        );
    }
    assert_fields(
        &format!(r#"{EARLY}.until({LATE}, {{ largestUnit: "weeks" }})"#),
        [0, 0, 0, 2, 5, 0, 0, 0, 0, 0],
    );
}

/// `DifferenceISODateTime` steps 4-7: when the time-of-day runs against the
/// date direction the date part is one day too long, so that day is borrowed
/// back into the time part — `1 day - 6 hours` is `18 hours`, never a
/// negative time on a positive date.
#[test]
fn a_time_of_day_running_against_the_date_direction_borrows_one_day() {
    let early = "new Temporal.PlainDateTime(2020, 1, 2, 12)";
    let late = "new Temporal.PlainDateTime(2020, 1, 3, 6)";
    assert_fields(
        &format!("{early}.until({late})"),
        [0, 0, 0, 0, 18, 0, 0, 0, 0, 0],
    );
    assert_fields(
        &format!("{late}.until({early})"),
        [0, 0, 0, 0, -18, 0, 0, 0, 0, 0],
    );
    assert_fields(
        &format!("{late}.since({early})"),
        [0, 0, 0, 0, 18, 0, 0, 0, 0, 0],
    );
    // The borrowed day is taken from the *date* difference, so a month-end
    // anchor sees Feb 28, not Feb 29: 28 days 18 hours, not 1 month.
    assert_fields(
        r#"new Temporal.PlainDateTime(2020, 1, 31, 12).until(
             new Temporal.PlainDateTime(2020, 2, 29, 6), { largestUnit: "months" })"#,
        [0, 0, 0, 28, 18, 0, 0, 0, 0, 0],
    );
    assert_fields(
        r#"new Temporal.PlainDateTime(2020, 2, 29, 6).until(
             new Temporal.PlainDateTime(2020, 1, 31, 12), { largestUnit: "months" })"#,
        [0, 0, 0, -28, -18, 0, 0, 0, 0, 0],
    );
}

/// Rounding to whole days measures the time-of-day: 1 day 12 hours is exactly
/// halfway. `since` reflects the asymmetric modes, so it matches the reversed
/// `until`.
#[test]
fn smallest_unit_day_rounds_the_time_of_day() {
    let early = "new Temporal.PlainDateTime(2020, 1, 1)";
    let late = "new Temporal.PlainDateTime(2020, 1, 2, 12)";
    // (mode, until early->late, until late->early)
    let cases = [
        ("trunc", 1, -1),
        ("ceil", 2, -1),
        ("floor", 1, -2),
        ("expand", 2, -2),
        ("halfExpand", 2, -2),
        ("halfTrunc", 1, -1),
        ("halfCeil", 2, -1),
        ("halfFloor", 1, -2),
        ("halfEven", 2, -2),
    ];
    for (mode, forward, backward) in cases {
        let options = format!(r#"{{ smallestUnit: "days", roundingMode: "{mode}" }}"#);
        let expect = |days: i64| [0, 0, 0, days, 0, 0, 0, 0, 0, 0];
        assert_fields(
            &format!("{early}.until({late}, {options})"),
            expect(forward),
        );
        assert_fields(
            &format!("{late}.until({early}, {options})"),
            expect(backward),
        );
        assert_fields(
            &format!("{late}.since({early}, {options})"),
            expect(forward),
        );
        assert_fields(
            &format!("{early}.since({late}, {options})"),
            expect(backward),
        );
    }
}

/// The same for the calendar units: the fraction is measured against the
/// bracket the argument's exact position falls in, time-of-day included.
#[test]
fn calendar_unit_rounding_measures_the_time_of_day() {
    let range = |from: &str, to: &str, unit: &str, mode: &str| {
        format!(
            r#"{from}.until({to}, {{ largestUnit: "{unit}", smallestUnit: "{unit}", roundingMode: "{mode}" }})"#
        )
    };
    // One month and twelve hours (a 29-day February bracket).
    let jan1 = "new Temporal.PlainDateTime(2020, 1, 1)";
    let feb1_noon = "new Temporal.PlainDateTime(2020, 2, 1, 12)";
    for (mode, months) in [("ceil", 2), ("expand", 2), ("trunc", 1), ("halfExpand", 1)] {
        assert_fields(
            &range(jan1, feb1_noon, "months", mode),
            [0, months, 0, 0, 0, 0, 0, 0, 0, 0],
        );
    }
    // Two and a half days past one week.
    let jan10_noon = "new Temporal.PlainDateTime(2020, 1, 10, 12)";
    for (mode, weeks) in [("ceil", 2), ("trunc", 1), ("halfExpand", 1)] {
        assert_fields(
            &range(jan1, jan10_noon, "weeks", mode),
            [0, 0, weeks, 0, 0, 0, 0, 0, 0, 0],
        );
    }
    // A minute short of a whole (leap) year.
    let dec31 = "new Temporal.PlainDateTime(2020, 12, 31, 23, 59)";
    for (mode, years) in [("halfExpand", 1), ("ceil", 1), ("trunc", 0), ("floor", 0)] {
        assert_fields(
            &range(jan1, dec31, "years", mode),
            [years, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        );
    }
    // Negative direction: -1 month -12 hours; `ceil` is toward +infinity.
    let backwards = |mode: &str| range(feb1_noon, jan1, "months", mode);
    for (mode, months) in [("ceil", -1), ("trunc", -1), ("floor", -2), ("expand", -2)] {
        assert_fields(&backwards(mode), [0, months, 0, 0, 0, 0, 0, 0, 0, 0]);
    }
}

/// Rounding a time remainder up can reach the next calendar unit's boundary,
/// and must bubble up to `largestUnit` (`until/round-cross-unit-boundary.js`).
#[test]
fn rounding_up_a_time_remainder_bubbles_into_larger_units() {
    assert_fields(
        r#"new Temporal.PlainDateTime(2022, 1, 1).until(new Temporal.PlainDateTime(2023, 12, 25),
             { largestUnit: "years", smallestUnit: "months", roundingMode: "expand" })"#,
        [2, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    );
    assert_fields(
        r#"new Temporal.PlainDateTime(2000, 5, 2).until(new Temporal.PlainDateTime(2000, 5, 2, 1, 59, 59),
             { largestUnit: "hours", smallestUnit: "minutes", roundingMode: "expand" })"#,
        [0, 0, 0, 0, 2, 0, 0, 0, 0, 0],
    );
    assert_fields(
        r#"new Temporal.PlainDateTime(1970, 1, 1).until(
             new Temporal.PlainDateTime(1971, 12, 31, 23, 59, 59, 999, 999, 999),
             { largestUnit: "years", smallestUnit: "microseconds", roundingMode: "expand" })"#,
        [2, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    );
}

/// Rounding overflows a whole day but `largestUnit` is a time unit, so the
/// day is reported as 24 hours (`until/bubble-time-unit.js`), in either
/// direction.
#[test]
fn a_rounded_up_day_stays_in_the_time_unit_that_is_the_largest() {
    // (unit, increment, later's time fields, expected field index, expected value)
    let cases: [(&str, u32, &str, usize, i64); 6] = [
        ("hours", 12, "14", 4, 24),
        ("minutes", 30, "23, 35", 5, 1440),
        ("seconds", 30, "23, 59, 35", 6, 86_400),
        ("milliseconds", 500, "23, 59, 59, 650", 7, 86_400_000),
        (
            "microseconds",
            500,
            "23, 59, 59, 999, 650",
            8,
            86_400_000_000,
        ),
        (
            "nanoseconds",
            500,
            "23, 59, 59, 999, 999, 650",
            9,
            86_400_000_000_000,
        ),
    ];
    for (unit, increment, later, index, value) in cases {
        let options = format!(
            r#"{{ largestUnit: "{unit}", smallestUnit: "{unit}", roundingIncrement: {increment}, roundingMode: "ceil" }}"#
        );
        let mut expected = [0_i64; 10];
        expected[index] = value;
        let earlier = "new Temporal.PlainDateTime(2025, 6, 14)";
        let later = format!("new Temporal.PlainDateTime(2025, 6, 14, {later})");
        assert_fields(&format!("{earlier}.until({later}, {options})"), expected);
        // `since` reflects `floor` to `ceil` and then negates the result.
        let options = options.replace("ceil", "floor");
        expected[index] = -value;
        assert_fields(&format!("{earlier}.since({later}, {options})"), expected);
    }
}

/// `ValidateTemporalRoundingIncrement(increment, maximum, false)`: a time-unit
/// increment must be smaller than, and evenly divide, the next larger unit.
#[test]
fn a_time_unit_rounding_increment_must_divide_the_next_larger_unit() {
    let call = |method: &str, unit: &str, increment: u32| {
        format!(
            r#"{EARLY}.{method}({LATE}, {{ smallestUnit: "{unit}", roundingIncrement: {increment} }})"#
        )
    };
    let rejected = [
        ("hour", 24),
        ("hour", 5),
        ("minute", 60),
        ("minute", 7),
        ("second", 60),
        ("second", 7),
        ("millisecond", 1000),
        ("millisecond", 300),
        ("microsecond", 1000),
        ("microsecond", 300),
        ("nanosecond", 1000),
        ("nanosecond", 300),
    ];
    for (unit, increment) in rejected {
        for method in ["until", "since"] {
            assert_range_error(&call(method, unit, increment));
        }
    }
    let accepted = [
        ("hour", 12),
        ("minute", 30),
        ("second", 30),
        ("millisecond", 500),
        ("microsecond", 500),
        ("nanosecond", 500),
    ];
    for (unit, increment) in accepted {
        for method in ["until", "since"] {
            evaluate(&call(method, unit, increment));
        }
    }
    // The date units have no such maximum: any 1..=1e9 increment is valid.
    assert_fields(
        &format!(
            r#"{EARLY}.until(new Temporal.PlainDateTime(2020, 1, 20), {{ smallestUnit: "days", roundingIncrement: 7 }})"#
        ),
        [0, 0, 0, 14, 0, 0, 0, 0, 0, 0],
    );
}

/// A time-unit `roundingIncrement` rounds the whole duration, day part
/// included, and a non-integer increment truncates
/// (`until/roundingincrement-non-integer.js`).
#[test]
fn rounding_increments_apply_to_the_whole_difference() {
    // 53 hours to the nearest 12: 48.
    assert_fields(
        &format!(
            r#"{EARLY}.until({LATE}, {{ largestUnit: "hours", smallestUnit: "hours", roundingIncrement: 12 }})"#
        ),
        [0, 0, 0, 0, 48, 0, 0, 0, 0, 0],
    );
    // 2 days 5 hours to the nearest 12 hours, trunc: 2 days 0 hours.
    assert_fields(
        &format!(r#"{EARLY}.until({LATE}, {{ smallestUnit: "hours", roundingIncrement: 12 }})"#),
        [0, 0, 0, 2, 0, 0, 0, 0, 0, 0],
    );
    let earlier = "new Temporal.PlainDateTime(2000, 5, 2, 12, 34, 56)";
    let later = "new Temporal.PlainDateTime(2000, 5, 2, 12, 34, 56, 0, 0, 5)";
    assert_fields(
        &format!(
            r#"{earlier}.until({later}, {{ roundingIncrement: 2.5, roundingMode: "trunc" }})"#
        ),
        [0, 0, 0, 0, 0, 0, 0, 0, 0, 4],
    );
    assert_fields(
        &format!(
            r#"{earlier}.until({later}, {{ smallestUnit: "days", roundingIncrement: 1e9 + 0.5, roundingMode: "expand" }})"#
        ),
        [0, 0, 0, 1_000_000_000, 0, 0, 0, 0, 0, 0],
    );
}

#[test]
fn smallest_unit_larger_than_largest_unit_throws() {
    for options in [
        r#"{ largestUnit: "hours", smallestUnit: "days" }"#,
        r#"{ largestUnit: "days", smallestUnit: "weeks" }"#,
        r#"{ largestUnit: "nanoseconds", smallestUnit: "microseconds" }"#,
    ] {
        for method in ["until", "since"] {
            assert_range_error(&format!("{EARLY}.{method}({LATE}, {options})"));
        }
    }
}

/// Rounding to a calendar bracket that needs a date outside the representable
/// range throws (`until/throws-if-rounded-date-outside-valid-iso-range.js`).
#[test]
fn a_rounded_calendar_bracket_outside_the_valid_range_throws() {
    for method in ["until", "since"] {
        assert_range_error(&format!(
            r#"new Temporal.PlainDateTime(1970, 1, 1).{method}(new Temporal.PlainDateTime(1971, 1, 1),
                 {{ roundingIncrement: 100000000, smallestUnit: "months" }})"#
        ));
    }
}

/// Every Duration field is a Number, so a total past 2^53 is rounded to the
/// nearest double (`until/float64-representable-integer.js`) rather than being
/// kept exactly — or, worse, truncated through a 64-bit integer.
#[test]
fn duration_fields_are_rounded_to_float64() {
    assert_true(
        r#"(function() {
          const result = new Temporal.PlainDateTime(1970, 1, 1).until(
            new Temporal.PlainDateTime(2554, 7, 21, 23, 34, 33, 709, 551, 616),
            { largestUnit: "microseconds" });
          return result.microseconds === 18446744073709552
              && result.toString() === "PT18446744073.709552616S"
              && Temporal.Duration.compare(result.add({ microseconds: 1 }), result) === 0;
        })()"#,
    );
    // 600 years of nanoseconds exceeds `i64::MAX`; the field must be the
    // (rounded) true total, not a wrapped 64-bit remainder.
    assert_true(
        r#"(function() {
          const from = new Temporal.PlainDateTime(2000, 1, 1);
          const to = new Temporal.PlainDateTime(2600, 1, 1);
          const days = from.until(to, { largestUnit: "days" }).days;
          const nanoseconds = from.until(to, { largestUnit: "nanoseconds" }).nanoseconds;
          return nanoseconds > 9223372036854775807 && nanoseconds === days * 86400000000000;
        })()"#,
    );
}

/// `2020-01-31T12:00` -> `2020-03-01T06:00` is "29 days 18 hours" (there is no
/// Feb 31, so no whole month), yet the argument lies *beyond* the first month
/// bracket `[Jan 31, Feb 29]`. `NudgeToCalendarUnit` slides the window one
/// increment outward to `[Feb 29, Mar 31]` and measures inside that, so the
/// result is one month (`trunc`) or two (`ceil`) — never a bracket the argument
/// is not in. The reverse direction is not the mirror image (`-1 month 18
/// hours`), and rounds against its own bracket.
#[test]
fn a_month_end_bracket_that_misses_the_argument_slides_outward() {
    let early = "new Temporal.PlainDateTime(2020, 1, 31, 12)";
    let late = "new Temporal.PlainDateTime(2020, 3, 1, 6)";
    assert_fields(
        &format!(r#"{early}.until({late}, {{ largestUnit: "months" }})"#),
        [0, 0, 0, 29, 18, 0, 0, 0, 0, 0],
    );
    assert_fields(
        &format!(r#"{late}.until({early}, {{ largestUnit: "months" }})"#),
        [0, -1, 0, 0, -18, 0, 0, 0, 0, 0],
    );
    // (mode, until early->late, until late->early)
    let cases = [
        ("trunc", 1, -1),
        ("ceil", 2, -1),
        ("floor", 1, -2),
        ("expand", 2, -2),
        ("halfExpand", 1, -1),
    ];
    for (mode, forward, backward) in cases {
        let options = format!(
            r#"{{ largestUnit: "months", smallestUnit: "months", roundingMode: "{mode}" }}"#
        );
        let expect = |months: i64| [0, months, 0, 0, 0, 0, 0, 0, 0, 0];
        assert_fields(
            &format!("{early}.until({late}, {options})"),
            expect(forward),
        );
        assert_fields(
            &format!("{late}.until({early}, {options})"),
            expect(backward),
        );
    }
}

/// A `roundingIncrement` so large that the bracket needs a date millions of
/// years out is a `RangeError` on every calendar — never an arithmetic
/// overflow panic on the way to discovering it.
#[test]
fn a_huge_rounding_increment_is_a_range_error_on_every_calendar() {
    let cases = [
        ("iso8601", "years"),
        ("iso8601", "months"),
        ("iso8601", "weeks"),
        ("gregory", "months"),
        ("hebrew", "years"),
        ("chinese", "months"),
        ("islamic", "weeks"),
    ];
    for (calendar, unit) in cases {
        for method in ["until", "since"] {
            assert_range_error(&format!(
                r#"new Temporal.PlainDateTime(2020, 1, 1, 0, 0, 0, 0, 0, 0, "{calendar}").{method}(
                     new Temporal.PlainDateTime(2021, 1, 1, 0, 0, 0, 0, 0, 0, "{calendar}"),
                     {{ smallestUnit: "{unit}", roundingIncrement: 1000000000 }})"#
            ));
            assert_range_error(&format!(
                r#"new Temporal.PlainDate(2020, 1, 1, "{calendar}").{method}(
                     new Temporal.PlainDate(2021, 1, 1, "{calendar}"),
                     {{ smallestUnit: "{unit}", roundingIncrement: 1000000000 }})"#
            ));
        }
    }
}

/// `DifferenceTemporalPlainDate` is `DifferenceTemporalPlainDateTime` at
/// midnight, so the two receivers must agree on every calendar, direction,
/// unit pair and rounding mode — including the errors they raise.
#[test]
fn a_plain_date_and_a_midnight_plain_date_time_agree() {
    assert_true(
        r#"(function() {
          const spans = [["2020-01-31", "2021-03-15"], ["2019-12-25", "2020-02-29"], ["2020-03-01", "2020-03-01"]];
          const pairs = [["years", "months"], ["years", "days"], ["months", "weeks"],
                         ["months", "months"], ["weeks", "days"], ["days", "days"]];
          const modes = ["trunc", "expand", "ceil", "floor", "halfExpand", "halfEven"];
          const show = (run) => { try { const d = run(); return d.toString(); } catch (e) { return e.name; } };
          for (const calendar of ["iso8601", "gregory", "hebrew", "chinese", "islamic-civil", "japanese"]) {
            for (const [a, b] of spans) {
              const date = [Temporal.PlainDate.from(a + "[u-ca=" + calendar + "]"),
                            Temporal.PlainDate.from(b + "[u-ca=" + calendar + "]")];
              const time = date.map((d) => d.toPlainDateTime());
              for (const [largestUnit, smallestUnit] of pairs) {
                for (const roundingMode of modes) {
                  for (const [i, j] of [[0, 1], [1, 0]]) {
                    for (const method of ["until", "since"]) {
                      const options = { largestUnit, smallestUnit, roundingMode, roundingIncrement: largestUnit === "years" ? 3 : 1 };
                      const one = show(() => date[i][method](date[j], options));
                      const two = show(() => time[i][method](time[j], options));
                      if (one !== two) return calendar + " " + a + " " + b + " " + [largestUnit, smallestUnit, roundingMode, i, method]
                        + ": PlainDate " + one + " vs PlainDateTime " + two;
                    }
                  }
                }
              }
            }
          }
          return true;
        })()"#,
    );
}

/// A difference near the ends of the supported range still resolves
/// (`until/rounding-near-minimum-date.js`'s shape): nothing here may throw.
#[test]
fn differences_at_the_range_limits_do_not_throw() {
    assert_true(
        r#"(function() {
          const min = new Temporal.PlainDateTime(-271821, 4, 19, 0, 0, 0, 0, 0, 1);
          const max = new Temporal.PlainDateTime(275760, 9, 13, 23, 59, 59, 999, 999, 999);
          return min.until(max, { largestUnit: "years" }).sign === 1
              && max.since(min, { largestUnit: "years" }).sign === 1
              && min.until(max, { largestUnit: "hours" }).sign === 1;
        })()"#,
    );
}

/// Identical date-times are exactly zero whatever the options (the
/// specification returns before any rounding).
#[test]
fn identical_date_times_are_a_zero_duration() {
    assert_true(
        r#"(function() {
          const dt = new Temporal.PlainDateTime(2020, 3, 4, 5, 6, 7, 8, 9, 10);
          return ["years", "months", "weeks", "days", "hours", "nanoseconds"].every(
            (unit) => dt.until(dt, { largestUnit: unit, smallestUnit: "nanoseconds" }).blank
                   && dt.since(dt, { largestUnit: unit }).blank);
        })()"#,
    );
}

/// `PlainDate/prototype/{until,since}/throws-if-rounded-date-outside-valid-iso-range.js`:
/// rounding to a bracket that needs a date millions of years out is a
/// `RangeError` for a `PlainDate` too, not a silent zero duration.
#[test]
fn a_plain_date_rounded_bracket_outside_the_valid_range_throws() {
    for method in ["until", "since"] {
        for unit in ["months", "years", "weeks"] {
            assert_range_error(&format!(
                r#"new Temporal.PlainDate(1970, 1, 1).{method}(new Temporal.PlainDate(1971, 1, 1),
                     {{ roundingIncrement: 100000000, smallestUnit: "{unit}" }})"#
            ));
        }
    }
    // A day increment is arithmetic on the duration alone, so it is not a bracket.
    assert_fields(
        r#"new Temporal.PlainDate(1970, 1, 1).until(new Temporal.PlainDate(1971, 1, 1),
             { roundingIncrement: 100000000, smallestUnit: "days" })"#,
        [0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    );
}

/// `ComputeNudgeWindow` truncates only the *months* remainder to the
/// increment, keeping `years` fixed: 1 year 3 months to a 5-month increment
/// brackets `[1y 0m, 1y 5m]`, and 3 months is 90 of the bracket's 151 days.
/// Flattening to 15 months first gave `1y 3m` for every mode.
#[test]
fn a_plain_date_month_increment_rounds_the_months_remainder() {
    let early = "new Temporal.PlainDate(2020, 1, 1)";
    let late = "new Temporal.PlainDate(2021, 4, 1)";
    for (mode, months) in [("trunc", 0), ("floor", 0), ("halfExpand", 5), ("ceil", 5)] {
        assert_fields(
            &format!(
                r#"{early}.until({late}, {{ largestUnit: "years", smallestUnit: "months",
                     roundingIncrement: 5, roundingMode: "{mode}" }})"#
            ),
            [1, months, 0, 0, 0, 0, 0, 0, 0, 0],
        );
    }
    // The same rounding for a `PlainDateTime` at midnight agrees.
    assert_fields(
        r#"new Temporal.PlainDateTime(2020, 1, 1).until(new Temporal.PlainDateTime(2021, 4, 1),
             { largestUnit: "years", smallestUnit: "months", roundingIncrement: 5,
               roundingMode: "halfExpand" })"#,
        [1, 5, 0, 0, 0, 0, 0, 0, 0, 0],
    );
}

/// A day increment on a `PlainDate` rounds whole days.
#[test]
fn a_plain_date_day_increment_rounds_whole_days() {
    let early = "new Temporal.PlainDate(2020, 1, 1)";
    assert_fields(
        &format!(
            r#"{early}.until(new Temporal.PlainDate(2020, 1, 20),
                 {{ smallestUnit: "days", roundingIncrement: 7, roundingMode: "ceil" }})"#
        ),
        [0, 0, 0, 21, 0, 0, 0, 0, 0, 0],
    );
    // 2 months (60 days) is exactly two 30-day steps.
    assert_fields(
        &format!(
            r#"{early}.until(new Temporal.PlainDate(2020, 3, 1),
                 {{ smallestUnit: "days", roundingIncrement: 30, roundingMode: "expand" }})"#
        ),
        [0, 0, 0, 60, 0, 0, 0, 0, 0, 0],
    );
}

/// Date arithmetic must never overflow a host integer on its way to reporting
/// an out-of-range date (`epoch::nanoseconds_since_epoch` used to overflow an
/// `i64` near two-billion-year results): each of these is an ordinary
/// `RangeError`.
#[test]
fn out_of_range_date_arithmetic_is_a_range_error_not_an_overflow() {
    for source in [
        r#"Temporal.PlainDate.from("2020-01-01").add({ years: 2000000000 })"#,
        r#"Temporal.PlainDate.from("2020-01-01").subtract({ years: 2000000000 })"#,
        r#"Temporal.PlainDateTime.from("2020-01-01T00:00").add({ years: 2000000000 })"#,
        r#"Temporal.PlainDateTime.from("2020-01-01T00:00").subtract({ years: 2000000000 })"#,
        r#"new Temporal.PlainDate(2020, 1, 1).until(new Temporal.PlainDate(2021, 1, 1),
             { smallestUnit: "years", roundingIncrement: 1000000000 })"#,
        r#"new Temporal.PlainDateTime(2020, 1, 1).until(new Temporal.PlainDateTime(2021, 1, 1),
             { smallestUnit: "years", roundingIncrement: 1000000000 })"#,
    ] {
        assert_range_error(source);
    }
}
