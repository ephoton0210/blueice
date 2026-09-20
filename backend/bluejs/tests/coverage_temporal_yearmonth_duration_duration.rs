// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Extra coverage for `Temporal.Duration`'s `relativeTo`-dependent surface
//! (`vm/temporal/duration_relative.rs`, `duration_operations.rs`):
//! `relativeTo` resolution from objects, strings and property bags, and
//! `round`/`total`/`compare` against `PlainDate`, fixed-offset and named
//! time-zone (daylight-saving) anchors under every rounding mode. Every
//! expectation is what the ECMAScript Temporal specification requires; each
//! test runs inside one JavaScript program that collects mismatches so a
//! failure lists every deviating case at once.

use blueice_bluejs::{compile, parse, Value, Vm};

/// Runs `body` after a small prelude defining `check(label, expected, thunk)`,
/// which compares `String(thunk())` (or the thrown error's class name) with
/// `expected`, and `near(label, expected, thunk)`, which compares a number
/// within `1e-9`. The program's result is the newline-joined list of
/// mismatches and must be empty.
fn run_checks(body: &str) {
    let source = format!(
        r#"
        (function() {{
          const mismatches = [];
          function check(label, expected, thunk) {{
            let actual;
            try {{
              actual = String(thunk());
            }} catch (e) {{
              actual = e instanceof RangeError ? "RangeError"
                : e instanceof TypeError ? "TypeError" : "Error:" + e;
            }}
            if (actual !== expected) {{
              mismatches.push(label + ": expected <" + expected + "> got <" + actual + ">");
            }}
          }}
          function near(label, expected, thunk) {{
            let actual;
            try {{
              actual = thunk();
            }} catch (e) {{
              actual = e instanceof RangeError ? "RangeError"
                : e instanceof TypeError ? "TypeError" : "Error:" + e;
            }}
            if (typeof actual !== "number" || !(Math.abs(actual - expected) < 1e-9)) {{
              mismatches.push(label + ": expected ~" + expected + " got <" + actual + ">");
            }}
          }}
          function throwsSome(label, thunk) {{
            try {{
              thunk();
              mismatches.push(label + ": expected an exception");
            }} catch (e) {{
              if (!(e instanceof RangeError || e instanceof TypeError)) {{
                mismatches.push(label + ": unexpected exception " + e);
              }}
            }}
          }}
          const D = Temporal.Duration;
          const NY = "[America/New_York]";
          {body}
          return mismatches.join("\n");
        }})()
        "#
    );
    let program = compile(&parse(&source).unwrap()).unwrap();
    let result = Vm::default()
        .execute(&program)
        .unwrap_or_else(|error| panic!("{error:?}"));
    match result {
        Value::String(text) => assert!(
            text.is_empty(),
            "mismatches:\n{}",
            String::from_utf16_lossy(text.as_code_units())
        ),
        other => panic!("unexpected result {other:?}"),
    }
}

#[test]
fn relative_to_resolves_plain_objects_and_strings() {
    run_checks(
        r#"
        const forty = new D(0, 0, 0, 40);
        const opts = (relativeTo) => ({ largestUnit: "month", relativeTo });
        check("PlainDate", "P1M9D", () => forty.round(opts(new Temporal.PlainDate(2020, 1, 1))));
        check("PlainDateTime", "P1M9D", () => forty.round(opts(new Temporal.PlainDateTime(2020, 1, 1, 12))));
        check("ZonedDateTime", "P1M9D",
              () => forty.round(opts(new Temporal.ZonedDateTime(1577880000000000000n, "UTC"))));
        check("date string", "P1M9D", () => forty.round(opts("2020-01-01")));
        check("date-time string", "P1M9D", () => forty.round(opts("2020-01-01T10:00")));
        check("utc zone string", "P1M9D", () => forty.round(opts("2020-01-01T00:00[UTC]")));
        check("offset zone string", "P1M9D", () => forty.round(opts("2020-01-01T00:00+01:00[+01:00]")));
        check("named zone string", "P1M9D", () => forty.round(opts("2020-01-01T00:00" + NY)));
        check("other month", "P1M11D", () => forty.round(opts("2020-02-01")));
        check("calendar annotation", "P1M9D", () => forty.round(opts("2020-01-01[u-ca=gregory]")));
        check("bare Z rejected", "RangeError", () => forty.round(opts("2020-01-01T00:00Z")));
        check("garbage rejected", "RangeError", () => forty.round(opts("nonsense")));
        check("empty string rejected", "RangeError", () => forty.round(opts("")));
        check("number rejected", "TypeError", () => forty.round(opts(5)));
        check("boolean rejected", "TypeError", () => forty.round(opts(true)));
        check("null rejected", "TypeError", () => forty.round(opts(null)));
        check("symbol rejected", "TypeError", () => forty.round(opts(Symbol())));
        check("bigint rejected", "TypeError", () => forty.round(opts(5n)));
        check("year-month object needs a day", "TypeError",
              () => forty.round(opts(new Temporal.PlainYearMonth(2020, 1))));
        check("month-day object needs a year", "TypeError",
              () => forty.round(opts(new Temporal.PlainMonthDay(1, 1))));
        check("total accepts the same strings", "1", () => Math.floor(forty.total({ unit: "month", relativeTo: "2020-01-01" })));
        near("total fraction", 1 + 9 / 29, () => forty.total({ unit: "month", relativeTo: "2020-01-01" }));
        check("undefined relativeTo means none", "RangeError",
              () => forty.round({ largestUnit: "month", relativeTo: undefined }));
        check("date limit", "RangeError", () => forty.round(opts("+275760-09-13")));
        check("date beyond limit", "RangeError", () => forty.round(opts("+275760-09-14")));
        check("date below limit", "RangeError", () => forty.round(opts("-271821-04-18")));
        check("blank tolerates limit date", "PT0S", () => new D().round({ largestUnit: "day", relativeTo: "+275760-09-13" }));
        "#,
    );
}

