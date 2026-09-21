// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Coverage for the string and conversion surface of
//! `Temporal.ZonedDateTime` (`vm/temporal/zoned.rs`): `toString` options
//! (`fractionalSecondDigits`, `smallestUnit`, `roundingMode`, `offset`,
//! `timeZoneName`, `calendarName`), `toJSON`, `valueOf`, `toInstant`,
//! `toPlainDate`/`toPlainTime`/`toPlainDateTime`, `startOfDay` and
//! `getTimeZoneTransition`. (`toPlainYearMonth`, `toPlainMonthDay` and
//! `getISOFields` were removed from `ZonedDateTime` by the June 2024 Temporal
//! consensus; `temporal_removed_methods.rs` asserts they are gone.)
//!
//! Only fixed IANA identifiers, fixed offsets and fixed instants are used, so
//! nothing here depends on the host's own time zone or locale. Every
//! expectation follows the ECMAScript Temporal specification; each test runs a
//! batch of cases in one script and reports every failing case (with its
//! source line) at once.

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
fn to_string_fractional_second_digits_and_smallest_unit() {
    run(r#"
      const z = Z.from("2020-06-15T12:34:56.789123456[America/New_York]");
      const head = "2020-06-15T";
      const tail = "-04:00[America/New_York]";
      same(() => z.toString(), head + "12:34:56.789123456" + tail);
      same(() => z.toString({}), head + "12:34:56.789123456" + tail);
      same(() => z.toString(undefined), head + "12:34:56.789123456" + tail);
      same(() => z.toString({ fractionalSecondDigits: "auto" }), head + "12:34:56.789123456" + tail);
      const expected = ["12:34:56", "12:34:56.7", "12:34:56.78", "12:34:56.789", "12:34:56.7891",
        "12:34:56.78912", "12:34:56.789123", "12:34:56.7891234", "12:34:56.78912345", "12:34:56.789123456"];
      for (let digits = 0; digits <= 9; digits++) {
        CTX = "digits " + digits;
        same(() => z.toString({ fractionalSecondDigits: digits }), head + expected[digits] + tail);
      }
      same(() => z.toString({ fractionalSecondDigits: 2.9 }), head + "12:34:56.78" + tail);
      same(() => z.toString({ smallestUnit: "minute" }), head + "12:34" + tail);
      same(() => z.toString({ smallestUnit: "second" }), head + "12:34:56" + tail);
      same(() => z.toString({ smallestUnit: "millisecond" }), head + "12:34:56.789" + tail);
      same(() => z.toString({ smallestUnit: "microsecond" }), head + "12:34:56.789123" + tail);
      same(() => z.toString({ smallestUnit: "nanosecond" }), head + "12:34:56.789123456" + tail);
      same(() => z.toString({ smallestUnit: "minutes" }), head + "12:34" + tail);
      same(() => z.toString({ smallestUnit: "seconds" }), head + "12:34:56" + tail);
      same(() => z.toString({ smallestUnit: "milliseconds" }), head + "12:34:56.789" + tail);
      same(() => z.toString({ smallestUnit: "microseconds" }), head + "12:34:56.789123" + tail);
      same(() => z.toString({ smallestUnit: "nanoseconds" }), head + "12:34:56.789123456" + tail);
      same(() => z.toString({ smallestUnit: "minute", fractionalSecondDigits: 5 }), head + "12:34" + tail);
      same(() => z.toString({ smallestUnit: "millisecond", fractionalSecondDigits: 0 }), head + "12:34:56.789" + tail);
      same(() => Z.from("2020-06-15T12:34:00[UTC]").toString(), "2020-06-15T12:34:00+00:00[UTC]");
      same(() => Z.from("2020-06-15T12:34:00.5[UTC]").toString(), "2020-06-15T12:34:00.5+00:00[UTC]");
      same(() => Z.from("2020-06-15T12:34[UTC]").toString({ fractionalSecondDigits: 3 }), "2020-06-15T12:34:00.000+00:00[UTC]");
      range(() => z.toString({ smallestUnit: "hour" }));
      range(() => z.toString({ smallestUnit: "day" }));
      range(() => z.toString({ smallestUnit: "year" }));
      range(() => z.toString({ smallestUnit: "bogus" }));
      range(() => z.toString({ fractionalSecondDigits: 10 }));
      range(() => z.toString({ fractionalSecondDigits: -1 }));
      range(() => z.toString({ fractionalSecondDigits: NaN }));
      range(() => z.toString({ fractionalSecondDigits: Infinity }));
      range(() => z.toString({ fractionalSecondDigits: "3" }));
      range(() => z.toString({ fractionalSecondDigits: true }));
      type(() => z.toString({ fractionalSecondDigits: Symbol("x") }));
      type(() => z.toString(5));
      type(() => z.toString(null));
      type(() => z.toString("minute"));
      type(() => Z.prototype.toString.call({}));
      type(() => Z.prototype.toString.call(Temporal.Instant.from("2020-01-01T00:00Z")));
      type(() => Z.prototype.toJSON.call({}));
    "#);
}

#[test]
fn to_string_rounding_modes_and_instant_based_rounding() {
    run(r#"
      const half = Z.from("2020-06-15T12:34:56.785[America/New_York]");
      const modes = {
        ceil: ".79", floor: ".78", trunc: ".78", expand: ".79",
        halfCeil: ".79", halfFloor: ".78", halfTrunc: ".78", halfExpand: ".79", halfEven: ".78",
      };
      for (const mode of Object.keys(modes)) {
        CTX = mode;
        same(() => half.toString({ fractionalSecondDigits: 2, roundingMode: mode }), "2020-06-15T12:34:56" + modes[mode] + "-04:00[America/New_York]");
      }
      same(() => half.toString({ smallestUnit: "second", roundingMode: "ceil" }), "2020-06-15T12:34:57-04:00[America/New_York]");
      same(() => half.toString({ smallestUnit: "minute", roundingMode: "ceil" }), "2020-06-15T12:35-04:00[America/New_York]");
      same(() => half.toString({ smallestUnit: "minute", roundingMode: "floor" }), "2020-06-15T12:34-04:00[America/New_York]");
      const fine = Z.from("2020-06-15T12:34:56.123456789[UTC]");
      same(() => fine.toString({ fractionalSecondDigits: 4, roundingMode: "ceil" }), "2020-06-15T12:34:56.1235+00:00[UTC]");
      same(() => fine.toString({ fractionalSecondDigits: 5, roundingMode: "halfExpand" }), "2020-06-15T12:34:56.12346+00:00[UTC]");
      same(() => fine.toString({ fractionalSecondDigits: 7, roundingMode: "ceil" }), "2020-06-15T12:34:56.1234568+00:00[UTC]");
      same(() => fine.toString({ smallestUnit: "microsecond", roundingMode: "ceil" }), "2020-06-15T12:34:56.123457+00:00[UTC]");
      same(() => fine.toString({ smallestUnit: "nanosecond", roundingMode: "ceil" }), "2020-06-15T12:34:56.123456789+00:00[UTC]");
      // rounding acts on the exact instant, so the result carries across a day boundary
      same(() => Z.from("2020-06-15T23:59:59.9995[UTC]").toString({ fractionalSecondDigits: 3, roundingMode: "halfExpand" }), "2020-06-16T00:00:00.000+00:00[UTC]");
      same(() => Z.from("2020-12-31T23:59:30[UTC]").toString({ smallestUnit: "minute", roundingMode: "halfExpand" }), "2021-01-01T00:00+00:00[UTC]");
      // ... and the offset is the one in force at the *rounded* instant
      same(() => Z.from("2020-03-08T01:59:59.9995[America/New_York]").toString({ fractionalSecondDigits: 3, roundingMode: "ceil" }), "2020-03-08T03:00:00.000-04:00[America/New_York]");
      same(() => Z.from("2020-11-01T01:59:59.9995-04:00[America/New_York]").toString({ fractionalSecondDigits: 3, roundingMode: "ceil" }), "2020-11-01T01:00:00.000-05:00[America/New_York]");
      // negative epoch values round as if positive-directional ("floor"/"ceil" are on the number line)
      same(() => Z.from("1969-12-31T23:59:59.9995[UTC]").toString({ fractionalSecondDigits: 3, roundingMode: "ceil" }), "1970-01-01T00:00:00.000+00:00[UTC]");
      same(() => Z.from("1969-12-31T23:59:59.9995[UTC]").toString({ fractionalSecondDigits: 3, roundingMode: "floor" }), "1969-12-31T23:59:59.999+00:00[UTC]");
      range(() => half.toString({ roundingMode: "bogus" }));
      range(() => half.toString({ roundingMode: "" }));
      // offset, timeZoneName and calendarName
      same(() => half.toString({ offset: "auto" }), "2020-06-15T12:34:56.785-04:00[America/New_York]");
      same(() => half.toString({ offset: "never" }), "2020-06-15T12:34:56.785[America/New_York]");
      range(() => half.toString({ offset: "always" }));
      range(() => half.toString({ offset: "" }));
      same(() => half.toString({ timeZoneName: "auto" }), "2020-06-15T12:34:56.785-04:00[America/New_York]");
      same(() => half.toString({ timeZoneName: "never" }), "2020-06-15T12:34:56.785-04:00");
      same(() => half.toString({ timeZoneName: "critical" }), "2020-06-15T12:34:56.785-04:00[!America/New_York]");
      range(() => half.toString({ timeZoneName: "always" }));
      same(() => half.toString({ offset: "never", timeZoneName: "never" }), "2020-06-15T12:34:56.785");
      same(() => half.toString({ calendarName: "auto" }), "2020-06-15T12:34:56.785-04:00[America/New_York]");
      same(() => half.toString({ calendarName: "always" }), "2020-06-15T12:34:56.785-04:00[America/New_York][u-ca=iso8601]");
      same(() => half.toString({ calendarName: "never" }), "2020-06-15T12:34:56.785-04:00[America/New_York]");
      same(() => half.toString({ calendarName: "critical" }), "2020-06-15T12:34:56.785-04:00[America/New_York][!u-ca=iso8601]");
      range(() => half.toString({ calendarName: "bogus" }));
      same(() => half.toString({ timeZoneName: "critical", calendarName: "critical" }), "2020-06-15T12:34:56.785-04:00[!America/New_York][!u-ca=iso8601]");
      const g = Z.from("2020-06-15T12:34:56[America/New_York][u-ca=gregory]");
      same(() => g.toString(), "2020-06-15T12:34:56-04:00[America/New_York][u-ca=gregory]");
      same(() => g.toString({ calendarName: "never" }), "2020-06-15T12:34:56-04:00[America/New_York]");
      same(() => g.toString({ calendarName: "critical" }), "2020-06-15T12:34:56-04:00[America/New_York][!u-ca=gregory]");
      same(() => g.toJSON(), "2020-06-15T12:34:56-04:00[America/New_York][u-ca=gregory]");
      // offset time zones and sub-minute historical offsets
      const fixed = Z.from("2020-06-15T12:00:00+05:30[+05:30]");
      same(() => fixed.toString(), "2020-06-15T12:00:00+05:30[+05:30]");
      same(() => fixed.toString({ timeZoneName: "critical" }), "2020-06-15T12:00:00+05:30[!+05:30]");
      same(() => fixed.toString({ offset: "never" }), "2020-06-15T12:00:00[+05:30]");
      same(() => fixed.offset, "+05:30");
      // `toString` prints `FormatDateTimeUTCOffsetRounded` (a minute-precision offset, halves away
      // from zero); the `offset` getter below keeps the exact sub-minute value.
      same(() => Z.from("1970-01-01T00:00:00-00:44:30[Africa/Monrovia]").toString(), "1970-01-01T00:00:00-00:45[Africa/Monrovia]");
      same(() => Z.from("1970-01-01T00:00:00-00:44:30[Africa/Monrovia]").offset, "-00:44:30");
      same(() => Z.from("1970-01-01T00:00:00-00:44:30[Africa/Monrovia]").offsetNanoseconds, -2670000000000);
      // extended years
      same(() => Z.from("+010000-01-01T00:00:00+00:00[UTC]").toString(), "+010000-01-01T00:00:00+00:00[UTC]");
      same(() => Z.from("-000001-06-01T00:00:00+00:00[UTC]").toString(), "-000001-06-01T00:00:00+00:00[UTC]");
      same(() => Z.from("0001-06-01T00:00:00+00:00[UTC]").toString(), "0001-06-01T00:00:00+00:00[UTC]");
      same(() => new Z(8640000000000000000000n, "UTC").toString(), "+275760-09-13T00:00:00+00:00[UTC]");
      same(() => new Z(-8640000000000000000000n, "UTC").toString(), "-271821-04-20T00:00:00+00:00[UTC]");
      // valueOf never converts
      type(() => half.valueOf());
      type(() => half + 1);
      type(() => half < half);
      same(() => JSON.stringify(half), '"2020-06-15T12:34:56.785-04:00[America/New_York]"');
      same(() => Object.prototype.toString.call(half), "[object Temporal.ZonedDateTime]");
    "#);
}

#[test]
fn conversions_to_instant_and_plain_types() {
    run(r#"
      const z = Z.from("2020-06-15T12:34:56.789123456[America/New_York]");
      same(() => z.toInstant().toString(), "2020-06-15T16:34:56.789123456Z");
      same(() => z.toInstant() instanceof Temporal.Instant, true);
      same(() => z.toInstant().epochNanoseconds, 1592238896789123456n);
      same(() => z.toPlainDate().toString(), "2020-06-15");
      same(() => z.toPlainTime().toString(), "12:34:56.789123456");
      same(() => z.toPlainDateTime().toString(), "2020-06-15T12:34:56.789123456");
      same(() => z.toPlainDate() instanceof Temporal.PlainDate, true);
      same(() => z.toPlainTime() instanceof Temporal.PlainTime, true);
      same(() => z.toPlainDateTime() instanceof Temporal.PlainDateTime, true);
      // the plain views use the zone's wall clock, which differs from UTC
      const late = Z.from("2020-06-15T23:30:00-04:00[America/New_York]");
      same(() => late.toPlainDate().toString(), "2020-06-15");
      same(() => late.toInstant().toString(), "2020-06-16T03:30:00Z");
      same(() => Z.from("2020-12-31T23:30:00[Pacific/Kiritimati]").toPlainDate().toString(), "2020-12-31");
      // calendars are preserved by every conversion
      const g = Z.from("2020-06-15T12:34:56[America/New_York][u-ca=gregory]");
      same(() => g.toPlainDate().calendarId, "gregory");
      same(() => g.toPlainDateTime().calendarId, "gregory");
      same(() => g.toPlainDateTime().toString(), "2020-06-15T12:34:56[u-ca=gregory]");
      const heb = Z.from({ year: 5784, monthCode: "M05L", day: 10, timeZone: "UTC", calendar: "hebrew" });
      same(() => heb.toPlainDate().monthCode, "M05L");
      same(() => heb.toPlainDate().year, 5784);
      same(() => heb.toPlainDate().day, 10);
      const jp = Z.from("2020-06-15T00:00:00[UTC][u-ca=japanese]");
      same(() => jp.toPlainDate().era, "reiwa");
      same(() => jp.toPlainDate().eraYear, 2);
      same(() => jp.toPlainDate().monthCode, "M06");
      type(() => Z.prototype.toInstant.call({}));
      type(() => Z.prototype.toPlainDate.call({}));
      type(() => Z.prototype.toPlainTime.call({}));
      type(() => Z.prototype.toPlainDateTime.call({}));
      type(() => Z.prototype.startOfDay.call({}));
      type(() => Z.prototype.getTimeZoneTransition.call({}, "next"));
      type(() => Z.prototype.valueOf.call({}));
    "#);
}

#[test]
fn start_of_day_follows_the_real_length_of_the_day() {
    run(r#"
      same(() => Z.from("2020-06-15T12:34:56.789[America/New_York]").startOfDay().toString(), "2020-06-15T00:00:00-04:00[America/New_York]");
      same(() => Z.from("2020-03-08T12:00[America/New_York]").startOfDay().toString(), "2020-03-08T00:00:00-05:00[America/New_York]");
      same(() => Z.from("2020-11-01T12:00[America/New_York]").startOfDay().toString(), "2020-11-01T00:00:00-04:00[America/New_York]");
      // midnight does not exist: São Paulo sprang forward from 00:00 to 01:00
      same(() => Z.from("2018-11-04T12:00[America/Sao_Paulo]").startOfDay().toString(), "2018-11-04T01:00:00-02:00[America/Sao_Paulo]");
      same(() => Z.from("2018-11-04T23:59:59[America/Sao_Paulo]").startOfDay().toString(), "2018-11-04T01:00:00-02:00[America/Sao_Paulo]");
      same(() => Z.from("2020-06-15T12:00[UTC]").startOfDay().toString(), "2020-06-15T00:00:00+00:00[UTC]");
      same(() => Z.from("2020-06-15T12:00+05:30[+05:30]").startOfDay().toString(), "2020-06-15T00:00:00+05:30[+05:30]");
      same(() => Z.from("2020-06-15T12:00[UTC][u-ca=gregory]").startOfDay().toString(), "2020-06-15T00:00:00+00:00[UTC][u-ca=gregory]");
      same(() => Z.from("2020-06-15T00:00[UTC]").startOfDay().epochNanoseconds, 1592179200000000000n);
      same(() => Z.from("2020-06-15T12:00[UTC]").startOfDay().hoursInDay, 24);
      same(() => Z.from("2018-11-04T12:00[America/Sao_Paulo]").hoursInDay, 23);
      same(() => Z.from("2020-11-01T12:00[America/New_York]").hoursInDay, 25);
      same(() => Z.from("2020-03-08T12:00[America/New_York]").hoursInDay, 23);
      // arguments are ignored
      same(() => Z.from("2020-06-15T12:00[UTC]").startOfDay(5).toString(), "2020-06-15T00:00:00+00:00[UTC]");
    "#);
}

#[test]
fn get_time_zone_transition_finds_adjacent_offset_changes() {
    run(r#"
      const summer = Z.from("2020-06-15T12:00[America/New_York]");
      same(() => summer.getTimeZoneTransition("next").toString(), "2020-11-01T01:00:00-05:00[America/New_York]");
      same(() => summer.getTimeZoneTransition("previous").toString(), "2020-03-08T03:00:00-04:00[America/New_York]");
      same(() => summer.getTimeZoneTransition({ direction: "next" }).toString(), "2020-11-01T01:00:00-05:00[America/New_York]");
      same(() => summer.getTimeZoneTransition({ direction: "previous" }).toString(), "2020-03-08T03:00:00-04:00[America/New_York]");
      same(() => summer.getTimeZoneTransition("next") instanceof Z, true);
      same(() => summer.getTimeZoneTransition("next").timeZoneId, "America/New_York");
      // the transition instant itself is never its own neighbour
      const transition = Z.from("2020-11-01T01:00:00-05:00[America/New_York]");
      same(() => transition.getTimeZoneTransition("previous").toString(), "2020-03-08T03:00:00-04:00[America/New_York]");
      same(() => transition.getTimeZoneTransition("next").toString(), "2021-03-14T03:00:00-04:00[America/New_York]");
      same(() => Z.from("2020-11-01T00:59:59.999999999-04:00[America/New_York]").getTimeZoneTransition("next").toString(), "2020-11-01T01:00:00-05:00[America/New_York]");
      // calendars are preserved
      same(() => Z.from("2020-06-15T12:00[America/New_York][u-ca=gregory]").getTimeZoneTransition("next").toString(), "2020-11-01T01:00:00-05:00[America/New_York][u-ca=gregory]");
      // history reaches back to the adoption of standard time
      same(() => Z.from("1900-01-01T00:00[America/New_York]").getTimeZoneTransition("previous").toString(), "1883-11-18T12:00:00-05:00[America/New_York]");
      same(() => Z.from("1800-01-01T00:00[America/New_York]").getTimeZoneTransition("previous"), null);
      // zones without transitions
      same(() => Z.from("2020-06-15T12:00[UTC]").getTimeZoneTransition("next"), null);
      same(() => Z.from("2020-06-15T12:00[UTC]").getTimeZoneTransition("previous"), null);
      same(() => Z.from("2020-06-15T12:00+05:30[+05:30]").getTimeZoneTransition("next"), null);
      same(() => Z.from("2020-06-15T12:00+05:30[+05:30]").getTimeZoneTransition("previous"), null);
      same(() => Z.from("2020-06-15T12:00[UTC]").getTimeZoneTransition({ direction: "next" }), null);
      // direction is required and validated
      type(() => summer.getTimeZoneTransition());
      type(() => summer.getTimeZoneTransition(undefined));
      type(() => summer.getTimeZoneTransition(5));
      type(() => summer.getTimeZoneTransition(null));
      type(() => summer.getTimeZoneTransition(true));
      range(() => summer.getTimeZoneTransition({}));
      range(() => summer.getTimeZoneTransition({ direction: undefined }));
      range(() => summer.getTimeZoneTransition({ direction: "bogus" }));
      range(() => summer.getTimeZoneTransition({ direction: "" }));
      range(() => summer.getTimeZoneTransition({ direction: 5 }));
      range(() => summer.getTimeZoneTransition({ direction: "NEXT" }));
      range(() => summer.getTimeZoneTransition("bogus"));
      range(() => summer.getTimeZoneTransition(""));
      same(() => Z.prototype.getTimeZoneTransition.length, 1);
    "#);
}
