// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Coverage for `Temporal.ZonedDateTime`'s arithmetic and comparison surface
//! (`vm/temporal/zoned.rs` and `vm/temporal/zoned_date_time.rs`): `add`,
//! `subtract`, `until`, `since`, `round`, `equals` and `compare`, including
//! DST-transition days (23 and 25 hours long), calendar-unit rounding with
//! bubbling, non-ISO calendars, range limits and option-bag validation.
//!
//! Only fixed IANA identifiers and fixed instants are used, so nothing here
//! depends on the host's own time zone or locale. Every expectation follows
//! the ECMAScript Temporal specification.

use blueice_bluejs::{compile, parse, RuntimeError, Value, Vm};

fn run(source: &str) -> Result<Value, RuntimeError> {
    Vm::default().execute(&compile(&parse(source).unwrap()).unwrap())
}

fn assert_str(source: &str, expected: &str) {
    match run(source) {
        Ok(Value::String(actual)) => assert_eq!(actual, expected, "{source}"),
        other => panic!("{source}\n  -> expected string {expected:?}, got {other:?}"),
    }
}

fn assert_true(source: &str) {
    assert_eq!(run(source), Ok(Value::Bool(true)), "{source}");
}

fn assert_range_errors(sources: &[String]) {
    for source in sources {
        match run(source) {
            Err(RuntimeError::RangeError(_)) => {}
            other => panic!("{source}\n  -> expected RangeError, got {other:?}"),
        }
    }
}

fn assert_type_errors(sources: &[String]) {
    for source in sources {
        match run(source) {
            Err(RuntimeError::TypeError(_)) => {}
            other => panic!("{source}\n  -> expected TypeError, got {other:?}"),
        }
    }
}