#[test]
fn relative_to_resolves_property_bags() {
    run_checks(
        r#"
        const forty = new D(0, 0, 0, 40);
        const opts = (relativeTo) => ({ largestUnit: "month", relativeTo });
        check("basic", "P1M9D", () => forty.round(opts({ year: 2020, month: 1, day: 1 })));
        check("monthCode", "P1M9D", () => forty.round(opts({ year: 2020, monthCode: "M01", day: 1 })));
        check("both agree", "P1M9D", () => forty.round(opts({ year: 2020, month: 1, monthCode: "M01", day: 1 })));
        check("both disagree", "RangeError",
              () => forty.round(opts({ year: 2020, month: 2, monthCode: "M01", day: 1 })));
        check("no year", "TypeError", () => forty.round(opts({ month: 1, day: 1 })));
        check("no month", "TypeError", () => forty.round(opts({ year: 2020, day: 1 })));
        check("no day", "TypeError", () => forty.round(opts({ year: 2020, month: 1 })));
        check("empty bag", "TypeError", () => forty.round(opts({})));
        check("year infinity", "RangeError", () => forty.round(opts({ year: Infinity, month: 1, day: 1 })));
        check("month zero", "RangeError", () => forty.round(opts({ year: 2020, month: 0, day: 1 })));
        check("day zero", "RangeError", () => forty.round(opts({ year: 2020, month: 1, day: 0 })));
        check("time field range", "RangeError", () => forty.round(opts({ year: 2020, month: 1, day: 1, hour: 24 })));
        check("minute range", "RangeError", () => forty.round(opts({ year: 2020, month: 1, day: 1, minute: 60 })));
        check("millisecond range", "RangeError", () => forty.round(opts({ year: 2020, month: 1, day: 1, millisecond: 1000 })));
        check("time fields ignored without zone", "P1M9D",
              () => forty.round(opts({ year: 2020, month: 1, day: 1, hour: 5, minute: 6, second: 7,
                                       millisecond: 8, microsecond: 9, nanosecond: 10 })));
        check("utc zone", "P1M9D", () => forty.round(opts({ year: 2020, month: 1, day: 1, timeZone: "UTC" })));
        check("named zone with time", "P1M9D",
              () => forty.round(opts({ year: 2020, month: 1, day: 1, hour: 12, timeZone: "America/New_York" })));
        check("offset zone", "P1M9D", () => forty.round(opts({ year: 2020, month: 1, day: 1, timeZone: "+01:00" })));
        check("second 60 clamps", "P1M9D",
              () => forty.round(opts({ year: 2020, month: 1, day: 1, second: 60, timeZone: "UTC" })));
        check("matching offset", "P1M9D",
              () => forty.round(opts({ year: 2020, month: 1, day: 1, timeZone: "UTC", offset: "+00:00" })));
        check("mismatching offset", "RangeError",
              () => forty.round(opts({ year: 2020, month: 1, day: 1, timeZone: "UTC", offset: "+05:00" })));
        check("bad offset string", "RangeError",
              () => forty.round(opts({ year: 2020, month: 1, day: 1, timeZone: "UTC", offset: "nonsense" })));
        check("offset number", "TypeError",
              () => forty.round(opts({ year: 2020, month: 1, day: 1, timeZone: "UTC", offset: 5 })));
        check("offset without zone ignored", "P1M9D", () => forty.round(opts({ year: 2020, month: 1, day: 1, offset: "+01:00" })));
        check("bad time zone", "RangeError", () => forty.round(opts({ year: 2020, month: 1, day: 1, timeZone: "Nowhere/Land" })));
        check("time zone wrong type", "TypeError", () => forty.round(opts({ year: 2020, month: 1, day: 1, timeZone: 5 })));
        check("gregory calendar", "P1M9D", () => forty.round(opts({ year: 2020, month: 1, day: 1, calendar: "gregory" })));
        check("gregory era", "P1M9D",
              () => forty.round(opts({ era: "ce", eraYear: 2020, month: 1, day: 1, calendar: "gregory" })));
        throwsSome("eraYear without era", () => forty.round(opts({ eraYear: 2020, month: 1, day: 1, calendar: "gregory" })));
        check("gregory era infinity", "RangeError",
              () => forty.round(opts({ era: "ce", eraYear: Infinity, month: 1, day: 1, calendar: "gregory" })));
        check("bad calendar", "RangeError", () => forty.round(opts({ year: 2020, month: 1, day: 1, calendar: "nonsense" })));
        check("calendar wrong type", "TypeError", () => forty.round(opts({ year: 2020, month: 1, day: 1, calendar: 5 })));
        check("hebrew calendar", "P1M10D", () => forty.round(opts({ year: 5784, monthCode: "M01", day: 1, calendar: "hebrew" })));
        check("bad monthCode", "RangeError", () => forty.round(opts({ year: 2020, monthCode: "M13", day: 1 })));
        check("field order is observable", "day,hour,microsecond,millisecond,minute,month,monthCode,nanosecond,offset,second,timeZone,year",
              () => {
                const log = [];
                const bag = new Proxy({ year: 2020, month: 1, day: 1 }, {
                  get(target, key) { if (typeof key === "string" && key !== "calendar") log.push(key); return target[key]; },
                });
                forty.round(opts(bag));
                return log.join(",");
              });
        "#,
    );
}

