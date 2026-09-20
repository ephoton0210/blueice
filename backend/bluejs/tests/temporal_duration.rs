// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `Temporal.Duration`'s arithmetic surface (Phase 26 Stage 1 Track B, plus
//! its own `relativeTo`-dependent follow-up and that follow-up's own
//! named-IANA-zone gap-closure pass).
//!
//! Every expectation here is taken from a named fixture in the pinned
//! Test262 corpus rather than from memory, so the same boundary this engine
//! implements is the one the corpus checks. The calendar-dependent cases the
//! corpus also covers (a non-zero `years`/`months`/`weeks`, a calendar
//! `largestUnit`/`smallestUnit`/`unit`, or a usable `relativeTo` anchor) are
//! real for every accepted `relativeTo` shape: a `PlainDate`/`PlainDateTime`
//! anchor, a `ZonedDateTime` anchor in `UTC`, a fixed offset, or now a real
//! named IANA zone (`zoned_date_time.rs`'s real transition data, day-length-
//! aware rounding for `smallestUnit` finer than `day`, and real epoch-
//! nanosecond bracketing for `day`/`week`/`month`/`year`), a date-only or
//! zoned ISO string, or a property bag (see
//! `relative_to_is_accepted_only_where_it_cannot_change_the_answer`'s own
//! updated doc comment below for exactly which). Calendar-unit arithmetic
//! with no `relativeTo`, or with a `relativeTo` shape the specification
//! itself never accepts (a `PlainYearMonth`/`PlainMonthDay` anchor, a bare
//! `Z`-designated string naming no real zone), is asserted here only where
//! the specification requires a throw anyway.

use blueice_bluejs::{compile, parse, RuntimeError, Value, Vm};

fn evaluate(source: &str) -> Result<Value, RuntimeError> {
    Vm::default().execute(&compile(&parse(source).unwrap()).unwrap())
}

fn assert_true(source: &str) {
    assert_eq!(evaluate(source), Ok(Value::Bool(true)), "{source}");
}

/// `built-ins/Temporal/Duration/prototype/sign/*` and `.../blank/*`: both are
/// accessors on the prototype, not methods.
#[test]
fn sign_and_blank_are_prototype_getters() {
    assert_true(
        r#"
        let descriptor = Object.getOwnPropertyDescriptor(Temporal.Duration.prototype, "sign");
        let blank = Object.getOwnPropertyDescriptor(Temporal.Duration.prototype, "blank");
        typeof descriptor.get === "function" && descriptor.set === undefined
            && descriptor.enumerable === false && descriptor.configurable === true
            && typeof blank.get === "function" && blank.set === undefined
    "#,
    );
    assert_true(
        r#"
        new Temporal.Duration(1, 2, 3, 4, 5, 6, 7, 8, 9, 10).sign === 1
            && new Temporal.Duration(0, 0, 0, 0, 0, 0, 0, 0, 0, 1).sign === 1
            && new Temporal.Duration(-1, -2, -3, -4, -5, -6, -7, -8, -9, -10).sign === -1
            && new Temporal.Duration(0, 0, 0, 0, 0, 0, 0, 0, 0, -1).sign === -1
            && new Temporal.Duration().sign === 0
    "#,
    );
    assert_true(
        r#"
        Temporal.Duration.from("P3DT1H").blank === false
            && Temporal.Duration.from("-PT2H20M30S").blank === false
            && Temporal.Duration.from("PT0S").blank === true
            && new Temporal.Duration().blank === true
    "#,
    );
    for source in [
        r#"Object.getOwnPropertyDescriptor(Temporal.Duration.prototype, "blank").get.call({})"#,
        r#"Object.getOwnPropertyDescriptor(Temporal.Duration.prototype, "sign").get.call(1)"#,
    ] {
        assert!(
            matches!(evaluate(source), Err(RuntimeError::TypeError(_))),
            "{source}"
        );
    }
}

/// `.../prototype/with/{all-positive,partial-positive,sign-replace}.js`.
#[test]
fn with_overrides_only_the_named_fields() {
    assert_true(
        r#"
        function fields(duration) {
            return [duration.years, duration.months, duration.weeks, duration.days,
                    duration.hours, duration.minutes, duration.seconds,
                    duration.milliseconds, duration.microseconds, duration.nanoseconds]
                .join(",");
        }
        let argAllPositive = {
            years: 9, months: 8, weeks: 7, days: 6, hours: 5,
            minutes: 4, seconds: 3, milliseconds: 2, microseconds: 1, nanoseconds: 10
        };
        let replaced = "9,8,7,6,5,4,3,2,1,10";
        fields(new Temporal.Duration().with(argAllPositive)) === replaced
            && fields(new Temporal.Duration(1, 2, 3, 4, 5, 6, 7, 8, 9, 10).with(argAllPositive)) === replaced
            && fields(new Temporal.Duration(-1, -2, -3, -4, -5, -6, -7, -8, -9, -10).with(argAllPositive)) === replaced
            && fields(new Temporal.Duration(1, 2, 3, 4, 5, 6, 7, 8, 9, 10).with({ years: 9, hours: 5 }))
                === "9,2,3,4,5,6,7,8,9,10"
            && fields(new Temporal.Duration().with({ microseconds: 987, nanoseconds: 123 }))
                === "0,0,0,0,0,0,0,0,987,123"
            && fields(Temporal.Duration.from({ years: 5, days: 1 }).with({ years: -1, days: 0, minutes: -1 }))
                === "-1,0,0,0,0,-1,0,0,0,0"
    "#,
    );
}

