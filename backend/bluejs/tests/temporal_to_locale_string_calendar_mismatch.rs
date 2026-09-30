// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A Temporal value's calendar must match the formatter's calendar (ECMA-402
//! `HandleDateTimeTemporalDate`, `...YearMonth`, `...MonthDay`, and the
//! `ZonedDateTime` equivalent).
//!
//! `Temporal.PlainDate(2000, 5, 2, "buddhist").toLocaleString()` in an
//! English (Gregorian-calendar) locale must throw a `RangeError`: the value is
//! not in the formatter's calendar and is not the ISO calendar. `PlainYearMonth`
//! and `PlainMonthDay` are stricter -- even the ISO calendar mismatches, since
//! their ISO reference day/year would be misleading in another calendar. The
//! formatting layer never made the comparison, so it printed the ISO fields in
//! the locale's calendar.

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

fn assert_range_error(source: &str) {
    assert!(
        matches!(evaluate_err(source), RuntimeError::RangeError(_)),
        "{source}"
    );
}

/// Every plain type, in the locale's calendar, in the ISO calendar (where
/// allowed) and in a different calendar.
#[test]
fn plain_date_and_plain_date_time_allow_iso_but_not_other_calendars() {
    for constructor in ["PlainDate", "PlainDateTime"] {
        let make = |calendar: &str| {
            format!(
                r#"new Temporal.{constructor}(2000, 5, 2{}, "{calendar}")"#,
                if constructor == "PlainDateTime" {
                    ", 0, 0, 0, 0, 0, 0"
                } else {
                    ""
                }
            )
        };
        // The default locale's calendar is Gregorian.
        assert_true(&format!(
            r#"typeof {}.toLocaleString() === "string""#,
            make("gregory")
        ));
        assert_true(&format!(
            r#"{}.toLocaleString() === {}.toLocaleString()"#,
            make("iso8601"),
            make("gregory")
        ));
        for other in ["buddhist", "hebrew", "japanese", "chinese", "islamic-civil"] {
            assert_range_error(&format!("{}.toLocaleString()", make(other)));
        }
    }
}

#[test]
fn year_month_and_month_day_reject_even_the_iso_calendar() {
    let year_month = |calendar: &str| {
        format!(r#"new Temporal.PlainDate(2000, 1, 1, "{calendar}").toPlainYearMonth()"#)
    };
    assert_true(&format!(
        r#"typeof {}.toLocaleString() === "string""#,
        year_month("gregory")
    ));
    assert_range_error(&format!("{}.toLocaleString()", year_month("iso8601")));
    assert_range_error(&format!("{}.toLocaleString()", year_month("buddhist")));

    let month_day = |calendar: &str| {
        format!(
            r#"Temporal.PlainMonthDay.from({{ monthCode: "M01", day: 1, calendar: "{calendar}" }})"#
        )
    };
    assert_true(&format!(
        r#"typeof {}.toLocaleString() === "string""#,
        month_day("gregory")
    ));
    assert_range_error(&format!("{}.toLocaleString()", month_day("iso8601")));
    assert_range_error(&format!("{}.toLocaleString()", month_day("buddhist")));
}

#[test]
fn zoned_date_time_allows_iso_but_not_other_calendars() {
    let make = |calendar: &str| format!(r#"new Temporal.ZonedDateTime(0n, "UTC", "{calendar}")"#);
    assert_true(&format!(
        r#"typeof {}.toLocaleString() === "string""#,
        make("gregory")
    ));
    assert_true(&format!(
        "{}.toLocaleString() === {}.toLocaleString()",
        make("iso8601"),
        make("gregory")
    ));
    assert_range_error(&format!("{}.toLocaleString()", make("buddhist")));
    assert_range_error(&format!("{}.toLocaleString()", make("hebrew")));
}

/// An `Instant` and a `PlainTime` have no calendar, so there is nothing to
/// mismatch: any formatter accepts them.
#[test]
fn instants_and_plain_times_have_no_calendar_to_mismatch() {
    assert_true(
        r#"(function() {
          const formatters = [new Intl.DateTimeFormat("en"),
                              new Intl.DateTimeFormat("en", { calendar: "buddhist" }),
                              new Intl.DateTimeFormat("en-u-ca-hebrew")];
          const instant = Temporal.Instant.fromEpochMilliseconds(0);
          const time = new Temporal.PlainTime(1, 2, 3);
          for (const formatter of formatters) {
            if (typeof formatter.format(instant) !== "string") return "instant";
            if (typeof formatter.format(time) !== "string") return "time";
          }
          return typeof instant.toLocaleString() === "string" && typeof time.toLocaleString() === "string";
        })()"#,
    );
}