#[test]
fn round_and_total_validate_options() {
    run_checks(
        r#"
        const d = new D(0, 0, 0, 1, 2, 3);
        check("round no args", "TypeError", () => d.round());
        check("round undefined", "TypeError", () => d.round(undefined));
        check("round null", "TypeError", () => d.round(null));
        check("round number", "TypeError", () => d.round(5));
        check("round empty", "RangeError", () => d.round({}));
        check("round unit string", "P1DT2H3M", () => d.round("minute"));
        check("round unit string hour", "P1DT2H", () => d.round("hour"));
        check("round bad unit string", "RangeError", () => d.round("nonsense"));
        check("round plural unit", "P1DT2H", () => d.round({ smallestUnit: "hours" }));
        check("round auto smallest", "RangeError", () => d.round({ smallestUnit: "auto" }));
        check("round auto largest", "P1DT2H3M", () => d.round({ largestUnit: "auto" }));
        check("round largest smaller than smallest", "RangeError",
              () => d.round({ largestUnit: "minute", smallestUnit: "hour" }));
        check("round largest hour", "PT26H3M", () => d.round({ largestUnit: "hour" }));
        check("round largest minute", "PT1563M", () => d.round({ largestUnit: "minute" }));
        check("round largest second", "PT93780S", () => d.round({ largestUnit: "second" }));
        check("round largest millisecond", "PT93780S", () => d.round({ largestUnit: "millisecond" }));
        check("round largest microsecond", "PT93780S", () => d.round({ largestUnit: "microsecond" }));
        check("round largest nanosecond", "PT93780S", () => d.round({ largestUnit: "nanosecond" }));
        check("round bad mode", "RangeError", () => d.round({ smallestUnit: "hour", roundingMode: "nonsense" }));
        check("round mode not string coerced", "RangeError", () => d.round({ smallestUnit: "hour", roundingMode: 5 }));
        check("round increment zero", "RangeError", () => d.round({ smallestUnit: "hour", roundingIncrement: 0 }));
        check("round increment negative", "RangeError", () => d.round({ smallestUnit: "hour", roundingIncrement: -1 }));
        check("round increment infinity", "RangeError", () => d.round({ smallestUnit: "hour", roundingIncrement: Infinity }));
        check("round increment NaN", "RangeError", () => d.round({ smallestUnit: "hour", roundingIncrement: NaN }));
        check("round increment too big for hour", "RangeError", () => d.round({ smallestUnit: "hour", roundingIncrement: 24 }));
        check("round increment not dividing hour", "RangeError", () => d.round({ smallestUnit: "hour", roundingIncrement: 5 }));
        check("round increment 4 hours", "P1DT4H", () => d.round({ smallestUnit: "hour", roundingIncrement: 4 }));
        check("round increment 30 minutes", "P1DT2H",
              () => d.round({ smallestUnit: "minute", roundingIncrement: 30 }));
        check("round increment 60 minutes", "RangeError", () => d.round({ smallestUnit: "minute", roundingIncrement: 60 }));
        check("round increment 1000 milliseconds", "RangeError",
              () => d.round({ smallestUnit: "millisecond", roundingIncrement: 1000 }));
        check("round increment 500 milliseconds", "P1DT2H3M",
              () => d.round({ smallestUnit: "millisecond", roundingIncrement: 500 }));
        check("round day increment", "P2D", () => new D(0, 0, 0, 3).round({ smallestUnit: "day", roundingIncrement: 2, roundingMode: "floor" }));
        check("round increment with fraction truncates", "P1DT2H",
              () => d.round({ smallestUnit: "hour", roundingIncrement: 1.9 }));
        check("round calendar unit without relativeTo", "RangeError", () => d.round({ largestUnit: "year" }));
        check("round week without relativeTo", "RangeError", () => d.round({ smallestUnit: "week" }));
        check("round years without relativeTo", "RangeError", () => new D(1).round({ smallestUnit: "day" }));
        check("round months without relativeTo", "RangeError", () => new D(0, 1).round({ largestUnit: "day" }));
        check("round weeks without relativeTo", "RangeError", () => new D(0, 0, 1).round({ smallestUnit: "day" }));
        check("round bad relativeTo beats calendar", "RangeError", () => d.round({ largestUnit: "day", relativeTo: "nonsense" }));
        check("round bad option object", "TypeError", () => d.round({ smallestUnit: "hour", relativeTo: 5 }));
        check("round accepts symbol unit", "TypeError", () => d.round({ smallestUnit: Symbol() }));
        check("round option reads are ordered", "largestUnit,relativeTo,roundingIncrement,roundingMode,smallestUnit",
              () => {
                const log = [];
                const opts = new Proxy({ smallestUnit: "hour" }, {
                  get(target, key) { log.push(key); return target[key]; },
                });
                d.round(opts);
                return log.join(",");
              });
        check("total no args", "TypeError", () => d.total());
        check("total undefined", "TypeError", () => d.total(undefined));
        check("total empty", "RangeError", () => d.total({}));
        check("total unit string", "1.0854166666666667", () => d.total("day"));
        check("total unit option", "26", () => new D(0, 0, 0, 1, 2).total({ unit: "hour" }));
        check("total plural", "26", () => new D(0, 0, 0, 1, 2).total({ unit: "hours" }));
        check("total auto", "RangeError", () => d.total({ unit: "auto" }));
        check("total bad unit", "RangeError", () => d.total({ unit: "nonsense" }));
        check("total calendar unit without relativeTo", "RangeError", () => d.total({ unit: "month" }));
        check("total calendar duration without relativeTo", "RangeError", () => new D(1).total({ unit: "day" }));
        check("total week without relativeTo", "RangeError", () => d.total({ unit: "week" }));
        check("total year without relativeTo", "RangeError", () => d.total({ unit: "year" }));
        check("total minutes", "1563", () => d.total({ unit: "minute" }));
        check("total seconds", "93780", () => d.total({ unit: "second" }));
        check("total milliseconds", "93780000", () => d.total({ unit: "millisecond" }));
        check("total microseconds", "93780000000", () => d.total({ unit: "microsecond" }));
        check("total nanoseconds", "93780000000000", () => d.total({ unit: "nanosecond" }));
        check("total ignores rounding options", "26", () => new D(0, 0, 0, 1, 2).total({ unit: "hour", roundingMode: "nonsense" }));
        check("total relativeTo type", "TypeError", () => d.total({ unit: "hour", relativeTo: 5 }));
        check("valueOf throws", "TypeError", () => d.valueOf());
        check("round foreign receiver", "TypeError", () => D.prototype.round.call({}, { smallestUnit: "hour" }));
        check("total foreign receiver", "TypeError", () => D.prototype.total.call({}, { unit: "hour" }));
        check("total foreign PlainDate receiver", "TypeError",
              () => D.prototype.total.call(new Temporal.PlainDate(2020, 1, 1), { unit: "hour" }));
        check("round foreign PlainDate receiver", "TypeError",
              () => D.prototype.round.call(new Temporal.PlainDate(2020, 1, 1), { unit: "hour" }));
        "#,
    );
}

