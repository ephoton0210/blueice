// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Coverage for `Temporal.PlainDate` / `Temporal.PlainDateTime` construction
//! and conversion (`vm/temporal/conversion.rs` and the `ToTemporalDate` /
//! `ToTemporalDateTime` half of `vm/temporal/dates.rs`): the constructors,
//! `from` with strings, property bags, existing Temporal objects and option
//! bags, calendar-aware field resolution, `withCalendar`, the receiver
//! brand checks and the calendar/field getters.
//!
//! Every assertion states behaviour the ECMAScript Temporal specification
//! requires; each test runs a whole batch of cases inside one script and
//! reports every failing case at once.

use blueice_bluejs::{compile, parse, Value, Vm};

const PRELUDE: &str = r#"
const fails = [];
let L = 0;
function nameOf(e) {
  if (e instanceof RangeError) return "RangeError";
  if (e instanceof TypeError) return "TypeError";
  if (e instanceof SyntaxError) return "SyntaxError";
  return "other:" + String(e);
}
function thrown(f) { try { f(); } catch (e) { return nameOf(e); } return "none"; }
function range(f) { const t = thrown(f); if (t !== "RangeError") fails.push("@" + L + " " + " -> " + t); }
function type(f) { const t = thrown(f); if (t !== "TypeError") fails.push("@" + L + " " + " -> " + t); }
function same(f, expected) {
  let actual;
  try { actual = f(); } catch (e) { actual = "threw " + nameOf(e); }
  if (actual !== expected) fails.push("@" + L + " " + " => " + String(actual) + " !== " + String(expected));
}
const D = Temporal.PlainDate;
const DT = Temporal.PlainDateTime;
function getter(ctor, name) { return Object.getOwnPropertyDescriptor(ctor.prototype, name).get; }
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
fn constructors_validate_every_argument() {
    run(r#"
      type(() => D(2020, 1, 1));
      type(() => DT(2020, 1, 1));
      range(() => new D(2020, 13, 1));
      range(() => new D(2020, 0, 1));
      range(() => new D(2020, 1, 0));
      range(() => new D(2020, 1, 32));
      range(() => new D(2020, 2, 30));
      range(() => new D(2021, 2, 29));
      range(() => new D(NaN, 1, 1));
      range(() => new D(Infinity, 1, 1));
      range(() => new D(2020, -Infinity, 1));
      range(() => new D(2020, 1, Infinity));
      range(() => new D(275761, 1, 1));
      range(() => new D(-271822, 1, 1));
      range(() => new D(2020, 1, 1, "bogus"));
      range(() => new D(2020, 1, 1, "2020-01-01"));
      type(() => new D(2020, 1, 1, 5));
      type(() => new D(2020, 1, 1, null));
      type(() => new D(2020, 1, 1, {}));
      type(() => new D(2020, 1, 1, true));
      same(() => new D(2020.9, 1.9, 1.9).toString(), "2020-01-01");
      same(() => new D("2020", "2", "29").toString(), "2020-02-29");
      same(() => new D(2020, 1, 1, "GREGORY").calendarId, "gregory");
      same(() => new D(2020, 1, 1, "ISO8601").calendarId, "iso8601");
      same(() => new D(2020, 1, 1, undefined).calendarId, "iso8601");
      same(() => new D(-271821, 4, 19).toString(), "-271821-04-19");
      same(() => new D(275760, 9, 13).toString(), "+275760-09-13");

      range(() => new DT(2020, 1, 1, 24));
      range(() => new DT(2020, 1, 1, 0, 60));
      range(() => new DT(2020, 1, 1, 0, 0, 60));
      range(() => new DT(2020, 1, 1, 0, 0, 0, 1000));
      range(() => new DT(2020, 1, 1, 0, 0, 0, 0, 1000));
      range(() => new DT(2020, 1, 1, 0, 0, 0, 0, 0, 1000));
      range(() => new DT(2020, 1, 1, -1));
      range(() => new DT(2020, 1, 1, 0, 0, 0, 0, 0, 0, "bogus"));
      type(() => new DT(2020, 1, 1, 0, 0, 0, 0, 0, 0, 7));
      range(() => new DT(2020, 2, 30));
      same(() => new DT(2020, 1, 1, 12, 30, 45, 123, 456, 789).toString(), "2020-01-01T12:30:45.123456789");
      same(() => new DT(2020, 1, 1, 12.9, 30.9).toString(), "2020-01-01T12:30:00");
      same(() => new DT(2020, 1, 1, 0, 0, 0, 0, 0, 0, "gregory").calendarId, "gregory");
      same(() => new DT(2020, 1, 1).toString(), "2020-01-01T00:00:00");
    "#);
}

#[test]
fn from_a_string_parses_or_throws_range_error() {
    run(r#"
      same(() => D.from("2020-01-31").toString(), "2020-01-31");
      same(() => D.from("20200131").toString(), "2020-01-31");
      same(() => D.from("2020-01-31T12:34:56").toString(), "2020-01-31");
      same(() => D.from("2020-01-31T12:34:56+01:00").toString(), "2020-01-31");
      same(() => D.from("2020-01-31[u-ca=gregory]").calendarId, "gregory");
      same(() => D.from("2020-01-31[u-ca=gregory]").toString(), "2020-01-31[u-ca=gregory]");
      same(() => D.from("2020-01-31[u-ca=iso8601]").calendarId, "iso8601");
      same(() => D.from("+002020-01-31").toString(), "2020-01-31");
      same(() => D.from("-271821-04-19").toString(), "-271821-04-19");
      same(() => D.from("+275760-09-13").toString(), "+275760-09-13");
      range(() => D.from("-271821-04-18"));
      range(() => D.from("+275760-09-14"));
      range(() => D.from("2020-02-30"));
      range(() => D.from("2020-13-01"));
      range(() => D.from("2020-01-31Z"));
      range(() => D.from("garbage"));
      range(() => D.from(""));
      range(() => D.from("2020-01-31[u-ca=bogus]"));
      range(() => D.from("-000000-01-01"));
      range(() => D.from("2020-01-31", { overflow: "bad" }));
      type(() => D.from("2020-01-31", 5));
      type(() => D.from("2020-01-31", "reject"));
      same(() => D.from("2020-01-31", { overflow: "reject" }).toString(), "2020-01-31");
      same(() => D.from("2020-01-31", undefined).toString(), "2020-01-31");

      same(() => DT.from("2020-01-31").toString(), "2020-01-31T00:00:00");
      same(() => DT.from("2020-01-31T12:34:56.789").toString(), "2020-01-31T12:34:56.789");
      same(() => DT.from("2020-01-31t12:34").toString(), "2020-01-31T12:34:00");
      same(() => DT.from("2020-01-31 12:34:56").toString(), "2020-01-31T12:34:56");
      same(() => DT.from("2020-01-31T12:34:56.123456789+05:30[u-ca=gregory]").toString(),
           "2020-01-31T12:34:56.123456789[u-ca=gregory]");
      range(() => DT.from("2020-01-31T12:34:56Z"));
      range(() => DT.from("2020-01-31T25:00"));
      range(() => DT.from("-271821-04-19T00:00:00"));
      same(() => DT.from("-271821-04-19T00:00:00.000000001").toString(), "-271821-04-19T00:00:00.000000001");
      range(() => DT.from("+275760-09-13T23:59:59.999999999X"));
      same(() => DT.from("+275760-09-13T23:59:59.999999999").toString(), "+275760-09-13T23:59:59.999999999");
      range(() => DT.from("+275760-09-14T00:00:00"));
      range(() => DT.from("2020-01-31T12:34:56", { overflow: "nope" }));
      type(() => DT.from("2020-01-31T12:34:56", null));
    "#);
}

#[test]
fn from_a_non_string_primitive_is_a_type_error() {
    run(r#"
      type(() => D.from(Symbol("x")));
      type(() => DT.from(Symbol("x")));
    "#);
}

#[test]
fn from_a_property_bag_requires_and_validates_fields() {
    run(r#"
      same(() => D.from({ year: 2020, month: 2, day: 29 }).toString(), "2020-02-29");
      same(() => D.from({ year: 2020, monthCode: "M02", day: 29 }).toString(), "2020-02-29");
      same(() => D.from({ year: 2020, month: 2, monthCode: "M02", day: 29 }).toString(), "2020-02-29");
      same(() => D.from({ year: "2020", month: "3", day: "4" }).toString(), "2020-03-04");
      same(() => D.from({ year: 2020.7, month: 3.7, day: 4.7 }).toString(), "2020-03-04");
      same(() => D.from({ year: 2020, month: 1, day: 1, hour: 99, era: "bce" }).toString(), "2020-01-01");
      // required fields
      type(() => D.from({}));
      type(() => D.from({ year: 2020, month: 1 }));
      type(() => D.from({ year: 2020, day: 1 }));
      type(() => D.from({ month: 1, day: 1 }));
      type(() => D.from({ monthCode: "M01", day: 1 }));
      type(() => D.from({ year: undefined, month: 1, day: 1 }));
      // range failures inside a field
      range(() => D.from({ year: Infinity, month: 1, day: 1 }));
      range(() => D.from({ year: NaN, month: 1, day: 1 }));
      range(() => D.from({ year: 2020, month: 0, day: 1 }));
      range(() => D.from({ year: 2020, month: -1, day: 1 }));
      range(() => D.from({ year: 2020, month: 1, day: 0 }));
      range(() => D.from({ year: 2020, month: 1, day: -3 }));
      range(() => D.from({ year: 2020, month: 1, day: Infinity }));
      range(() => D.from({ year: 275761, month: 1, day: 1 }));
      same(() => D.from({ year: 275760, month: 9, day: 13 }).toString(), "+275760-09-13");
      same(() => D.from({ year: -271821, month: 4, day: 19 }).toString(), "-271821-04-19");
      // month codes
      range(() => D.from({ year: 2020, monthCode: "M13", day: 1 }));
      range(() => D.from({ year: 2020, monthCode: "M00", day: 1 }));
      range(() => D.from({ year: 2020, monthCode: "m01", day: 1 }));
      range(() => D.from({ year: 2020, monthCode: "M1", day: 1 }));
      range(() => D.from({ year: 2020, monthCode: "M01L", day: 1 }));
      range(() => D.from({ year: 2020, monthCode: "M01L", day: 1 }, { overflow: "reject" }));
      range(() => D.from({ year: 2020, monthCode: "", day: 1 }));
      // `ToMonthCode` never stringifies: a non-String monthCode is a TypeError.
      type(() => D.from({ year: 2020, monthCode: 5, day: 1 }));
      type(() => D.from({ year: 2020, monthCode: Symbol("x"), day: 1 }));
      // conflicting month and monthCode
      range(() => D.from({ year: 2020, month: 1, monthCode: "M02", day: 1 }));
      // overflow handling
      same(() => D.from({ year: 2020, month: 2, day: 31 }).toString(), "2020-02-29");
      same(() => D.from({ year: 2021, month: 2, day: 31 }).toString(), "2021-02-28");
      same(() => D.from({ year: 2020, month: 4, day: 31 }, { overflow: "constrain" }).toString(), "2020-04-30");
      same(() => D.from({ year: 2020, month: 13, day: 1 }).toString(), "2020-12-01");
      same(() => D.from({ year: 2020, month: 13, day: 1 }, { overflow: "constrain" }).toString(), "2020-12-01");
      range(() => D.from({ year: 2020, month: 13, day: 1 }, { overflow: "reject" }));
      range(() => D.from({ year: 2020, month: 2, day: 30 }, { overflow: "reject" }));
      range(() => D.from({ year: 2020, month: 1, day: 1 }, { overflow: "bad" }));
      range(() => D.from({ year: 2020, month: 1, day: 1 }, { overflow: "" }));
      type(() => D.from({ year: 2020, month: 1, day: 1 }, 7));
      type(() => D.from({ year: 2020, month: 1, day: 1 }, "constrain"));
      same(() => D.from({ year: 2020, month: 1, day: 1 }, { overflow: "reject" }).toString(), "2020-01-01");
      // era fields
      type(() => D.from({ year: 2020, month: 1, day: 1, calendar: "gregory", era: Symbol("x"), eraYear: 1 }));
      type(() => D.from({ era: "ce", eraYear: 2020, month: 1, day: 1 }));
      // bad calendar values
      range(() => D.from({ year: 2020, month: 1, day: 1, calendar: "bogus" }));
      type(() => D.from({ year: 2020, month: 1, day: 1, calendar: 5 }));
      type(() => D.from({ year: 2020, month: 1, day: 1, calendar: null }));
      type(() => D.from({ year: 2020, month: 1, day: 1, calendar: {} }));
      type(() => D.from({ year: 2020, month: 1, day: 1, calendar: Symbol("x") }));
      same(() => D.from({ year: 2020, month: 1, day: 1, calendar: "2021-05-06[u-ca=gregory]" }).calendarId, "gregory");
      same(() => D.from({ year: 2020, month: 1, day: 1, calendar: "2021-05-06" }).calendarId, "iso8601");
      same(() => D.from({ year: 2020, month: 1, day: 1, calendar: "T10:00[u-ca=hebrew]" }).calendarId, "hebrew");
      same(() => D.from({ year: 2020, month: 1, day: 1, calendar: "12:34" }).calendarId, "iso8601");
      same(() => D.from({ year: 2020, month: 1, day: 1, calendar: "2021-05" }).calendarId, "iso8601");
      same(() => D.from({ year: 2020, month: 1, day: 1, calendar: "05-06" }).calendarId, "iso8601");
      same(() => D.from({ year: 2020, month: 1, day: 1, calendar: "05-06[u-ca=buddhist]" }).calendarId, "buddhist");
      same(() => D.from({ year: 2020, month: 1, day: 1, calendar: D.from("2000-01-01[u-ca=roc]") }).calendarId, "roc");
      range(() => D.from({ year: 2020, month: 1, day: 1, calendar: "not a calendar" }));
      range(() => D.from({ year: 2020, month: 1, day: 1, calendar: "2021-05-06[u-ca=bogus]" }));
      same(() => D.from({ year: 2020, month: 1, day: 1, calendar: undefined }).calendarId, "iso8601");
    "#);
}

#[test]
fn from_a_property_bag_for_plain_date_time_reads_time_fields() {
    run(r#"
      same(() => DT.from({ year: 2020, month: 1, day: 1 }).toString(), "2020-01-01T00:00:00");
      same(() => DT.from({ year: 2020, month: 1, day: 1, hour: 1, minute: 2, second: 3,
                           millisecond: 4, microsecond: 5, nanosecond: 6 }).toString(),
           "2020-01-01T01:02:03.004005006");
      same(() => DT.from({ year: 2020, month: 1, day: 1, second: 60 }).toString(), "2020-01-01T00:00:59");
      same(() => DT.from({ year: 2020, month: 1, day: 1, hour: "5", minute: "6.9" }).toString(), "2020-01-01T05:06:00");
      // `RegulateTime`: an out-of-range time field is clamped by the default
      // `overflow: "constrain"` and only rejected under `"reject"`.
      same(() => DT.from({ year: 2020, month: 1, day: 1, minute: 60 }).toString(), "2020-01-01T00:59:00");
      same(() => DT.from({ year: 2020, month: 1, day: 1, millisecond: 1000 }).toString(), "2020-01-01T00:00:00.999");
      same(() => DT.from({ year: 2020, month: 1, day: 1, microsecond: -1 }).toString(), "2020-01-01T00:00:00");
      same(() => DT.from({ year: 2020, month: 1, day: 1, nanosecond: 1000 }).toString(), "2020-01-01T00:00:00.000000999");
      same(() => DT.from({ year: 2020, month: 1, day: 1, hour: 24 }).toString(), "2020-01-01T23:00:00");
      range(() => DT.from({ year: 2020, month: 1, day: 1, minute: 60 }, { overflow: "reject" }));
      range(() => DT.from({ year: 2020, month: 1, day: 1, millisecond: 1000 }, { overflow: "reject" }));
      range(() => DT.from({ year: 2020, month: 1, day: 1, microsecond: -1 }, { overflow: "reject" }));
      range(() => DT.from({ year: 2020, month: 1, day: 1, nanosecond: 1000 }, { overflow: "reject" }));
      range(() => DT.from({ year: 2020, month: 1, day: 1, hour: Infinity }));
      type(() => DT.from({ year: 2020, month: 1 }));
      same(() => DT.from({ year: 2020, month: 1, day: 1, hour: 12 }, { overflow: "reject" }).hour, 12);
      same(() => DT.from({ year: 2020, month: 2, day: 30, hour: 12 }).toString(), "2020-02-29T12:00:00");
      range(() => DT.from({ year: 2020, month: 2, day: 30, hour: 12 }, { overflow: "reject" }));
      // a PlainDate as the bag source keeps its calendar and gains midnight
      same(() => DT.from(DT.from("2020-05-06T07:08:09")).toString(), "2020-05-06T07:08:09");
      same(() => DT.from(DT.from("2020-05-06T07:08:09"), { overflow: "reject" }).toString(), "2020-05-06T07:08:09");
      range(() => DT.from(DT.from("2020-05-06T07:08:09"), { overflow: "bad" }));
      type(() => DT.from(DT.from("2020-05-06T07:08:09"), 3));
      same(() => D.from(DT.from("2020-05-06T07:08:09")).toString(), "2020-05-06");
      same(() => D.from(D.from("2020-05-06[u-ca=gregory]")).toString(), "2020-05-06[u-ca=gregory]");
      range(() => D.from(D.from("2020-05-06"), { overflow: "bad" }));
      type(() => D.from(D.from("2020-05-06"), 3));
    "#);
}

#[test]
fn conversions_through_equals_accept_zoned_and_plain_values() {
    run(r#"
      const utc = new Temporal.ZonedDateTime(1_700_000_000_123_456_789n, "UTC");
      const off = new Temporal.ZonedDateTime(1_700_000_000_123_456_789n, "+05:30");
      const named = new Temporal.ZonedDateTime(1_700_000_000_123_456_789n, "Europe/Paris");
      // 2023-11-14T22:13:20.123456789Z
      same(() => D.from("2023-11-14").equals(utc), true);
      same(() => D.from("2023-11-15").equals(off), true);
      same(() => D.from("2023-11-14").equals(off), false);
      same(() => DT.from("2023-11-14T22:13:20.123456789").equals(utc), true);
      same(() => DT.from("2023-11-15T03:43:20.123456789").equals(off), true);
      same(() => D.from("2023-11-14").equals(DT.from("2023-11-14T01:02:03")), true);
      same(() => DT.from("2023-11-14T00:00:00").equals(D.from("2023-11-14")), true);
      same(() => D.compare(utc, "2023-11-14"), 0);
      same(() => D.compare(off, utc), 1);
      same(() => DT.compare(utc, off), -1);
      // a named IANA zone converts through the zoned value's stored local
      // fields: 2023-11-14T23:13:20.123456789 in Paris (UTC+1 in November)
      same(() => D.from("2023-11-14").equals(named), true);
      same(() => D.from("2023-11-15").equals(named), false);
      same(() => DT.from("2023-11-14T23:13:20.123456789").equals(named), true);
      same(() => DT.from("2023-11-14T22:13:20").equals(named), false);
      same(() => D.compare(named, "2023-11-14"), 0);
      // options are validated on the object fast paths too
      same(() => D.from("2023-11-14").equals({ year: 2023, month: 11, day: 14 }), true);
      same(() => DT.from("2023-11-14T01:02:03").equals({ year: 2023, month: 11, day: 14, hour: 1, minute: 2, second: 3 }), true);
      type(() => D.from("2023-11-14").equals(5));
      type(() => D.from("2023-11-14").equals(null));
      type(() => D.from("2023-11-14").equals(undefined));
      type(() => DT.from("2023-11-14T00:00").equals(true));
      type(() => DT.from("2023-11-14T00:00").equals(Symbol("x")));
      range(() => D.from("2023-11-14").equals("nonsense"));
      range(() => DT.from("2023-11-14T00:00").equals("nonsense"));
      type(() => D.compare(5, "2020-01-01"));
      type(() => D.compare("2020-01-01", null));
      range(() => D.compare("2020-01-01", "bad"));
      type(() => DT.compare("2020-01-01T00:00", 7));
      // a PlainMonthDay / PlainYearMonth is an object but not a date: read as a bag
      type(() => D.from("2020-01-01").equals(Temporal.PlainYearMonth.from("2020-01")));
      type(() => D.from("2020-01-01").equals(new Temporal.PlainTime(1, 2, 3)));
    "#);
}

#[test]
fn compare_and_equals_order_dates_and_date_times() {
    run(r#"
      same(() => D.compare("2020-01-01", "2020-01-02"), -1);
      same(() => D.compare("2020-01-02", "2020-01-01"), 1);
      same(() => D.compare("2020-01-01", "2020-01-01"), 0);
      same(() => D.compare("2019-12-31", "2020-01-01"), -1);
      same(() => D.compare("2020-02-01", "2020-01-31"), 1);
      same(() => D.compare({ year: 2020, month: 1, day: 1 }, "2020-01-01"), 0);
      same(() => DT.compare("2020-01-01T00:00:00.000000001", "2020-01-01T00:00:00"), 1);
      same(() => DT.compare("2020-01-01T00:00:00.000001", "2020-01-01T00:00:00.000002"), -1);
      same(() => DT.compare("2020-01-01T00:00:00.001", "2020-01-01T00:00:00.001"), 0);
      same(() => DT.compare("2020-01-01T10:00", "2020-01-01T09:59"), 1);
      same(() => DT.compare("2020-01-01T10:00:01", "2020-01-01T10:00:00"), 1);
      same(() => DT.compare("2020-01-02", "2020-01-01T23:59"), 1);
      // the calendar does not participate in compare, only the ISO fields
      same(() => D.compare("2020-01-01[u-ca=gregory]", "2020-01-01"), 0);
      // equals also compares calendars
      same(() => D.from("2020-01-01[u-ca=gregory]").equals("2020-01-01"), false);
      same(() => D.from("2020-01-01[u-ca=gregory]").equals("2020-01-01[u-ca=gregory]"), true);
      same(() => D.from("2020-01-01").equals("2020-01-02"), false);
      same(() => D.from("2020-01-01").equals("2020-02-01"), false);
      same(() => D.from("2020-01-01").equals("2021-01-01"), false);
      same(() => DT.from("2020-01-01T01:02:03.004005006").equals("2020-01-01T01:02:03.004005006"), true);
      for (const other of ["2020-01-01T00:02:03.004005006", "2020-01-01T01:00:03.004005006",
                           "2020-01-01T01:02:00.004005006", "2020-01-01T01:02:03.000005006",
                           "2020-01-01T01:02:03.004000006", "2020-01-01T01:02:03.004005000"]) {
        same(() => DT.from("2020-01-01T01:02:03.004005006").equals(other), false);
      }
      type(() => D.prototype.equals.call({}, "2020-01-01"));
      type(() => D.prototype.equals.call(new Temporal.PlainTime(), "2020-01-01"));
      type(() => D.prototype.equals.call(5, "2020-01-01"));
      type(() => DT.prototype.equals.call(Temporal.PlainYearMonth.from("2020-01"), "2020-01-01T00:00"));
    "#);
}

#[test]
fn receivers_are_brand_checked() {
    run(r#"
      const d = D.from("2020-05-06");
      const dt = DT.from("2020-05-06T07:08:09");
      for (const name of ["with", "add", "subtract", "until", "since", "equals", "toString", "toJSON",
                          "toLocaleString", "valueOf", "withCalendar", "toZonedDateTime",
                          "toPlainDateTime", "toPlainYearMonth", "toPlainMonthDay"]) {
        type(() => D.prototype[name].call({}));
        type(() => D.prototype[name].call(undefined));
        type(() => D.prototype[name].call(5));
        if (name !== "withCalendar") type(() => D.prototype[name].call(new Temporal.PlainTime()));
      }
      for (const name of ["with", "add", "subtract", "until", "since", "equals", "toString", "toJSON",
                          "toLocaleString", "valueOf", "withCalendar", "toZonedDateTime",
                          "toPlainDate", "toPlainTime", "withPlainTime", "round"]) {
        type(() => DT.prototype[name].call({}));
        type(() => DT.prototype[name].call(null));
        if (name !== "withCalendar") type(() => DT.prototype[name].call(new Temporal.Instant(0n)));
      }
      // a PlainDate method accepts a PlainDateTime receiver (shared adapter) only where the spec brand
      // allows it: the two prototypes are distinct objects, but their natives share one implementation
      same(() => typeof D.prototype.toString.call(d), "string");
      type(() => D.prototype.valueOf.call(d));
      type(() => DT.prototype.valueOf.call(dt));
      type(() => d + 1);
      type(() => dt < dt);
      same(() => JSON.stringify(d), '"2020-05-06"');
      same(() => JSON.stringify({ x: dt }), '{"x":"2020-05-06T07:08:09"}');
      same(() => d.toJSON(), "2020-05-06");
      same(() => Object.prototype.toString.call(d), "[object Temporal.PlainDate]");
      same(() => Object.prototype.toString.call(dt), "[object Temporal.PlainDateTime]");
    "#);
}

#[test]
fn getters_report_iso_fields_and_reject_wrong_receivers() {
    run(r#"
      const d = D.from("2024-02-29");
      same(() => d.year, 2024);
      same(() => d.month, 2);
      same(() => d.monthCode, "M02");
      same(() => d.day, 29);
      same(() => d.era, undefined);
      same(() => d.eraYear, undefined);
      same(() => d.dayOfWeek, 4);
      same(() => d.dayOfYear, 60);
      same(() => d.weekOfYear, 9);
      same(() => d.yearOfWeek, 2024);
      same(() => d.daysInWeek, 7);
      same(() => d.daysInMonth, 29);
      same(() => d.daysInYear, 366);
      same(() => d.monthsInYear, 12);
      same(() => d.inLeapYear, true);
      same(() => D.from("2023-01-01").weekOfYear, 52);
      same(() => D.from("2023-01-01").yearOfWeek, 2022);
      same(() => D.from("2021-01-03").weekOfYear, 53);
      same(() => D.from("2021-01-03").yearOfWeek, 2020);
      same(() => D.from("2021-01-04").weekOfYear, 1);
      same(() => D.from("2021-01-04").dayOfWeek, 1);
      same(() => D.from("2021-01-03").dayOfWeek, 7);
      same(() => D.from("2023-03-01").daysInMonth, 31);
      same(() => D.from("2023-04-01").daysInMonth, 30);
      same(() => D.from("2023-02-01").daysInMonth, 28);
      same(() => D.from("2023-02-01").daysInYear, 365);
      same(() => D.from("2023-02-01").inLeapYear, false);
      same(() => D.from("1900-02-01").inLeapYear, false);
      same(() => D.from("2000-02-01").inLeapYear, true);
      // extreme ISO years take the ISO fast path (no ICU conversion)
      same(() => D.from("-271821-04-19").year, -271821);
      same(() => D.from("-271821-04-19").monthCode, "M04");
      same(() => D.from("+275760-09-13").daysInMonth, 30);
      same(() => D.from("+275760-09-13").inLeapYear, true);

      const dt = DT.from("2024-02-29T13:14:15.123456789");
      same(() => dt.hour, 13);
      same(() => dt.minute, 14);
      same(() => dt.second, 15);
      same(() => dt.millisecond, 123);
      same(() => dt.microsecond, 456);
      same(() => dt.nanosecond, 789);
      same(() => dt.dayOfYear, 60);
      same(() => dt.calendarId, "iso8601");

      // wrong receivers / cross-type getter use
      for (const name of ["year", "month", "monthCode", "day", "era", "eraYear", "monthsInYear",
                          "dayOfWeek", "dayOfYear", "weekOfYear", "yearOfWeek", "daysInWeek",
                          "daysInMonth", "daysInYear", "inLeapYear", "calendarId"]) {
        const g = getter(D, name);
        type(() => g.call({}));
        type(() => g.call(undefined));
        type(() => g.call(5));
        if (name !== "calendarId") {
          type(() => g.call(new Temporal.PlainTime()));
          type(() => g.call(new Temporal.Duration(1)));
          type(() => g.call(new Temporal.Instant(0n)));
        }
      }
      for (const name of ["hour", "minute", "second", "millisecond", "microsecond", "nanosecond"]) {
        const g = getter(DT, name);
        type(() => g.call(d));
        type(() => g.call({}));
        type(() => g.call(Temporal.PlainYearMonth.from("2020-01")));
        type(() => g.call(new Temporal.Instant(0n)));
        // A time of day is not a `PlainDateTime`; only the exact type passes
        // the receiver brand check.
        type(() => g.call(new Temporal.PlainTime()));
        type(() => g.call(new Temporal.ZonedDateTime(0n, "UTC")));
        same(() => typeof g.call(dt), "number");
      }
      // day/year on receivers that lack that field
      const md = Temporal.PlainMonthDay.from("01-15");
      const ym = Temporal.PlainYearMonth.from("2020-03");
      type(() => getter(D, "year").call(md));
      type(() => getter(D, "day").call(ym));
      type(() => getter(D, "era").call(md));
      type(() => getter(D, "eraYear").call(md));
      type(() => getter(D, "monthsInYear").call(md));
      type(() => getter(D, "daysInMonth").call(md));
      type(() => getter(D, "daysInYear").call(md));
      type(() => getter(D, "inLeapYear").call(md));
      // `PlainDate`'s getters do not accept a `PlainMonthDay` or a
      // `PlainYearMonth` either, even for the fields both types have; those
      // are read through the getter of the type's own prototype.
      type(() => getter(D, "month").call(md));
      type(() => getter(D, "monthCode").call(md));
      type(() => getter(D, "day").call(md));
      type(() => getter(D, "year").call(ym));
      type(() => getter(D, "month").call(ym));
      type(() => getter(D, "daysInMonth").call(ym));
      type(() => getter(D, "monthsInYear").call(ym));
      // the specification gives PlainMonthDay no `month` accessor, only `monthCode`
      same(() => Object.getOwnPropertyDescriptor(Temporal.PlainMonthDay.prototype, "month"), undefined);
      same(() => getter(Temporal.PlainMonthDay, "monthCode").call(md), "M01");
      same(() => getter(Temporal.PlainMonthDay, "day").call(md), 15);
      same(() => getter(Temporal.PlainYearMonth, "year").call(ym), 2020);
      same(() => getter(Temporal.PlainYearMonth, "month").call(ym), 3);
      same(() => getter(Temporal.PlainYearMonth, "daysInMonth").call(ym), 31);
      same(() => getter(Temporal.PlainYearMonth, "monthsInYear").call(ym), 12);
      type(() => getter(D, "dayOfWeek").call(ym));
      type(() => getter(D, "dayOfYear").call(md));
      type(() => getter(D, "weekOfYear").call(md));
      type(() => getter(D, "yearOfWeek").call(ym));
      type(() => getter(D, "daysInWeek").call(ym));
      type(() => getter(D, "dayOfWeek").call(dt));
      type(() => getter(D, "daysInWeek").call(new Temporal.ZonedDateTime(0n, "UTC")));
      same(() => getter(DT, "dayOfWeek").call(dt), 4);
      same(() => getter(Temporal.ZonedDateTime, "daysInWeek").call(new Temporal.ZonedDateTime(0n, "UTC")), 7);
      // Instant / ZonedDateTime / Duration accessors given the wrong receiver
      type(() => getter(Temporal.Instant, "epochMilliseconds").call(d));
      type(() => getter(Temporal.Instant, "epochNanoseconds").call(d));
      type(() => getter(Temporal.ZonedDateTime, "timeZoneId").call(d));
      type(() => getter(Temporal.ZonedDateTime, "offset").call(d));
      type(() => getter(Temporal.ZonedDateTime, "offsetNanoseconds").call(d));
      type(() => getter(Temporal.ZonedDateTime, "hoursInDay").call(d));
      type(() => getter(Temporal.Duration, "years").call(d));
      type(() => getter(Temporal.Duration, "sign").call(d));
      type(() => getter(Temporal.Duration, "blank").call(d));
      type(() => getter(Temporal.Duration, "years").call({}));
      type(() => getter(Temporal.Instant, "epochMilliseconds").call(new Temporal.ZonedDateTime(-1n, "UTC")));
      type(() => getter(Temporal.ZonedDateTime, "epochMilliseconds").call(new Temporal.Instant(-1n)));
      same(() => getter(Temporal.ZonedDateTime, "epochMilliseconds").call(new Temporal.ZonedDateTime(-1n, "UTC")), -1);
      same(() => getter(Temporal.Instant, "epochMilliseconds").call(new Temporal.Instant(-1_000_001n)), -2);
      same(() => getter(Temporal.Instant, "epochMilliseconds").call(new Temporal.Instant(-1_000_000n)), -1);
      same(() => getter(Temporal.Instant, "epochMilliseconds").call(new Temporal.Instant(1_999_999n)), 1);
      same(() => getter(Temporal.Instant, "epochNanoseconds").call(new Temporal.Instant(5n)), 5n);
    "#);
}

#[test]
fn with_calendar_swaps_the_calendar_and_validates_its_argument() {
    run(r#"
      const d = D.from("2020-05-06");
      const dt = DT.from("2020-05-06T07:08:09");
      same(() => d.withCalendar("gregory").toString(), "2020-05-06[u-ca=gregory]");
      same(() => d.withCalendar("GREGORY").calendarId, "gregory");
      same(() => dt.withCalendar("hebrew").calendarId, "hebrew");
      same(() => d.withCalendar("iso8601").toString(), "2020-05-06");
      same(() => d.withCalendar("2021-01-01[u-ca=roc]").calendarId, "roc");
      same(() => d.withCalendar("2021-01-01T10:00[u-ca=buddhist]").calendarId, "buddhist");
      same(() => d.withCalendar("2021-01-01").calendarId, "iso8601");
      same(() => d.withCalendar("12:30").calendarId, "iso8601");
      same(() => d.withCalendar(D.from("2000-01-01[u-ca=japanese]")).calendarId, "japanese");
      same(() => d.withCalendar(dt.withCalendar("persian")).calendarId, "persian");
      same(() => d.withCalendar("gregory").withCalendar("iso8601").equals(d), true);
      range(() => d.withCalendar("bogus"));
      range(() => d.withCalendar("2021-01-01[u-ca=bogus]"));
      range(() => d.withCalendar(""));
      type(() => d.withCalendar(5));
      type(() => d.withCalendar(null));
      type(() => d.withCalendar({}));
      type(() => d.withCalendar(true));
      type(() => d.withCalendar(Symbol("x")));
      type(() => Temporal.PlainDate.prototype.withCalendar.call({}, "iso8601"));
    "#);
}

#[test]
fn calendar_aware_bags_resolve_eras_and_leap_months() {
    run(r#"
      // gregory eras
      same(() => D.from({ era: "ce", eraYear: 2020, month: 3, day: 4, calendar: "gregory" }).year, 2020);
      same(() => D.from({ era: "bce", eraYear: 1, month: 3, day: 4, calendar: "gregory" }).year, 0);
      same(() => D.from({ era: "bce", eraYear: 1, month: 3, day: 4, calendar: "gregory" }).era, "bce");
      same(() => D.from({ era: "bce", eraYear: 1, month: 3, day: 4, calendar: "gregory" }).eraYear, 1);
      same(() => D.from({ year: 2020, month: 3, day: 4, calendar: "gregory" }).era, "ce");
      same(() => D.from({ year: 2020, month: 3, day: 4, calendar: "gregory" }).eraYear, 2020);
      same(() => D.from({ year: 2020, month: 3, day: 4, calendar: "gregory" }).toString(), "2020-03-04[u-ca=gregory]");
      range(() => D.from({ era: "bogus", eraYear: 1, month: 3, day: 4, calendar: "gregory" }));
      range(() => D.from({ era: "ce", eraYear: 2020, year: 2021, month: 3, day: 4, calendar: "gregory" }));
      range(() => D.from({ era: "ce", eraYear: Infinity, month: 3, day: 4, calendar: "gregory" }));
      // `eraYear` without `era` is a missing field: a TypeError, not a RangeError
      // (`intl402/Temporal/PlainDate/from/one-of-era-erayear-undefined.js`).
      type(() => D.from({ year: 2020, month: 1, day: 1, eraYear: 1, calendar: "gregory" }));
      // japanese / roc / buddhist / islamic families
      same(() => D.from({ era: "reiwa", eraYear: 2, month: 3, day: 4, calendar: "japanese" }).withCalendar("iso8601").toString(), "2020-03-04");
      same(() => D.from({ era: "reiwa", eraYear: 2, month: 3, day: 4, calendar: "japanese" }).era, "reiwa");
      same(() => D.from({ era: "reiwa", eraYear: 2, month: 3, day: 4, calendar: "japanese" }).eraYear, 2);
      same(() => D.from({ era: "heisei", eraYear: 10, month: 3, day: 4, calendar: "japanese" }).year, 1998);
      same(() => D.from({ era: "roc", eraYear: 1, month: 1, day: 1, calendar: "roc" }).withCalendar("iso8601").toString(), "1912-01-01");
      same(() => D.from({ era: "roc", eraYear: 109, month: 3, day: 4, calendar: "roc" }).withCalendar("iso8601").toString(), "2020-03-04");
      same(() => D.from({ era: "be", eraYear: 2563, month: 3, day: 4, calendar: "buddhist" }).withCalendar("iso8601").toString(), "2020-03-04");
      same(() => D.from({ year: 2020, month: 3, day: 4, calendar: "buddhist" }).era, "be");
      same(() => D.from({ year: 2020, month: 3, day: 4, calendar: "indian" }).era, "shaka");
      same(() => D.from({ year: 1399, month: 1, day: 1, calendar: "persian" }).withCalendar("iso8601").toString(), "2020-03-20");
      same(() => D.from({ year: 1399, month: 1, day: 1, calendar: "persian" }).era, "ap");
      same(() => D.from("2020-03-20").withCalendar("persian").year, 1399);
      same(() => D.from("2020-03-20").withCalendar("persian").monthCode, "M01");
      // hebrew leap month M05L
      const heb = D.from({ year: 5784, monthCode: "M05L", day: 1, calendar: "hebrew" });
      same(() => heb.monthCode, "M05L");
      same(() => heb.month, 6);
      same(() => heb.monthsInYear, 13);
      same(() => heb.inLeapYear, true);
      same(() => heb.year, 5784);
      same(() => D.from({ year: 5784, month: 1, day: 1, calendar: "hebrew" }).withCalendar("iso8601").toString(), "2023-09-16");
      same(() => D.from({ year: 5783, monthCode: "M05L", day: 1, calendar: "hebrew" }).monthCode, "M06");
      range(() => D.from({ year: 5783, monthCode: "M05L", day: 1, calendar: "hebrew" }, { overflow: "reject" }));
      same(() => D.from({ year: 5784, month: 13, day: 29, calendar: "hebrew" }).monthCode, "M12");
      // chinese leap month
      const chi = D.from({ year: 2020, monthCode: "M04L", day: 1, calendar: "chinese" });
      same(() => chi.monthCode, "M04L");
      same(() => chi.withCalendar("iso8601").toString(), "2020-05-23");
      same(() => chi.era, undefined);
      same(() => chi.eraYear, undefined);
      same(() => chi.inLeapYear, true);
      same(() => chi.monthsInYear, 13);
      same(() => D.from({ year: 2020, monthCode: "M01", day: 1, calendar: "chinese" }).withCalendar("iso8601").toString(), "2020-01-25");
      same(() => D.from({ year: 2021, monthCode: "M04L", day: 1, calendar: "chinese" }).monthCode, "M04");
      range(() => D.from({ year: 2021, monthCode: "M04L", day: 1, calendar: "chinese" }, { overflow: "reject" }));
      same(() => D.from({ year: 2020, monthCode: "M01", day: 1, calendar: "dangi" }).calendarId, "dangi");
      // round trips for the remaining calendars
      for (const calendar of ["coptic", "ethiopic", "ethioaa", "islamic-civil", "islamic-tbla",
                              "islamic-umalqura", "indian", "persian", "roc", "buddhist",
                              "japanese", "gregory", "hebrew", "chinese", "dangi"]) {
        const date = D.from("2021-07-08").withCalendar(calendar);
        const back = D.from({ year: date.year, monthCode: date.monthCode, day: date.day, calendar });
        same(() => back.equals(date), true);
        same(() => back.withCalendar("iso8601").toString(), "2021-07-08");
        const viaOrdinal = D.from({ year: date.year, month: date.month, day: date.day, calendar });
        same(() => viaOrdinal.equals(date), true);
        same(() => date.calendarId, calendar);
        same(() => date.daysInMonth >= 28 && date.daysInMonth <= 31, true);
        same(() => date.daysInYear > 300, true);
        same(() => date.monthsInYear >= 12, true);
        same(() => typeof date.inLeapYear, "boolean");
        same(() => date.dayOfWeek, 4);
        // `dayOfYear` counts within the calendar's own year; the ISO week
        // numbers are undefined outside `iso8601`.
        const firstOfYear = D.from({ year: date.year, month: 1, day: 1, calendar });
        same(() => date.dayOfYear, firstOfYear.until(date, { largestUnit: "days" }).days + 1);
        same(() => date.weekOfYear, undefined);
        same(() => date.yearOfWeek, undefined);
      }
    "#);
}

#[test]
fn with_replaces_fields_and_validates_the_like_object() {
    run(r#"
      const d = D.from("2020-05-06");
      same(() => d.with({ day: 30 }).toString(), "2020-05-30");
      same(() => d.with({ month: 2 }).toString(), "2020-02-06");
      same(() => d.with({ monthCode: "M12" }).toString(), "2020-12-06");
      same(() => d.with({ year: 1999 }).toString(), "1999-05-06");
      same(() => d.with({ year: 2021, month: 2, day: 28 }).toString(), "2021-02-28");
      same(() => d.with({ month: 2, day: 31 }).toString(), "2020-02-29");
      same(() => d.with({ month: 2, day: 31 }, { overflow: "constrain" }).toString(), "2020-02-29");
      range(() => d.with({ month: 2, day: 31 }, { overflow: "reject" }));
      same(() => d.with({ month: 13 }).toString(), "2020-12-06");
      range(() => d.with({ month: 13 }, { overflow: "reject" }));
      same(() => D.from("2020-01-31").with({ month: 2 }).toString(), "2020-02-29");
      same(() => D.from("2020-02-29").with({ year: 2021 }).toString(), "2021-02-28");
      range(() => D.from("2020-02-29").with({ year: 2021 }, { overflow: "reject" }));
      same(() => d.with({ monthCode: "M05", month: 5 }).toString(), "2020-05-06");
      range(() => d.with({ monthCode: "M05", month: 6 }));
      range(() => d.with({ monthCode: "M13" }));
      range(() => d.with({ monthCode: "M00" }));
      range(() => d.with({ monthCode: "bad" }));
      range(() => d.with({ month: 0 }));
      range(() => d.with({ day: 0 }));
      range(() => d.with({ day: -1 }));
      range(() => d.with({ year: Infinity }));
      range(() => d.with({ day: Infinity }));
      range(() => d.with({ year: 275761 }));
      range(() => d.with({ day: 1 }, { overflow: "bad" }));
      // era / eraYear are inert for iso8601 (they are never read)
      same(() => d.with({ day: 30, era: "bce" }).toString(), "2020-05-30");
      same(() => d.with({ day: 30, eraYear: 5 }).toString(), "2020-05-30");
      // rejected shapes
      type(() => d.with());
      type(() => d.with(undefined));
      type(() => d.with(null));
      type(() => d.with(5));
      type(() => d.with("2020-01-01"));
      type(() => d.with(true));
      type(() => d.with({}));
      type(() => d.with({ unrelated: 1 }));
      type(() => d.with(D.from("2020-01-01")));
      type(() => d.with(DT.from("2020-01-01T00:00")));
      type(() => d.with(new Temporal.PlainTime()));
      type(() => d.with({ calendar: "iso8601", day: 1 }));
      type(() => d.with({ timeZone: "UTC", day: 1 }));
      type(() => d.with({ day: 1 }, 5));
      type(() => d.with({ day: 1 }, "reject"));
      type(() => d.with({ day: Symbol("x") }));
      // time fields are ignored on a PlainDate receiver
      same(() => d.with({ day: 7, hour: 99 }).toString(), "2020-05-07");
      type(() => d.with({ hour: 5 }));

      const dt = DT.from("2020-05-06T07:08:09.010011012");
      same(() => dt.with({ hour: 1 }).toString(), "2020-05-06T01:08:09.010011012");
      same(() => dt.with({ minute: 1 }).toString(), "2020-05-06T07:01:09.010011012");
      same(() => dt.with({ second: 1 }).toString(), "2020-05-06T07:08:01.010011012");
      same(() => dt.with({ millisecond: 1 }).toString(), "2020-05-06T07:08:09.001011012");
      same(() => dt.with({ microsecond: 1 }).toString(), "2020-05-06T07:08:09.010001012");
      same(() => dt.with({ nanosecond: 1 }).toString(), "2020-05-06T07:08:09.010011001");
      same(() => dt.with({ year: 2021, hour: 23, minute: 59 }).toString(), "2021-05-06T23:59:09.010011012");
      same(() => dt.with({ day: 1 }).toString(), "2020-05-01T07:08:09.010011012");
      range(() => dt.with({ hour: 24 }, { overflow: "reject" }));
      range(() => dt.with({ minute: 60 }, { overflow: "reject" }));
      range(() => dt.with({ hour: -1 }, { overflow: "reject" }));
      // The default `overflow: "constrain"` clamps an out-of-range time field.
      same(() => dt.with({ hour: -1 }).hour, 0);
      same(() => dt.with({ hour: 24 }).hour, 23);
      same(() => dt.with({ minute: 67 }).minute, 59);
      same(() => dt.with({ millisecond: 1000 }).millisecond, 999);
      range(() => dt.with({ hour: Infinity }));
      type(() => dt.with({ }));
      type(() => dt.with({ calendar: "iso8601", hour: 1 }));
      type(() => dt.with({ timeZone: "UTC", hour: 1 }));
      type(() => dt.with(dt));
      type(() => dt.with(d));
      type(() => DT.prototype.with.call({}, { hour: 1 }));
    "#);
}

#[test]
fn with_on_non_iso_calendars_handles_era_pairs_and_leap_months() {
    run(r#"
      const g = D.from("2020-05-06[u-ca=gregory]");
      same(() => g.with({ era: "bce", eraYear: 5 }).year, -4);
      same(() => g.with({ era: "ce", eraYear: 1999 }).toString(), "1999-05-06[u-ca=gregory]");
      type(() => g.with({ era: "ce" }));
      type(() => g.with({ eraYear: 2000 }));
      range(() => g.with({ era: "bogus", eraYear: 2000 }));
      range(() => g.with({ era: "ce", eraYear: 2000, year: 1999 }));
      same(() => g.with({ year: 1999 }).toString(), "1999-05-06[u-ca=gregory]");
      same(() => g.with({ day: 31, month: 4 }).toString(), "2020-04-30[u-ca=gregory]");
      range(() => g.with({ day: 31, month: 4 }, { overflow: "reject" }));
      same(() => g.with({ monthCode: "M02", day: 30 }).toString(), "2020-02-29[u-ca=gregory]");

      // chinese/dangi have no eras at all and reject era fields
      const c = D.from({ year: 2020, monthCode: "M04L", day: 1, calendar: "chinese" });
      type(() => c.with({ era: "x", eraYear: 1 }));
      type(() => c.with({ era: "x" }));
      type(() => c.with({ eraYear: 1 }));
      same(() => c.with({ day: 2 }).monthCode, "M04L");
      same(() => c.with({ day: 2 }).day, 2);
      same(() => c.with({ year: 2021 }).monthCode, "M04");
      range(() => c.with({ year: 2021 }, { overflow: "reject" }));
      same(() => c.with({ monthCode: "M01" }).withCalendar("iso8601").toString(), "2020-01-25");
      const dg = D.from({ year: 2020, monthCode: "M01", day: 1, calendar: "dangi" });
      type(() => dg.with({ era: "x", eraYear: 1 }));

      // hebrew month numbering follows the leap month
      const h = D.from({ year: 5784, monthCode: "M05L", day: 1, calendar: "hebrew" });
      same(() => h.with({ month: 7 }).monthCode, "M06");
      same(() => h.with({ monthCode: "M06" }).month, 7);
      same(() => h.with({ year: 5783 }).monthCode, "M06");

      // japanese carries an era
      const j = D.from({ era: "reiwa", eraYear: 2, month: 3, day: 4, calendar: "japanese" });
      same(() => j.with({ era: "heisei", eraYear: 20 }).year, 2008);
      type(() => j.with({ eraYear: 3 }));
      // era + monthCode / month cross-check
      range(() => g.with({ monthCode: "M05", month: 6 }));
      same(() => g.with({ monthCode: "M05", month: 5 }).month, 5);

      // buddhist: no era means the year is the extended year
      const b = D.from({ year: 2563, month: 3, day: 4, calendar: "buddhist" });
      same(() => b.with({ year: 2564 }).withCalendar("iso8601").toString(), "2021-03-04");
      // with keeps the calendar on a PlainDateTime too
      const bdt = DT.from("2020-03-04T05:06:07[u-ca=gregory]");
      same(() => bdt.with({ hour: 1 }).toString(), "2020-03-04T01:06:07[u-ca=gregory]");
      same(() => bdt.with({ era: "bce", eraYear: 1 }).year, 0);
      type(() => bdt.with({ era: "ce" }));
      type(() => bdt.with({ eraYear: 1 }));
    "#);
}
