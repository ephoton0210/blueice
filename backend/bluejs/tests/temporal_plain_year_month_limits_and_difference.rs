// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Regression coverage for `Temporal.PlainYearMonth` arithmetic at the ends of
//! the representable range, and for its `until`/`since` rounding (Phase 26
//! Stage 3, `development/browser_core/phase-26-ecma262-temporal/PLAN.md`).
//!
//! `AddDurationToYearMonth` and `DifferenceTemporalPlainYearMonth` both turn
//! the year-month into its *first day* (`CalendarDateFromFields`), and that day
//! must be a valid `PlainDate`: `-271821-04-01` is not (the earliest date is
//! `-271821-04-19`), so `PlainYearMonth(-271821, 4).add(blank)` is a
//! `RangeError` even though the year-month itself is representable. The
//! implementation never made that check.
//!
//! `DifferenceTemporalPlainYearMonth` is also `RoundRelativeDuration` at
//! midnight, exactly `DifferenceTemporalPlainDate` with a different
//! rounding-skip rule, so it shares `plain_date_time_difference.rs`: a months
//! increment rounds the months *remainder* (never a flattened
//! `years * 12 + months`), and a rounded bracket beyond the range throws.
//!
//! `add`/`subtract` also read `options.overflow` *before* rejecting a duration
//! with week/day/time fields (`add/options-read-before-algorithmic-validation.js`).

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
    match evaluate(source) {
        Value::Bool(true) => {}
        Value::String(reason) => panic!("{}", reason.to_utf8().unwrap()),
        other => panic!("{source}\n  -> {other:?}"),
    }
}

fn assert_string(source: &str, expected: &str) {
    match evaluate(source) {
        Value::String(actual) => assert_eq!(actual.to_utf8().unwrap(), expected, "{source}"),
        other => panic!("{source}\n  -> {other:?}"),
    }
}

fn assert_range_error(source: &str) {
    assert!(
        matches!(evaluate_err(source), RuntimeError::RangeError(_)),
        "{source}"
    );
}

const MIN: &str = "new Temporal.PlainYearMonth(-271821, 4)";
const MAX: &str = "new Temporal.PlainYearMonth(275760, 9)";
const EPOCH: &str = "new Temporal.PlainYearMonth(1970, 1)";

/// `add/throws-if-year-outside-valid-iso-range.js` and its `subtract` twin.
#[test]
fn add_and_subtract_reject_a_first_day_outside_the_valid_range() {
    for method in ["add", "subtract"] {
        assert_range_error(&format!("{MIN}.{method}(new Temporal.Duration())"));
        assert_range_error(&format!("{MIN}.{method}({{ months: 0 }})"));
    }
    // One month later the first day (-271821-05-01) is representable again.
    assert_string(
        "new Temporal.PlainYearMonth(-271821, 5).add(new Temporal.Duration()).toString()",
        "-271821-05",
    );
    assert_string(
        &format!("{MAX}.subtract({{ months: 1 }}).toString()"),
        "+275760-08",
    );
    // The maximum year-month's first day is fine.
    assert_string(
        &format!("{MAX}.add(new Temporal.Duration()).toString()"),
        "+275760-09",
    );
    assert_range_error(&format!("{MAX}.add({{ months: 1 }})"));
}

/// `add/options-read-before-algorithmic-validation.js`: `overflow` is read (once)
/// before either RangeError, and week/day/time fields are still rejected.
#[test]
fn add_reads_options_before_any_algorithmic_validation() {
    for method in ["add", "subtract"] {
        assert_true(&format!(
            r#"(function() {{
              const reads = [];
              const options = {{
                get overflow() {{ reads.push("overflow"); return "constrain"; }},
              }};
              try {{ {MIN}.{method}(new Temporal.Duration(0, 1), options); return "no throw (limit)"; }}
              catch (error) {{ if (!(error instanceof RangeError)) return "wrong error (limit)"; }}
              if (reads.join() !== "overflow") return "limit reads: " + reads.join();
              reads.length = 0;
              try {{ new Temporal.PlainYearMonth(1999, 12).{method}(new Temporal.Duration(0, 0, 1), options); return "no throw (unit)"; }}
              catch (error) {{ if (!(error instanceof RangeError)) return "wrong error (unit)"; }}
              return reads.join() === "overflow" ? true : "unit reads: " + reads.join();
            }})()"#
        ));
    }
}