#[test]
fn round_with_plain_anchor_uses_calendar_windows() {
    run_checks(
        r#"
        const jan = { relativeTo: "2024-01-01" };
        const r = (duration, options) => duration.round({ ...jan, ...options });
        check("1 month 15 days to months", "P2M", () => r(new D(0, 1, 0, 15), { smallestUnit: "month" }));
        check("floor", "P1M", () => r(new D(0, 1, 0, 15), { smallestUnit: "month", roundingMode: "floor" }));
        check("trunc", "P1M", () => r(new D(0, 1, 0, 15), { smallestUnit: "month", roundingMode: "trunc" }));
        check("ceil", "P2M", () => r(new D(0, 1, 0, 1), { smallestUnit: "month", roundingMode: "ceil" }));
        check("expand", "P2M", () => r(new D(0, 1, 0, 1), { smallestUnit: "month", roundingMode: "expand" }));
        check("halfTrunc below", "P1M", () => r(new D(0, 1, 0, 14), { smallestUnit: "month", roundingMode: "halfTrunc" }));
        check("halfEven above", "P2M", () => r(new D(0, 1, 0, 20), { smallestUnit: "month", roundingMode: "halfEven" }));
        check("negative floor", "-P2M", () => r(new D(0, -1, 0, -1), { smallestUnit: "month", roundingMode: "floor" }));
        check("negative ceil", "-P1M", () => r(new D(0, -1, 0, -1), { smallestUnit: "month", roundingMode: "ceil" }));
        check("negative trunc", "-P1M", () => r(new D(0, -1, 0, -1), { smallestUnit: "month", roundingMode: "trunc" }));
        check("negative expand", "-P2M", () => r(new D(0, -1, 0, -1), { smallestUnit: "month", roundingMode: "expand" }));
        check("days to weeks", "P1W", () => r(new D(0, 0, 0, 10), { smallestUnit: "week" }));
        check("days to weeks ceil", "P2W", () => r(new D(0, 0, 0, 10), { smallestUnit: "week", roundingMode: "ceil" }));
        check("days to months default largest", "P1M", () => r(new D(0, 0, 0, 40), { smallestUnit: "month" }));
        check("days to years", "P1Y", () => r(new D(0, 0, 0, 200), { smallestUnit: "year" }));
        check("days to years floor", "PT0S", () => r(new D(0, 0, 0, 200), { smallestUnit: "year", roundingMode: "floor" }));
        check("months to years", "P1Y", () => r(new D(0, 7), { smallestUnit: "year" }));
        check("months to years half boundary", "PT0S", () => r(new D(0, 6), { smallestUnit: "year" }));
        check("years increment", "P4Y", () => r(new D(3, 6), { smallestUnit: "year", roundingIncrement: 4 }));
        check("months increment", "P6M", () => r(new D(0, 5, 0, 20), { smallestUnit: "month", roundingIncrement: 6 }));
        check("weeks increment", "P4W", () => r(new D(0, 0, 3, 5), { smallestUnit: "week", roundingIncrement: 4 }));
        check("days increment", "P12D", () => r(new D(0, 0, 0, 10), { smallestUnit: "day", roundingIncrement: 4 }));
        check("days increment halfEven", "P8D", () => r(new D(0, 0, 0, 10), { smallestUnit: "day", roundingIncrement: 4, roundingMode: "halfEven" }));
        check("largest week", "P5W5D", () => r(new D(0, 0, 0, 40), { largestUnit: "week" }));
        check("largest month from weeks", "P1M", () => r(new D(0, 0, 4, 3), { largestUnit: "month" }));
        check("largest year from months", "P1Y1M", () => r(new D(0, 13), { largestUnit: "year" }));
        check("largest year from days", "P1Y", () => r(new D(0, 0, 0, 366), { largestUnit: "year" }));
        check("largest day from months", "P60D", () => r(new D(0, 2), { largestUnit: "day" }));
        check("largest day from years", "P366D", () => r(new D(1), { largestUnit: "day" }));
        check("largest week from months", "P8W4D", () => r(new D(0, 2), { largestUnit: "week" }));
        check("largest hour from days", "PT1464H", () => new D(0, 0, 0, 61).round({ largestUnit: "hour" }));
        check("time to days", "P1DT1H", () => r(new D(0, 0, 0, 0, 25), { largestUnit: "day" }));
        check("time to months", "P1M", () => r(new D(0, 0, 0, 0, 744), { largestUnit: "month" }));
        check("time to months with remainder", "P1MT1H", () => r(new D(0, 0, 0, 0, 745), { largestUnit: "month" }));
        check("smallest hour keeps months", "P1MT2H", () => r(new D(0, 1, 0, 0, 2, 30), { smallestUnit: "hour", roundingMode: "floor" }));
        check("smallest hour rounds up", "P1MT3H", () => r(new D(0, 1, 0, 0, 2, 30), { smallestUnit: "hour" }));
        check("smallest minute increments", "P1MT2H30M", () => r(new D(0, 1, 0, 0, 2, 29, 40), { smallestUnit: "minute", roundingIncrement: 30, roundingMode: "ceil" }));
        check("day boundary carry", "P1M1D", () => r(new D(0, 1, 0, 0, 23, 40), { smallestUnit: "hour" }));
        check("negative time carry", "-P1M1D", () => r(new D(0, -1, 0, 0, -23, -40), { smallestUnit: "hour" }));
        check("far future year rounding", "RangeError", () => new D(0, 0, 0, 200000000).round({ largestUnit: "year", relativeTo: "+275000-01-01" }));
        check("far future total", "RangeError", () => new D(0, 0, 0, 200000000).total({ unit: "year", relativeTo: "+275000-01-01" }));
        check("far past", "RangeError", () => new D(0, 0, 0, -200000000).round({ largestUnit: "year", relativeTo: "-271000-01-01" }));
        check("leap year total", "1", () => new D(0, 0, 0, 366).total({ unit: "year", relativeTo: "2024-01-01" }));
        near("non-leap year total", 1 + 1 / 366, () => new D(0, 0, 0, 366).total({ unit: "year", relativeTo: "2023-01-01" }));
        near("month total", 1 + 15 / 29, () => new D(0, 1, 0, 15).total({ unit: "month", relativeTo: "2024-01-01" }));
        near("month total negative", -1.5, () => new D(0, -1, 0, -15).total({ unit: "month", relativeTo: "2024-01-01" }));
        near("week total", 10 / 7, () => new D(0, 0, 0, 10).total({ unit: "week", relativeTo: "2024-01-01" }));
        near("day total from months", 31, () => new D(0, 1).total({ unit: "day", relativeTo: "2024-01-01" }));
        near("hour total from months", 31 * 24, () => new D(0, 1).total({ unit: "hour", relativeTo: "2024-01-01" }));
        near("year total from months", 0.5, () => new D(0, 0, 0, 0, 12 * 366).total({ unit: "year", relativeTo: "2024-01-01" }));
        near("total sub-day unit with date part", 45 * 24, () => new D(0, 1, 0, 14).total({ unit: "hour", relativeTo: "2024-01-01" }));
        "#,
    );
}

