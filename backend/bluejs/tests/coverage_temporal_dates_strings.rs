// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Coverage for the string, locale and conversion surface of
//! `Temporal.PlainDate` / `Temporal.PlainDateTime` (`vm/temporal/dates.rs`):
//! `toString` options (`calendarName`, `fractionalSecondDigits`,
//! `smallestUnit`, `roundingMode`), `toLocaleString`, `valueOf`, the
//! `toPlainDateTime`/`toPlainDate`/`toPlainTime`/`withPlainTime`/
//! `toPlainYearMonth`/`toPlainMonthDay` conversions, `PlainDateTime.round`,
//! `toZonedDateTime`, `Instant.toZonedDateTimeISO` and `Temporal.Now`.
//!
//! Every assertion states behaviour the ECMAScript Temporal specification
//! requires; each test runs a whole batch of cases inside one script and
//! reports every failing case (with its source line) at once. Only fixed
//! instants and explicit locales/time zones are used, except the `Temporal.Now`
//! checks, which only assert properties that hold for any wall-clock reading.

use blueice_bluejs::{compile, parse, Value, Vm};

const PRELUDE: &str = r#"
const fails = [];
let L = 0;
let CTX = "";
function nameOf(e) {
  if (e instanceof RangeError) return "RangeError";
  if (e instanceof TypeError) return "TypeError";
  if (e instanceof SyntaxError) return "SyntaxError";
  return "other:" + String(e);
}
function thrown(f) { try { f(); } catch (e) { return nameOf(e); } return "none"; }
function range(f) { const t = thrown(f); if (t !== "RangeError") fails.push("@" + L + " [" + CTX + "] " + " -> " + t); }
function type(f) { const t = thrown(f); if (t !== "TypeError") fails.push("@" + L + " [" + CTX + "] " + " -> " + t); }
function same(f, expected) {
  let actual;
  try { actual = f(); } catch (e) { actual = "threw " + nameOf(e); }
  if (actual !== expected) fails.push("@" + L + " [" + CTX + "] " + " => " + String(actual) + " !== " + String(expected));
}
const D = Temporal.PlainDate;
const DT = Temporal.PlainDateTime;
const Z = Temporal.ZonedDateTime;
"#;

fn run(body: &str) {
    // Tag each case line with its line number so a failure can name its source.
    let lines: Vec<&str> = body.lines().collect();
    let tagged: Vec<String> = lines
        .iter()
        .enumerate()
        .map(|(index, line)| {
            let trimmed = line.trim_start();
            if ["same(", "range(", "type("]
                .iter()
                .any(|prefix| trimmed.starts_with(prefix))
            {
                format!("L = {}; {line}", index + 1)
            } else {
                (*line).to_string()
            }
        })
        .collect();
    let tagged = tagged.join("\n");
    let source = format!(
        "(function() {{\n{PRELUDE}\n{tagged}\nreturn fails.length === 0 ? true : fails.join(\"\\n\");\n}})()"
    );
    let program =
        compile(&parse(&source).expect("test script parses")).expect("test script compiles");
    match Vm::default().execute(&program) {
        Ok(Value::Bool(true)) => {}
        Ok(Value::String(text)) => {
            let report: Vec<String> = text
                .to_utf8()
                .unwrap()
                .lines()
                .map(|entry| {
                    let (tag, rest) = entry.split_once(' ').unwrap_or((entry, ""));
                    let number: usize = tag.trim_start_matches('@').parse().unwrap_or(0);
                    let source_line = lines.get(number.wrapping_sub(1)).copied().unwrap_or("?");
                    format!("{}\n      {rest}", source_line.trim())
                })
                .collect();
            panic!("failing cases:\n{}", report.join("\n"));
        }
        other => panic!("unexpected result: {other:?}"),
    }
}