/// `.../prototype/with/{argument-not-object,argument-invalid-property,
/// argument-singular-properties,argument-mixed-sign,sign-conflict-throws-rangeerror}.js`.
#[test]
fn with_rejects_unusable_arguments() {
    for source in [
        "new Temporal.Duration(0, 0, 0, 1).with(undefined)",
        "new Temporal.Duration(0, 0, 0, 1).with(null)",
        r#"new Temporal.Duration(0, 0, 0, 1).with("P1D")"#,
        "new Temporal.Duration(0, 0, 0, 1).with(7)",
        "new Temporal.Duration(0, 0, 0, 1).with({})",
        "new Temporal.Duration(0, 0, 0, 1).with([])",
        "new Temporal.Duration(0, 0, 0, 1).with({ nonsense: true })",
        "new Temporal.Duration(0, 0, 0, 1).with({ sign: 1 })",
        "new Temporal.Duration(0, 0, 0, 1).with({ day: 4 })",
        "new Temporal.Duration(0, 0, 0, 1).with({ nanosecond: 10 })",
    ] {
        assert!(
            matches!(evaluate(source), Err(RuntimeError::TypeError(_))),
            "{source}"
        );
    }
    for source in [
        "new Temporal.Duration(0, 0, 0, 1).with({ hours: 1, minutes: -30 })",
        "new Temporal.Duration(1, 2, 3, 4, 5, 6, 7, 8, 9, 10).with({ days: -1 })",
        "new Temporal.Duration(-1, -2, -3, -4, -5, -6, -7, -8, -9, -10).with({ years: 1 })",
        "new Temporal.Duration(0, 0, 0, 1).with({ days: 1.5 })",
    ] {
        assert!(
            matches!(evaluate(source), Err(RuntimeError::RangeError(_))),
            "{source}"
        );
    }
}

/// `.../prototype/{negated,abs}/basic.js`.
#[test]
fn negated_and_abs_map_every_field() {
    assert_true(
        r#"
        function fields(duration) {
            return [duration.years, duration.months, duration.weeks, duration.days,
                    duration.hours, duration.minutes, duration.seconds,
                    duration.milliseconds, duration.microseconds, duration.nanoseconds]
                .join(",");
        }
        fields(new Temporal.Duration(1, 2, 3, 4, 5, 6, 7, 8, 9, 10).negated())
                === "-1,-2,-3,-4,-5,-6,-7,-8,-9,-10"
            && fields(new Temporal.Duration(-1, 0, -3, 0, -5, 0, -7, 0, -9, 0).negated())
                === "1,0,3,0,5,0,7,0,9,0"
            && fields(new Temporal.Duration().negated()) === "0,0,0,0,0,0,0,0,0,0"
            && fields(new Temporal.Duration(-1, -2, -3, -4, -5, -6, -7, -8, -9, -10).abs())
                === "1,2,3,4,5,6,7,8,9,10"
            && fields(new Temporal.Duration(1, 0, 3, 0, 5, 0, 7, 0, 9, 0).abs())
                === "1,0,3,0,5,0,7,0,9,0"
    "#,
    );
}

/// `.../prototype/add/{basic,balance-negative-result,balance-negative-time-units,
/// argument-string,blank-duration}.js`, `.../subtract/basic.js`.
#[test]
fn add_and_subtract_balance_at_twenty_four_hour_days() {
    assert_true(
        r#"
        function fields(duration) {
            return [duration.years, duration.months, duration.weeks, duration.days,
                    duration.hours, duration.minutes, duration.seconds,
                    duration.milliseconds, duration.microseconds, duration.nanoseconds]
                .join(",");
        }
        let duration1 = Temporal.Duration.from({ days: 1, minutes: 5 });
        let duration2 = Temporal.Duration.from("P50DT50H50M50.500500500S");
        let duration3 = Temporal.Duration.from({ hours: -1, seconds: -60 });
        let duration4 = Temporal.Duration.from({ hours: -1, seconds: -3721 });
        let hoursMinus60 = new Temporal.Duration(0, 0, 0, 0, -60);
        let one = new Temporal.Duration(0, 0, 0, 0, 1, 1, 1, 1, 1, 1);
        fields(duration1.add({ days: 2, minutes: 5 })) === "0,0,0,3,0,10,0,0,0,0"
            && fields(duration1.add({ hours: 12, seconds: 30 })) === "0,0,0,1,12,5,30,0,0,0"
            && fields(duration2.add(duration2)) === "0,0,0,104,5,41,41,1,1,0"
            && fields(duration3.add({ minutes: 122 })) === "0,0,0,0,1,1,0,0,0,0"
            && fields(duration4.add({ minutes: 61, nanoseconds: 3722000000001 }))
                === "0,0,0,0,0,1,1,0,0,1"
            && fields(hoursMinus60.add(new Temporal.Duration(0, 0, 0, -1))) === "0,0,0,-3,-12,0,0,0,0,0"
            && fields(one.add(new Temporal.Duration(0, 0, 0, 0, 0, 0, 0, 0, 0, -2)))
                === "0,0,0,0,1,1,1,1,0,999"
            && fields(one.add(new Temporal.Duration(0, 0, 0, 0, -2)))
                === "0,0,0,0,0,-58,-58,-998,-998,-999"
            && fields(duration1.add("P2DT5M")) === "0,0,0,3,0,10,0,0,0,0"
            && fields(duration1.add({ month: 1, days: 1 })) === "0,0,0,2,0,5,0,0,0,0"
            && fields(new Temporal.Duration().add(new Temporal.Duration())) === "0,0,0,0,0,0,0,0,0,0"
            && fields(Temporal.Duration.from("P3DT10M").subtract({ days: 2, minutes: 5 }))
                === "0,0,0,1,0,5,0,0,0,0"
            && fields(Temporal.Duration.from("P1DT12H5M30S").subtract({ hours: 12, seconds: 30 }))
                === "0,0,0,1,0,5,0,0,0,0"
    "#,
    );
}

/// `.../prototype/add/{no-calendar-units,argument-not-object,argument-string}.js`.
#[test]
fn add_rejects_calendar_units_without_a_relative_anchor() {
    for source in [
        "new Temporal.Duration(1).add(new Temporal.Duration())",
        "new Temporal.Duration(0, 1).add(new Temporal.Duration())",
        "new Temporal.Duration(0, 0, 1).add(new Temporal.Duration())",
        "new Temporal.Duration(0, 0, 0, 1).add(new Temporal.Duration(1))",
        "new Temporal.Duration(0, 0, 0, 1).add({ months: 1 })",
        r#"new Temporal.Duration(0, 0, 0, 1).add("P1W")"#,
        r#"new Temporal.Duration(0, 0, 0, 1).add("")"#,
        r#"Temporal.Duration.from({ days: 1 }).add("2DT5M")"#,
    ] {
        assert!(
            matches!(evaluate(source), Err(RuntimeError::RangeError(_))),
            "{source}"
        );
    }
    for source in [
        "new Temporal.Duration(0, 0, 0, 1).add(undefined)",
        "new Temporal.Duration(0, 0, 0, 1).add(7)",
        "new Temporal.Duration(0, 0, 0, 1).add([])",
    ] {
        assert!(
            matches!(evaluate(source), Err(RuntimeError::TypeError(_))),
            "{source}"
        );
    }
}