#[test]
fn round_and_total_with_named_zone_anchor_respect_daylight_saving() {
    run_checks(
        r#"
        // 2024-03-10 is the 23-hour spring-forward day in New York and
        // 2024-11-03 the 25-hour fall-back day.
        const spring = "2024-03-09T12:00" + NY;
        const springDay = "2024-03-10T12:00" + NY;
        const fall = "2024-11-02T12:00" + NY;
        check("day is 23 hours", "PT23H", () => new D(0, 0, 0, 1).round({ largestUnit: "hour", relativeTo: spring }));
        check("day is 25 hours", "PT25H", () => new D(0, 0, 0, 1).round({ largestUnit: "hour", relativeTo: fall }));
        check("day is 24 hours elsewhere", "PT24H", () => new D(0, 0, 0, 1).round({ largestUnit: "hour", relativeTo: "2024-06-01T12:00" + NY }));
        check("negative day backwards", "-PT23H", () => new D(0, 0, 0, -1).round({ largestUnit: "hour", relativeTo: springDay }));
        near("total hours 23", 23, () => new D(0, 0, 0, 1).total({ unit: "hour", relativeTo: spring }));
        near("total hours 25", 25, () => new D(0, 0, 0, 1).total({ unit: "hour", relativeTo: fall }));
        near("total minutes", 23 * 60, () => new D(0, 0, 0, 1).total({ unit: "minute", relativeTo: spring }));
        near("total 23 hours is one day", 1, () => new D(0, 0, 0, 0, 23).total({ unit: "day", relativeTo: spring }));
        near("total 12 hours of 23", 12 / 23, () => new D(0, 0, 0, 0, 12).total({ unit: "day", relativeTo: spring }));
        near("total 12 hours of 25", 12 / 25, () => new D(0, 0, 0, 0, 12).total({ unit: "day", relativeTo: fall }));
        near("total 12 hours of 24", 0.5, () => new D(0, 0, 0, 0, 12).total({ unit: "day", relativeTo: "2024-06-01T12:00" + NY }));
        near("total negative day", -12 / 23, () => new D(0, 0, 0, 0, -12).total({ unit: "day", relativeTo: springDay }));
        near("total month across spring", 1, () => new D(0, 0, 0, 0, 743).total({ unit: "month", relativeTo: "2024-03-01T00:00" + NY }));
        near("total 1 month 15 days", 1 + 15 / 29, () => new D(0, 1, 0, 15).total({ unit: "month", relativeTo: "2024-01-01T00:00" + NY }));
        near("total 1 month in hours across spring", 743, () => new D(0, 1).total({ unit: "hour", relativeTo: "2024-03-01T00:00" + NY }));
        near("total week", 10 / 7, () => new D(0, 0, 0, 10).total({ unit: "week", relativeTo: "2024-01-01T00:00" + NY }));
        near("total year", 1 + 31 / 365, () => new D(1, 0, 0, 31).total({ unit: "year", relativeTo: "2025-01-01T00:00" + NY }));
        near("total year negative", -(1 + 31 / 365), () => new D(-1, 0, 0, -31).total({ unit: "year", relativeTo: "2027-01-01T00:00" + NY }));
        near("total day from weeks", 14, () => new D(0, 0, 2).total({ unit: "day", relativeTo: "2024-01-01T00:00" + NY }));

        // Day-unit rounding against the real (23-hour) day length.
        const day = (hours, minutes, mode, anchor) => new D(0, 0, 0, 0, hours, minutes)
          .round({ smallestUnit: "day", roundingMode: mode, relativeTo: anchor || spring });
        check("11h of 23 halfExpand", "PT0S", () => day(11, 0, "halfExpand"));
        check("12h of 23 halfExpand", "P1D", () => day(12, 0, "halfExpand"));
        check("11h30 halfExpand", "P1D", () => day(11, 30, "halfExpand"));
        check("11h30 halfTrunc", "PT0S", () => day(11, 30, "halfTrunc"));
        check("11h30 halfEven", "PT0S", () => day(11, 30, "halfEven"));
        check("11h30 halfCeil", "P1D", () => day(11, 30, "halfCeil"));
        check("11h30 halfFloor", "PT0S", () => day(11, 30, "halfFloor"));
        check("1h ceil", "P1D", () => day(1, 0, "ceil"));
        check("1h expand", "P1D", () => day(1, 0, "expand"));
        check("1h floor", "PT0S", () => day(1, 0, "floor"));
        check("1h trunc", "PT0S", () => day(1, 0, "trunc"));
        check("22h halfTrunc", "P1D", () => day(22, 0, "halfTrunc"));
        check("22h halfEven", "P1D", () => day(22, 0, "halfEven"));
        check("22h halfFloor", "P1D", () => day(22, 0, "halfFloor"));
        check("negative 11h30 halfCeil", "PT0S", () => day(-11, -30, "halfCeil", springDay));
        check("negative 11h30 halfFloor", "-P1D", () => day(-11, -30, "halfFloor", springDay));
        check("negative 11h30 halfExpand", "-P1D", () => day(-11, -30, "halfExpand", springDay));
        check("negative 11h30 halfTrunc", "PT0S", () => day(-11, -30, "halfTrunc", springDay));
        check("negative 11h30 halfEven", "PT0S", () => day(-11, -30, "halfEven", springDay));
        check("negative 1h ceil", "PT0S", () => day(-1, 0, "ceil", springDay));
        check("negative 1h floor", "-P1D", () => day(-1, 0, "floor", springDay));
        check("negative 1h expand", "-P1D", () => day(-1, 0, "expand", springDay));
        check("negative 1h trunc", "PT0S", () => day(-1, 0, "trunc", springDay));
        check("days plus hours", "P3D", () => new D(0, 0, 0, 2, 12).round({ smallestUnit: "day", relativeTo: "2024-06-01T00:00" + NY }));
        check("days increment", "P12D", () => new D(0, 0, 0, 10).round({ smallestUnit: "day", roundingIncrement: 4, relativeTo: spring }));
        check("days increment halfEven", "P8D", () => new D(0, 0, 0, 10).round({ smallestUnit: "day", roundingIncrement: 4, roundingMode: "halfEven", relativeTo: spring }));

        // Sub-day rounding through the zone (NudgeToZonedTime).
        const hour = (record, options, anchor) => record.round({ relativeTo: anchor || spring, ...options });
        check("13h to 12h increments", "PT12H", () => hour(new D(0, 0, 0, 0, 13), { smallestUnit: "hour", roundingIncrement: 12 }));
        // A time `largestUnit` (the default here) is `DifferenceInstant`: the
        // zone's day length is never consulted, so 18h rounds to a full 24h even
        // though this particular day is 23h long. Only a `day`-or-larger
        // `largestUnit` (next line) carries the excess into `days`.
        check("18h to 12h increments does not spill into the 23h day", "PT24H", () => hour(new D(0, 0, 0, 0, 18), { smallestUnit: "hour", roundingIncrement: 12 }));
        check("18h spill with day largest", "P1D", () => hour(new D(0, 0, 0, 0, 18), { smallestUnit: "hour", roundingIncrement: 12, largestUnit: "day" }));
        check("negative 18h does not spill", "-PT24H", () => hour(new D(0, 0, 0, 0, -18), { smallestUnit: "hour", roundingIncrement: 12 }, springDay));
        check("negative 18h spill with day largest", "-P1D", () => hour(new D(0, 0, 0, 0, -18), { smallestUnit: "hour", roundingIncrement: 12, largestUnit: "day" }, springDay));
        check("minutes round within a day", "PT5H10M", () => hour(new D(0, 0, 0, 0, 5, 7), { smallestUnit: "minute", roundingIncrement: 10, roundingMode: "ceil" }));
        check("seconds round", "PT1M1S", () => hour(new D(0, 0, 0, 0, 0, 1, 0, 700), { smallestUnit: "second" }));
        check("milliseconds round", "PT1S", () => hour(new D(0, 0, 0, 0, 0, 0, 0, 999, 600), { smallestUnit: "millisecond" }));
        check("date part plus time part", "P1DT6H", () => hour(new D(0, 0, 0, 1, 5, 40), { smallestUnit: "hour" }));
        check("date part and largest hour", "PT30H", () => hour(new D(0, 0, 0, 1, 5, 40), { smallestUnit: "hour", largestUnit: "hour" }, "2024-03-09T00:00" + NY));
        check("months date part with time rounding", "P1MT6H", () => hour(new D(0, 1, 0, 0, 5, 40), { smallestUnit: "hour" }, "2024-01-01T00:00" + NY));
        check("largest year with time rounding", "P1Y1MT6H", () => hour(new D(0, 13, 0, 0, 5, 40), { smallestUnit: "hour", largestUnit: "year" }, "2024-01-01T00:00" + NY));
        check("largest week with time rounding", "P2W1DT1H", () => hour(new D(0, 0, 0, 15, 0, 40), { smallestUnit: "hour", largestUnit: "week" }, "2024-01-01T00:00" + NY));
        check("largest hour with months across spring", "PT743H", () => hour(new D(0, 1), { largestUnit: "hour" }, "2024-03-01T00:00" + NY));
        check("largest hour negative", "-PT743H", () => hour(new D(0, -1), { largestUnit: "hour" }, "2024-04-01T00:00" + NY));

        // Calendar-unit rounding against real epoch brackets.
        const cal = (duration, options, anchor) => duration.round({ relativeTo: anchor || "2024-01-01T00:00" + NY, ...options });
        check("1m15d to months", "P2M", () => cal(new D(0, 1, 0, 15), { smallestUnit: "month" }));
        check("1m14d to months", "P1M", () => cal(new D(0, 1, 0, 14), { smallestUnit: "month" }));
        check("half month halfExpand", "P1M", () => cal(new D(0, 0, 0, 15, 12), { smallestUnit: "month" }));
        check("half month halfEven", "PT0S", () => cal(new D(0, 0, 0, 15, 12), { smallestUnit: "month", roundingMode: "halfEven" }));
        check("half month halfTrunc", "PT0S", () => cal(new D(0, 0, 0, 15, 12), { smallestUnit: "month", roundingMode: "halfTrunc" }));
        check("half month halfCeil", "P1M", () => cal(new D(0, 0, 0, 15, 12), { smallestUnit: "month", roundingMode: "halfCeil" }));
        check("half month halfFloor", "PT0S", () => cal(new D(0, 0, 0, 15, 12), { smallestUnit: "month", roundingMode: "halfFloor" }));
        check("half month negative halfCeil", "PT0S", () => cal(new D(0, 0, 0, -15, -12), { smallestUnit: "month", roundingMode: "halfCeil" }));
        check("half month negative halfFloor", "-P1M", () => cal(new D(0, 0, 0, -15, -12), { smallestUnit: "month", roundingMode: "halfFloor" }));
        check("half month negative halfExpand", "-P1M", () => cal(new D(0, 0, 0, -15, -12), { smallestUnit: "month", roundingMode: "halfExpand" }));
        check("half month negative halfTrunc", "PT0S", () => cal(new D(0, 0, 0, -15, -12), { smallestUnit: "month", roundingMode: "halfTrunc" }));
        check("half month negative halfEven", "PT0S", () => cal(new D(0, 0, 0, -15, -12), { smallestUnit: "month", roundingMode: "halfEven" }));
        check("second half month halfEven", "P2M", () => cal(new D(0, 1, 0, 14, 12), { smallestUnit: "month", roundingMode: "halfEven" }, "2024-01-01T00:00" + NY));
        check("month ceil", "P2M", () => cal(new D(0, 1, 0, 1), { smallestUnit: "month", roundingMode: "ceil" }));
        check("month floor", "P1M", () => cal(new D(0, 1, 0, 28), { smallestUnit: "month", roundingMode: "floor" }));
        check("month trunc", "P1M", () => cal(new D(0, 1, 0, 28), { smallestUnit: "month", roundingMode: "trunc" }));
        check("month expand", "P2M", () => cal(new D(0, 1, 0, 1), { smallestUnit: "month", roundingMode: "expand" }));
        check("negative month floor", "-P2M", () => cal(new D(0, -1, 0, -1), { smallestUnit: "month", roundingMode: "floor" }, "2024-03-01T00:00" + NY));
        check("negative month ceil", "-P1M", () => cal(new D(0, -1, 0, -1), { smallestUnit: "month", roundingMode: "ceil" }, "2024-03-01T00:00" + NY));
        check("months increment", "P6M", () => cal(new D(0, 5, 0, 20), { smallestUnit: "month", roundingIncrement: 6 }));
        check("years", "PT0S", () => cal(new D(0, 6), { smallestUnit: "year" }));
        check("years up", "P1Y", () => cal(new D(0, 7), { smallestUnit: "year" }));
        check("years ceil", "P1Y", () => cal(new D(0, 0, 0, 1), { smallestUnit: "year", roundingMode: "ceil" }));
        check("years increment", "P4Y", () => cal(new D(3, 6), { smallestUnit: "year", roundingIncrement: 4 }));
        check("weeks", "P1W", () => cal(new D(0, 0, 0, 10), { smallestUnit: "week" }));
        check("weeks ceil", "P2W", () => cal(new D(0, 0, 0, 10), { smallestUnit: "week", roundingMode: "ceil" }));
        check("weeks increment", "P4W", () => cal(new D(0, 0, 3, 5), { smallestUnit: "week", roundingIncrement: 4 }));
        check("largest year from months", "P1Y1M", () => cal(new D(0, 13), { largestUnit: "year" }));
        check("largest month from days", "P1M9D", () => cal(new D(0, 0, 0, 40), { largestUnit: "month" }));
        check("largest week from days", "P5W5D", () => cal(new D(0, 0, 0, 40), { largestUnit: "week" }));
        check("largest day from months", "P60D", () => cal(new D(0, 2), { largestUnit: "day" }));
        check("smallest month largest year", "P1Y2M", () => cal(new D(0, 14, 0, 3), { smallestUnit: "month", largestUnit: "year" }));
        check("target out of range", "RangeError", () => new D(0, 0, 0, 200000000).round({ largestUnit: "year", relativeTo: "+275000-01-01T00:00" + NY }));
        check("target time out of range", "RangeError", () => new D(0, 0, 0, 1).round({ largestUnit: "hour", relativeTo: "+275760-09-13T00:00Z[UTC]" }));
        check("time target out of range", "RangeError", () => new D(0, 0, 0, 0, 24).round({ smallestUnit: "hour", relativeTo: "+275760-09-13T00:00Z[UTC]" }));
        check("total target out of range", "RangeError", () => new D(0, 0, 0, 1).total({ unit: "hour", relativeTo: "+275760-09-13T00:00Z[UTC]" }));
        check("total month out of range", "RangeError", () => new D(0, 1).total({ unit: "month", relativeTo: "+275760-09-13T00:00Z[UTC]" }));
        check("blank total zoned", "0", () => new D().total({ unit: "day", relativeTo: spring }));
        check("blank round zoned", "PT0S", () => new D().round({ largestUnit: "year", relativeTo: spring }));
        "#,
    );
}