/// Builds `Temporal.ZonedDateTime.from(<text>)` for a string literal.
fn zdt(text: &str) -> String {
    format!(r#"Temporal.ZonedDateTime.from("{text}")"#)
}

/// `<a>.<method>(<b>, <options>).toString()`.
fn diff(a: &str, method: &str, b: &str, options: &str) -> String {
    format!("{}.{method}({}, {options}).toString()", zdt(a), zdt(b))
}

fn ny(local: &str) -> String {
    format!("{local}[America/New_York]")
}

fn utc(local: &str) -> String {
    format!("{local}[UTC]")
}

#[test]
fn add_and_subtract_exact_time_ignores_wall_clock_gaps() {
    // 01:30 EST + 1h crosses the spring-forward gap: exactly one hour later.
    assert_str(
        &format!(
            "{}.add({{ hours: 1 }}).toString()",
            zdt(&ny("2020-03-08T01:30:00"))
        ),
        "2020-03-08T03:30:00-04:00[America/New_York]",
    );
    assert_str(
        &format!(
            "{}.subtract({{ hours: 1 }}).toString()",
            zdt(&ny("2020-03-08T03:30:00"))
        ),
        "2020-03-08T01:30:00-05:00[America/New_York]",
    );
    // Fall-back: 01:30 EDT + 1h is 01:30 EST.
    assert_str(
        &format!(
            "{}.add({{ hours: 1 }}).toString()",
            zdt(&ny("2020-11-01T01:30:00-04:00"))
        ),
        "2020-11-01T01:30:00-05:00[America/New_York]",
    );
    // Sub-second parts are exact time too.
    assert_str(
        &format!(
            "{}.add({{ milliseconds: 1, microseconds: 2, nanoseconds: 3 }}).toString()",
            zdt(&utc("2020-01-01T00:00:00"))
        ),
        "2020-01-01T00:00:00.001002003+00:00[UTC]",
    );
    assert_str(
        &format!(
            "{}.subtract({{ minutes: 1, seconds: 1 }}).toString()",
            zdt(&utc("2020-01-01T00:00:00"))
        ),
        "2019-12-31T23:58:59+00:00[UTC]",
    );
}

#[test]
fn add_and_subtract_calendar_units_go_through_the_calendar_and_zone() {
    // One calendar day across a spring-forward is 23 elapsed hours.
    assert_str(
        &format!(
            "{}.add({{ days: 1 }}).toString()",
            zdt(&ny("2020-03-07T12:00:00"))
        ),
        "2020-03-08T12:00:00-04:00[America/New_York]",
    );
    // A landing time in the gap resolves with "compatible" (later).
    assert_str(
        &format!(
            "{}.add({{ days: 1 }}).toString()",
            zdt(&ny("2020-03-07T02:30:00"))
        ),
        "2020-03-08T03:30:00-04:00[America/New_York]",
    );
    assert_str(
        &format!(
            "{}.add({{ weeks: 1, days: 1 }}).toString()",
            zdt(&ny("2020-01-01T09:00:00"))
        ),
        "2020-01-09T09:00:00-05:00[America/New_York]",
    );
    assert_str(
        &format!(
            "{}.add({{ years: 1, months: 1 }}).toString()",
            zdt(&ny("2020-01-31T09:00:00"))
        ),
        "2021-02-28T09:00:00-05:00[America/New_York]",
    );
    assert_str(
        &format!(
            "{}.subtract({{ years: 1, months: 2, days: 3 }}).toString()",
            zdt(&utc("2020-03-31T00:00:00"))
        ),
        "2019-01-28T00:00:00+00:00[UTC]",
    );
    // Calendar and time parts combine: the date moves first, then the exact
    // time is added.
    assert_str(
        &format!(
            "{}.add({{ days: 1, hours: 2 }}).toString()",
            zdt(&ny("2020-03-07T02:30:00"))
        ),
        "2020-03-08T05:30:00-04:00[America/New_York]",
    );
    // Duration strings and Duration objects are accepted.
    assert_str(
        &format!(
            "{}.add(\"P1DT2H\").toString()",
            zdt(&utc("2020-01-01T00:00:00"))
        ),
        "2020-01-02T02:00:00+00:00[UTC]",
    );
    assert_str(
        &format!(
            "{}.add(new Temporal.Duration(0, 0, 0, 1)).toString()",
            zdt(&utc("2020-01-01T00:00:00"))
        ),
        "2020-01-02T00:00:00+00:00[UTC]",
    );
    // Overflow: constrain clamps the day, reject throws.
    assert_str(
        &format!(
            "{}.add({{ months: 1 }}).toString()",
            zdt(&utc("2020-01-31T00:00:00"))
        ),
        "2020-02-29T00:00:00+00:00[UTC]",
    );
    assert_str(
        &format!(
            "{}.add({{ months: 1 }}, {{ overflow: \"constrain\" }}).toString()",
            zdt(&utc("2020-01-31T00:00:00"))
        ),
        "2020-02-29T00:00:00+00:00[UTC]",
    );
    assert_range_errors(&[format!(
        "{}.add({{ months: 1 }}, {{ overflow: \"reject\" }})",
        zdt(&utc("2020-01-31T00:00:00"))
    )]);
}

#[test]
fn add_and_subtract_in_non_iso_calendars() {
    assert_str(
        &format!(
            "{}.add({{ months: 1 }}).toString()",
            zdt("2020-01-31T00:00:00[UTC][u-ca=gregory]")
        ),
        "2020-02-29T00:00:00+00:00[UTC][u-ca=gregory]",
    );
    // 2023-09-20 is 5 Tishrei 5784; one Hebrew year later is 5 Tishrei 5785.
    assert_str(
        &format!(
            "{}.add({{ years: 1 }}).monthCode",
            zdt("2023-09-20T00:00:00[UTC][u-ca=hebrew]")
        ),
        "M01",
    );
    assert_str(
        &format!(
            "{}.subtract({{ months: 1 }}).toString()",
            zdt("2020-03-15T00:00:00[UTC][u-ca=japanese]")
        ),
        "2020-02-15T00:00:00+00:00[UTC][u-ca=japanese]",
    );
    assert_str(
        &format!(
            "{}.add({{ days: 10 }}).toString()",
            zdt("2020-06-15T00:00:00[UTC][u-ca=chinese]")
        ),
        "2020-06-25T00:00:00+00:00[UTC][u-ca=chinese]",
    );
}

#[test]
fn add_and_subtract_reject_bad_arguments_and_out_of_range_results() {
    let z = zdt(&utc("2020-01-01T00:00:00"));
    assert_type_errors(&[
        format!("{z}.add()"),
        format!("{z}.add(undefined)"),
        format!("{z}.add(5)"),
        format!("{z}.add(null)"),
        format!("{z}.add({{}})"),
        format!("{z}.add({{ unrelated: 1 }})"),
        format!("{z}.subtract()"),
        format!("{z}.add({{ days: 1 }}, 5)"),
        "Temporal.ZonedDateTime.prototype.add.call({}, { days: 1 })".to_string(),
        "Temporal.ZonedDateTime.prototype.subtract.call({}, { days: 1 })".to_string(),
    ]);
    assert_range_errors(&[
        format!("{z}.add(\"garbage\")"),
        format!("{z}.add({{ days: 1, hours: -1 }})"),
        format!("{z}.add({{ days: 1.5 }})"),
        format!("{z}.add({{ days: 1 }}, {{ overflow: \"bogus\" }})"),
        format!("{z}.add({{ years: 300000 }})"),
        format!("{z}.subtract({{ years: 300000 }})"),
        // The very edges of the representable range.
        "new Temporal.ZonedDateTime(8640000000000000000000n, \"UTC\").add({ nanoseconds: 1 })"
            .to_string(),
        "new Temporal.ZonedDateTime(8640000000000000000000n, \"UTC\").add({ days: 1 })"
            .to_string(),
        "new Temporal.ZonedDateTime(-8640000000000000000000n, \"UTC\").subtract({ nanoseconds: 1 })"
            .to_string(),
        "new Temporal.ZonedDateTime(-8640000000000000000000n, \"UTC\").subtract({ days: 1 })"
            .to_string(),
    ]);
    // Landing exactly on the edge is still representable.
    assert_str(
        "new Temporal.ZonedDateTime(8639999999999999999999n, \"UTC\").add({ nanoseconds: 1 }).toString()",
        "+275760-09-13T00:00:00+00:00[UTC]",
    );
}

#[test]
fn round_time_units_and_increments() {
    let z = zdt(&ny("2020-06-15T12:34:56.789012345"));
    let round = |arg: &str| format!("{z}.round({arg}).toString()");
    assert_str(
        &round("\"hour\""),
        "2020-06-15T13:00:00-04:00[America/New_York]",
    );
    assert_str(
        &round("\"minute\""),
        "2020-06-15T12:35:00-04:00[America/New_York]",
    );
    assert_str(
        &round("\"second\""),
        "2020-06-15T12:34:57-04:00[America/New_York]",
    );
    assert_str(
        &round("\"millisecond\""),
        "2020-06-15T12:34:56.789-04:00[America/New_York]",
    );
    assert_str(
        &round("\"microsecond\""),
        "2020-06-15T12:34:56.789012-04:00[America/New_York]",
    );
    assert_str(
        &round("\"nanosecond\""),
        "2020-06-15T12:34:56.789012345-04:00[America/New_York]",
    );
    assert_str(
        &round("{ smallestUnit: \"minute\", roundingIncrement: 15 }"),
        "2020-06-15T12:30:00-04:00[America/New_York]",
    );
    assert_str(
        &round("{ smallestUnit: \"hour\", roundingIncrement: 6 }"),
        "2020-06-15T12:00:00-04:00[America/New_York]",
    );
    assert_str(
        &round("{ smallestUnit: \"second\", roundingIncrement: 30, roundingMode: \"ceil\" }"),
        "2020-06-15T12:35:00-04:00[America/New_York]",
    );
    // Plural unit names are accepted too.
    assert_str(
        &round("{ smallestUnit: \"minutes\" }"),
        "2020-06-15T12:35:00-04:00[America/New_York]",
    );
    // Every rounding mode.
    for (mode, expected) in [
        ("ceil", "2020-06-15T13:00:00"),
        ("floor", "2020-06-15T12:00:00"),
        ("expand", "2020-06-15T13:00:00"),
        ("trunc", "2020-06-15T12:00:00"),
        ("halfCeil", "2020-06-15T13:00:00"),
        ("halfFloor", "2020-06-15T13:00:00"),
        ("halfExpand", "2020-06-15T13:00:00"),
        ("halfTrunc", "2020-06-15T13:00:00"),
        ("halfEven", "2020-06-15T13:00:00"),
    ] {
        assert_str(
            &round(&format!(
                "{{ smallestUnit: \"hour\", roundingMode: \"{mode}\" }}"
            )),
            &format!("{expected}-04:00[America/New_York]"),
        );
    }
    // Rounding can carry into the next day, month and year.
    assert_str(
        &format!(
            "{}.round(\"hour\").toString()",
            zdt(&utc("2020-12-31T23:40:00"))
        ),
        "2021-01-01T00:00:00+00:00[UTC]",
    );
    // Negative years round the same way.
    assert_str(
        &format!(
            "{}.round(\"minute\").toString()",
            zdt(&utc("0000-01-01T00:00:31"))
        ),
        "0000-01-01T00:01:00+00:00[UTC]",
    );
}

#[test]
fn round_day_uses_the_real_length_of_the_dst_day() {
    // 2020-03-08 in New York is 23 hours long: 05:00Z .. 04:00Z next day.
    let round = |local: &str, args: &str| format!("{}.round({args}).toString()", zdt(&ny(local)));
    // 11 of 23 hours in: rounds down to the start of the day.
    assert_str(
        &round("2020-03-08T12:00:00", "\"day\""),
        "2020-03-08T00:00:00-05:00[America/New_York]",
    );
    // 12 of 23 hours in (13:00 EDT): past halfway, rounds to the next start.
    assert_str(
        &round("2020-03-08T13:00:00", "\"day\""),
        "2020-03-09T00:00:00-04:00[America/New_York]",
    );
    assert_str(
        &round(
            "2020-03-08T05:00:00",
            "{ smallestUnit: \"day\", roundingMode: \"ceil\" }",
        ),
        "2020-03-09T00:00:00-04:00[America/New_York]",
    );
    assert_str(
        &round(
            "2020-03-08T23:00:00",
            "{ smallestUnit: \"days\", roundingMode: \"floor\" }",
        ),
        "2020-03-08T00:00:00-05:00[America/New_York]",
    );
    // An ordinary day: noon is exactly halfway, halfExpand goes up.
    assert_str(
        &round("2020-06-15T12:00:00", "\"day\""),
        "2020-06-16T00:00:00-04:00[America/New_York]",
    );
    // Exactly half-way with halfEven goes to the even candidate (the start).
    assert_str(
        &round(
            "2020-06-15T12:00:00",
            "{ smallestUnit: \"day\", roundingMode: \"halfEven\" }",
        ),
        "2020-06-15T00:00:00-04:00[America/New_York]",
    );
    assert_str(
        &round(
            "2020-06-14T12:00:00",
            "{ smallestUnit: \"day\", roundingMode: \"halfEven\" }",
        ),
        "2020-06-14T00:00:00-04:00[America/New_York]",
    );
    // A day with no local midnight starts at 01:00 (America/Sao_Paulo's
    // 2018-11-04 DST start skipped 00:00-01:00).
    assert_str(
        "Temporal.ZonedDateTime.from(\"2018-11-04T03:00:00[America/Sao_Paulo]\").round({ smallestUnit: \"day\", roundingMode: \"floor\" }).toString()",
        "2018-11-04T01:00:00-02:00[America/Sao_Paulo]",
    );
}

#[test]
fn round_rejects_bad_arguments() {
    let z = zdt(&ny("2020-06-15T12:34:56"));
    assert_type_errors(&[
        format!("{z}.round()"),
        format!("{z}.round(5)"),
        format!("{z}.round(null)"),
        format!("{z}.round(true)"),
        "Temporal.ZonedDateTime.prototype.round.call({}, \"hour\")".to_string(),
    ]);
    assert_range_errors(&[
        format!("{z}.round({{}})"),
        format!("{z}.round(\"bogus\")"),
        format!("{z}.round(\"year\")"),
        format!("{z}.round(\"month\")"),
        format!("{z}.round(\"week\")"),
        format!("{z}.round({{ smallestUnit: \"hour\", roundingMode: \"bogus\" }})"),
        format!("{z}.round({{ smallestUnit: \"hour\", roundingIncrement: 0 }})"),
        format!("{z}.round({{ smallestUnit: \"hour\", roundingIncrement: -1 }})"),
        format!("{z}.round({{ smallestUnit: \"hour\", roundingIncrement: Infinity }})"),
        format!("{z}.round({{ smallestUnit: \"hour\", roundingIncrement: 5 }})"),
        format!("{z}.round({{ smallestUnit: \"minute\", roundingIncrement: 7 }})"),
        format!("{z}.round({{ smallestUnit: \"day\", roundingIncrement: 2 }})"),
    ]);
}

#[test]
fn until_and_since_default_to_hours_across_dst() {
    let a = ny("2020-03-07T12:00:00");
    let b = ny("2020-03-08T12:00:00");
    // Exact elapsed time: only 23 hours.
    assert_str(&diff(&a, "until", &b, "undefined"), "PT23H");
    assert_str(&diff(&b, "since", &a, "undefined"), "PT23H");
    assert_str(&diff(&a, "since", &b, "undefined"), "-PT23H");
    assert_str(&diff(&b, "until", &a, "undefined"), "-PT23H");
    // As a calendar day it is exactly one day.
    assert_str(&diff(&a, "until", &b, "{ largestUnit: \"day\" }"), "P1D");
    assert_str(&diff(&b, "since", &a, "{ largestUnit: \"day\" }"), "P1D");
    assert_str(&diff(&a, "since", &b, "{ largestUnit: \"day\" }"), "-P1D");
    assert_str(&diff(&b, "until", &a, "{ largestUnit: \"days\" }"), "-P1D");
    // Across the fall-back day the exact time is 25 hours.
    assert_str(
        &diff(
            &ny("2020-10-31T12:00:00"),
            "until",
            &ny("2020-11-01T12:00:00"),
            "undefined",
        ),
        "PT25H",
    );
    assert_str(
        &diff(
            &ny("2020-10-31T12:00:00"),
            "until",
            &ny("2020-11-01T12:00:00"),
            "{ largestUnit: \"day\" }",
        ),
        "P1D",
    );
    // A day-plus-remainder difference.
    assert_str(
        &diff(
            &ny("2020-03-07T12:00:00"),
            "until",
            &ny("2020-03-08T15:30:00"),
            "{ largestUnit: \"day\" }",
        ),
        "P1DT3H30M",
    );
    assert_str(
        &diff(
            &ny("2020-03-07T12:00:00"),
            "until",
            &ny("2020-03-08T15:30:00"),
            "{ largestUnit: \"day\", smallestUnit: \"hour\" }",
        ),
        "P1DT3H",
    );
    // The remainder's time of day is earlier than the start's: the day count
    // is corrected.
    assert_str(
        &diff(
            &ny("2020-03-07T15:30:00"),
            "until",
            &ny("2020-03-08T12:00:00"),
            "{ largestUnit: \"day\" }",
        ),
        "PT19H30M",
    );
    assert_str(
        &diff(
            &ny("2020-03-08T12:00:00"),
            "until",
            &ny("2020-03-07T15:30:00"),
            "{ largestUnit: \"day\" }",
        ),
        "-PT19H30M",
    );
}

#[test]
fn until_and_since_calendar_units() {
    let a = utc("2020-01-15T00:00:00");
    let b = utc("2021-03-20T00:00:00");
    for (unit, expected) in [
        ("year", "P1Y2M5D"),
        ("month", "P14M5D"),
        ("week", "P61W3D"),
        ("day", "P430D"),
        ("hour", "PT10320H"),
        ("minute", "PT619200M"),
        ("second", "PT37152000S"),
        ("auto", "PT10320H"),
    ] {
        assert_str(
            &diff(&a, "until", &b, &format!("{{ largestUnit: \"{unit}\" }}")),
            expected,
        );
    }
    assert_str(
        &diff(&b, "since", &a, "{ largestUnit: \"year\" }"),
        "P1Y2M5D",
    );
    assert_str(
        &diff(&b, "until", &a, "{ largestUnit: \"year\" }"),
        "-P1Y2M5D",
    );
    assert_str(
        &diff(&a, "since", &b, "{ largestUnit: \"month\" }"),
        "-P14M5D",
    );
    // Sub-second largest units.
    assert_str(
        &diff(
            &utc("2020-01-01T00:00:00"),
            "until",
            &utc("2020-01-01T00:00:01.5"),
            "{ largestUnit: \"millisecond\" }",
        ),
        "PT1.5S",
    );
    assert_str(
        &diff(
            &utc("2020-01-01T00:00:00"),
            "until",
            &utc("2020-01-01T00:00:00.000001"),
            "{ largestUnit: \"nanosecond\" }",
        ),
        "PT0.000001S",
    );
    assert_str(
        &diff(
            &utc("2020-01-01T00:00:00"),
            "until",
            &utc("2020-01-01T00:00:00.000001"),
            "{ largestUnit: \"microsecond\" }",
        ),
        "PT0.000001S",
    );
}

#[test]
fn until_and_since_round_to_calendar_units_with_bubbling() {
    let a = utc("2020-01-15T00:00:00");
    // 2 months 5 days: rounding to whole months.
    let b = utc("2020-03-20T00:00:00");
    let months = |mode: &str| {
        format!("{{ largestUnit: \"month\", smallestUnit: \"month\", roundingMode: \"{mode}\" }}")
    };
    assert_str(&diff(&a, "until", &b, &months("trunc")), "P2M");
    assert_str(&diff(&a, "until", &b, &months("halfExpand")), "P2M");
    assert_str(&diff(&a, "until", &b, &months("expand")), "P3M");
    assert_str(&diff(&a, "until", &b, &months("ceil")), "P3M");
    assert_str(&diff(&a, "until", &b, &months("floor")), "P2M");
    // `since`/`until` in the opposite direction round symmetrically, and the
    // asymmetric modes are reflected.
    assert_str(&diff(&b, "until", &a, &months("ceil")), "-P2M");
    assert_str(&diff(&b, "until", &a, &months("floor")), "-P3M");
    assert_str(&diff(&b, "since", &a, &months("ceil")), "P3M");
    assert_str(&diff(&a, "since", &b, &months("ceil")), "-P2M");
    assert_str(&diff(&a, "since", &b, &months("floor")), "-P3M");
    assert_str(&diff(&a, "since", &b, &months("halfCeil")), "-P2M");
    assert_str(&diff(&a, "since", &b, &months("halfFloor")), "-P2M");
    assert_str(&diff(&a, "until", &b, &months("halfEven")), "P2M");
    assert_str(&diff(&a, "until", &b, &months("halfTrunc")), "P2M");
    // Exactly half-way: 2020-01-01 -> 2020-01-16 is 15 of 31 days... use a
    // pair with an exact midpoint in the unit itself (0.5 week = 3.5 days).
    // A remainder that pushes 11 months to a whole year bubbles up.
    assert_str(
        &diff(
            &a,
            "until",
            &utc("2020-12-20T00:00:00"),
            "{ largestUnit: \"year\", smallestUnit: \"month\", roundingMode: \"expand\" }",
        ),
        "P1Y",
    );
    // Rounding to years, with and without bubbling.
    assert_str(
        &diff(
            &a,
            "until",
            &utc("2021-08-15T00:00:00"),
            "{ largestUnit: \"year\", smallestUnit: \"year\", roundingMode: \"halfExpand\" }",
        ),
        "P2Y",
    );
    assert_str(
        &diff(
            &a,
            "until",
            &utc("2021-08-15T00:00:00"),
            "{ largestUnit: \"year\", smallestUnit: \"year\" }",
        ),
        "P1Y",
    );
    // Rounding to weeks and days.
    assert_str(
        &diff(
            &a,
            "until",
            &utc("2020-02-20T00:00:00"),
            "{ largestUnit: \"week\", smallestUnit: \"week\", roundingMode: \"expand\" }",
        ),
        "P6W",
    );
    assert_str(
        &diff(
            &a,
            "until",
            &utc("2020-02-20T12:00:00"),
            "{ largestUnit: \"day\", smallestUnit: \"day\", roundingMode: \"halfExpand\" }",
        ),
        "P37D",
    );
    assert_str(
        &diff(
            &a,
            "until",
            &utc("2020-02-20T11:00:00"),
            "{ largestUnit: \"day\", smallestUnit: \"day\", roundingMode: \"halfExpand\" }",
        ),
        "P36D",
    );
    assert_str(
        &diff(
            &a,
            "until",
            &utc("2020-02-20T00:00:00"),
            "{ largestUnit: \"day\", smallestUnit: \"day\", roundingIncrement: 10, roundingMode: \"ceil\" }",
        ),
        "P40D",
    );
    assert_str(
        &diff(
            &a,
            "until",
            &utc("2020-02-20T00:00:00"),
            "{ largestUnit: \"month\", smallestUnit: \"week\", roundingMode: \"halfExpand\" }",
        ),
        "P1M1W",
    );
    // A zoned month difference across DST measures the fraction in real
    // elapsed time.
    assert_str(
        &diff(
            &ny("2020-03-01T12:00:00"),
            "until",
            &ny("2020-04-20T12:00:00"),
            "{ largestUnit: \"month\", smallestUnit: \"month\", roundingMode: \"halfExpand\" }",
        ),
        "P2M",
    );
    assert_str(
        &diff(
            &ny("2020-03-01T12:00:00"),
            "until",
            &ny("2020-04-20T12:00:00"),
            "{ largestUnit: \"month\", smallestUnit: \"month\", roundingMode: \"halfEven\" }",
        ),
        "P2M",
    );
}

#[test]
fn until_and_since_round_sub_day_units() {
    let a = utc("2020-01-01T12:00:00");
    let b = utc("2020-01-01T13:22:30");
    assert_str(&diff(&a, "until", &b, "undefined"), "PT1H22M30S");
    assert_str(
        &diff(
            &a,
            "until",
            &b,
            "{ smallestUnit: \"minute\", roundingIncrement: 15 }",
        ),
        "PT1H15M",
    );
    assert_str(
        &diff(
            &a,
            "until",
            &b,
            "{ smallestUnit: \"minute\", roundingIncrement: 15, roundingMode: \"halfExpand\" }",
        ),
        "PT1H30M",
    );
    assert_str(
        &diff(
            &a,
            "until",
            &b,
            "{ smallestUnit: \"hour\", roundingMode: \"ceil\" }",
        ),
        "PT2H",
    );
    assert_str(
        &diff(
            &a,
            "since",
            &b,
            "{ smallestUnit: \"hour\", roundingMode: \"ceil\" }",
        ),
        "-PT1H",
    );
    assert_str(
        &diff(
            &a,
            "since",
            &b,
            "{ smallestUnit: \"hour\", roundingMode: \"floor\" }",
        ),
        "-PT2H",
    );
    assert_str(
        &diff(
            &a,
            "until",
            &b,
            "{ largestUnit: \"minute\", smallestUnit: \"minute\", roundingMode: \"expand\" }",
        ),
        "PT83M",
    );
    // A calendar `largestUnit` with a sub-day `smallestUnit` keeps the day
    // part and rounds only the remainder.
    assert_str(
        &diff(
            &ny("2020-03-07T12:00:00"),
            "until",
            &ny("2020-03-09T12:45:30"),
            "{ largestUnit: \"day\", smallestUnit: \"minute\" }",
        ),
        "P2DT45M",
    );
    assert_str(
        &diff(
            &utc("2020-01-01T00:00:00"),
            "until",
            &utc("2020-03-05T06:30:00"),
            "{ largestUnit: \"month\", smallestUnit: \"hour\", roundingIncrement: 4, roundingMode: \"halfExpand\" }",
        ),
        "P2M4DT8H",
    );
}

#[test]
fn until_and_since_of_equal_instants_are_blank_for_every_unit() {
    let a = utc("2020-01-01T00:00:00");
    for unit in [
        "year",
        "month",
        "week",
        "day",
        "hour",
        "minute",
        "second",
        "millisecond",
        "microsecond",
        "nanosecond",
    ] {
        assert_str(
            &diff(&a, "until", &a, &format!("{{ largestUnit: \"{unit}\" }}")),
            "PT0S",
        );
        assert_str(
            &diff(
                &a,
                "since",
                &a,
                &format!("{{ smallestUnit: \"{unit}\", largestUnit: \"year\" }}"),
            ),
            "PT0S",
        );
    }
    // Same instant, differently spelled zone of the same real zone.
    assert_str(
        "Temporal.ZonedDateTime.from(\"2020-01-01T00:00:00[Asia/Calcutta]\").until(Temporal.ZonedDateTime.from(\"2020-01-01T00:00:00[Asia/Kolkata]\"), { largestUnit: \"day\" }).toString()",
        "PT0S",
    );
}

#[test]
fn until_and_since_zone_and_calendar_compatibility_rules() {
    let tokyo = "Temporal.ZonedDateTime.from(\"2020-01-01T00:00:00[Asia/Tokyo]\")";
    let utc_value = zdt(&utc("2020-01-01T00:00:00"));
    // Pure time differences never consult the zones, so they may differ.
    assert_str(&format!("{utc_value}.until({tokyo}).toString()"), "-PT9H");
    assert_str(
        &format!("{utc_value}.until({tokyo}, {{ largestUnit: \"minute\" }}).toString()"),
        "-PT540M",
    );
    // Calendar-unit differences require the same zone.
    assert_range_errors(&[
        format!("{utc_value}.until({tokyo}, {{ largestUnit: \"day\" }})"),
        format!("{utc_value}.since({tokyo}, {{ largestUnit: \"month\" }})"),
        format!("{utc_value}.until({tokyo}, {{ largestUnit: \"year\" }})"),
        format!("{utc_value}.until({tokyo}, {{ smallestUnit: \"day\" }})"),
    ]);
    // Differing calendars are always rejected.
    let gregory = "Temporal.ZonedDateTime.from(\"2020-01-01T00:00:00[UTC][u-ca=gregory]\")";
    assert_range_errors(&[
        format!("{utc_value}.until({gregory})"),
        format!("{utc_value}.since({gregory})"),
    ]);
    // A string argument is parsed as a ZonedDateTime.
    assert_str(
        &format!("{utc_value}.until(\"2020-01-01T05:00:00[UTC]\").toString()"),
        "PT5H",
    );
    // Aliases of one zone are the same zone for calendar-unit differences.
    assert_str(
        "Temporal.ZonedDateTime.from(\"2020-01-01T00:00:00[Asia/Calcutta]\").until(\"2020-01-03T00:00:00[Asia/Kolkata]\", { largestUnit: \"day\" }).toString()",
        "P2D",
    );
}

#[test]
fn until_and_since_in_non_iso_calendars() {
    assert_str(
        "Temporal.ZonedDateTime.from(\"2020-01-15T00:00:00[UTC][u-ca=gregory]\").until(\"2021-03-20T00:00:00[UTC][u-ca=gregory]\", { largestUnit: \"year\" }).toString()",
        "P1Y2M5D",
    );
    assert_str(
        "Temporal.ZonedDateTime.from(\"2020-01-15T00:00:00[UTC][u-ca=japanese]\").until(\"2020-03-15T00:00:00[UTC][u-ca=japanese]\", { largestUnit: \"month\" }).toString()",
        "P2M",
    );
    assert_str(
        "Temporal.ZonedDateTime.from(\"2020-01-15T00:00:00[UTC][u-ca=hebrew]\").until(\"2020-01-15T00:00:00[UTC][u-ca=hebrew]\", { largestUnit: \"month\" }).toString()",
        "PT0S",
    );
}

#[test]
fn until_and_since_reject_bad_options() {
    let z = zdt(&utc("2020-01-01T00:00:00"));
    let other = "\"2020-06-01T00:00:00[UTC]\"";
    let with = |method: &str, options: &str| format!("{z}.{method}({other}, {options})");
    let mut bad = Vec::new();
    for method in ["until", "since"] {
        bad.push(with(method, "{ largestUnit: \"bogus\" }"));
        bad.push(with(method, "{ smallestUnit: \"bogus\" }"));
        // smallestUnit larger than largestUnit.
        bad.push(with(
            method,
            "{ largestUnit: \"hour\", smallestUnit: \"day\" }",
        ));
        bad.push(with(
            method,
            "{ largestUnit: \"day\", smallestUnit: \"month\" }",
        ));
        bad.push(with(method, "{ roundingMode: \"bogus\" }"));
        bad.push(with(method, "{ roundingIncrement: 0 }"));
        bad.push(with(method, "{ roundingIncrement: 1e10 }"));
        bad.push(with(method, "{ roundingIncrement: NaN }"));
        bad.push(format!("{z}.{method}(\"garbage\")"));
        bad.push(format!("{z}.{method}(\"2020-06-01T00:00:00\")"));
    }
    assert_range_errors(&bad);
    assert_type_errors(&[
        format!("{z}.until()"),
        format!("{z}.since(5)"),
        format!("{z}.until(null)"),
        format!("{z}.until({other}, 5)"),
        "Temporal.ZonedDateTime.prototype.until.call({}, \"2020-01-01T00:00:00[UTC]\")".to_string(),
        "Temporal.ZonedDateTime.prototype.since.call({}, \"2020-01-01T00:00:00[UTC]\")".to_string(),
    ]);
}

#[test]
fn until_at_the_extreme_edges_of_the_instant_range() {
    let min = "new Temporal.ZonedDateTime(-8640000000000000000000n, \"UTC\")";
    let max = "new Temporal.ZonedDateTime(8640000000000000000000n, \"UTC\")";
    assert_str(
        &format!("{min}.until({max}, {{ largestUnit: \"year\" }}).toString()"),
        "P547581Y4M24D",
    );
    assert_str(
        &format!("{max}.since({min}, {{ largestUnit: \"day\" }}).toString()"),
        "P200000000D",
    );
    assert_str(
        &format!("{min}.until({max}, {{ largestUnit: \"hour\" }}).toString()"),
        "PT4800000000H",
    );
}

#[test]
fn equals_and_compare_use_instant_zone_and_calendar() {
    let a = zdt("2020-01-01T00:00:00[UTC]");
    assert_true(&format!("{a}.equals(\"2020-01-01T00:00:00[UTC]\")"));
    assert_true(&format!("{a}.equals({a})"));
    assert_true(&format!(
        "{a}.equals({{ year: 2020, month: 1, day: 1, timeZone: \"UTC\" }})"
    ));
    // Same instant, different zone or calendar: not equal.
    assert_true(&format!(
        "{a}.equals(\"2020-01-01T09:00:00[Asia/Tokyo]\") === false"
    ));
    assert_true(&format!(
        "{a}.equals(\"2020-01-01T00:00:00[UTC][u-ca=gregory]\") === false"
    ));
    assert_true(&format!(
        "{a}.equals(\"2020-01-01T00:00:01[UTC]\") === false"
    ));
    // IANA aliases are the same zone.
    assert_true(
        "Temporal.ZonedDateTime.from(\"2020-01-01T00:00:00[Asia/Calcutta]\").equals(\"2020-01-01T00:00:00[Asia/Kolkata]\")",
    );
    // compare orders by instant only.
    assert_true(&format!(
        "Temporal.ZonedDateTime.compare({a}, \"2020-01-01T09:00:00[Asia/Tokyo]\") === 0"
    ));
    assert_true(&format!(
        "Temporal.ZonedDateTime.compare({a}, \"2020-01-01T00:00:00.000000001[UTC]\") === -1"
    ));
    assert_true(&format!(
        "Temporal.ZonedDateTime.compare(\"2020-01-01T00:00:00.000000001[UTC]\", {a}) === 1"
    ));
    assert_true(&format!(
        "Temporal.ZonedDateTime.compare({a}, {{ year: 2019, month: 12, day: 31, timeZone: \"UTC\" }}) === 1"
    ));
    assert_type_errors(&[
        format!("{a}.equals()"),
        format!("{a}.equals(5)"),
        "Temporal.ZonedDateTime.compare()".to_string(),
        format!("Temporal.ZonedDateTime.compare({a})"),
        format!("Temporal.ZonedDateTime.compare({a}, 5)"),
        format!("Temporal.ZonedDateTime.compare(null, {a})"),
        "Temporal.ZonedDateTime.prototype.equals.call({}, \"2020-01-01T00:00:00[UTC]\")"
            .to_string(),
    ]);
    assert_range_errors(&[
        format!("{a}.equals(\"garbage\")"),
        format!("Temporal.ZonedDateTime.compare({a}, \"garbage\")"),
        format!("Temporal.ZonedDateTime.compare(\"garbage\", {a})"),
        format!("{a}.equals({{ year: 2020, month: 1, day: 1, timeZone: \"Nope/Zone\" }})"),
    ]);
}