/// `.../prototype/round/{relativeto-not-required-to-round-non-calendar-units,
/// days-24-hours,succeeds-with-largest-unit-auto,largestunit-smallestunit-default,
/// round-negative-result,balance-negative-result,balance-subseconds,
/// round-cross-unit-boundary,durations-do-not-balance-beyond-largest-unit,
/// rounding-is-noop,roundingincrement-days-large,smallestunit-string-shorthand-string}.js`.
#[test]
fn round_works_without_a_relative_anchor_for_day_and_smaller_units() {
    assert_true(
        r#"
        function fields(duration) {
            return [duration.years, duration.months, duration.weeks, duration.days,
                    duration.hours, duration.minutes, duration.seconds,
                    duration.milliseconds, duration.microseconds, duration.nanoseconds]
                .join(",");
        }
        let d2 = new Temporal.Duration(0, 0, 0, 5, 5, 5, 5, 5, 5, 5);
        fields(d2.round("days")) === "0,0,0,5,0,0,0,0,0,0"
            && fields(d2.round("hours")) === "0,0,0,5,5,0,0,0,0,0"
            && fields(d2.round({ smallestUnit: "minutes" })) === "0,0,0,5,5,5,0,0,0,0"
            && fields(d2.round({ smallestUnit: "microseconds" })) === "0,0,0,5,5,5,5,5,5,0"
            && fields(new Temporal.Duration(0, 0, 0, 0, 25).round({ largestUnit: "days" }))
                === "0,0,0,1,1,0,0,0,0,0"
            && fields(new Temporal.Duration(0, 0, 0, 0, 25).round({ largestUnit: "auto" }))
                === "0,0,0,0,25,0,0,0,0,0"
            && fields(Temporal.Duration.from({ seconds: 86399 }).round({ smallestUnit: "days" }))
                === "0,0,0,1,0,0,0,0,0,0"
            && fields(Temporal.Duration.from({ nanoseconds: 999999999 }).round({ smallestUnit: "seconds" }))
                === "0,0,0,0,0,0,1,0,0,0"
            && fields(new Temporal.Duration(0, 0, 0, 0, -60).round({ smallestUnit: "days" }))
                === "0,0,0,-3,0,0,0,0,0,0"
            && fields(new Temporal.Duration(0, 0, 0, 0, -60).round({ largestUnit: "days" }))
                === "0,0,0,-2,-12,0,0,0,0,0"
            && fields(new Temporal.Duration(0, 0, 0, 0, 0, 0, 0, 999, 999999, 999999999)
                    .round({ largestUnit: "seconds" })) === "0,0,0,0,0,0,2,998,998,999"
            && fields(new Temporal.Duration(0, 0, 0, 0, 1, 59, 59, 900)
                    .round({ smallestUnit: "seconds", roundingMode: "expand" }))
                === "0,0,0,0,2,0,0,0,0,0"
            && fields(new Temporal.Duration(0, 0, 0, 0, 185).round({ smallestUnit: "seconds" }))
                === "0,0,0,0,185,0,0,0,0,0"
            && fields(new Temporal.Duration(0, 0, 0, 0, 23, 59, 59, 999, 999, 997)
                    .round({ largestUnit: "hours" })) === "0,0,0,0,23,59,59,999,999,997"
            && fields(new Temporal.Duration(0, 0, 0, 0, 0, 0, 9007199254, 740, 991, 0)
                    .round({ smallestUnit: "days", roundingIncrement: 1e7 }))
                === "0,0,0,0,0,0,0,0,0,0"
            && fields(new Temporal.Duration(0, 0, 0, 1)
                    .round({ smallestUnit: "days", roundingIncrement: 1e8 - 1, roundingMode: "ceil" }))
                === "0,0,0,99999999,0,0,0,0,0,0"
            && fields(new Temporal.Duration(0, 0, 0, 4, 5, 6, 7, 987, 654, 321).round("day"))
                === fields(new Temporal.Duration(0, 0, 0, 4, 5, 6, 7, 987, 654, 321)
                    .round({ smallestUnit: "day" }))
    "#,
    );
}