#[test]
fn round_and_total_with_fixed_offset_anchors() {
    run_checks(
        r#"
        for (const anchor of ["2024-01-31T00:00[UTC]", "2024-01-31T00:00+05:30[+05:30]",
                              { year: 2024, month: 1, day: 31, timeZone: "UTC" },
                              { year: 2024, month: 1, day: 31, hour: 3, timeZone: "-08:00", offset: "-08:00" }]) {
          const label = typeof anchor === "string" ? anchor : JSON.stringify(anchor);
          check("month end constrain " + label, "P1M1D", () => new D(0, 1, 0, 1).round({ largestUnit: "month", relativeTo: anchor }));
          check("months to days " + label, "P29D", () => new D(0, 1).round({ largestUnit: "day", relativeTo: anchor }));
          check("day to hours " + label, "PT24H", () => new D(0, 0, 0, 1).round({ largestUnit: "hour", relativeTo: anchor }));
          check("years round " + label, "P1Y", () => new D(0, 7).round({ smallestUnit: "year", relativeTo: anchor }));
          check("weeks round " + label, "P1W", () => new D(0, 0, 0, 10).round({ smallestUnit: "week", relativeTo: anchor }));
          check("time rounding " + label, "P1DT2H", () => new D(0, 0, 0, 1, 1, 40).round({ smallestUnit: "hour", relativeTo: anchor }));
          check("time spill " + label, "P2D", () => new D(0, 0, 0, 1, 12).round({ smallestUnit: "day", relativeTo: anchor }));
          near("total month " + label, 1 + 1 / 31, () => new D(0, 1, 0, 1).total({ unit: "month", relativeTo: anchor }));
          near("total hours " + label, 24, () => new D(0, 0, 0, 1).total({ unit: "hour", relativeTo: anchor }));
          check("compare " + label, "0", () => D.compare(new D(0, 0, 0, 1), new D(0, 0, 0, 0, 24), { relativeTo: anchor }));
        }
        "#,
    );
}