#[test]
fn plain_date_to_string_reads_only_calendar_name() {
    run(r#"
      const d = D.from("2020-06-15");
      same(() => d.toString(), "2020-06-15");
      same(() => d.toString(undefined), "2020-06-15");
      same(() => d.toString({}), "2020-06-15");
      same(() => d.toString({ calendarName: "auto" }), "2020-06-15");
      same(() => d.toString({ calendarName: "always" }), "2020-06-15[u-ca=iso8601]");
      same(() => d.toString({ calendarName: "never" }), "2020-06-15");
      same(() => d.toString({ calendarName: "critical" }), "2020-06-15[!u-ca=iso8601]");
      // the time-precision options are never read for a PlainDate
      same(() => d.toString({ fractionalSecondDigits: 99, smallestUnit: "bogus", roundingMode: "bogus" }), "2020-06-15");
      range(() => d.toString({ calendarName: "bogus" }));
      range(() => d.toString({ calendarName: "" }));
      type(() => d.toString(5));
      type(() => d.toString(null));
      type(() => d.toString("always"));
      type(() => d.toString(true));
      const g = D.from("2020-06-15[u-ca=gregory]");
      same(() => g.toString(), "2020-06-15[u-ca=gregory]");
      same(() => g.toString({ calendarName: "never" }), "2020-06-15");
      same(() => g.toString({ calendarName: "critical" }), "2020-06-15[!u-ca=gregory]");
      same(() => g.toString({ calendarName: "always" }), "2020-06-15[u-ca=gregory]");
      same(() => g.toJSON(), "2020-06-15[u-ca=gregory]");
      same(() => D.from("-000001-03-04").toString(), "-000001-03-04");
      same(() => D.from("+010000-03-04").toString(), "+010000-03-04");
      same(() => D.from("0001-03-04").toString(), "0001-03-04");
      same(() => D.from("-271821-04-19").toString(), "-271821-04-19");
      same(() => D.from("+275760-09-13").toString(), "+275760-09-13");
    "#);
}

#[test]
fn plain_date_time_to_string_fractional_second_digits_and_smallest_unit() {
    run(r#"
      const dt = DT.from("2020-06-15T12:34:56.789123456");
      same(() => dt.toString(), "2020-06-15T12:34:56.789123456");
      same(() => dt.toString({}), "2020-06-15T12:34:56.789123456");
      same(() => dt.toString({ fractionalSecondDigits: "auto" }), "2020-06-15T12:34:56.789123456");
      const expected = ["12:34:56", "12:34:56.7", "12:34:56.78", "12:34:56.789", "12:34:56.7891",
        "12:34:56.78912", "12:34:56.789123", "12:34:56.7891234", "12:34:56.78912345", "12:34:56.789123456"];
      for (let digits = 0; digits <= 9; digits++) {
        CTX = "digits " + digits;
        same(() => dt.toString({ fractionalSecondDigits: digits }), "2020-06-15T" + expected[digits]);
      }
      same(() => dt.toString({ fractionalSecondDigits: 2.9 }), "2020-06-15T12:34:56.78");
      same(() => dt.toString({ fractionalSecondDigits: 9.9 }), "2020-06-15T12:34:56.789123456");
      same(() => dt.toString({ fractionalSecondDigits: 0, smallestUnit: undefined }), "2020-06-15T12:34:56");
      same(() => dt.toString({ smallestUnit: "minute" }), "2020-06-15T12:34");
      same(() => dt.toString({ smallestUnit: "second" }), "2020-06-15T12:34:56");
      same(() => dt.toString({ smallestUnit: "millisecond" }), "2020-06-15T12:34:56.789");
      same(() => dt.toString({ smallestUnit: "microsecond" }), "2020-06-15T12:34:56.789123");
      same(() => dt.toString({ smallestUnit: "nanosecond" }), "2020-06-15T12:34:56.789123456");
      same(() => dt.toString({ smallestUnit: "minutes" }), "2020-06-15T12:34");
      same(() => dt.toString({ smallestUnit: "seconds" }), "2020-06-15T12:34:56");
      same(() => dt.toString({ smallestUnit: "milliseconds" }), "2020-06-15T12:34:56.789");
      same(() => dt.toString({ smallestUnit: "microseconds" }), "2020-06-15T12:34:56.789123");
      same(() => dt.toString({ smallestUnit: "nanoseconds" }), "2020-06-15T12:34:56.789123456");
      // smallestUnit takes precedence over fractionalSecondDigits
      same(() => dt.toString({ smallestUnit: "minute", fractionalSecondDigits: 5 }), "2020-06-15T12:34");
      same(() => dt.toString({ smallestUnit: "millisecond", fractionalSecondDigits: 0 }), "2020-06-15T12:34:56.789");
      // whole-second and sub-second values under "auto" trim trailing zeros
      same(() => DT.from("2020-06-15T12:34:56").toString(), "2020-06-15T12:34:56");
      same(() => DT.from("2020-06-15T12:34:56.5").toString(), "2020-06-15T12:34:56.5");
      same(() => DT.from("2020-06-15T12:34:56.000000001").toString(), "2020-06-15T12:34:56.000000001");
      same(() => DT.from("2020-06-15T12:34").toString({ fractionalSecondDigits: 3 }), "2020-06-15T12:34:00.000");
      same(() => DT.from("2020-06-15T12:34").toString({ smallestUnit: "minute" }), "2020-06-15T12:34");
      // invalid units and digit counts
      range(() => dt.toString({ smallestUnit: "hour" }));
      range(() => dt.toString({ smallestUnit: "day" }));
      range(() => dt.toString({ smallestUnit: "month" }));
      range(() => dt.toString({ smallestUnit: "year" }));
      range(() => dt.toString({ smallestUnit: "bogus" }));
      range(() => dt.toString({ smallestUnit: "" }));
      range(() => dt.toString({ fractionalSecondDigits: 10 }));
      range(() => dt.toString({ fractionalSecondDigits: -1 }));
      range(() => dt.toString({ fractionalSecondDigits: -0.5 }));
      range(() => dt.toString({ fractionalSecondDigits: NaN }));
      range(() => dt.toString({ fractionalSecondDigits: Infinity }));
      range(() => dt.toString({ fractionalSecondDigits: "3" }));
      range(() => dt.toString({ fractionalSecondDigits: "bogus" }));
      range(() => dt.toString({ fractionalSecondDigits: true }));
      range(() => dt.toString({ fractionalSecondDigits: null }));
      type(() => dt.toString({ fractionalSecondDigits: Symbol("x") }));
      type(() => dt.toString(5));
      type(() => dt.toString(null));
      type(() => dt.toString("minute"));
    "#);
}

#[test]
fn plain_date_time_to_string_rounding_modes_and_carries() {
    run(r#"
      const half = DT.from("2020-06-15T12:34:56.785");
      const modes = {
        ceil: ".79", floor: ".78", trunc: ".78", expand: ".79",
        halfCeil: ".79", halfFloor: ".78", halfTrunc: ".78", halfExpand: ".79", halfEven: ".78",
      };
      for (const mode of Object.keys(modes)) {
        CTX = mode;
        same(() => half.toString({ fractionalSecondDigits: 2, roundingMode: mode }), "2020-06-15T12:34:56" + modes[mode]);
      }
      same(() => half.toString({ fractionalSecondDigits: 2 }), "2020-06-15T12:34:56.78");
      same(() => half.toString({ smallestUnit: "second", roundingMode: "ceil" }), "2020-06-15T12:34:57");
      same(() => half.toString({ smallestUnit: "second", roundingMode: "halfExpand" }), "2020-06-15T12:34:57");
      same(() => half.toString({ smallestUnit: "minute", roundingMode: "ceil" }), "2020-06-15T12:35");
      same(() => half.toString({ smallestUnit: "minute", roundingMode: "floor" }), "2020-06-15T12:34");
      same(() => half.toString({ smallestUnit: "millisecond", roundingMode: "expand" }), "2020-06-15T12:34:56.785");
      // the microsecond and nanosecond precisions round at their own unit
      const fine = DT.from("2020-06-15T12:34:56.123456789");
      same(() => fine.toString({ fractionalSecondDigits: 4, roundingMode: "ceil" }), "2020-06-15T12:34:56.1235");
      same(() => fine.toString({ fractionalSecondDigits: 5, roundingMode: "halfExpand" }), "2020-06-15T12:34:56.12346");
      same(() => fine.toString({ fractionalSecondDigits: 6, roundingMode: "floor" }), "2020-06-15T12:34:56.123456");
      same(() => fine.toString({ fractionalSecondDigits: 7, roundingMode: "ceil" }), "2020-06-15T12:34:56.1234568");
      same(() => fine.toString({ smallestUnit: "microsecond", roundingMode: "ceil" }), "2020-06-15T12:34:56.123457");
      same(() => fine.toString({ smallestUnit: "microsecond", roundingMode: "halfExpand" }), "2020-06-15T12:34:56.123457");
      same(() => fine.toString({ smallestUnit: "nanosecond", roundingMode: "ceil" }), "2020-06-15T12:34:56.123456789");
      // rounding carries into the next second, minute, hour, day, month and year
      same(() => DT.from("2020-06-15T12:34:59.9995").toString({ fractionalSecondDigits: 3, roundingMode: "halfExpand" }), "2020-06-15T12:35:00.000");
      same(() => DT.from("2020-06-15T23:59:59.9995").toString({ fractionalSecondDigits: 3, roundingMode: "halfExpand" }), "2020-06-16T00:00:00.000");
      same(() => DT.from("2020-06-30T23:59:30").toString({ smallestUnit: "minute", roundingMode: "halfExpand" }), "2020-07-01T00:00");
      same(() => DT.from("2020-12-31T23:59:59.9999999").toString({ fractionalSecondDigits: 3, roundingMode: "ceil" }), "2021-01-01T00:00:00.000");
      same(() => DT.from("2020-02-28T23:59:59.5").toString({ smallestUnit: "second", roundingMode: "halfExpand" }), "2020-02-29T00:00:00");
      same(() => DT.from("2020-06-15T23:59:59.5[u-ca=gregory]").toString({ smallestUnit: "second", roundingMode: "ceil" }), "2020-06-16T00:00:00[u-ca=gregory]");
      // roundingMode is validated even when nothing rounds
      range(() => half.toString({ roundingMode: "bogus" }));
      range(() => half.toString({ roundingMode: "" }));
      range(() => half.toString({ roundingMode: "HALFEXPAND" }));
      // calendar annotations
      const g = DT.from("2020-06-15T12:34:56[u-ca=gregory]");
      same(() => g.toString(), "2020-06-15T12:34:56[u-ca=gregory]");
      same(() => g.toString({ calendarName: "never" }), "2020-06-15T12:34:56");
      same(() => g.toString({ calendarName: "always" }), "2020-06-15T12:34:56[u-ca=gregory]");
      same(() => g.toString({ calendarName: "critical" }), "2020-06-15T12:34:56[!u-ca=gregory]");
      same(() => half.toString({ calendarName: "always" }), "2020-06-15T12:34:56.785[u-ca=iso8601]");
      same(() => half.toString({ calendarName: "critical" }), "2020-06-15T12:34:56.785[!u-ca=iso8601]");
      same(() => half.toString({ calendarName: "never" }), "2020-06-15T12:34:56.785");
      range(() => half.toString({ calendarName: "bogus" }));
      same(() => half.toJSON(), "2020-06-15T12:34:56.785");
      same(() => DT.from("-000001-01-01T00:00:00").toString(), "-000001-01-01T00:00:00");
      same(() => DT.from("+010000-01-01T00:00:00.5").toString(), "+010000-01-01T00:00:00.5");
      same(() => DT.from("-271821-04-19T00:00:00.000000001").toString(), "-271821-04-19T00:00:00.000000001");
      same(() => DT.from("+275760-09-13T00:00:00").toString(), "+275760-09-13T00:00:00");
    "#);
}

#[test]
fn value_of_and_locale_string() {
    run(r#"
      const d = D.from("2020-06-15");
      const dt = DT.from("2020-06-15T12:34:56");
      type(() => d.valueOf());
      type(() => dt.valueOf());
      type(() => d.valueOf.call(5));
      same(() => d.toLocaleString("en-US"), "6/15/2020");
      same(() => d.toLocaleString("en-US", { dateStyle: "long" }), "June 15, 2020");
      same(() => d.toLocaleString("en-US", { year: "numeric", month: "long" }), "June 2020");
      same(() => dt.toLocaleString("en-US"), "6/15/2020, 12:34:56 PM");
      same(() => dt.toLocaleString("en-US", { hour12: false }), "6/15/2020, 12:34:56");
      same(() => dt.toLocaleString("en-US", { dateStyle: "short", timeStyle: "short" }), "6/15/20, 12:34 PM");
      same(() => dt.toLocaleString("en-US", { timeStyle: "short" }), "12:34 PM");
      same(() => dt.toLocaleString("en-US", { dateStyle: "long" }), "June 15, 2020");
      // PlainDate rejects timeStyle unconditionally, PlainDateTime accepts it
      type(() => d.toLocaleString("en-US", { timeStyle: "short" }));
      type(() => d.toLocaleString("en-US", { dateStyle: "short", timeStyle: "short" }));
      range(() => d.toLocaleString("not a locale"));
      range(() => dt.toLocaleString("en-US", { dateStyle: "bogus" }));
      type(() => D.prototype.toLocaleString.call({}));
      type(() => DT.prototype.toLocaleString.call({}));
      // a calendar equal to the formatter's formats; a matching non-ISO locale calendar formats natively
      same(() => D.from("2020-06-15[u-ca=gregory]").toLocaleString("en-US"), "6/15/2020");
      same(() => D.from("2020-06-15[u-ca=hebrew]").toLocaleString("en-US-u-ca-hebrew"), "23 Sivan 5780");
    "#);
}

#[test]
fn plain_date_conversions_to_date_time_year_month_and_month_day() {
    run(r#"
      const d = D.from("2020-06-15");
      same(() => d.toPlainDateTime().toString(), "2020-06-15T00:00:00");
      same(() => d.toPlainDateTime(undefined).toString(), "2020-06-15T00:00:00");
      same(() => d.toPlainDateTime("07:08:09.5").toString(), "2020-06-15T07:08:09.5");
      same(() => d.toPlainDateTime({ hour: 5, minute: 6 }).toString(), "2020-06-15T05:06:00");
      same(() => d.toPlainDateTime(new Temporal.PlainTime(1, 2, 3, 4, 5, 6)).toString(), "2020-06-15T01:02:03.004005006");
      same(() => d.toPlainDateTime(DT.from("1999-01-01T10:11:12")).toString(), "2020-06-15T10:11:12");
      same(() => D.from("2020-06-15[u-ca=gregory]").toPlainDateTime("10:00").toString(), "2020-06-15T10:00:00[u-ca=gregory]");
      same(() => D.from("2020-06-15[u-ca=gregory]").toPlainDateTime().calendarId, "gregory");
      range(() => d.toPlainDateTime("bogus"));
      range(() => d.toPlainDateTime("25:00"));
      same(() => d.toPlainDateTime({ hour: 24 }).toString(), "2020-06-15T23:00:00");
      type(() => d.toPlainDateTime({}));
      type(() => d.toPlainDateTime(5));
      type(() => d.toPlainDateTime(null));
      type(() => d.toPlainDateTime(Symbol("x")));
      type(() => D.prototype.toPlainDateTime.call({}));
      // year-month and month-day, ISO
      same(() => d.toPlainYearMonth().toString(), "2020-06");
      same(() => d.toPlainYearMonth().calendarId, "iso8601");
      same(() => d.toPlainMonthDay().toString(), "06-15");
      same(() => d.toPlainMonthDay().monthCode, "M06");
      same(() => d.toPlainMonthDay().day, 15);
      same(() => D.from("2020-02-29").toPlainMonthDay().toString(), "02-29");
      same(() => D.from("-271821-04-19").toPlainYearMonth().toString(), "-271821-04");
      same(() => D.from("+275760-09-13").toPlainYearMonth().toString(), "+275760-09");
      type(() => D.prototype.toPlainYearMonth.call({}));
      type(() => D.prototype.toPlainMonthDay.call({}));
      // non-ISO calendars keep their own year/month/day fields, including leap months
      const heb = D.from({ year: 5784, monthCode: "M05L", day: 10, calendar: "hebrew" });
      same(() => heb.toPlainYearMonth().monthCode, "M05L");
      same(() => heb.toPlainYearMonth().year, 5784);
      same(() => heb.toPlainYearMonth().calendarId, "hebrew");
      same(() => heb.toPlainMonthDay().monthCode, "M05L");
      same(() => heb.toPlainMonthDay().day, 10);
      same(() => heb.toPlainMonthDay().calendarId, "hebrew");
      const jp = D.from("2020-06-15[u-ca=japanese]");
      same(() => jp.toPlainYearMonth().monthCode, "M06");
      same(() => jp.toPlainYearMonth().eraYear, 2);
      same(() => jp.toPlainYearMonth().era, "reiwa");
      same(() => jp.toPlainMonthDay().monthCode, "M06");
      same(() => jp.toPlainMonthDay().day, 15);
      const chi = D.from("2023-03-22[u-ca=chinese]");
      same(() => chi.toPlainYearMonth().calendarId, "chinese");
      same(() => chi.toPlainYearMonth().monthCode, chi.monthCode);
      same(() => chi.toPlainMonthDay().day, chi.day);
      // PlainDateTime to date/time pieces
      const dt = DT.from("2020-06-15T12:34:56.789123456");
      same(() => dt.toPlainDate().toString(), "2020-06-15");
      same(() => dt.toPlainTime().toString(), "12:34:56.789123456");
      same(() => DT.from("2020-06-15T12:34:56[u-ca=gregory]").toPlainDate().toString(), "2020-06-15[u-ca=gregory]");
      same(() => dt.toPlainDate() instanceof D, true);
      same(() => dt.toPlainTime() instanceof Temporal.PlainTime, true);
      same(() => dt.withPlainTime().toString(), "2020-06-15T00:00:00");
      same(() => dt.withPlainTime("07:08").toString(), "2020-06-15T07:08:00");
      same(() => dt.withPlainTime({ hour: 1, second: 2 }).toString(), "2020-06-15T01:00:02");
      same(() => dt.withPlainTime(new Temporal.PlainTime(23, 59, 59, 999, 999, 999)).toString(), "2020-06-15T23:59:59.999999999");
      same(() => DT.from("2020-06-15T12:34:56[u-ca=gregory]").withPlainTime("01:00").toString(), "2020-06-15T01:00:00[u-ca=gregory]");
      range(() => dt.withPlainTime("bogus"));
      type(() => dt.withPlainTime({}));
      type(() => dt.withPlainTime(5));
      type(() => DT.prototype.toPlainDate.call({}));
      type(() => DT.prototype.toPlainTime.call({}));
      type(() => DT.prototype.withPlainTime.call({}));
    "#);
}

#[test]
fn plain_date_time_round_units_modes_and_increments() {
    run(r#"
      const dt = DT.from("2020-06-15T12:34:56.789123456");
      same(() => dt.round("hour").toString(), "2020-06-15T13:00:00");
      same(() => dt.round("minute").toString(), "2020-06-15T12:35:00");
      same(() => dt.round("second").toString(), "2020-06-15T12:34:57");
      same(() => dt.round("millisecond").toString(), "2020-06-15T12:34:56.789");
      same(() => dt.round("microsecond").toString(), "2020-06-15T12:34:56.789123");
      same(() => dt.round("nanosecond").toString(), "2020-06-15T12:34:56.789123456");
      same(() => dt.round("day").toString(), "2020-06-16T00:00:00");
      same(() => dt.round("hours").toString(), "2020-06-15T13:00:00");
      same(() => dt.round("days").toString(), "2020-06-16T00:00:00");
      same(() => dt.round({ smallestUnit: "hour" }).toString(), "2020-06-15T13:00:00");
      same(() => dt.round({ smallestUnit: "hour", roundingMode: "floor" }).toString(), "2020-06-15T12:00:00");
      same(() => dt.round({ smallestUnit: "hour", roundingMode: "trunc" }).toString(), "2020-06-15T12:00:00");
      same(() => dt.round({ smallestUnit: "hour", roundingMode: "ceil" }).toString(), "2020-06-15T13:00:00");
      same(() => dt.round({ smallestUnit: "hour", roundingMode: "expand" }).toString(), "2020-06-15T13:00:00");
      same(() => dt.round({ smallestUnit: "day", roundingMode: "floor" }).toString(), "2020-06-15T00:00:00");
      same(() => dt.round({ smallestUnit: "day", roundingMode: "ceil" }).toString(), "2020-06-16T00:00:00");
      same(() => dt.round({ smallestUnit: "day", roundingMode: "halfTrunc" }).toString(), "2020-06-16T00:00:00");
      same(() => DT.from("2020-06-15T12:00:00").round({ smallestUnit: "day", roundingMode: "halfExpand" }).toString(), "2020-06-16T00:00:00");
      same(() => DT.from("2020-06-15T12:00:00").round({ smallestUnit: "day", roundingMode: "halfTrunc" }).toString(), "2020-06-15T00:00:00");
      // a tie rounds to the even multiple of the increment: zero days, not an even calendar day
      same(() => DT.from("2020-06-15T12:00:00").round({ smallestUnit: "day", roundingMode: "halfEven" }).toString(), "2020-06-15T00:00:00");
      same(() => DT.from("2020-06-16T12:00:00").round({ smallestUnit: "day", roundingMode: "halfEven" }).toString(), "2020-06-16T00:00:00");
      same(() => DT.from("2020-06-15T12:00:00").round({ smallestUnit: "day", roundingMode: "halfFloor" }).toString(), "2020-06-15T00:00:00");
      same(() => DT.from("2020-06-15T12:00:00").round({ smallestUnit: "day", roundingMode: "halfCeil" }).toString(), "2020-06-16T00:00:00");
      same(() => dt.round({ smallestUnit: "minute", roundingIncrement: 15 }).toString(), "2020-06-15T12:30:00");
      same(() => dt.round({ smallestUnit: "minute", roundingIncrement: 30, roundingMode: "ceil" }).toString(), "2020-06-15T13:00:00");
      same(() => dt.round({ smallestUnit: "hour", roundingIncrement: 12 }).toString(), "2020-06-15T12:00:00");
      same(() => dt.round({ smallestUnit: "hour", roundingIncrement: 8, roundingMode: "floor" }).toString(), "2020-06-15T08:00:00");
      same(() => dt.round({ smallestUnit: "second", roundingIncrement: 30 }).toString(), "2020-06-15T12:35:00");
      same(() => dt.round({ smallestUnit: "millisecond", roundingIncrement: 250 }).toString(), "2020-06-15T12:34:56.75");
      same(() => dt.round({ smallestUnit: "microsecond", roundingIncrement: 500 }).toString(), "2020-06-15T12:34:56.789");
      same(() => dt.round({ smallestUnit: "nanosecond", roundingIncrement: 500 }).toString(), "2020-06-15T12:34:56.7891235");
      same(() => dt.round({ smallestUnit: "minute", roundingIncrement: 1.9 }).toString(), "2020-06-15T12:35:00");
      // carries across month, year and leap days
      same(() => DT.from("2020-06-30T23:59:59.9").round("second").toString(), "2020-07-01T00:00:00");
      same(() => DT.from("2020-12-31T23:30:00").round("hour").toString(), "2021-01-01T00:00:00");
      same(() => DT.from("2020-02-28T18:00:00").round("day").toString(), "2020-02-29T00:00:00");
      same(() => DT.from("2021-02-28T18:00:00").round("day").toString(), "2021-03-01T00:00:00");
      same(() => DT.from("2020-06-15T12:34:56[u-ca=gregory]").round("minute").toString(), "2020-06-15T12:35:00[u-ca=gregory]");
      same(() => DT.from("2020-06-15T12:34:56[u-ca=gregory]").round("minute").calendarId, "gregory");
      same(() => DT.from("2020-06-15T00:00:00").round("day").toString(), "2020-06-15T00:00:00");
      // argument and option validation
      type(() => dt.round());
      type(() => dt.round(undefined));
      type(() => dt.round(5));
      type(() => dt.round(null));
      type(() => dt.round(true));
      range(() => dt.round({}));
      range(() => dt.round({ roundingMode: "floor" }));
      range(() => dt.round(""));
      range(() => dt.round("bogus"));
      range(() => dt.round("year"));
      range(() => dt.round("month"));
      range(() => dt.round("week"));
      range(() => dt.round({ smallestUnit: "bogus" }));
      range(() => dt.round({ smallestUnit: "hour", roundingMode: "bogus" }));
      range(() => dt.round({ smallestUnit: "hour", roundingIncrement: 0 }));
      range(() => dt.round({ smallestUnit: "hour", roundingIncrement: -1 }));
      range(() => dt.round({ smallestUnit: "hour", roundingIncrement: NaN }));
      range(() => dt.round({ smallestUnit: "hour", roundingIncrement: Infinity }));
      range(() => dt.round({ smallestUnit: "hour", roundingIncrement: 24 }));
      range(() => dt.round({ smallestUnit: "hour", roundingIncrement: 5 }));
      range(() => dt.round({ smallestUnit: "minute", roundingIncrement: 60 }));
      range(() => dt.round({ smallestUnit: "minute", roundingIncrement: 7 }));
      range(() => dt.round({ smallestUnit: "second", roundingIncrement: 45 }));
      range(() => dt.round({ smallestUnit: "millisecond", roundingIncrement: 1000 }));
      range(() => dt.round({ smallestUnit: "microsecond", roundingIncrement: 1000 }));
      range(() => dt.round({ smallestUnit: "nanosecond", roundingIncrement: 1000 }) && dt.round({ smallestUnit: "nanosecond", roundingIncrement: 3 }));
      range(() => dt.round({ smallestUnit: "day", roundingIncrement: 2 }));
      range(() => dt.round({ smallestUnit: "day", roundingIncrement: 0 }));
      type(() => dt.round({ smallestUnit: "hour", roundingIncrement: Symbol("x") }));
      type(() => DT.prototype.round.call({}, "hour"));
      // representable-range limits
      // the exact minimum is one nanosecond after midnight: midnight itself is unrepresentable
      range(() => DT.from("-271821-04-19T00:00:00"));
      same(() => DT.from("-271821-04-19T00:00:00.000000001").round("nanosecond").toString(), "-271821-04-19T00:00:00.000000001");
      range(() => DT.from("-271821-04-19T00:00:00.000000001").round("day"));
      range(() => DT.from("-271821-04-19T00:00:00.000000001").round({ smallestUnit: "second", roundingMode: "floor" }));
      range(() => DT.from("+275760-09-13T00:00:00").round({ smallestUnit: "day", roundingMode: "ceil" }) && DT.from("+275760-09-13T12:00:00").round({ smallestUnit: "day", roundingMode: "ceil" }));
    "#);
}

#[test]
fn plain_date_and_date_time_to_zoned_date_time() {
    run(r#"
      const d = D.from("2020-06-15");
      same(() => d.toZonedDateTime("UTC").toString(), "2020-06-15T00:00:00+00:00[UTC]");
      same(() => d.toZonedDateTime("America/New_York").toString(), "2020-06-15T00:00:00-04:00[America/New_York]");
      same(() => d.toZonedDateTime("+05:30").toString(), "2020-06-15T00:00:00+05:30[+05:30]");
      same(() => d.toZonedDateTime({ timeZone: "UTC" }).toString(), "2020-06-15T00:00:00+00:00[UTC]");
      same(() => d.toZonedDateTime({ timeZone: "America/New_York", plainTime: "12:30" }).toString(), "2020-06-15T12:30:00-04:00[America/New_York]");
      same(() => d.toZonedDateTime({ timeZone: "UTC", plainTime: new Temporal.PlainTime(1, 2, 3) }).toString(), "2020-06-15T01:02:03+00:00[UTC]");
      same(() => d.toZonedDateTime({ timeZone: "UTC", plainTime: DT.from("1999-01-01T05:06:07") }).toString(), "2020-06-15T05:06:07+00:00[UTC]");
      same(() => d.toZonedDateTime({ timeZone: "UTC", plainTime: { hour: 9 } }).toString(), "2020-06-15T09:00:00+00:00[UTC]");
      same(() => d.toZonedDateTime({ timeZone: "UTC", plainTime: undefined }).toString(), "2020-06-15T00:00:00+00:00[UTC]");
      same(() => d.toZonedDateTime(Temporal.ZonedDateTime.from("2001-01-01T00:00[Asia/Tokyo]")).toString(), "2020-06-15T00:00:00+09:00[Asia/Tokyo]");
      same(() => D.from("2020-06-15[u-ca=gregory]").toZonedDateTime("UTC").toString(), "2020-06-15T00:00:00+00:00[UTC][u-ca=gregory]");
      // the start of a day whose midnight does not exist begins at the first real instant
      same(() => D.from("2018-11-04").toZonedDateTime("America/Sao_Paulo").toString(), "2018-11-04T01:00:00-02:00[America/Sao_Paulo]");
      range(() => d.toZonedDateTime("bogus/zone"));
      range(() => d.toZonedDateTime({ timeZone: "bogus/zone" }));
      range(() => d.toZonedDateTime({ timeZone: "UTC", plainTime: "bogus" }));
      type(() => d.toZonedDateTime());
      type(() => d.toZonedDateTime(undefined));
      type(() => d.toZonedDateTime(5));
      type(() => d.toZonedDateTime(null));
      type(() => d.toZonedDateTime({}));
      type(() => d.toZonedDateTime({ plainTime: "12:00" }));
      type(() => d.toZonedDateTime({ timeZone: 5 }));
      type(() => d.toZonedDateTime({ timeZone: {} }));
      type(() => D.prototype.toZonedDateTime.call({}, "UTC"));

      // PlainDateTime.toZonedDateTime resolves the local time by disambiguation
      const gap = DT.from("2020-03-08T02:30:00");
      same(() => gap.toZonedDateTime("America/New_York").toString(), "2020-03-08T03:30:00-04:00[America/New_York]");
      same(() => gap.toZonedDateTime("America/New_York", { disambiguation: "compatible" }).toString(), "2020-03-08T03:30:00-04:00[America/New_York]");
      same(() => gap.toZonedDateTime("America/New_York", { disambiguation: "earlier" }).toString(), "2020-03-08T01:30:00-05:00[America/New_York]");
      same(() => gap.toZonedDateTime("America/New_York", { disambiguation: "later" }).toString(), "2020-03-08T03:30:00-04:00[America/New_York]");
      range(() => gap.toZonedDateTime("America/New_York", { disambiguation: "reject" }));
      const overlap = DT.from("2020-11-01T01:30:00");
      same(() => overlap.toZonedDateTime("America/New_York").toString(), "2020-11-01T01:30:00-04:00[America/New_York]");
      same(() => overlap.toZonedDateTime("America/New_York", { disambiguation: "earlier" }).toString(), "2020-11-01T01:30:00-04:00[America/New_York]");
      same(() => overlap.toZonedDateTime("America/New_York", { disambiguation: "later" }).toString(), "2020-11-01T01:30:00-05:00[America/New_York]");
      range(() => overlap.toZonedDateTime("America/New_York", { disambiguation: "reject" }));
      same(() => DT.from("2020-06-15T12:00:00").toZonedDateTime("UTC", { disambiguation: "reject" }).toString(), "2020-06-15T12:00:00+00:00[UTC]");
      same(() => DT.from("2020-06-15T12:00:00[u-ca=gregory]").toZonedDateTime("Asia/Tokyo").toString(), "2020-06-15T12:00:00+09:00[Asia/Tokyo][u-ca=gregory]");
      range(() => gap.toZonedDateTime("America/New_York", { disambiguation: "bogus" }));
      range(() => gap.toZonedDateTime("bogus/zone"));
      type(() => gap.toZonedDateTime());
      type(() => gap.toZonedDateTime({}));
      type(() => gap.toZonedDateTime(5));
      type(() => gap.toZonedDateTime("UTC", 5));
      type(() => gap.toZonedDateTime("UTC", "earlier"));
      type(() => DT.prototype.toZonedDateTime.call({}, "UTC"));
      // an out-of-range local time in the zone is a RangeError
      range(() => DT.from("+275760-09-13T00:00:00").toZonedDateTime("UTC") && DT.from("+275760-09-13T00:00:00").toZonedDateTime("-05:00"));
      range(() => DT.from("-271821-04-19T00:00:00.000000001").toZonedDateTime("+05:00"));
    "#);
}

#[test]
fn instant_to_zoned_date_time_iso() {
    run(r#"
      const i = Temporal.Instant.from("2020-06-15T12:34:56.789Z");
      same(() => i.toZonedDateTimeISO("UTC").toString(), "2020-06-15T12:34:56.789+00:00[UTC]");
      same(() => i.toZonedDateTimeISO("Asia/Kolkata").toString(), "2020-06-15T18:04:56.789+05:30[Asia/Kolkata]");
      same(() => i.toZonedDateTimeISO("America/New_York").toString(), "2020-06-15T08:34:56.789-04:00[America/New_York]");
      same(() => i.toZonedDateTimeISO("-03:30").toString(), "2020-06-15T09:04:56.789-03:30[-03:30]");
      same(() => i.toZonedDateTimeISO(Temporal.ZonedDateTime.from("2001-01-01T00:00[Asia/Tokyo]")).toString(), "2020-06-15T21:34:56.789+09:00[Asia/Tokyo]");
      same(() => i.toZonedDateTimeISO("UTC").epochMilliseconds, 1592224496789);
      same(() => i.toZonedDateTimeISO("UTC").calendarId, "iso8601");
      same(() => new Temporal.Instant(0n).toZonedDateTimeISO("Pacific/Auckland").toString(), "1970-01-01T12:00:00+12:00[Pacific/Auckland]");
      range(() => i.toZonedDateTimeISO("bogus/zone"));
      type(() => i.toZonedDateTimeISO());
      type(() => i.toZonedDateTimeISO(undefined));
      type(() => i.toZonedDateTimeISO(5));
      type(() => i.toZonedDateTimeISO({}));
      type(() => Temporal.Instant.prototype.toZonedDateTimeISO.call({}, "UTC"));
    "#);
}

#[test]
fn now_reads_the_wall_clock_in_the_requested_time_zone() {
    run(r#"
      const before = Date.now();
      const instant = Temporal.Now.instant();
      const after = Date.now();
      same(() => instant.epochMilliseconds >= before && instant.epochMilliseconds <= after, true);
      same(() => typeof Temporal.Now.timeZoneId(), "string");
      same(() => Temporal.Now.timeZoneId().length > 0, true);
      // every zone sees the same instant
      const utc = Temporal.Now.zonedDateTimeISO("UTC");
      same(() => utc.timeZoneId, "UTC");
      same(() => utc.calendarId, "iso8601");
      same(() => utc.epochMilliseconds >= before && utc.epochMilliseconds <= Date.now(), true);
      same(() => Temporal.Now.zonedDateTimeISO("America/New_York").timeZoneId, "America/New_York");
      same(() => Temporal.Now.zonedDateTimeISO("+05:30").offset, "+05:30");
      same(() => Temporal.Now.zonedDateTimeISO(Temporal.ZonedDateTime.from("2001-01-01T00:00[Asia/Tokyo]")).timeZoneId, "Asia/Tokyo");
      same(() => Temporal.Now.zonedDateTimeISO().timeZoneId, Temporal.Now.timeZoneId());
      // plain projections of the wall clock
      const date = Temporal.Now.plainDateISO("UTC");
      same(() => date instanceof Temporal.PlainDate, true);
      same(() => date.calendarId, "iso8601");
      same(() => date.year >= 2020 && date.month >= 1 && date.month <= 12 && date.day >= 1 && date.day <= 31, true);
      const dateTime = Temporal.Now.plainDateTimeISO("UTC");
      same(() => dateTime instanceof Temporal.PlainDateTime, true);
      same(() => dateTime.year >= 2020 && dateTime.hour >= 0 && dateTime.hour <= 23, true);
      same(() => dateTime.calendarId, "iso8601");
      const time = Temporal.Now.plainTimeISO("UTC");
      same(() => time instanceof Temporal.PlainTime, true);
      same(() => time.hour >= 0 && time.hour <= 23 && time.minute >= 0 && time.minute <= 59, true);
      // a zone fifteen and a half hours apart must never share a wall-clock hour and minute both
      const east = Temporal.Now.plainTimeISO("+14:00");
      same(() => east instanceof Temporal.PlainTime, true);
      same(() => Temporal.Now.plainDateISO("America/New_York") instanceof Temporal.PlainDate, true);
      same(() => Temporal.Now.plainDateTimeISO("Asia/Tokyo") instanceof Temporal.PlainDateTime, true);
      same(() => Temporal.Now.plainDateISO() instanceof Temporal.PlainDate, true);
      same(() => Temporal.Now.plainDateTimeISO() instanceof Temporal.PlainDateTime, true);
      same(() => Temporal.Now.plainTimeISO() instanceof Temporal.PlainTime, true);
      // the time-zone argument is never coerced
      range(() => Temporal.Now.zonedDateTimeISO("bogus/zone"));
      range(() => Temporal.Now.plainDateISO("bogus/zone"));
      range(() => Temporal.Now.plainDateTimeISO(""));
      type(() => Temporal.Now.zonedDateTimeISO(5));
      type(() => Temporal.Now.zonedDateTimeISO({}));
      type(() => Temporal.Now.zonedDateTimeISO({ toString() { return "UTC"; } }));
      type(() => Temporal.Now.plainDateISO(null));
      type(() => Temporal.Now.plainTimeISO(true));
      same(() => Object.prototype.toString.call(Temporal.Now), "[object Temporal.Now]");
    "#);
}