/// `.../prototype/round/{options-wrong-type,throws-if-neither-largestUnit-nor-
/// smallestUnit-is-given,relativeto-required-to-round-calendar-units,
/// relativeto-required-for-rounding-durations-with-calendar-units,
/// relativeto-undefined-throw-on-calendar-units,largestunit-smallestunit-mismatch,
/// invalid-increments,roundingincrement-out-of-range,roundto-invalid-string,
/// relativeto-wrong-type}.js`.
#[test]
fn round_rejects_calendar_units_and_invalid_options() {
    for source in [
        "new Temporal.Duration(0, 0, 0, 0, 1).round()",
        "new Temporal.Duration(0, 0, 0, 0, 1).round(undefined)",
        "new Temporal.Duration(0, 0, 0, 0, 1).round(null)",
        "new Temporal.Duration(0, 0, 0, 0, 1).round(1)",
        "new Temporal.Duration(0, 0, 0, 0, 1).round(true)",
        r#"new Temporal.Duration(1, 0, 0, 0, 24).round({ largestUnit: "years", relativeTo: 1 })"#,
        r#"new Temporal.Duration(1, 0, 0, 0, 24).round({ largestUnit: "years", relativeTo: {} })"#,
        r#"new Temporal.Duration(1, 0, 0, 0, 24).round({ largestUnit: "years", relativeTo: Temporal.PlainDate })"#,
    ] {
        assert!(
            matches!(evaluate(source), Err(RuntimeError::TypeError(_))),
            "{source}"
        );
    }
    for source in [
        "new Temporal.Duration(5, 5, 5, 5, 5, 5, 5, 5, 5, 5).round({})",
        "new Temporal.Duration(0, 0, 0, 0, 1).round({})",
        r#"new Temporal.Duration(0, 0, 0, 0, 1).round({ roundingMode: "ceil" })"#,
        r#"new Temporal.Duration(0, 0, 0, 5, 5, 5, 5, 5, 5, 5).round("years")"#,
        r#"new Temporal.Duration(0, 0, 0, 5, 5, 5, 5, 5, 5, 5).round({ smallestUnit: "weeks" })"#,
        r#"new Temporal.Duration(0, 0, 1).round({ largestUnit: "days" })"#,
        r#"new Temporal.Duration(1).round({ largestUnit: "days" })"#,
        r#"new Temporal.Duration(0, 0, 0, 1).round({ largestUnit: "weeks" })"#,
        r#"new Temporal.Duration(5, 5, 5, 5, 5, 5, 5, 5, 5, 5).round({ largestUnit: "hours" })"#,
        r#"new Temporal.Duration(0, 0, 0, 0, 5).round({ largestUnit: "minutes", smallestUnit: "hours" })"#,
        r#"new Temporal.Duration(0, 0, 0, 0, 5).round({ smallestUnit: "hours", roundingIncrement: 11 })"#,
        r#"new Temporal.Duration(0, 0, 0, 0, 5).round({ smallestUnit: "hours", roundingIncrement: 24 })"#,
        r#"new Temporal.Duration(0, 0, 0, 0, 5).round({ smallestUnit: "minutes", roundingIncrement: 29 })"#,
        r#"new Temporal.Duration(0, 0, 0, 0, 5).round({ smallestUnit: "nanoseconds", roundingIncrement: 1000 })"#,
        r#"new Temporal.Duration(0, 0, 0, 0, 5).round({ smallestUnit: "nanoseconds", roundingIncrement: 0 })"#,
        r#"new Temporal.Duration(0, 0, 0, 0, 5).round({ smallestUnit: "nonsense" })"#,
        r#"new Temporal.Duration(0, 0, 0, 0, 5).round("nonsense")"#,
        r#"new Temporal.Duration(0, 0, 0, 0, 5).round({ smallestUnit: "hours", roundingMode: "nonsense" })"#,
        r#"new Temporal.Duration(1, 0, 0, 0, 24).round({ largestUnit: "years", relativeTo: "" })"#,
    ] {
        assert!(
            matches!(evaluate(source), Err(RuntimeError::RangeError(_))),
            "{source}"
        );
    }
}

/// Regression for a duration whose calendar-month field is valid by itself
/// but carries the `relativeTo` date outside Temporal's supported range while
/// `round` resolves the calendar arithmetic. This must be an ordinary
/// `RangeError`, never an overflowing host-integer panic.
#[test]
fn round_reports_out_of_range_calendar_arithmetic_as_a_range_error() {
    assert!(matches!(
        evaluate(
            r#"new Temporal.Duration(0, 4294967295).round({ smallestUnit: "day", relativeTo: "2024-01-01" })"#
        ),
        Err(RuntimeError::RangeError(_))
    ));
}

/// `.../prototype/round/string-shorthand-no-object-prototype-pollution.js`: the
/// string form must not look up any other option on `Object.prototype`.
#[test]
fn round_and_total_string_shorthands_read_no_other_options() {
    assert_true(
        r#"
        let looked = [];
        for (let name of ["relativeTo", "roundingMode", "roundingIncrement", "largestUnit", "unit", "smallestUnit"]) {
            Object.defineProperty(Object.prototype, name, {
                get() { looked.push(name); return undefined; },
                configurable: true
            });
        }
        let rounded = new Temporal.Duration(0, 0, 0, 0, 1, 30).round("hour");
        let total = new Temporal.Duration(0, 0, 0, 0, 1, 30).total("hour");
        for (let name of ["relativeTo", "roundingMode", "roundingIncrement", "largestUnit", "unit", "smallestUnit"]) {
            delete Object.prototype[name];
        }
        looked.length === 0 && rounded.hours === 2 && total === 1.5
    "#,
    );
}

/// `.../prototype/total/{total-of-each-unit,relativeto-undefined-throw-on-
/// calendar-units,rounds-calendar-units-in-durations-without-calendar-units,
/// throws-if-unit-property-missing,throws-on-disallowed-or-invalid-unit,
/// unit-string-shorthand-string}.js`.
#[test]
fn total_returns_one_exact_number_per_unit() {
    assert_true(
        r#"
        let duration = new Temporal.Duration(0, 0, 0, 5, 5, 5, 5, 5, 5, 5);
        let dayMilliseconds = 24 * 3600 * 1000;
        let fullDays = 5;
        let fullMilliseconds = fullDays * dayMilliseconds + 5 * 3600000 + 5 * 60000 + 5000 + 5;
        let partialDayMilliseconds = fullMilliseconds - fullDays * dayMilliseconds + 0.005005;
        duration.total("days") === fullDays + partialDayMilliseconds / dayMilliseconds
            && duration.total("hours") === fullDays * 24 + partialDayMilliseconds / 3600000
            && duration.total("minutes") === fullDays * 24 * 60 + partialDayMilliseconds / 60000
            && duration.total("seconds") === fullDays * 24 * 60 * 60 + partialDayMilliseconds / 1000
            && duration.total({ unit: "milliseconds" }) === fullMilliseconds + 0.005005
            && duration.total({ unit: "microseconds" }) === fullMilliseconds * 1000 + 5.005
            && duration.total({ unit: "nanoseconds" }) === fullMilliseconds * 1000000 + 5005
            && new Temporal.Duration(0, 0, 0, 1).total("days") === 1
            && new Temporal.Duration().total("nanoseconds") === 0
            && new Temporal.Duration(0, 0, 0, 0, -1).total("minutes") === -60
    "#,
    );
    for source in [
        r#"new Temporal.Duration(5, 5, 5, 5, 5, 5, 5, 5, 5, 5).total({ unit: "era" })"#,
        r#"new Temporal.Duration(5, 5, 5, 5, 5, 5, 5, 5, 5, 5).total("nonsense")"#,
        "new Temporal.Duration(5, 5, 5, 5, 5, 5, 5, 5, 5, 5).total({})",
        r#"new Temporal.Duration(5, 5, 5, 5, 5, 5, 5, 5, 5, 5).total({ roundingMode: "ceil" })"#,
        r#"new Temporal.Duration(0, 0, 0, 5, 5, 5, 5, 5, 5, 5).total("years")"#,
        r#"new Temporal.Duration(0, 0, 0, 5, 5, 5, 5, 5, 5, 5).total({ unit: "weeks" })"#,
        r#"new Temporal.Duration(0, 0, 1).total("days")"#,
        r#"new Temporal.Duration(1).total("days")"#,
    ] {
        assert!(
            matches!(evaluate(source), Err(RuntimeError::RangeError(_))),
            "{source}"
        );
    }
    for source in [
        "new Temporal.Duration(0, 0, 0, 1).total()",
        r#"new Temporal.Duration(0, 0, 0, 1).total({ unit: "days", relativeTo: 20200101 })"#,
        r#"new Temporal.Duration(0, 0, 0, 1).total({ unit: "days", relativeTo: null })"#,
        r#"new Temporal.Duration(0, 0, 0, 1).total({ unit: "days", relativeTo: true })"#,
    ] {
        assert!(
            matches!(evaluate(source), Err(RuntimeError::TypeError(_))),
            "{source}"
        );
    }
}