#[test]
fn compare_uses_relative_to_for_calendar_units() {
    run_checks(
        r#"
        const cmp = (a, b, relativeTo) => D.compare(a, b, { relativeTo });
        check("no relativeTo hours", "0", () => D.compare(new D(0, 0, 0, 1), new D(0, 0, 0, 0, 24)));
        check("no relativeTo months", "RangeError", () => D.compare(new D(0, 1), new D(0, 0, 0, 30)));
        check("no relativeTo years on one side", "RangeError", () => D.compare(new D(1), new D(0, 0, 0, 30)));
        check("no relativeTo weeks", "RangeError", () => D.compare(new D(0, 0, 1), new D(0, 0, 0, 7)));
        check("equal calendar-free never needs relativeTo", "0", () => D.compare(new D(0, 0, 0, 3), new D(0, 0, 0, 3)));
        check("identical calendar durations short-circuit", "0", () => D.compare(new D(0, 1), new D(0, 1)));
        check("31 vs 30 days", "1", () => cmp(new D(0, 1), new D(0, 0, 0, 30), "2024-01-01"));
        check("30 vs 30 days", "0", () => cmp(new D(0, 1), new D(0, 0, 0, 30), "2024-04-01"));
        check("29 vs 30 days", "-1", () => cmp(new D(0, 1), new D(0, 0, 0, 30), "2024-02-01"));
        check("year leap", "1", () => cmp(new D(1), new D(0, 0, 0, 365), "2024-01-01"));
        check("year non-leap", "0", () => cmp(new D(1), new D(0, 0, 0, 365), "2023-01-01"));
        check("weeks", "0", () => cmp(new D(0, 0, 1), new D(0, 0, 0, 7), "2024-01-01"));
        check("time only", "1", () => cmp(new D(0, 0, 0, 0, 25), new D(0, 0, 0, 1), "2024-01-01"));
        check("PlainDate object", "1", () => cmp(new D(0, 1), new D(0, 0, 0, 30), new Temporal.PlainDate(2024, 1, 1)));
        check("property bag", "1", () => cmp(new D(0, 1), new D(0, 0, 0, 30), { year: 2024, month: 1, day: 1 }));
        check("zoned string", "1", () => cmp(new D(0, 1), new D(0, 0, 0, 30), "2024-01-01T00:00[UTC]"));
        check("bad relativeTo", "RangeError", () => cmp(new D(0, 1), new D(0, 0, 0, 30), "nonsense"));
        check("relativeTo type", "TypeError", () => cmp(new D(0, 1), new D(0, 0, 0, 30), 5));
        check("relativeTo still validated when unused", "RangeError", () => cmp(new D(0, 0, 0, 1), new D(0, 0, 0, 1), "nonsense"));
        check("options primitive", "TypeError", () => D.compare(new D(1), new D(1), 5));
        check("options null", "TypeError", () => D.compare(new D(1), new D(1), null));
        check("from strings", "1", () => cmp("P1M", "P30D", "2024-01-01"));
        check("from property bags", "-1", () => cmp({ months: 1 }, { days: 31, hours: 1 }, "2024-01-01"));
        check("bad first argument", "RangeError", () => D.compare("nonsense", new D(1)));
        check("no arguments", "TypeError", () => D.compare());
        check("one argument", "TypeError", () => D.compare(new D(1)));
        check("empty bag", "TypeError", () => D.compare({}, new D(1)));
        // A day is 23 hours long on the spring-forward day.
        const spring = "2024-03-09T12:00" + NY;
        check("day vs 24 hours across spring", "-1", () => cmp(new D(0, 0, 0, 1), new D(0, 0, 0, 0, 24), spring));
        check("day vs 23 hours across spring", "0", () => cmp(new D(0, 0, 0, 1), new D(0, 0, 0, 0, 23), spring));
        check("day vs 24 hours across fall", "1", () => cmp(new D(0, 0, 0, 1), new D(0, 0, 0, 0, 24), "2024-11-02T12:00" + NY));
        check("day vs 24 hours outside dst", "0", () => cmp(new D(0, 0, 0, 1), new D(0, 0, 0, 0, 24), "2024-06-01T12:00" + NY));
        // Calendar days are wall-clock days, so both land on 2024-04-01T00:00.
        check("month vs calendar days across spring", "0", () => cmp(new D(0, 1), new D(0, 0, 0, 31, 0), "2024-03-01T00:00" + NY));
        check("month in hours across spring", "0", () => cmp(new D(0, 1), new D(0, 0, 0, 0, 743), "2024-03-01T00:00" + NY));
        check("out of range", "RangeError", () => cmp(new D(0, 0, 0, 200000000), new D(1), "+275000-01-01T00:00" + NY));
        check("out of range plain", "RangeError", () => cmp(new D(0, 0, 0, 200000000), new D(1), "+275000-01-01"));
        check("near limit is fine", "-1", () => cmp(new D(0, 0, 0, 1), new D(0, 0, 0, 2), "+275760-09-10"));
        "#,
    );
}

