// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The exact set of own members every Temporal prototype and constructor
//! exposes (Phase 26 Stage 3,
//! `development/browser_core/phase-26-ecma262-temporal/PLAN.md`).
//!
//! Test262 checks that specified members exist and behave; it has almost no
//! test that an *unspecified* member is absent, so an extra accessor can sit
//! undetected (`PlainMonthDay.prototype.month` did: the specification defines
//! only `calendarId`, `monthCode` and `day` there). This test therefore
//! compares each own-property-name set with the specification's, member for
//! member.
//!
//! The expected lists were generated from the section headings of
//! `spec/{duration,instant,plaindate,plaindatetime,plainmonthday,plaintime,
//! plainyearmonth,zoneddatetime}.html` in `tc39/proposal-temporal` (Stage 4,
//! `main` at `e8cc03fc970a`, 2026-07-27), prototype members as
//! `Temporal.X.prototype.name` / `get Temporal.X.prototype.name` and statics
//! as `Temporal.X.name ( )`. `constructor`, `length`, `name` and `prototype`
//! are not listed. When the specification adds or removes a member, update the
//! list here together with the implementation.

use blueice_bluejs::{compile, parse, Value, Vm};

#[test]
fn every_temporal_type_exposes_exactly_the_specified_members() {
    let script = r#"(function() {
      const spec = {
__BODY__
      };
      const failures = [];
      const skip = ["constructor", "length", "name", "prototype"];
      function ownNames(target) {
        return Object.getOwnPropertyNames(target).filter(function (n) { return skip.indexOf(n) < 0; }).sort();
      }
      for (const kind of Object.keys(spec)) {
        for (const [label, target] of [["prototype", Temporal[kind].prototype], ["static", Temporal[kind]]]) {
          const expected = spec[kind][label];
          const actual = ownNames(target);
          const extra = actual.filter(function (n) { return expected.indexOf(n) < 0; });
          const missing = expected.filter(function (n) { return actual.indexOf(n) < 0; });
          if (extra.length) failures.push(kind + " " + label + " has unspecified: " + extra.join(", "));
          if (missing.length) failures.push(kind + " " + label + " lacks specified: " + missing.join(", "));
        }
      }
      const namespace = Object.getOwnPropertyNames(Temporal).sort().join(",");
      const expectedNamespace = "Duration,Instant,Now,PlainDate,PlainDateTime,PlainMonthDay,PlainTime,PlainYearMonth,ZonedDateTime";
      if (namespace !== expectedNamespace) failures.push("Temporal namespace: " + namespace);
      const now = Object.getOwnPropertyNames(Temporal.Now).sort().join(",");
      const expectedNow = "instant,plainDateISO,plainDateTimeISO,plainTimeISO,timeZoneId,zonedDateTimeISO";
      if (now !== expectedNow) failures.push("Temporal.Now: " + now);
      return failures.length === 0 ? "ok" : "\n" + failures.join("\n");
    })()"#
        .replace("__BODY__", BODY);
    let value = Vm::default()
        .execute(&compile(&parse(&script).unwrap()).unwrap())
        .unwrap_or_else(|error| panic!("{script}\n  -> {error:?}"));
    match value {
        Value::String(text) if text == "ok" => {}
        Value::String(text) => panic!("mismatches:{}", text.to_utf8().unwrap()),
        other => panic!("expected \"ok\" or a mismatch list, got {other:?}"),
    }
}

/// `{ Kind: { prototype: [...], static: [...] }, ... }` entries, sorted.
const BODY: &str = r#"
        Duration: {
          prototype: ["abs", "add", "blank", "days", "hours", "microseconds", "milliseconds", "minutes", "months", "nanoseconds", "negated", "round", "seconds", "sign", "subtract", "toJSON", "toLocaleString", "toString", "total", "valueOf", "weeks", "with", "years"],
          static: ["compare", "from"],
        },
        Instant: {
          prototype: ["add", "epochMilliseconds", "epochNanoseconds", "equals", "round", "since", "subtract", "toJSON", "toLocaleString", "toString", "toZonedDateTimeISO", "until", "valueOf"],
          static: ["compare", "from", "fromEpochMilliseconds", "fromEpochNanoseconds"],
        },
        PlainDate: {
          prototype: ["add", "calendarId", "day", "dayOfWeek", "dayOfYear", "daysInMonth", "daysInWeek", "daysInYear", "equals", "era", "eraYear", "inLeapYear", "month", "monthCode", "monthsInYear", "since", "subtract", "toJSON", "toLocaleString", "toPlainDateTime", "toPlainMonthDay", "toPlainYearMonth", "toString", "toZonedDateTime", "until", "valueOf", "weekOfYear", "with", "withCalendar", "year", "yearOfWeek"],
          static: ["compare", "from"],
        },
        PlainDateTime: {
          prototype: ["add", "calendarId", "day", "dayOfWeek", "dayOfYear", "daysInMonth", "daysInWeek", "daysInYear", "equals", "era", "eraYear", "hour", "inLeapYear", "microsecond", "millisecond", "minute", "month", "monthCode", "monthsInYear", "nanosecond", "round", "second", "since", "subtract", "toJSON", "toLocaleString", "toPlainDate", "toPlainTime", "toString", "toZonedDateTime", "until", "valueOf", "weekOfYear", "with", "withCalendar", "withPlainTime", "year", "yearOfWeek"],
          static: ["compare", "from"],
        },
        PlainMonthDay: {
          prototype: ["calendarId", "day", "equals", "monthCode", "toJSON", "toLocaleString", "toPlainDate", "toString", "valueOf", "with"],
          static: ["from"],
        },
        PlainTime: {
          prototype: ["add", "equals", "hour", "microsecond", "millisecond", "minute", "nanosecond", "round", "second", "since", "subtract", "toJSON", "toLocaleString", "toString", "until", "valueOf", "with"],
          static: ["compare", "from"],
        },
        PlainYearMonth: {
          prototype: ["add", "calendarId", "daysInMonth", "daysInYear", "equals", "era", "eraYear", "inLeapYear", "month", "monthCode", "monthsInYear", "since", "subtract", "toJSON", "toLocaleString", "toPlainDate", "toString", "until", "valueOf", "with", "year"],
          static: ["compare", "from"],
        },
        ZonedDateTime: {
          prototype: ["add", "calendarId", "day", "dayOfWeek", "dayOfYear", "daysInMonth", "daysInWeek", "daysInYear", "epochMilliseconds", "epochNanoseconds", "equals", "era", "eraYear", "getTimeZoneTransition", "hour", "hoursInDay", "inLeapYear", "microsecond", "millisecond", "minute", "month", "monthCode", "monthsInYear", "nanosecond", "offset", "offsetNanoseconds", "round", "second", "since", "startOfDay", "subtract", "timeZoneId", "toInstant", "toJSON", "toLocaleString", "toPlainDate", "toPlainDateTime", "toPlainTime", "toString", "until", "valueOf", "weekOfYear", "with", "withCalendar", "withPlainTime", "withTimeZone", "year", "yearOfWeek"],
          static: ["compare", "from"],
        },
"#;