/// `.../compare/{basic,instances-identical,options-wrong-type}.js`.
#[test]
fn compare_orders_calendar_agnostic_durations() {
    assert_true(
        r#"
        let td1pos = new Temporal.Duration(0, 0, 0, 0, 5, 5, 5, 5, 5, 5);
        let td2pos = new Temporal.Duration(0, 0, 0, 0, 5, 4, 5, 5, 5, 5);
        let td1neg = new Temporal.Duration(0, 0, 0, 0, -5, -5, -5, -5, -5, -5);
        let td2neg = new Temporal.Duration(0, 0, 0, 0, -5, -4, -5, -5, -5, -5);
        let identical = new Temporal.Duration(5, 5, 5, 5, 5, 5, 5, 5, 5, 5);
        Temporal.Duration.compare(td1pos, td1pos) === 0
            && Temporal.Duration.compare(td2pos, td1pos) === -1
            && Temporal.Duration.compare(td1pos, td2pos) === 1
            && Temporal.Duration.compare(td1neg, td1neg) === 0
            && Temporal.Duration.compare(td2neg, td1neg) === 1
            && Temporal.Duration.compare(td1neg, td2neg) === -1
            && Temporal.Duration.compare(td1neg, td2pos) === -1
            && Temporal.Duration.compare(td1pos, td2neg) === 1
            && Temporal.Duration.compare(identical, identical) === 0
            && Temporal.Duration.compare({ days: 1 }, { hours: 24 }) === 0
            && Temporal.Duration.compare("P1D", "PT23H") === 1
            && Temporal.Duration.compare.length === 2
    "#,
    );
    assert!(matches!(
        evaluate(
            "Temporal.Duration.compare(new Temporal.Duration(5, 5), new Temporal.Duration(5, 6))"
        ),
        Err(RuntimeError::RangeError(_))
    ));
    assert!(matches!(
        evaluate(
            "Temporal.Duration.compare(new Temporal.Duration(), new Temporal.Duration(), null)"
        ),
        Err(RuntimeError::TypeError(_))
    ));
}