/// The formatter's own calendar is what counts, not the default locale's.
#[test]
fn an_explicit_formatter_calendar_is_compared() {
    assert_true(
        r#"(function() {
          const buddhist = new Intl.DateTimeFormat("en", { calendar: "buddhist" });
          const gregory = new Intl.DateTimeFormat("en");
          const date = (calendar) => new Temporal.PlainDate(2000, 5, 2, calendar);
          if (typeof buddhist.format(date("buddhist")) !== "string") return "buddhist same";
          if (typeof buddhist.format(date("iso8601")) !== "string") return "iso in buddhist";
          for (const attempt of [() => buddhist.format(date("gregory")), () => gregory.format(date("buddhist"))]) {
            try { attempt(); return "no throw"; } catch (error) { if (!(error instanceof RangeError)) return "wrong error"; }
          }
          return true;
        })()"#,
    );
    // A locale that selects the calendar through its Unicode extension.
    assert_true(
        r#"(function() {
          const hebrew = new Intl.DateTimeFormat("en-u-ca-hebrew");
          try { hebrew.format(new Temporal.PlainDate(2000, 5, 2, "gregory")); return "no throw"; }
          catch (error) { return error instanceof RangeError ? true : "wrong error"; }
        })()"#,
    );
}

/// `formatToParts` and the range methods make the same comparison.
#[test]
fn parts_and_range_formatting_compare_calendars_too() {
    assert_true(
        r#"(function() {
          const gregory = new Intl.DateTimeFormat("en");
          const buddhist = new Temporal.PlainDate(2000, 5, 2, "buddhist");
          const attempts = [
            () => gregory.formatToParts(buddhist),
            () => gregory.formatRange(buddhist, buddhist),
            () => gregory.formatRangeToParts(buddhist, buddhist),
          ];
          for (const attempt of attempts) {
            try { attempt(); return "no throw"; } catch (error) { if (!(error instanceof RangeError)) return "wrong error"; }
          }
          const iso = new Temporal.PlainDate(2000, 5, 2);
          return gregory.formatToParts(iso).length > 0
              && typeof gregory.formatRange(iso, new Temporal.PlainDate(2000, 5, 9)) === "string";
        })()"#,
    );
}

/// The same check applies to `Temporal.PlainDate`'s own `toLocaleString`
/// arguments: a locale requesting another calendar must match the value's.
#[test]
fn a_locale_extension_can_select_a_matching_calendar() {
    assert_true(
        r#"(function() {
          const buddhist = new Temporal.PlainDate(2000, 5, 2, "buddhist");
          return typeof buddhist.toLocaleString("en-u-ca-buddhist") === "string"
              && typeof buddhist.toLocaleString("en", { calendar: "buddhist" }) === "string";
        })()"#,
    );
}

/// Test262's `intl402/Temporal/*/prototype/toLocaleString/calendar-mismatch.js`
/// picks the mismatching calendar with `Set` iteration:
/// `calendars.values().next().value`. Reproduce that selection end to end.
#[test]
fn the_fixture_selection_of_a_different_calendar_works() {
    assert_true(
        r#"(function() {
          const localeCalendar = new Intl.DateTimeFormat().resolvedOptions().calendar;
          if (localeCalendar === "iso8601") return "locale calendar";
          const calendars = new Set(Intl.supportedValuesOf("calendar"));
          calendars.delete("iso8601");
          calendars.delete(localeCalendar);
          const differentCalendar = calendars.values().next().value;
          if (typeof differentCalendar !== "string" || differentCalendar === localeCalendar) return "selection " + differentCalendar;
          const instance = new Temporal.PlainDate(2000, 5, 2, differentCalendar);
          try { instance.toLocaleString(); return "no throw"; }
          catch (error) { return error instanceof RangeError ? true : "wrong error"; }
        })()"#,
    );
}