/// `since`/`until` with `roundingIncrement`: the months remainder is rounded,
/// not a flattened month total (`since/roundingincrement-as-expected.js`).
#[test]
fn since_and_until_round_the_months_remainder_to_the_increment() {
    // 2019-01 .. 2021-09 is 2 years 8 months.
    let earlier = "new Temporal.PlainYearMonth(2019, 1)";
    let later = "new Temporal.PlainYearMonth(2021, 9)";
    for (call, fields) in [
        (
            r#"{ smallestUnit: "years", roundingIncrement: 4, roundingMode: "halfExpand" }"#,
            "4,0",
        ),
        (r#"{ smallestUnit: "months", roundingIncrement: 5 }"#, "2,5"),
        (
            r#"{ largestUnit: "months", smallestUnit: "months", roundingIncrement: 10 }"#,
            "0,30",
        ),
    ] {
        assert_string(
            &format!(
                r#"(function() {{ const d = {later}.since({earlier}, {call}); return d.years + "," + d.months; }})()"#
            ),
            fields,
        );
        // `until` is the same difference in the other direction, negated for `since`.
        assert_string(
            &format!(
                r#"(function() {{ const d = {earlier}.until({later}, {call}); return d.years + "," + d.months; }})()"#
            ),
            fields,
        );
    }
}

/// The whole rounding-mode matrix agrees between `until` and a reflected `since`.
#[test]
fn since_reflects_asymmetric_rounding_modes() {
    let earlier = "new Temporal.PlainYearMonth(2019, 1)";
    let later = "new Temporal.PlainYearMonth(2021, 9)";
    // (mode, until years, since years) for 2y 8m rounded to whole years.
    for (mode, forward, backward) in [
        ("ceil", 3, 2),
        ("floor", 2, 3),
        ("trunc", 2, 2),
        ("expand", 3, 3),
        ("halfExpand", 3, 3),
        ("halfTrunc", 3, 3),
        ("halfEven", 3, 3),
    ] {
        let options = format!(r#"{{ smallestUnit: "years", roundingMode: "{mode}" }}"#);
        assert_true(&format!(
            r#"{earlier}.until({later}, {options}).years === {forward}
            && {earlier}.since({later}, {options}).years === -{backward}"#
        ));
    }
}

/// `since/throws-if-rounded-date-outside-valid-iso-range.js`.
#[test]
fn since_and_until_reject_a_rounded_bracket_outside_the_valid_range() {
    for method in ["since", "until"] {
        assert_range_error(&format!(
            "new Temporal.PlainYearMonth(1970, 1).{method}(new Temporal.PlainYearMonth(1971, 1), \
             {{ roundingIncrement: 100000000 }})"
        ));
        assert_range_error(&format!(
            r#"new Temporal.PlainYearMonth(1970, 1).{method}(new Temporal.PlainYearMonth(1971, 1),
                 {{ smallestUnit: "years", roundingIncrement: 100000000 }})"#
        ));
    }
}

/// `since/throws-if-year-outside-valid-iso-range.js`: equal year-months short-circuit
/// (even at the minimum); otherwise both first days must be valid dates.
#[test]
fn since_and_until_require_valid_first_days() {
    for method in ["since", "until"] {
        assert_true(&format!(
            r#"{MIN}.{method}({MIN}).blank && {MAX}.{method}({MAX}).blank && {EPOCH}.{method}({EPOCH}).blank"#
        ));
        assert_range_error(&format!("{MIN}.{method}({MAX})"));
        assert_range_error(&format!("{MIN}.{method}({EPOCH})"));
        assert_range_error(&format!("{EPOCH}.{method}({MIN})"));
        assert_true(&format!(
            "{MAX}.{method}({EPOCH}).sign !== 0 && {EPOCH}.{method}({MAX}).sign !== 0"
        ));
    }
}

/// `since/argument-string-limits.js` and its `until` twin: the argument's first
/// day must be valid, which is stricter than a year-month string on its own.
#[test]
fn since_and_until_apply_the_first_day_limits_to_string_arguments() {
    let valid = [
        "-271821-05",
        "-271821-05-01",
        "-271821-05-01T00:00",
        "+275760-09",
        "+275760-09-30",
        "+275760-09-30T23:59:59.999999999",
    ];
    let invalid = [
        "-271821-04",
        "-271821-04-30",
        "-271821-04-30T23:59:59.999999999",
        "+275760-10",
        "+275760-10-01",
        "+275760-10-01T00:00",
    ];
    for method in ["since", "until"] {
        for text in valid {
            evaluate(&format!(r#"{EPOCH}.{method}("{text}")"#));
        }
        for text in invalid {
            assert_range_error(&format!(r#"{EPOCH}.{method}("{text}")"#));
        }
    }
}

/// The default (unrounded) difference and the option validation are unchanged.
#[test]
fn the_unrounded_difference_and_option_validation_are_unchanged() {
    let earlier = "new Temporal.PlainYearMonth(2019, 1)";
    let later = "new Temporal.PlainYearMonth(2021, 9)";
    assert_string(&format!("{earlier}.until({later}).toString()"), "P2Y8M");
    assert_string(&format!("{later}.until({earlier}).toString()"), "-P2Y8M");
    assert_string(&format!("{later}.since({earlier}).toString()"), "P2Y8M");
    assert_string(
        &format!(r#"{earlier}.until({later}, {{ largestUnit: "months" }}).toString()"#),
        "P32M",
    );
    assert_range_error(&format!(
        r#"{earlier}.until({later}, {{ smallestUnit: "days" }})"#
    ));
    assert_range_error(&format!(
        r#"{earlier}.until({later}, {{ largestUnit: "weeks" }})"#
    ));
    assert_range_error(&format!(
        r#"{earlier}.until({later}, {{ largestUnit: "months", smallestUnit: "years" }})"#
    ));
    assert_range_error(&format!(
        "{earlier}.until({later}, {{ roundingIncrement: 0 }})"
    ));
}