/// `.../prototype/toString/{balance,balance-subseconds,negative-components,
/// precision,blank-duration-precision,large-with-small-units,max-value,
/// no-precision-loss,fractionalseconddigits-auto,smallestunit-valid-units,
/// roundingmode-ceil,round-cross-unit-boundary}.js`.
#[test]
fn to_string_serializes_and_rounds_the_time_part() {
    assert_true(
        r#"
        Temporal.Duration.from({ milliseconds: 3500 }).toString() === "PT3.5S"
            && Temporal.Duration.from({ microseconds: 3500 }).toString() === "PT0.0035S"
            && Temporal.Duration.from({ nanoseconds: 3500 }).toString() === "PT0.0000035S"
            && new Temporal.Duration(0, 0, 0, 0, 0, 0, 0, 1111, 1111, 1111).toString() === "PT1.112112111S"
            && Temporal.Duration.from({ seconds: 120, milliseconds: 3500 }).toString() === "PT123.5S"
            && new Temporal.Duration(0, 0, 0, 0, 0, 0, 0, 999, 999999, 999999999).toString()
                === "PT2.998998999S"
            && new Temporal.Duration(0, 0, 0, 0, 0, 0, 0, -999, -999999, -999999999).toString()
                === "-PT2.998998999S"
            && new Temporal.Duration(-1, -1, -1, -1, -1, -1, -1, -1, -1, -1).toString()
                === "-P1Y1M1W1DT1H1M1.001001001S"
            && Temporal.Duration.from({ weeks: -1, days: -1 }).toString() === "-P1W1D"
            && Temporal.Duration.from({ milliseconds: -250 }).toString() === "-PT0.25S"
            && new Temporal.Duration().toString() === "PT0S"
            && new Temporal.Duration(1, 0, 0, 0, 0, 0, 0, 0, 0, 1).toString() === "P1YT0.000000001S"
            && new Temporal.Duration(0, 0, 0, 1, 0, 0, 1).toString() === "P1DT1S"
            && new Temporal.Duration(0, 0, 0, 0, 1, 1).toString() === "PT1H1M"
            && new Temporal.Duration(0, 0, 0, 0, 0, 0, 9007199254740991, 0, 0, 999999999).toString()
                === "PT9007199254740991.999999999S"
            && new Temporal.Duration(0, 0, 0, 0, 0, 0, 0, 9007199254740991, 2000).toString()
                === "PT9007199254740.993S"
            && new Temporal.Duration(0, 0, 0, 0, 0, 0, 0, 9007199254740991, 9007199254740991, 0).toString()
                === "PT9016206453995.731991S"
            && Temporal.Duration.from("PT0.084000159S").toString({ smallestUnit: "milliseconds" })
                === "PT0.084S"
    "#,
    );
    assert_true(
        r#"
        let blank = new Temporal.Duration();
        let duration = new Temporal.Duration(1, 2, 3, 4, 5, 6, 7, 987, 654, 321);
        let whole = new Temporal.Duration(1, 2, 3, 4, 5, 6, 7);
        let ceil = new Temporal.Duration(1, 2, 3, 4, 5, 6, 7, 123, 987, 500);
        blank.toString({ fractionalSecondDigits: "auto" }) === "PT0S"
            && blank.toString({ fractionalSecondDigits: 0 }) === "PT0S"
            && blank.toString({ fractionalSecondDigits: 2 }) === "PT0.00S"
            && blank.toString({ smallestUnit: "nanoseconds" }) === "PT0.000000000S"
            && duration.toString({ smallestUnit: "seconds" }) === "P1Y2M3W4DT5H6M7S"
            && duration.toString({ smallestUnit: "microseconds" }) === "P1Y2M3W4DT5H6M7.987654S"
            && whole.toString({ smallestUnit: "milliseconds" }) === "P1Y2M3W4DT5H6M7.000S"
            && whole.toString() === "P1Y2M3W4DT5H6M7S"
            && new Temporal.Duration(1, 2, 3, 4, 5, 6, 7, 987, 650).toString() === "P1Y2M3W4DT5H6M7.98765S"
            && ceil.toString({ smallestUnit: "microsecond", roundingMode: "ceil" }) === "P1Y2M3W4DT5H6M7.123988S"
            && ceil.toString({ fractionalSecondDigits: 6, roundingMode: "ceil" }) === "P1Y2M3W4DT5H6M7.123988S"
            && ceil.toString({ smallestUnit: "second", roundingMode: "ceil" }) === "P1Y2M3W4DT5H6M8S"
            && new Temporal.Duration(0, 0, 0, 0, 1, 59, 59, 900)
                .toString({ fractionalSecondDigits: 0, roundingMode: "expand" }) === "PT2H0S"
            && new Temporal.Duration(0, 0, 0, 0, -1, -59, -59, -900)
                .toString({ fractionalSecondDigits: 0, roundingMode: "expand" }) === "-PT2H0S"
            && new Temporal.Duration(1, 11, 0, 30, 23, 59, 59, 999, 999, 999)
                .toString({ fractionalSecondDigits: 8, roundingMode: "expand" }) === "P1Y11M31DT0.00000000S"
            && new Temporal.Duration(0, 0, 0, 0, 0, 0, 59, 900)
                .toString({ fractionalSecondDigits: 0, roundingMode: "expand" }) === "PT60S"
    "#,
    );
    for source in [
        r#"new Temporal.Duration(1, 2).toString({ smallestUnit: "hour" })"#,
        r#"new Temporal.Duration(1, 2).toString({ smallestUnit: "minute" })"#,
        r#"new Temporal.Duration(1, 2).toString({ smallestUnit: "day" })"#,
        r#"new Temporal.Duration(1, 2).toString({ smallestUnit: "era" })"#,
        "new Temporal.Duration(1, 2).toString({ fractionalSecondDigits: 10 })",
        "new Temporal.Duration(1, 2).toString({ fractionalSecondDigits: -1 })",
        r#"new Temporal.Duration(1, 2).toString({ fractionalSecondDigits: "nonsense" })"#,
        r#"new Temporal.Duration(0, 0, 0, 0, 0, 0, 9007199254740991, 1).toString({ smallestUnit: "seconds", roundingMode: "expand" })"#,
    ] {
        assert!(
            matches!(evaluate(source), Err(RuntimeError::RangeError(_))),
            "{source}"
        );
    }
}

/// `.../prototype/{toJSON/options,toLocaleString/return-string,valueOf/basic}.js`.
#[test]
fn to_json_ignores_options_and_value_of_throws() {
    assert_true(
        r#"
        let called = 0;
        let options = new Proxy({}, { get() { called += 1; } });
        let duration = new Temporal.Duration(1, 2);
        duration.toJSON(options) === "P1Y2M" && called === 0
            && typeof new Temporal.Duration().toLocaleString() === "string"
            && Temporal.Duration.prototype.toJSON.length === 0
            && Temporal.Duration.prototype.with.length === 1
            && Temporal.Duration.prototype.round.length === 1
            && Temporal.Duration.prototype.total.length === 1
            && Temporal.Duration.prototype.negated.length === 0
    "#,
    );
    for source in [
        r#"Temporal.Duration.from("P3DT1H").valueOf()"#,
        r#"Temporal.Duration.from("P3DT1H") < Temporal.Duration.from("P3DT1H")"#,
    ] {
        assert!(
            matches!(evaluate(source), Err(RuntimeError::TypeError(_))),
            "{source}"
        );
    }
}

/// `.../prototype/{with,add,round,total}/order-of-operations.js` and
/// `.../compare/order-of-operations.js`: duration-like property bags are read
/// in alphabetical field order, and option bags in alphabetical option order.
#[test]
fn property_and_option_bags_are_read_in_alphabetical_order() {
    assert_true(
        r#"
        function observer(log, label, fields) {
            let bag = {};
            for (let name of Object.keys(fields)) {
                Object.defineProperty(bag, name, {
                    get() { log.push(`${label}.${name}`); return fields[name]; },
                    enumerable: true
                });
            }
            return bag;
        }
        let expected = ["days", "hours", "microseconds", "milliseconds", "minutes",
                        "months", "nanoseconds", "seconds", "weeks", "years"];
        let all = {
            years: 0, months: 0, weeks: 0, days: 1, hours: 1, minutes: 1,
            seconds: 1, milliseconds: 1, microseconds: 1, nanoseconds: 1
        };

        let addLog = [];
        new Temporal.Duration(0, 0, 0, 1, 1, 1, 1, 1, 1, 1).add(observer(addLog, "f", all));
        let withLog = [];
        new Temporal.Duration(0, 0, 0, 1, 1, 1, 1, 1, 1, 1).with(observer(withLog, "f", all));

        let roundLog = [];
        new Temporal.Duration(0, 0, 0, 0, 2400).round(observer(roundLog, "o", {
            smallestUnit: "microseconds", largestUnit: "auto",
            roundingIncrement: 1, roundingMode: "halfExpand", relativeTo: undefined
        }));
        let totalLog = [];
        new Temporal.Duration(0, 0, 0, 0, 2400).total(observer(totalLog, "o", {
            unit: "nanoseconds", roundingMode: "halfExpand",
            roundingIncrement: 1, relativeTo: undefined
        }));
        let stringLog = [];
        new Temporal.Duration(1, 1, 1, 1, 1, 1, 1, 1, 1, 1).toString(observer(stringLog, "o", {
            fractionalSecondDigits: "auto", roundingMode: "halfExpand", smallestUnit: "millisecond"
        }));

        addLog.join(",") === expected.map(name => `f.${name}`).join(",")
            && withLog.join(",") === expected.map(name => `f.${name}`).join(",")
            && roundLog.join(",") === "o.largestUnit,o.relativeTo,o.roundingIncrement,o.roundingMode,o.smallestUnit"
            && totalLog.join(",") === "o.relativeTo,o.unit"
            && stringLog.join(",") === "o.fractionalSecondDigits,o.roundingMode,o.smallestUnit"
    "#,
    );
}

