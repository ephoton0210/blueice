// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Coverage for `Temporal.ZonedDateTime` edge cases (`vm/temporal/zoned.rs`,
//! `vm/temporal/zoned_date_time.rs`): receiver brand checks on every method,
//! invalid-string handling, era-aware `with`, results outside the
//! representable instant range, every rounding mode applied to calendar-unit
//! differences in both directions, and rounding that bubbles a completed unit
//! into the next larger one.
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
fn every_method_brand_checks_its_receiver() {
    run(r#"
      const methods = {
        add: [{ days: 1 }], subtract: [{ days: 1 }], until: ["2020-01-01T00:00[UTC]"], since: ["2020-01-01T00:00[UTC]"],
        round: ["hour"], equals: ["2020-01-01T00:00[UTC]"], toString: [], toJSON: [], toLocaleString: [], valueOf: [],
        toInstant: [], toPlainDate: [], toPlainTime: [], toPlainDateTime: [], toPlainYearMonth: [], toPlainMonthDay: [],
        startOfDay: [], getTimeZoneTransition: ["next"], with: [{ day: 1 }], withPlainTime: ["12:00"],
        withTimeZone: ["UTC"], getISOFields: [],
      };
      const receivers = { five: 5, undef: undefined, nul: null, string: "x", plain: {}, plainDate: D.from("2020-01-01"),
        plainDateTime: DT.from("2020-01-01T00:00"), instant: new Temporal.Instant(0n) };
      for (const name of Object.keys(methods)) {
        for (const label of Object.keys(receivers)) {
          CTX = name + " on " + label;
          type(() => Z.prototype[name].apply(receivers[label], methods[name]));
        }
      }
      for (const name of ["hour", "minute", "second", "millisecond", "microsecond", "nanosecond",
                          "offset", "offsetNanoseconds", "hoursInDay", "timeZoneId"]) {
        const g = Object.getOwnPropertyDescriptor(Z.prototype, name).get;
        CTX = name;
        type(() => g.call({}));
        type(() => g.call(undefined));
        type(() => g.call(5));
        type(() => g.call(D.from("2020-01-01")));
      }
    "#);
}

#[test]
fn invalid_strings_and_era_aware_with() {
    run(r#"
      range(() => Z.from("\ud800"));
      range(() => Z.from("\ud800", { overflow: "reject" }));
      range(() => Z.from("2020-06-15T12:00[UTC]\ud800"));
      range(() => DT.from("\ud800"));
      range(() => D.from("\ud800"));
      range(() => DT.from("2020-06-15T12:00\ud800"));
      range(() => Z.from("2020-06-15T12:00[\ud800]"));
      range(() => Z.from("2020-06-15T12:00[UTC]", { overflow: "bogus" }));
      range(() => Z.from("2020-06-15T12:00[UTC]", { disambiguation: "bogus" }));
      range(() => Z.from("2020-06-15T12:00[UTC]", { offset: "bogus" }));
      type(() => Z.from("2020-06-15T12:00[UTC]", 5));
      const g = Z.from("2020-06-15T12:00[UTC][u-ca=gregory]");
      same(() => g.with({ month: 2 }).toString(), "2020-02-15T12:00:00+00:00[UTC][u-ca=gregory]");
      same(() => g.with({ year: 2019 }).toString(), "2019-06-15T12:00:00+00:00[UTC][u-ca=gregory]");
      same(() => g.with({ monthCode: "M12" }).toString(), "2020-12-15T12:00:00+00:00[UTC][u-ca=gregory]");
      same(() => g.with({ day: 1 }).toString(), "2020-06-01T12:00:00+00:00[UTC][u-ca=gregory]");
      same(() => g.with({ era: "bce", eraYear: 5 }).toString(), "-000004-06-15T12:00:00+00:00[UTC][u-ca=gregory]");
      same(() => g.with({ era: "ce", eraYear: 2019 }).toString(), "2019-06-15T12:00:00+00:00[UTC][u-ca=gregory]");
      type(() => g.with({ eraYear: 2019 }));
      type(() => g.with({ era: "bce" }));
      type(() => g.with({ era: "ce", eraYear: 2019, year: undefined, month: undefined, timeZone: "UTC" }));
      range(() => g.with({ era: "bogus", eraYear: 5 }));
      // month-only and year-only bags in a leap-month calendar
      const h = Z.from({ year: 5784, monthCode: "M05L", day: 10, hour: 12, timeZone: "UTC", calendar: "hebrew" });
      same(() => h.with({ day: 11 }).monthCode, "M05L");
      same(() => h.with({ year: 5785 }).monthCode, "M06");
      same(() => h.with({ monthCode: "M06" }).year, 5784);
      // `with` and arithmetic that leave the representable instant range
      const max = new Z(8640000000000000000000n, "UTC");
      const min = new Z(-8640000000000000000000n, "UTC");
      range(() => max.with({ hour: 1 }));
      range(() => max.with({ day: 14 }));
      range(() => max.with({ year: 275761 }));
      range(() => min.with({ day: 19 }));
      range(() => max.add({ nanoseconds: 1 }));
      range(() => min.subtract({ nanoseconds: 1 }));
      range(() => max.add({ days: 1 }));
      range(() => min.add({ days: -1 }));
      same(() => max.with({ hour: 0 }).epochNanoseconds, 8640000000000000000000n);
      // rounding never leaves the range either
      same(() => max.round("hour").epochNanoseconds, 8640000000000000000000n);
      same(() => new Z(8640000000000000000000n - 1n, "UTC").round({ smallestUnit: "hour", roundingMode: "ceil" }).epochNanoseconds, 8640000000000000000000n);
      same(() => min.round({ smallestUnit: "hour", roundingMode: "floor" }).epochNanoseconds, -8640000000000000000000n);
      range(() => new Z(8640000000000000000000n, "America/New_York").round("day"));
      range(() => new Z(8640000000000000000000n - 1n, "America/New_York").round({ smallestUnit: "day", roundingMode: "ceil" }));
      range(() => new Z(-8640000000000000000000n, "America/New_York").round({ smallestUnit: "day", roundingMode: "floor" }));
      // differences across the whole range are exact
      same(() => min.until(max, { largestUnit: "years" }).toString(), "P547581Y4M24D");
      same(() => min.until(max, { largestUnit: "months" }).toString(), "P6570976M24D");
      same(() => max.until(min, { largestUnit: "years" }).toString(), "-P547581Y4M23D");
      same(() => min.until(max, { largestUnit: "years", smallestUnit: "years" }).toString(), "P547581Y");
      same(() => max.until(min, { largestUnit: "years", smallestUnit: "years" }).toString(), "-P547581Y");
      same(() => min.until(max, { largestUnit: "hours" }).hours, 4800000000);
    "#);
}

#[test]
fn calendar_unit_rounding_modes_apply_in_both_directions() {
    run(r#"
      // Jan 1 -> Mar 16 12:00 is 2 months and exactly half of March's 31 days
      const a = Z.from("2020-01-01T00:00[UTC]");
      const b = Z.from("2020-03-16T12:00[UTC]");
      const expected = {
        ceil: 3, floor: 2, trunc: 2, expand: 3,
        halfCeil: 3, halfFloor: 2, halfTrunc: 2, halfExpand: 3, halfEven: 2,
      };
      for (const mode of Object.keys(expected)) {
        CTX = "until " + mode;
        same(() => a.until(b, { smallestUnit: "month", roundingMode: mode }).months, expected[mode]);
        same(() => a.until(b, { smallestUnit: "month", roundingMode: mode }).toString(), "P" + expected[mode] + "M");
      }
      // backwards, the same tie rounds to the mirrored value: floor/ceil swap, the rest are symmetric
      const backwards = {
        ceil: -2, floor: -3, trunc: -2, expand: -3,
        halfCeil: -2, halfFloor: -3, halfTrunc: -2, halfExpand: -3, halfEven: -2,
      };
      for (const mode of Object.keys(backwards)) {
        CTX = "backwards " + mode;
        same(() => b.until(a, { smallestUnit: "month", roundingMode: mode }).toString(), "-P" + (-backwards[mode]) + "M");
      }
      // `since` negates the mode and the result
      const since = {
        ceil: -2, floor: -3, trunc: -2, expand: -3,
        halfCeil: -2, halfFloor: -3, halfTrunc: -2, halfExpand: -3, halfEven: -2,
      };
      for (const mode of Object.keys(since)) {
        CTX = "since " + mode;
        same(() => a.since(b, { smallestUnit: "month", roundingMode: mode }).toString(), "-P" + (-since[mode]) + "M");
      }
      // a half-way tie that lands on an odd month rounds to the even one
      const c = Z.from("2020-01-01T00:00[UTC]");
      const d = Z.from("2020-02-15T12:00[UTC]");
      same(() => c.until(d, { smallestUnit: "month", roundingMode: "halfEven" }).toString(), "P2M");
      same(() => c.until(d, { smallestUnit: "month", roundingMode: "halfTrunc" }).toString(), "P1M");
      same(() => c.until(d, { smallestUnit: "month", roundingMode: "halfExpand" }).toString(), "P2M");
      // backwards the month being rounded into is January's 31 days, so the same instant is not a tie
      same(() => d.until(c, { smallestUnit: "month", roundingMode: "halfEven" }).toString(), "-P1M");
      // not a tie: strictly less than or greater than half a month
      const under = Z.from("2020-03-16T00:00[UTC]");
      const over = Z.from("2020-03-17T00:00[UTC]");
      same(() => a.until(under, { smallestUnit: "month", roundingMode: "halfExpand" }).toString(), "P2M");
      same(() => a.until(over, { smallestUnit: "month", roundingMode: "halfExpand" }).toString(), "P3M");
      same(() => a.until(under, { smallestUnit: "month", roundingMode: "halfEven" }).toString(), "P2M");
      same(() => a.until(over, { smallestUnit: "month", roundingMode: "halfCeil" }).toString(), "P3M");
      same(() => a.until(under, { smallestUnit: "month", roundingMode: "halfFloor" }).toString(), "P2M");
      same(() => over.until(a, { smallestUnit: "month", roundingMode: "halfCeil" }).toString(), "-P3M");
      same(() => under.until(a, { smallestUnit: "month", roundingMode: "halfFloor" }).toString(), "-P2M");
      // year and week units
      same(() => a.until(Z.from("2021-07-02T00:00[UTC]"), { smallestUnit: "year", roundingMode: "ceil" }).toString(), "P2Y");
      same(() => a.until(Z.from("2021-07-03T00:00[UTC]"), { smallestUnit: "year", roundingMode: "halfExpand" }).toString(), "P2Y");
      same(() => a.until(Z.from("2021-06-01T00:00[UTC]"), { smallestUnit: "year", roundingMode: "halfExpand" }).toString(), "P1Y");
      same(() => a.until(Z.from("2020-01-20T00:00[UTC]"), { largestUnit: "weeks", smallestUnit: "week", roundingMode: "halfExpand" }).toString(), "P3W");
      same(() => a.until(Z.from("2020-01-20T00:00[UTC]"), { largestUnit: "weeks", smallestUnit: "week", roundingMode: "floor" }).toString(), "P2W");
      same(() => Z.from("2020-01-20T00:00[UTC]").until(a, { largestUnit: "weeks", smallestUnit: "week", roundingMode: "floor" }).toString(), "-P3W");
      same(() => Z.from("2020-01-20T00:00[UTC]").until(a, { largestUnit: "weeks", smallestUnit: "week", roundingMode: "ceil" }).toString(), "-P2W");
      // rounding increments apply to the calendar unit
      same(() => a.until(Z.from("2020-11-01T00:00[UTC]"), { smallestUnit: "month", roundingIncrement: 4 }).toString(), "P8M");
      same(() => a.until(Z.from("2020-11-01T00:00[UTC]"), { smallestUnit: "month", roundingIncrement: 4, roundingMode: "ceil" }).toString(), "P12M");
      same(() => a.until(Z.from("2020-11-01T00:00[UTC]"), { largestUnit: "years", smallestUnit: "month", roundingIncrement: 4, roundingMode: "ceil" }).toString(), "P1Y");
    "#);
}

#[test]
fn rounding_bubbles_a_completed_unit_into_the_next_larger_one() {
    run(r#"
      const start = Z.from("2020-01-01T00:00[UTC]");
      // 30 days 18 hours ceils to 31 days: one whole month
      same(() => start.until(Z.from("2020-01-31T18:00[UTC]"), { largestUnit: "months", smallestUnit: "days", roundingMode: "ceil" }).toString(), "P1M");
      // ... and eleven months plus that month is a whole year
      same(() => start.until(Z.from("2020-12-31T18:00[UTC]"), { largestUnit: "years", smallestUnit: "days", roundingMode: "ceil" }).toString(), "P1Y");
      same(() => start.until(Z.from("2020-12-31T18:00[UTC]"), { largestUnit: "months", smallestUnit: "days", roundingMode: "ceil" }).toString(), "P12M");
      // six days eighteen hours ceils to a whole week
      same(() => start.until(Z.from("2020-01-07T18:00[UTC]"), { largestUnit: "weeks", smallestUnit: "days", roundingMode: "ceil" }).toString(), "P1W");
      same(() => start.until(Z.from("2020-01-07T18:00[UTC]"), { largestUnit: "months", smallestUnit: "days", roundingMode: "ceil" }).toString(), "P7D");
      // backwards, "expand" carries the same way
      same(() => Z.from("2020-01-31T18:00[UTC]").until(start, { largestUnit: "months", smallestUnit: "days", roundingMode: "expand" }).toString(), "-P1M");
      same(() => Z.from("2020-01-31T18:00[UTC]").until(start, { largestUnit: "months", smallestUnit: "days", roundingMode: "trunc" }).toString(), "-P30D");
      same(() => Z.from("2020-01-31T18:00[UTC]").until(start, { largestUnit: "months", smallestUnit: "days", roundingMode: "ceil" }).toString(), "-P30D");
      // months that complete a year
      same(() => start.until(Z.from("2020-12-20T00:00[UTC]"), { largestUnit: "years", smallestUnit: "months", roundingMode: "ceil" }).toString(), "P1Y");
      same(() => start.until(Z.from("2020-12-20T00:00[UTC]"), { largestUnit: "years", smallestUnit: "months", roundingMode: "floor" }).toString(), "P11M");
      // in a zone, the completing instant is the next wall-clock day
      const ny = Z.from("2020-03-01T00:00[America/New_York]");
      same(() => ny.until(Z.from("2020-03-31T18:00[America/New_York]"), { largestUnit: "months", smallestUnit: "days", roundingMode: "ceil" }).toString(), "P1M");
      same(() => ny.until(Z.from("2020-03-31T18:00[America/New_York]"), { largestUnit: "days", smallestUnit: "days", roundingMode: "ceil" }).toString(), "P31D");
    "#);
}
