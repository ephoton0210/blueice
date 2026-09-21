// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Coverage for `Temporal.PlainDate.prototype.until`/`since` in every
//! non-ISO calendar (`vm/temporal/plain_date.rs`'s fixed-month and leap-month
//! calendar difference algorithms): the algebraic properties the
//! specification guarantees for a calendar difference -- adding the difference
//! back to the start gives the end, `since` is the negation of `until`,
//! `largestUnit: "years"` never leaves twelve or more whole months over, and
//! `largestUnit: "months"` never reports years -- checked over many date pairs
//! per calendar, plus exact values where a calendar's own month lengths are
//! irrelevant (same-day-of-month differences).
//!
//! Every assertion states behaviour the ECMAScript Temporal specification
//! requires; each test runs a batch of cases inside one script and reports
//! every failing case (with its source line) at once.

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
    run_raw(body);
}

fn run_raw(body: &str) {
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

const CALENDARS: &str = r#"
const twelveMonth = ["indian", "islamic-civil", "islamic-tbla", "islamic-umalqura", "persian",
                     "gregory", "roc", "buddhist", "japanese"];
const thirteenMonth = ["coptic", "ethiopic", "ethioaa"];
const leapMonth = ["hebrew", "chinese", "dangi"];
const starts = ["2019-01-01", "2019-03-15", "2020-02-29", "2020-06-30", "2021-01-31", "2021-12-31", "2022-07-04", "2023-09-30"];
const ends = ["2019-01-01", "2019-02-28", "2019-03-31", "2020-01-01", "2020-08-15", "2021-03-01", "2021-12-31",
              "2022-05-31", "2023-02-28", "2024-02-29", "2025-11-30", "2027-01-01"];
"#;

/// The properties every calendar difference must satisfy, checked for each
/// calendar named in `calendars`. `whole_months` additionally checks
/// `largestUnit: "months"`, which is only meaningful when a year has a fixed
/// number of months.
fn check_fixed_month_calendars(calendars: &str, whole_months: bool) {
    let script = format!(
        "{CALENDARS}const calendars = {calendars}; const wholeMonths = {whole_months};\n{}",
        r#"
      for (const calendar of calendars) {
        CTX = calendar;
        for (const s of starts) {
          for (const e of ends) {
            const a = D.from(s).withCalendar(calendar);
            const b = D.from(e).withCalendar(calendar);
            const sign = D.compare(a, b) < 0 ? 1 : D.compare(a, b) > 0 ? -1 : 0;
            const years = a.until(b, { largestUnit: "years" });
            CTX = calendar + " #1";
            same(() => years.sign, sign);
            if (wholeMonths) {
              CTX = calendar + " #2";
              same(() => a.add(years).equals(b), true);
              CTX = calendar + " #3";
              same(() => Math.abs(years.months) < a.monthsInYear, true);
            }
            CTX = calendar + " #4";
            same(() => a.since(b, { largestUnit: "years" }).toString(), years.negated().toString());
            const days = a.until(b);
            CTX = calendar + " #5";
            same(() => days.years === 0 && days.months === 0 && days.weeks === 0, true);
            CTX = calendar + " #6";
            same(() => a.add(days).equals(b), true);
            CTX = calendar + " #7";
            same(() => a.until(b, { largestUnit: "weeks" }).sign, sign);
            CTX = calendar + " #8";
            same(() => a.add(a.until(b, { largestUnit: "weeks" })).equals(b), true);
            if (wholeMonths) {
              const months = a.until(b, { largestUnit: "months" });
              CTX = calendar + " #9";
              same(() => months.years, 0);
              CTX = calendar + " #10";
              same(() => a.add(months).equals(b), true);
              CTX = calendar + " #11";
              same(() => months.sign, sign);
              CTX = calendar + " #12";
              same(() => a.since(b, { largestUnit: "months" }).toString(), months.negated().toString());
            }
          }
        }
      }
    "#
    );
    run_raw(&script);
}

#[test]
fn round_trip_and_sign_properties_hold_in_islamic_calendars() {
    check_fixed_month_calendars(
        r#"["islamic-civil", "islamic-tbla", "islamic-umalqura"]"#,
        true,
    );
}

#[test]
fn round_trip_and_sign_properties_hold_in_indian_and_persian_calendars() {
    check_fixed_month_calendars(r#"["indian", "persian"]"#, true);
}

#[test]
fn round_trip_and_sign_properties_hold_in_gregorian_variant_calendars() {
    check_fixed_month_calendars(r#"["gregory", "roc", "buddhist", "japanese"]"#, true);
}

#[test]
fn round_trip_and_sign_properties_hold_in_thirteen_month_calendars() {
    // A year here has thirteen months (twelve 30-day months plus a 5/6-day
    // intercalary one), so the full property set applies, including the
    // year- and month-based round trip (`a.add(a.until(b, { largestUnit })) ==
    // b`). See `temporal_thirteen_month_calendar_difference.rs` for the
    // fixture-derived expectations.
    check_fixed_month_calendars(r#"["coptic", "ethiopic", "ethioaa"]"#, true);
}

#[test]
fn round_trip_and_sign_properties_hold_in_leap_month_calendars() {
    let script = format!(
        "{CALENDARS}{}",
        r#"
      for (const calendar of leapMonth) {
        CTX = calendar;
        for (const s of starts) {
          for (const e of ends) {
            const a = D.from(s).withCalendar(calendar);
            const b = D.from(e).withCalendar(calendar);
            const sign = D.compare(a, b) < 0 ? 1 : D.compare(a, b) > 0 ? -1 : 0;
            const years = a.until(b, { largestUnit: "years" });
            CTX = calendar + " #1";
            same(() => years.sign, sign);
            CTX = calendar + " #2";
            same(() => a.add(years).equals(b), true);
            CTX = calendar + " #3";
            same(() => a.since(b, { largestUnit: "years" }).toString(), years.negated().toString());
            const months = a.until(b, { largestUnit: "months" });
            CTX = calendar + " #5";
            same(() => months.years, 0);
            CTX = calendar + " #6";
            same(() => months.sign, sign);
            CTX = calendar + " #7";
            same(() => a.add(months).equals(b), true);
            CTX = calendar + " #8";
            same(() => a.since(b, { largestUnit: "months" }).toString(), months.negated().toString());
            CTX = calendar + " #9";
            same(() => a.add(a.until(b, { largestUnit: "weeks" })).equals(b), true);
            CTX = calendar + " #10";
            same(() => a.add(a.until(b)).equals(b), true);
          }
        }
      }
    "#
    );
    run_raw(&script);
}

#[test]
fn same_day_of_month_differences_have_exact_values() {
    run(r#"
      const cals = ["coptic", "ethiopic", "ethioaa", "indian", "islamic-civil", "islamic-tbla", "islamic-umalqura",
                    "persian", "gregory", "roc", "buddhist", "japanese", "hebrew"];
      for (const calendar of cals) {
        CTX = calendar;
        const at = (y, m, d) => D.from({ year: y, month: m, day: d, calendar });
        const a = at(1400, 1, 15);
        same(() => a.until(at(1402, 3, 15), { largestUnit: "years" }).toString(), "P2Y2M");
        same(() => at(1402, 3, 15).until(a, { largestUnit: "years" }).toString(), "-P2Y2M");
        same(() => at(1402, 3, 15).since(a, { largestUnit: "years" }).toString(), "P2Y2M");
        same(() => a.since(at(1402, 3, 15), { largestUnit: "years" }).toString(), "-P2Y2M");
        same(() => a.until(at(1401, 1, 15), { largestUnit: "years" }).toString(), "P1Y");
        same(() => a.until(at(1400, 2, 15), { largestUnit: "years" }).toString(), "P1M");
        same(() => a.until(at(1400, 1, 15), { largestUnit: "years" }).toString(), "PT0S");
        same(() => a.until(at(1400, 1, 22), { largestUnit: "weeks" }).toString(), "P1W");
        same(() => a.until(at(1400, 1, 22)).toString(), "P7D");
        same(() => a.until(at(1400, 1, 16), { largestUnit: "years" }).toString(), "P1D");
        same(() => at(1400, 1, 16).until(a, { largestUnit: "years" }).toString(), "-P1D");
      }
      // twelve-month calendars agree exactly on a whole number of months
      for (const calendar of ["indian", "islamic-civil", "islamic-tbla", "islamic-umalqura", "persian", "gregory", "roc", "buddhist", "japanese"]) {
        CTX = calendar;
        const at = (y, m, d) => D.from({ year: y, month: m, day: d, calendar });
        same(() => at(1400, 1, 15).until(at(1402, 3, 15), { largestUnit: "months" }).toString(), "P26M");
        same(() => at(1402, 3, 15).until(at(1400, 1, 15), { largestUnit: "months" }).toString(), "-P26M");
        same(() => at(1400, 1, 1).until(at(1401, 1, 1), { largestUnit: "months" }).toString(), "P12M");
      }
      // thirteen-month calendars: whole years and months below a year
      for (const calendar of ["coptic", "ethiopic", "ethioaa"]) {
        CTX = calendar;
        const at = (y, m, d) => D.from({ year: y, month: m, day: d, calendar });
        same(() => at(1400, 1, 15).monthsInYear, 13);
        same(() => at(1400, 1, 15).until(at(1400, 13, 3), { largestUnit: "years" }).toString(), "P11M18D");
        same(() => at(1400, 1, 15).until(at(1401, 13, 3), { largestUnit: "years" }).toString(), "P1Y11M18D");
      }
    "#);
}