/// `.../from/argument-string.js` plus `Duration/length.js`: `Temporal.Duration`'s
/// own entry points, which `ToTemporalDuration` is now the single source of.
#[test]
fn from_accepts_property_bags_strings_and_fractional_time_units() {
    assert_true(
        r#"
        function fields(duration) {
            return [duration.years, duration.months, duration.weeks, duration.days,
                    duration.hours, duration.minutes, duration.seconds,
                    duration.milliseconds, duration.microseconds, duration.nanoseconds]
                .join(",");
        }
        fields(Temporal.Duration.from({ years: 5, days: 1 })) === "5,0,0,1,0,0,0,0,0,0"
            && fields(Temporal.Duration.from("P1D")) === "0,0,0,1,0,0,0,0,0,0"
            && fields(Temporal.Duration.from("p1y1m1dt1h1m1s")) === "1,1,0,1,1,1,1,0,0,0"
            && fields(Temporal.Duration.from("P1DT0.5M")) === "0,0,0,1,0,0,30,0,0,0"
            && fields(Temporal.Duration.from("P1DT0,5H")) === "0,0,0,1,0,30,0,0,0,0"
            && fields(Temporal.Duration.from("P1Y1M1W1DT1H1M1,12S")) === "1,1,1,1,1,1,1,120,0,0"
            && fields(Temporal.Duration.from("-P1D")) === "0,0,0,-1,0,0,0,0,0,0"
            && fields(Temporal.Duration.from(new Temporal.Duration(1, 2))) === "1,2,0,0,0,0,0,0,0,0"
            && Temporal.Duration.length === 0
    "#,
    );
    for source in [
        r#"Temporal.Duration.from("2DT5M")"#,
        r#"Temporal.Duration.from("PT0.5H30M")"#,
        r#"Temporal.Duration.from("P1.5D")"#,
        r#"Temporal.Duration.from("P1M1Y")"#,
    ] {
        assert!(
            matches!(evaluate(source), Err(RuntimeError::RangeError(_))),
            "{source}"
        );
    }
    assert!(matches!(
        evaluate("Temporal.Duration.from({})"),
        Err(RuntimeError::TypeError(_))
    ));
}

/// Branding, plus the two validation branches no other test reaches: a
/// date-category `smallestUnit` with an increment above one may not also
/// balance upwards, and `fractionalSecondDigits` must be finite.
#[test]
fn methods_require_a_duration_receiver_and_reject_the_remaining_edge_options() {
    for source in [
        "Temporal.Duration.prototype.with.call({}, { days: 1 })",
        "Temporal.Duration.prototype.negated.call(1)",
        "Temporal.Duration.prototype.abs.call(undefined)",
        "Temporal.Duration.prototype.add.call(new Temporal.Instant(0n), { days: 1 })",
        r#"Temporal.Duration.prototype.round.call(new Temporal.PlainDate(2020, 1, 1), "days")"#,
        r#"Temporal.Duration.prototype.total.call("P1D", "days")"#,
        "Temporal.Duration.prototype.toString.call({})",
        "Temporal.Duration.prototype.toLocaleString.call({})",
    ] {
        assert!(
            matches!(evaluate(source), Err(RuntimeError::TypeError(_))),
            "{source}"
        );
    }
    for source in [
        r#"new Temporal.Duration(0, 0, 0, 31).round({ largestUnit: "weeks", smallestUnit: "days", roundingIncrement: 30 })"#,
        "new Temporal.Duration(1, 2).toString({ fractionalSecondDigits: NaN })",
        "new Temporal.Duration(1, 2).toString({ fractionalSecondDigits: Infinity })",
    ] {
        assert!(
            matches!(evaluate(source), Err(RuntimeError::RangeError(_))),
            "{source}"
        );
    }
    // A non-integral fractionalSecondDigits is floored, not rejected.
    assert_true(
        r#"new Temporal.Duration(0, 0, 0, 0, 0, 0, 1, 500)
            .toString({ fractionalSecondDigits: 2.9 }) === "PT1.50S""#,
    );
}

/// `.../prototype/{add,round}/float64-representable-integer.js` and
/// `.../round/out-of-range-when-converting-from-normalized-duration.js`: a
/// balanced field is a Number, so it is rounded to the nearest double before
/// the range check rather than kept at exact nanosecond precision.
#[test]
fn balanced_fields_round_trip_through_a_double() {
    assert_true(
        r#"
        let d = new Temporal.Duration(0, 0, 0, 0, 0, 0, 0, 0, 9007199254740991, 0);
        let result = d.add({ microseconds: 9007199254740990 });
        result.microseconds === 18014398509481980
            && result.toString() === "PT18014398509.48198S"
            && Temporal.Duration.compare(result.add({ microseconds: 1 }), result) === 0
            && new Temporal.Duration(0, 0, 0, 0, 0, 0, 0, 18014398509481, 981, 0)
                .round({ largestUnit: "microseconds" }).microseconds === 18014398509481980
    "#,
    );
    // A component that only exceeds the duration range once rounded to the
    // nearest double still has to be rejected.
    for source in [
        r#"new Temporal.Duration(0, 0, 0, 0, 0, 0, 9007199254740991, 488).round({ largestUnit: "nanoseconds" })"#,
        r#"new Temporal.Duration().add({ milliseconds: 4503599627370497000, microseconds: 4503599627370495000000 })"#,
    ] {
        assert!(
            matches!(evaluate(source), Err(RuntimeError::RangeError(_))),
            "{source}"
        );
    }
}

