// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Coverage for `Temporal.PlainDate` / `Temporal.PlainDateTime` arithmetic
//! (`vm/temporal/dates.rs`'s `add`/`subtract`, `since`/`until`,
//! `PlainDateTime.prototype.round`) including the option-bag validation and
//! calendar arithmetic that back them (`vm/temporal/plain_date.rs`).
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
fn add_and_subtract_fold_units_and_regulate_overflow() {
    run(r#"
      const d = D.from("2020-01-31");
      same(() => d.add({ months: 1 }).toString(), "2020-02-29");
      same(() => d.add({ months: 1 }, { overflow: "constrain" }).toString(), "2020-02-29");
      range(() => d.add({ months: 1 }, { overflow: "reject" }));
      same(() => D.from("2020-02-29").add({ years: 1 }).toString(), "2021-02-28");
      range(() => D.from("2020-02-29").add({ years: 1 }, { overflow: "reject" }));
      same(() => d.add({ days: 1 }).toString(), "2020-02-01");
      same(() => d.add({ weeks: 2 }).toString(), "2020-02-14");
      same(() => d.add({ years: 1, months: 1, weeks: 1, days: 1 }).toString(), "2021-03-08");
      same(() => d.add("P1Y1M1W1D").toString(), "2021-03-08");
      same(() => d.add(Temporal.Duration.from({ days: 366 })).toString(), "2021-01-31");
      // time units fold into whole days on a PlainDate (truncating toward zero)
      same(() => d.add({ hours: 24 }).toString(), "2020-02-01");
      same(() => d.add({ hours: 25 }).toString(), "2020-02-01");
      same(() => d.add({ hours: 23 }).toString(), "2020-01-31");
      same(() => d.add({ hours: -23 }).toString(), "2020-01-31");
      same(() => d.add({ hours: -24 }).toString(), "2020-01-30");
      same(() => d.add({ minutes: 1440 }).toString(), "2020-02-01");
      same(() => d.add({ seconds: 86400 }).toString(), "2020-02-01");
      same(() => d.add({ milliseconds: 86400000 }).toString(), "2020-02-01");
      same(() => d.add({ microseconds: 86400000000 }).toString(), "2020-02-01");
      same(() => d.add({ nanoseconds: 86400000000000 }).toString(), "2020-02-01");
      same(() => d.add({ days: 1, hours: 24 }).toString(), "2020-02-02");
      // subtract
      same(() => d.subtract({ months: 1 }).toString(), "2019-12-31");
      same(() => d.subtract({ months: 2 }).toString(), "2019-11-30");
      range(() => d.subtract({ months: 2 }, { overflow: "reject" }));
      same(() => d.subtract({ days: 31 }).toString(), "2019-12-31");
      same(() => d.subtract({ years: 1, days: 1 }).toString(), "2019-01-30");
      same(() => d.subtract("P1D").toString(), "2020-01-30");
      same(() => d.subtract({ hours: 25 }).toString(), "2020-01-30");
      same(() => d.add({ days: -1 }).toString(), "2020-01-30");
      same(() => d.subtract({ days: -1 }).toString(), "2020-02-01");
      // time-of-day is preserved / carried on a PlainDateTime
      const dt = DT.from("2020-01-31T22:30:00.5");
      same(() => dt.add({ hours: 2 }).toString(), "2020-02-01T00:30:00.5");
      same(() => dt.add({ hours: 25, minutes: 40 }).toString(), "2020-02-02T00:10:00.5");
      same(() => dt.add({ nanoseconds: 500000000 }).toString(), "2020-01-31T22:30:01");
      same(() => dt.subtract({ hours: 23 }).toString(), "2020-01-30T23:30:00.5");
      same(() => dt.subtract({ hours: 22, minutes: 31 }).toString(), "2020-01-30T23:59:00.5");
      same(() => dt.subtract({ months: 1, milliseconds: 1 }).toString(), "2019-12-31T22:30:00.499");
      same(() => dt.add({ microseconds: 1, nanoseconds: 1 }).toString(), "2020-01-31T22:30:00.500001001");
      same(() => dt.add({ months: 1 }).toString(), "2020-02-29T22:30:00.5");
      range(() => dt.add({ months: 1 }, { overflow: "reject" }));
      // invalid durations and options
      type(() => d.add());
      type(() => d.add(undefined));
      type(() => d.add(null));
      type(() => d.add(5));
      type(() => d.add(true));
      type(() => d.add({}));
      type(() => d.add({ unrelated: 1 }));
      range(() => d.add("bogus"));
      range(() => d.add("P1"));
      range(() => d.add({ years: 1.5 }));
      range(() => d.add({ days: Infinity }));
      range(() => d.add({ days: 1, hours: -1 }));
      range(() => d.add({ days: NaN }));
      range(() => d.add({ days: 1 }, { overflow: "bad" }));
      type(() => d.add({ days: 1 }, 5));
      type(() => d.add({ days: 1 }, "reject"));
      type(() => d.subtract());
      type(() => d.subtract({}));
      range(() => d.subtract("bogus"));
      type(() => dt.add({}));
      type(() => dt.add(5));
      range(() => dt.add({ days: 1 }, { overflow: "x" }));
      type(() => D.prototype.add.call({}, { days: 1 }));
      type(() => D.prototype.subtract.call(dt.toPlainTime(), { days: 1 }));
      // results outside the representable range
      range(() => D.from("2020-01-01").add({ years: 275760 }));
      range(() => D.from("2020-01-01").subtract({ years: 275760 }));
      range(() => D.from("2020-01-01").add({ days: 9007199254740991 }));
      range(() => D.from("2020-01-01").add({ months: 1e9 }));
      range(() => D.from("+275760-09-13").add({ days: 1 }));
      range(() => D.from("-271821-04-19").subtract({ days: 1 }));
      range(() => D.from("+275760-09-13").add({ hours: 24 }));
      range(() => DT.from("+275760-09-13T23:59:59.999999999").add({ nanoseconds: 1 }));
      range(() => DT.from("-271821-04-19T00:00:00.000000001").subtract({ nanoseconds: 1 }));
      same(() => D.from("+275760-09-12").add({ days: 1 }).toString(), "+275760-09-13");
      same(() => DT.from("+275760-09-13T23:59:59.999999998").add({ nanoseconds: 1 }).toString(), "+275760-09-13T23:59:59.999999999");
    "#);
}

#[test]
fn add_on_non_iso_calendars_uses_calendar_months() {
    run(r#"
      const greg = D.from("2020-01-31[u-ca=gregory]");
      same(() => greg.add({ months: 1 }).toString(), "2020-02-29[u-ca=gregory]");
      range(() => greg.add({ months: 1 }, { overflow: "reject" }));
      same(() => greg.add({ years: 1, months: 1 }).toString(), "2021-02-28[u-ca=gregory]");
      const heb = D.from({ year: 5784, monthCode: "M05L", day: 30, calendar: "hebrew" });
      same(() => heb.add({ months: 1 }).monthCode, "M06");
      same(() => heb.add({ months: 1 }).day, 29);
      range(() => heb.add({ months: 1 }, { overflow: "reject" }));
      same(() => heb.add({ years: 1 }).monthCode, "M06");
      same(() => heb.subtract({ months: 1 }).monthCode, "M05");
      same(() => heb.add({ days: 1 }).monthCode, "M06");
      const chi = D.from({ year: 2020, monthCode: "M03", day: 30, calendar: "chinese" });
      same(() => chi.add({ months: 1 }).monthCode, "M04");
      same(() => chi.add({ months: 2 }).monthCode, "M04L");
      same(() => chi.add({ years: 1 }).year, 2021);
      same(() => chi.subtract({ years: 1 }).year, 2019);
      const persian = D.from({ year: 1399, month: 12, day: 30, calendar: "persian" });
      same(() => persian.add({ years: 1 }).day, 29);
      range(() => persian.add({ years: 1 }, { overflow: "reject" }));
      same(() => persian.add({ days: 1 }).year, 1400);
      same(() => persian.add({ days: 1 }).monthCode, "M01");
      const coptic = D.from({ year: 1735, month: 13, day: 6, calendar: "coptic" });
      same(() => coptic.add({ years: 1 }).day, 5);
      same(() => coptic.add({ months: 1 }).year, 1736);
      const dt = DT.from("2020-01-31T10:00[u-ca=gregory]");
      same(() => dt.add({ months: 1, hours: 15 }).toString(), "2020-03-01T01:00:00[u-ca=gregory]");
    "#);
}

#[test]
fn until_and_since_balance_into_the_largest_unit() {
    run(r#"
      const a = D.from("2020-01-15");
      const b = D.from("2021-03-20");
      same(() => a.until(b).toString(), "P430D");
      same(() => a.until(b, { largestUnit: "auto" }).toString(), "P430D");
      same(() => a.until(b, { largestUnit: "days" }).toString(), "P430D");
      same(() => a.until(b, { largestUnit: "weeks" }).toString(), "P61W3D");
      same(() => a.until(b, { largestUnit: "months" }).toString(), "P14M5D");
      same(() => a.until(b, { largestUnit: "years" }).toString(), "P1Y2M5D");
      same(() => a.until(b, { largestUnit: "year" }).toString(), "P1Y2M5D");
      same(() => a.since(b).toString(), "-P430D");
      same(() => b.since(a).toString(), "P430D");
      same(() => b.until(a).toString(), "-P430D");
      same(() => b.until(a, { largestUnit: "years" }).toString(), "-P1Y2M5D");
      same(() => b.since(a, { largestUnit: "years" }).toString(), "P1Y2M5D");
      same(() => a.until(a).toString(), "PT0S");
      same(() => a.since(a, { largestUnit: "years" }).toString(), "PT0S");
      same(() => a.until(b, { string: 1 }).toString(), "P430D");
      // the argument can be anything ToTemporalDate accepts
      same(() => a.until("2021-03-20").toString(), "P430D");
      same(() => a.until({ year: 2021, month: 3, day: 20 }).toString(), "P430D");
      same(() => a.until(DT.from("2021-03-20T05:00")).toString(), "P430D");
      // smallestUnit rounding
      same(() => a.until(b, { largestUnit: "years", smallestUnit: "months" }).toString(), "P1Y2M");
      same(() => a.until(b, { largestUnit: "years", smallestUnit: "months", roundingMode: "expand" }).toString(), "P1Y3M");
      same(() => a.until(b, { largestUnit: "years", smallestUnit: "months", roundingMode: "ceil" }).toString(), "P1Y3M");
      same(() => a.until(b, { largestUnit: "years", smallestUnit: "months", roundingMode: "floor" }).toString(), "P1Y2M");
      same(() => a.since(b, { largestUnit: "years", smallestUnit: "months", roundingMode: "ceil" }).toString(), "-P1Y2M");
      same(() => a.since(b, { largestUnit: "years", smallestUnit: "months", roundingMode: "floor" }).toString(), "-P1Y3M");
      same(() => a.since(b, { largestUnit: "years", smallestUnit: "months", roundingMode: "halfCeil" }).toString(), "-P1Y2M");
      same(() => a.since(b, { largestUnit: "years", smallestUnit: "months", roundingMode: "halfFloor" }).toString(), "-P1Y2M");
      same(() => a.until(b, { largestUnit: "years", smallestUnit: "years" }).toString(), "P1Y");
      same(() => a.until(b, { largestUnit: "years", smallestUnit: "years", roundingMode: "expand" }).toString(), "P2Y");
      same(() => a.until(b, { smallestUnit: "weeks" }).toString(), "P61W");
      same(() => a.until(b, { smallestUnit: "weeks", roundingMode: "ceil" }).toString(), "P62W");
      same(() => a.until(b, { smallestUnit: "months" }).toString(), "P14M");
      same(() => a.until(b, { smallestUnit: "years" }).toString(), "P1Y");
      same(() => a.until(b, { smallestUnit: "day", roundingIncrement: 100 }).toString(), "P400D");
      same(() => a.until(b, { smallestUnit: "day", roundingIncrement: 100, roundingMode: "halfExpand" }).toString(), "P400D");
      same(() => a.until(b, { largestUnit: "months", smallestUnit: "months", roundingIncrement: 5 }).toString(), "P10M");
      same(() => a.until(b, { largestUnit: "months", smallestUnit: "months", roundingIncrement: 5, roundingMode: "ceil" }).toString(), "P15M");
      same(() => a.until(b, { roundingMode: "halfTrunc", smallestUnit: "weeks" }).toString(), "P61W");
      same(() => a.until(b, { roundingMode: "halfEven", smallestUnit: "weeks" }).toString(), "P61W");
      // end-of-month anchoring (not anti-symmetric)
      same(() => D.from("2020-01-31").until("2020-03-01", { largestUnit: "months" }).toString(), "P1M1D");
      same(() => D.from("2020-03-31").until("2020-02-29", { largestUnit: "months" }).toString(), "-P1M");
      same(() => D.from("2020-03-31").since("2020-02-29", { largestUnit: "months" }).toString(), "P1M");
      same(() => D.from("1996-05-31").until("1997-06-01", { largestUnit: "years" }).toString(), "P1Y1D");
      // option validation
      range(() => a.until(b, { largestUnit: "hours" }));
      range(() => a.until(b, { largestUnit: "nanoseconds" }));
      range(() => a.until(b, { smallestUnit: "hours" }));
      range(() => a.until(b, { smallestUnit: "minute" }));
      range(() => a.until(b, { largestUnit: "bogus" }));
      range(() => a.until(b, { smallestUnit: "bogus" }));
      range(() => a.until(b, { smallestUnit: "auto" }));
      range(() => a.until(b, { largestUnit: "days", smallestUnit: "years" }));
      range(() => a.until(b, { largestUnit: "weeks", smallestUnit: "months" }));
      range(() => a.until(b, { roundingIncrement: 0 }));
      range(() => a.until(b, { roundingIncrement: -1 }));
      range(() => a.until(b, { roundingIncrement: NaN }));
      range(() => a.until(b, { roundingIncrement: Infinity }));
      range(() => a.until(b, { roundingIncrement: 1e9 + 1 }));
      range(() => a.until(b, { roundingMode: "bogus" }));
      range(() => a.until(b, { roundingMode: "" }));
      type(() => a.until(b, 5));
      type(() => a.until(b, "years"));
      type(() => a.until(b, null));
      type(() => a.until());
      type(() => a.until(undefined));
      type(() => a.until(null));
      type(() => a.until(5));
      type(() => a.until(true));
      range(() => a.until("bogus"));
      range(() => a.until(D.from("2020-01-15[u-ca=gregory]")));
      range(() => a.since(D.from("2020-01-15[u-ca=hebrew]")));
      type(() => D.prototype.until.call({}, b));
      type(() => D.prototype.since.call(new Temporal.PlainTime(), b));
    "#);
}

#[test]
fn until_and_since_on_non_iso_calendars() {
    run(r#"
      const g1 = D.from("2020-01-31[u-ca=gregory]");
      const g2 = D.from("2021-03-01[u-ca=gregory]");
      same(() => g1.until(g2, { largestUnit: "years" }).toString(), "P1Y1M1D");
      same(() => g1.until(g2, { largestUnit: "months" }).toString(), "P13M1D");
      same(() => g1.until(g2).toString(), "P395D");
      const h1 = D.from({ year: 5783, monthCode: "M01", day: 1, calendar: "hebrew" });
      const h2 = D.from({ year: 5784, monthCode: "M06", day: 15, calendar: "hebrew" });
      same(() => h1.until(h2, { largestUnit: "years" }).years, 1);
      same(() => h1.until(h2, { largestUnit: "years" }).months, 6);
      same(() => h1.until(h2, { largestUnit: "years" }).days, 14);
      same(() => h1.until(h2, { largestUnit: "months" }).months, 18);
      same(() => h2.until(h1, { largestUnit: "months" }).months, -18);
      same(() => h1.since(h2, { largestUnit: "months" }).months, -18);
      const c1 = D.from({ year: 2020, monthCode: "M03", day: 1, calendar: "chinese" });
      const c2 = D.from({ year: 2020, monthCode: "M05", day: 1, calendar: "chinese" });
      same(() => c1.until(c2, { largestUnit: "months" }).months, 3);
      same(() => c1.until(c2, { largestUnit: "years" }).months, 3);
      same(() => c2.until(c1, { largestUnit: "months" }).months, -3);
      same(() => c1.until(c2, { largestUnit: "months", smallestUnit: "months", roundingMode: "halfExpand" }).months, 3);
      const dt1 = DT.from("2020-01-15T10:00[u-ca=gregory]");
      const dt2 = DT.from("2021-03-20T09:00[u-ca=gregory]");
      same(() => dt1.until(dt2, { largestUnit: "years" }).toString(), "P1Y2M4DT23H");
      same(() => dt1.since(dt2, { largestUnit: "years" }).toString(), "-P1Y2M4DT23H");
      same(() => dt1.until(dt2, { largestUnit: "months" }).toString(), "P14M4DT23H");
    "#);
}

#[test]
fn plain_date_time_differences_balance_time_fields() {
    run(r#"
      const a = DT.from("2020-01-01T00:00:00");
      const b = DT.from("2020-01-02T12:30:15.123456789");
      same(() => a.until(b).toString(), "P1DT12H30M15.123456789S");
      same(() => a.since(b).toString(), "-P1DT12H30M15.123456789S");
      same(() => b.until(a).toString(), "-P1DT12H30M15.123456789S");
      same(() => b.since(a).toString(), "P1DT12H30M15.123456789S");
      same(() => a.until(b, { largestUnit: "weeks" }).toString(), "P1DT12H30M15.123456789S");
      same(() => a.until(b, { largestUnit: "months" }).toString(), "P1DT12H30M15.123456789S");
      same(() => a.until(b, { largestUnit: "years" }).toString(), "P1DT12H30M15.123456789S");
      // rounding at each time unit
      same(() => a.until(b, { smallestUnit: "hour", roundingMode: "halfExpand" }).toString(), "P1DT13H");
      same(() => a.until(b, { smallestUnit: "hour", roundingMode: "trunc" }).toString(), "P1DT12H");
      same(() => a.until(b, { smallestUnit: "minute" }).toString(), "P1DT12H30M");
      same(() => a.until(b, { smallestUnit: "second" }).toString(), "P1DT12H30M15S");
      same(() => a.until(b, { smallestUnit: "millisecond" }).toString(), "P1DT12H30M15.123S");
      same(() => a.until(b, { smallestUnit: "microsecond" }).toString(), "P1DT12H30M15.123456S");
      same(() => a.until(b, { smallestUnit: "nanosecond" }).toString(), "P1DT12H30M15.123456789S");
      same(() => a.until(b, { smallestUnit: "minute", roundingIncrement: 15 }).toString(), "P1DT12H30M");
      same(() => a.until(b, { smallestUnit: "hour", roundingIncrement: 8, roundingMode: "ceil" }).toString(), "P1DT16H");
      same(() => a.until(b, { smallestUnit: "second", roundingMode: "ceil" }).toString(), "P1DT12H30M16S");
      same(() => a.since(b, { smallestUnit: "second", roundingMode: "ceil" }).toString(), "-P1DT12H30M15S");
      same(() => a.since(b, { smallestUnit: "second", roundingMode: "floor" }).toString(), "-P1DT12H30M16S");
      same(() => a.since(b, { smallestUnit: "second", roundingMode: "halfCeil" }).toString(), "-P1DT12H30M15S");
      same(() => a.since(b, { smallestUnit: "second", roundingMode: "halfFloor" }).toString(), "-P1DT12H30M15S");
      same(() => a.until(b, { smallestUnit: "day" }).toString(), "P1D");
      // the time difference has the opposite sign of the date difference
      same(() => DT.from("2020-01-01T12:00").until("2020-01-03T06:00").toString(), "P1DT18H");
      same(() => DT.from("2020-01-03T06:00").until("2020-01-01T12:00").toString(), "-P1DT18H");
      same(() => DT.from("2020-01-01T12:00").until("2020-01-02T06:00").toString(), "PT18H");
      same(() => DT.from("2020-01-01T12:00").until("2020-01-02T12:00").toString(), "P1D");
      same(() => DT.from("2020-01-01T12:00").until("2020-01-01T12:00").toString(), "PT0S");
      same(() => DT.from("2020-01-01T12:00").until("2020-01-01T11:59:59").toString(), "-PT1S");
      // options and receiver validation
      range(() => a.until(b, { largestUnit: "nanosecond", smallestUnit: "day" }));
      range(() => a.until(b, { largestUnit: "hours", smallestUnit: "days" }));
      range(() => a.until(b, { smallestUnit: "bogus" }));
      range(() => a.until(b, { largestUnit: "bogus" }));
      range(() => a.until(b, { roundingIncrement: 0 }));
      range(() => a.until(b, { roundingMode: "bogus" }));
      type(() => a.until(b, 5));
      type(() => a.until());
      range(() => a.until(D.from("2020-01-01[u-ca=gregory]")));
      type(() => DT.prototype.until.call({}, b));
    "#);
}