#[test]
fn add_and_subtract_reject_calendar_units_without_an_anchor() {
    run_checks(
        r#"
        check("hours", "PT3H", () => new D(0, 0, 0, 0, 1).add(new D(0, 0, 0, 0, 2)));
        check("days and hours", "P2DT1H", () => new D(0, 0, 0, 1, 12).add(new D(0, 0, 0, 0, 13)));
        check("subtract", "PT1H", () => new D(0, 0, 0, 0, 3).subtract(new D(0, 0, 0, 0, 2)));
        check("negative result", "-PT1H", () => new D(0, 0, 0, 0, 2).subtract(new D(0, 0, 0, 0, 3)));
        check("years without anchor", "RangeError", () => new D(1).add(new D(0, 0, 0, 1)));
        check("months on other side", "RangeError", () => new D(0, 0, 0, 1).add(new D(0, 1)));
        check("weeks", "RangeError", () => new D(0, 0, 1).subtract(new D(0, 0, 0, 1)));
        check("from string", "PT2H", () => new D(0, 0, 0, 0, 1).add("PT1H"));
        check("from bag", "PT2H", () => new D(0, 0, 0, 0, 1).add({ hours: 1 }));
        check("bad other", "RangeError", () => new D(0, 0, 0, 0, 1).add("nonsense"));
        check("no other", "TypeError", () => new D().add());
        check("number other", "TypeError", () => new D().add(5));
        check("empty bag", "TypeError", () => new D().add({}));
        check("overflow", "RangeError", () => new D(0, 0, 0, 0, 0, 0, Number.MAX_SAFE_INTEGER).add(new D(0, 0, 0, 0, 0, 0, Number.MAX_SAFE_INTEGER)));
        check("mixed signs to zero", "PT0S", () => new D(0, 0, 0, 0, 1).add(new D(0, 0, 0, 0, -1)));
        check("largest unit follows the larger", "PT25H", () => new D(0, 0, 0, 0, 25).add(new D()));
        check("keeps days", "P1DT1H", () => new D(0, 0, 0, 1).add(new D(0, 0, 0, 0, 1)));
        check("sub-second precision", "PT0.000003S", () => new D(0, 0, 0, 0, 0, 0, 0, 0, 1, 1000).add(new D(0, 0, 0, 0, 0, 0, 0, 0, 0, 1000)));
        check("with empty", "TypeError", () => new D(1, 2, 3, 4, 5, 6, 7).with({}));
        "#,
    );
}