/// Originally the Stage 1/Stage 2 boundary itself (a `relativeTo` anchor
/// honoured only where it could not change a calendar-agnostic answer,
/// everything else rejected). Updated for this phase's own `relativeTo`
/// follow-up (`development/browser_core/phase-26-ecma262-temporal/PLAN.md`'s
/// Track B entry): a calendar-unit duration with a `relativeTo` this engine
/// can actually resolve (a `PlainDate`/`PlainDateTime` anchor, a
/// `ZonedDateTime` anchor in `UTC`/a fixed offset, a date-only or zoned ISO
/// string, or a property bag) now gets a real, calendar-aware answer instead
/// of a throw — the three assertions moved out of the "rejected" list below,
/// each with its real expected value, not just "does not throw". **Updated
/// again** for this same Track B's own named-IANA-zone gap-closure pass: a
/// `ZonedDateTime`/zoned-string anchor in a *named* zone (`zoned_date_time.rs`
/// now has real transition data) is likewise a real answer now — two more
/// assertions moved out of "rejected" below, each away from a DST transition
/// so the answer is the same as a fixed-offset zone would give (`America/
/// Vancouver` observes no DST near January). Still rejected: a
/// `Temporal.Duration` itself as `relativeTo` (never a valid anchor shape), a
/// bare `Z`-designated string with no bracketed zone annotation (names no
/// real zone), and a malformed string.
#[test]
fn relative_to_is_accepted_only_where_it_cannot_change_the_answer() {
    assert_true(
        r#"
        let plain = new Temporal.PlainDate(2017, 1, 1);
        let plainDateTime = new Temporal.PlainDateTime(2017, 1, 1, 6);
        let utc = new Temporal.ZonedDateTime(0n, "UTC");
        let fixedOffset = new Temporal.ZonedDateTime(1000000000000000000n, "+04:30");
        let oneDay = new Temporal.Duration(0, 0, 0, 1);
        let hours48 = new Temporal.Duration(0, 0, 0, 0, 48);
        let blank = new Temporal.Duration();
        oneDay.total({ unit: "hours", relativeTo: plain }) === 24
            && oneDay.total({ unit: "hours", relativeTo: plainDateTime }) === 24
            && hours48.total({ unit: "days", relativeTo: utc }) === 2
            && oneDay.total({ unit: "hours", relativeTo: fixedOffset }) === 24
            && oneDay.total({ unit: "hours", relativeTo: "2017-01-01" }) === 24
            && oneDay.round({ smallestUnit: "hours", relativeTo: plain }).days === 1
            && blank.round({ smallestUnit: "years", relativeTo: plain }).blank === true
            && blank.round({ smallestUnit: "weeks", relativeTo: utc }).blank === true
            && blank.total({ unit: "months", relativeTo: plain }) === 0
            && Temporal.Duration.compare(blank, blank, { relativeTo: utc }) === 0
            // A one-year duration is exactly 365 days from 2017-01-01
            // (2017 is not a leap year) — real calendar-aware `total`,
            // previously rejected outright for any nonzero `years` field.
            && new Temporal.Duration(1).total({ unit: "days", relativeTo: plain }) === 365
            // One day out of January's 31 is an exact, real fraction of a
            // month, not the fixed-length answer a calendar-agnostic path
            // would have to reject `unit: "months"` for.
            && oneDay.total({ unit: "months", relativeTo: plain }) === 1 / 31
            // A property-bag `relativeTo` resolves through the same
            // `ToTemporalDate`-shaped field reading `PlainDate.from` uses,
            // previously rejected outright as unsupported.
            && oneDay.total({ unit: "days", relativeTo: { year: 2017, month: 1, day: 1 } }) === 1
            // A named-IANA-zone `ZonedDateTime`/string anchor, away from any
            // DST transition (`America/Vancouver` observes none near
            // January): once genuinely blocked on real transition data,
            // `zoned_date_time.rs` now supplies it, so one calendar day here
            // is exactly 24 real hours, same as a fixed-offset zone.
            && oneDay.total({ unit: "days", relativeTo: new Temporal.ZonedDateTime(0n, "America/Vancouver") }) === 1
            && oneDay.total({ unit: "days", relativeTo: "2017-01-01T00:00[America/Vancouver]" }) === 1
    "#,
    );
    // Rejected, each for its own documented reason.
    for source in [
        r#"new Temporal.Duration(0, 0, 0, 1).total({ unit: "days", relativeTo: "2017-01-01T00:00Z" })"#,
        r#"new Temporal.Duration(0, 0, 0, 1).total({ unit: "days", relativeTo: "nonsense" })"#,
    ] {
        assert!(
            matches!(evaluate(source), Err(RuntimeError::RangeError(_))),
            "{source}"
        );
    }
    {
        let source = r#"new Temporal.Duration(0, 0, 0, 1).total({ unit: "days", relativeTo: new Temporal.Duration(1) })"#;
        assert!(
            matches!(evaluate(source), Err(RuntimeError::TypeError(_))),
            "{source}"
        );
    }
}

/// `intl402/.../toLocaleString/returns-same-results-as-DurationFormat.js`:
/// ECMA-402 defines `toLocaleString` through `Intl.DurationFormat`, not as the
/// ISO string `toString` produces.
#[test]
fn to_locale_string_formats_through_intl_duration_format() {
    assert_true(
        r#"
        let durationLike = {
            years: 1, months: 2, weeks: 3, days: 4, hours: 5,
            minutes: 6, seconds: 7, milliseconds: 8, microseconds: 9, nanoseconds: 10
        };
        let duration = Temporal.Duration.from(durationLike);
        let same = true;
        for (let locale of [undefined, "en", "de"]) {
            for (let options of [undefined, { style: "long" }]) {
                let formatter = new Intl.DurationFormat(locale, options);
                if (duration.toLocaleString(locale, options) !== formatter.format(durationLike)) {
                    same = false;
                }
            }
        }
        same && duration.toLocaleString() !== duration.toString()
    "#,
    );
}
