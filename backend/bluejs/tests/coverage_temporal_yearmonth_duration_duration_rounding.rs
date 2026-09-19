// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Extra coverage for `Temporal.Duration.prototype.round`/`total`/`compare`
//! rounding arithmetic (`vm/temporal/duration_relative.rs`,
//! `duration_operations.rs`): every rounding mode at, above and below the
//! exact midpoint of a calendar unit, against plain, fixed-offset and named
//! time-zone anchors, plus limit and error paths. Every
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
fn every_rounding_mode_at_the_midpoint_of_each_calendar_unit() {
    run_checks(
        r#"
        // [mode, positive midpoint rounds away?, negative midpoint rounds away?]
        const modes = [
          ["ceil", 1, 0], ["floor", 0, 1], ["expand", 1, 1], ["trunc", 0, 0],
          ["halfCeil", 1, 0], ["halfFloor", 0, 1], ["halfExpand", 1, 1], ["halfTrunc", 0, 0],
          ["halfEven", 0, 0],
        ];
        const isHalf = (mode) => mode.startsWith("half");
        const letter = { month: "M", week: "W", day: "D", year: "Y" };
        // [unit, half, above, below] as [days, hours] pairs for the positive
        // direction; the negative direction negates every field.
        const shapes = {
          month: { half: [15, 12], above: [15, 13], below: [15, 11] },
          week: { half: [3, 12], above: [3, 13], below: [3, 11] },
          day: { half: [0, 12], above: [0, 13], below: [0, 11] },
        };
        const anchors = ["2024-01-01", "2024-01-01T00:00[UTC]", "2024-01-01T00:00" + NY,
                         "2024-01-01T00:00+05:30[+05:30]"];
        for (const anchor of anchors) {
          for (const unit of Object.keys(shapes)) {
            for (const [mode, posAway, negAway] of modes) {
              for (const [kind, shape] of Object.entries(shapes[unit])) {
                for (const sign of [1, -1]) {
                  const duration = new D(0, 0, 0, sign * shape[0], sign * shape[1]);
                  const away = kind === "half" ? (sign > 0 ? posAway : negAway)
                    : kind === "above" ? (isHalf(mode) ? 1 : sign > 0 ? (mode === "ceil" || mode === "expand" ? 1 : 0)
                                                                        : (mode === "floor" || mode === "expand" ? 1 : 0))
                    : (isHalf(mode) ? 0 : sign > 0 ? (mode === "ceil" || mode === "expand" ? 1 : 0)
                                                    : (mode === "floor" || mode === "expand" ? 1 : 0));
                  const expected = away ? (sign < 0 ? "-" : "") + "P1" + letter[unit] : "PT0S";
                  check([anchor, unit, mode, kind, sign].join(" "), expected,
                        () => duration.round({ smallestUnit: unit, roundingMode: mode, relativeTo: anchor }));
                }
              }
            }
          }
        }
        // Whole multiples never move, whatever the mode.
        for (const [mode] of modes) {
          for (const anchor of anchors) {
            check(anchor + " exact month " + mode, "P1M",
                  () => new D(0, 1).round({ smallestUnit: "month", roundingMode: mode, relativeTo: anchor }));
            check(anchor + " exact negative month " + mode, "-P1M",
                  () => new D(0, -1).round({ smallestUnit: "month", roundingMode: mode, relativeTo: anchor }));
            check(anchor + " exact year " + mode, "P2Y",
                  () => new D(2).round({ smallestUnit: "year", roundingMode: mode, relativeTo: anchor }));
            check(anchor + " exact week " + mode, "P2W",
                  () => new D(0, 0, 2).round({ smallestUnit: "week", roundingMode: mode, relativeTo: anchor }));
            check(anchor + " exact day " + mode, "P3D",
                  () => new D(0, 0, 0, 3).round({ smallestUnit: "day", roundingMode: mode, relativeTo: anchor }));
          }
        }
        "#,
    );
}

#[test]
fn year_midpoints_and_half_even_parity_with_calendar_anchors() {
    run_checks(
        r#"
        // 2024 has 366 days, so 183 days is exactly half a year; the year
        // before has 365 days, so 182 days 12 hours is exactly half.
        const plain = ["2024-01-01", "2024-01-01T00:00[UTC]"];
        for (const anchor of plain) {
          const y = (duration, mode) => duration.round({ smallestUnit: "year", roundingMode: mode, relativeTo: anchor });
          check("half year halfExpand", "P1Y", () => y(new D(0, 0, 0, 183), "halfExpand"));
          check("half year halfEven", "PT0S", () => y(new D(0, 0, 0, 183), "halfEven"));
          check("half year halfCeil", "P1Y", () => y(new D(0, 0, 0, 183), "halfCeil"));
          check("half year halfFloor", "PT0S", () => y(new D(0, 0, 0, 183), "halfFloor"));
          check("half year halfTrunc", "PT0S", () => y(new D(0, 0, 0, 183), "halfTrunc"));
          check("half year ceil", "P1Y", () => y(new D(0, 0, 0, 183), "ceil"));
          check("negative half year halfExpand", "-P1Y", () => y(new D(0, 0, 0, -182, -12), "halfExpand"));
          check("negative half year halfCeil", "PT0S", () => y(new D(0, 0, 0, -182, -12), "halfCeil"));
          check("negative half year halfFloor", "-P1Y", () => y(new D(0, 0, 0, -182, -12), "halfFloor"));
          check("negative half year halfEven", "PT0S", () => y(new D(0, 0, 0, -182, -12), "halfEven"));
          check("negative half year halfTrunc", "PT0S", () => y(new D(0, 0, 0, -182, -12), "halfTrunc"));
          // Odd lower multiple: halfEven rounds a midpoint up to the even one.
          const odd = (duration, mode) => duration.round({ smallestUnit: "month", roundingMode: mode, relativeTo: anchor });
          check("odd month midpoint halfEven up", "P2M", () => odd(new D(0, 1, 0, 14, 12), "halfEven"));
          check("odd month midpoint halfExpand", "P2M", () => odd(new D(0, 1, 0, 14, 12), "halfExpand"));
          check("odd month midpoint halfTrunc", "P1M", () => odd(new D(0, 1, 0, 14, 12), "halfTrunc"));
          check("odd negative month midpoint halfEven", "-P2M", () => odd(new D(0, -1, 0, -15), "halfEven"));
          check("even month midpoint halfEven down", "P2M", () => odd(new D(0, 2, 0, 14, 12), "halfEven"));
          check("increment with midpoint", "P4M", () => new D(0, 0, 0, 15, 12).round({ smallestUnit: "month", roundingIncrement: 4, roundingMode: "ceil", relativeTo: anchor }));
        }
        "#,
    );
}

#[test]
fn round_and_total_time_units_against_calendar_anchors() {
    run_checks(
        r#"
        const jan = "2024-01-01";
        const one = (fields, options, anchor) => new D(...fields).round({ relativeTo: anchor || jan, ...options });
        // Calendar-carrying durations rounded at every sub-day unit.
        check("smallest millisecond", "P1MT0.501S", () => one([0, 1, 0, 0, 0, 0, 0, 500, 600, 700], { smallestUnit: "millisecond" }));
        check("smallest microsecond", "P1MT0.500601S", () => one([0, 1, 0, 0, 0, 0, 0, 500, 600, 700], { smallestUnit: "microsecond" }));
        check("smallest nanosecond", "P1MT0.5006007S", () => one([0, 1, 0, 0, 0, 0, 0, 500, 600, 700], { smallestUnit: "nanosecond" }));
        check("smallest second", "P1MT31S", () => one([0, 1, 0, 0, 0, 0, 30, 500], { smallestUnit: "second" }));
        check("smallest minute", "P1MT31M", () => one([0, 1, 0, 0, 0, 30, 30], { smallestUnit: "minute" }));
        check("smallest hour", "P1MT13H", () => one([0, 1, 0, 0, 12, 30], { smallestUnit: "hour" }));
        check("smallest hour carries day", "P1M1D", () => one([0, 1, 0, 0, 23, 40], { smallestUnit: "hour" }));
        check("time only calendar largest", "PT5H", () => one([0, 0, 0, 0, 5], { largestUnit: "month" }));
        check("time only negative calendar largest", "-PT5H", () => one([0, 0, 0, 0, -5], { largestUnit: "month" }));
        check("time only days largest", "P1DT5H", () => one([0, 0, 0, 0, 29], { largestUnit: "year" }));
        check("blank calendar largest", "PT0S", () => one([0, 0, 0, 0, 0], { largestUnit: "year" }));
        check("date only sub-day smallest", "P1M", () => one([0, 1], { largestUnit: "month", smallestUnit: "hour" }));
        check("negative sub-day smallest", "-P1MT2H", () => one([0, -1, 0, 0, -1, -40], { smallestUnit: "hour" }));
        check("week smallest largest month", "P1M1W", () => one([0, 0, 0, 40], { smallestUnit: "week", largestUnit: "month" }));
        check("week smallest largest year", "P1Y1W", () => one([0, 0, 0, 372], { smallestUnit: "week", largestUnit: "year" }));
        check("week smallest with months", "P1M2W", () => one([0, 1, 0, 12], { smallestUnit: "week" }));
        check("week smallest keeps weeks", "P1M1W", () => one([0, 1, 1, 2], { smallestUnit: "week" }));
        check("day smallest largest month", "P1M9D", () => one([0, 0, 0, 40], { smallestUnit: "day", largestUnit: "month" }));
        check("day smallest with hours largest year", "P1Y1D", () => one([0, 0, 0, 366, 13], { smallestUnit: "day", largestUnit: "year" }));
        check("year smallest largest year with days", "P2Y", () => one([1, 7], { smallestUnit: "year", largestUnit: "year" }));

        near("total hours of a time-only duration", 5 / 744, () => new D(0, 0, 0, 0, 5).total({ unit: "month", relativeTo: jan }));
        near("total negative time-only", -5 / 744, () => new D(0, 0, 0, 0, -5).total({ unit: "month", relativeTo: jan }));
        near("total day with time", 1.25, () => new D(0, 0, 0, 0, 30).total({ unit: "day", relativeTo: jan }));
        near("total week with time", 1.5, () => new D(0, 0, 0, 10, 12).total({ unit: "week", relativeTo: jan }));
        near("total negative week", -1.5, () => new D(0, 0, 0, -10, -12).total({ unit: "week", relativeTo: jan }));
        near("total year", 1 + 1 / 365, () => new D(0, 0, 0, 367).total({ unit: "year", relativeTo: jan }));
        near("total month exact", 2, () => new D(0, 2).total({ unit: "month", relativeTo: jan }));
        near("total minute", 60 * 24 * 31, () => new D(0, 1).total({ unit: "minute", relativeTo: jan }));
        near("total nanosecond", 86400e9, () => new D(0, 0, 0, 1).total({ unit: "nanosecond", relativeTo: jan }));
        check("total month near limit upper out of range", "RangeError", () => new D(0, 0, 0, 1).total({ unit: "month", relativeTo: "+275760-08-20" }));
        check("total year near limit", "RangeError", () => new D(0, 0, 0, 1).total({ unit: "year", relativeTo: "+275760-01-01" }));
        check("total intermediate near limit", "RangeError", () => new D(0, 0, 0, 30).total({ unit: "month", relativeTo: "+275760-09-01" }));
        check("round near limit day", "RangeError", () => new D(0, 0, 0, 2).round({ smallestUnit: "day", largestUnit: "month", relativeTo: "+275760-09-12" }));
        check("round near limit time fold", "RangeError", () => new D(0, 0, 0, 0, 49).round({ largestUnit: "month", relativeTo: "+275760-09-12" }));
        check("round huge years plain", "RangeError", () => new D(1000000).round({ largestUnit: "year", relativeTo: jan }));
        check("round huge months plain", "RangeError", () => new D(0, 4000000).round({ smallestUnit: "month", relativeTo: jan }));
        check("total huge years plain", "RangeError", () => new D(1000000).total({ unit: "day", relativeTo: jan }));
        check("total huge weeks plain", "RangeError", () => new D(0, 0, 100000000).total({ unit: "day", relativeTo: jan }));
        check("compare huge", "RangeError", () => D.compare(new D(1000000), new D(1), { relativeTo: jan }));
        check("compare huge second", "RangeError", () => D.compare(new D(1), new D(1000000), { relativeTo: jan }));
        "#,
    );
}

#[test]
fn zoned_rounding_windows_shift_when_the_time_part_overflows_the_date_part() {
    run_checks(
        r#"
        const jan = "2024-01-01T00:00" + NY;
        const z = (fields, options, anchor) => new D(...fields).round({ relativeTo: anchor || jan, ...options });
        // The date part alone brackets the wrong window; the time part carries
        // the target past it, so the window is re-derived one step further.
        check("35 days in hours to months", "P1M", () => z([0, 0, 0, 0, 24 * 35], { smallestUnit: "month" }));
        check("35 days in hours to months ceil", "P2M", () => z([0, 0, 0, 0, 24 * 35], { smallestUnit: "month", roundingMode: "ceil" }));
        check("negative 35 days in hours", "-P1M", () => z([0, 0, 0, 0, -24 * 35], { smallestUnit: "month" }, "2024-03-01T00:00" + NY));
        check("hours to days", "P1D", () => z([0, 0, 0, 0, 30], { smallestUnit: "day" }));
        check("hours to days ceil", "P2D", () => z([0, 0, 0, 0, 30], { smallestUnit: "day", roundingMode: "ceil" }));
        check("negative hours to days", "-P1D", () => z([0, 0, 0, 0, -30], { smallestUnit: "day" }, "2024-01-05T00:00" + NY));
        check("hours to weeks", "P1W", () => z([0, 0, 0, 0, 24 * 9], { smallestUnit: "week" }));
        check("hours to years", "P1Y", () => z([0, 0, 0, 0, 24 * 400], { smallestUnit: "year" }));
        check("hours to years floor", "P1Y", () => z([0, 0, 0, 0, 24 * 400], { smallestUnit: "year", roundingMode: "floor" }));
        check("hours to years largest", "P1Y", () => z([0, 0, 0, 0, 24 * 400], { largestUnit: "year", smallestUnit: "year" }));
        check("exact month", "P1M", () => z([0, 1], { smallestUnit: "month" }));
        check("exact negative month", "-P1M", () => z([0, -1], { smallestUnit: "month" }, "2024-03-01T00:00" + NY));
        check("exact year", "P2Y", () => z([2], { smallestUnit: "year" }));
        check("exact week", "P2W", () => z([0, 0, 2], { smallestUnit: "week" }));
        check("exact day", "P3D", () => z([0, 0, 0, 3], { smallestUnit: "day" }));
        check("exact week in days", "P1W", () => z([0, 0, 0, 7], { smallestUnit: "week" }));
        check("exact year in days", "P1Y", () => z([0, 0, 0, 366], { smallestUnit: "year" }));
        check("exact multi-unit", "P1Y2M25D", () => z([1, 2, 3, 4], { smallestUnit: "day" }));
        for (const mode of ["ceil", "floor", "expand", "trunc", "halfCeil", "halfFloor", "halfExpand", "halfTrunc", "halfEven"]) {
          check("exact month " + mode, "P1M", () => z([0, 1], { smallestUnit: "month", roundingMode: mode }));
          check("exact year " + mode, "P1Y", () => z([1], { smallestUnit: "year", roundingMode: mode }));
          check("exact negative week " + mode, "-P1W", () => z([0, 0, -1], { smallestUnit: "week", roundingMode: mode }, "2024-02-01T00:00" + NY));
        }
        check("total exact", "1", () => new D(0, 1).total({ unit: "month", relativeTo: jan }));
        check("total negative exact", "-2", () => new D(0, -2).total({ unit: "month", relativeTo: "2024-03-01T00:00" + NY }));
        check("total hours to days", "1.25", () => new D(0, 0, 0, 0, 30).total({ unit: "day", relativeTo: jan }));
        check("total hours to months", "1", () => new D(0, 0, 0, 0, 24 * 31).total({ unit: "month", relativeTo: jan }));
        check("total hours to weeks", "1.2857142857142858", () => new D(0, 0, 0, 0, 24 * 9).total({ unit: "week", relativeTo: jan }));
        // Range errors from the zone-aware paths.
        const limit = "+275760-09-13T00:00Z[UTC]";
        check("zoned target out of range", "RangeError", () => new D(0, 0, 0, 1).round({ smallestUnit: "day", relativeTo: "+275760-09-12T12:00" + NY }));
        check("zoned target out of range hours", "RangeError", () => new D(0, 0, 0, 0, 48).round({ smallestUnit: "month", relativeTo: "+275760-09-12T00:00" + NY }));
        check("zoned month out of range", "RangeError", () => new D(0, 0, 0, 1).round({ smallestUnit: "month", relativeTo: "+275760-08-20T00:00" + NY }));
        check("zoned year out of range", "RangeError", () => new D(0, 0, 0, 1).round({ smallestUnit: "year", relativeTo: "+275760-01-20T00:00" + NY }));
        check("zoned total out of range", "RangeError", () => new D(0, 0, 0, 1).total({ unit: "month", relativeTo: "+275760-08-20T00:00" + NY }));
        check("zoned total year out of range", "RangeError", () => new D(0, 0, 0, 1).total({ unit: "year", relativeTo: "+275760-01-20T00:00" + NY }));
        check("zoned total target out of range", "RangeError", () => new D(0, 0, 0, 100).total({ unit: "day", relativeTo: "+275760-09-12T00:00" + NY }));
        check("zoned nudge out of range", "RangeError", () => new D(0, 0, 0, 0, 10).round({ smallestUnit: "hour", relativeTo: "+275760-09-12T20:00" + NY }));
        check("zoned nudge next day out of range", "RangeError", () => new D(0, 0, 0, 0, 1).round({ smallestUnit: "hour", relativeTo: "+275760-09-13T00:00Z[UTC]" }));
        check("zoned nudge negative lower limit", "RangeError", () => new D(0, 0, 0, 0, -1).round({ smallestUnit: "hour", relativeTo: "-271821-04-20T00:00" + NY }));
        check("compare second out of range", "RangeError", () => D.compare(new D(0, 0, 0, 1), new D(0, 0, 0, 200000000), { relativeTo: "+275000-01-01T00:00" + NY }));
        check("compare first out of range", "RangeError", () => D.compare(new D(0, 0, 0, 200000000), new D(0, 0, 0, 1), { relativeTo: "+275000-01-01T00:00" + NY }));
        check("compare equal blank", "0", () => D.compare(new D(), new D(), { relativeTo: jan }));
        check("compare zoned less", "-1", () => D.compare(new D(0, 0, 0, 1), new D(0, 0, 0, 2), { relativeTo: jan }));
        check("compare zoned greater", "1", () => D.compare(new D(0, 1), new D(0, 0, 0, 1), { relativeTo: jan }));
        check("string with wrong offset", "RangeError", () => new D(0, 0, 0, 1).round({ largestUnit: "hour", relativeTo: "2024-01-01T00:00-01:00[America/New_York]" }));
        check("string with matching offset", "PT24H", () => new D(0, 0, 0, 1).round({ largestUnit: "hour", relativeTo: "2024-01-01T00:00-05:00[America/New_York]" }));
        check("bag with time zone out of range", "RangeError", () => new D(0, 0, 0, 1).round({ largestUnit: "hour", relativeTo: { year: 275760, month: 9, day: 13, hour: 23, timeZone: "UTC" } }));
        check("bag with calendar era but no era year", "RangeError", () => new D(0, 0, 0, 1).round({ largestUnit: "hour", relativeTo: { era: "ce", year: 2020, month: 1, day: 1, calendar: "gregory" } }) );
        "#,
    );
}

#[test]
fn relative_to_string_limits_and_huge_durations() {
    run_checks(
        r#"
        // Mirrors Test262's `Duration/prototype/round/relativeto-string-limits.js`.
        const instance = new D(0, 0, 0, 0, 0, 5);
        const blank = new D();
        const valid = ["-271821-04-20T00:00Z[UTC]", "+275760-09-13", "+275760-09-13T23:00"];
        for (const relativeTo of valid) {
          check("valid " + relativeTo, "PT5M", () => instance.round({ smallestUnit: "minutes", relativeTo }));
          check("valid blank " + relativeTo, "PT0S", () => blank.round({ smallestUnit: "minutes", relativeTo }));
        }
        const failAfterEarlyReturn = [
          "+275760-09-13T00:00Z[UTC]", "+275760-09-13T01:00+01:00[+01:00]", "+275760-09-13T23:59+23:59[+23:59]",
          "-271821-04-19", "-271821-04-19T01:00",
        ];
        for (const relativeTo of failAfterEarlyReturn) {
          check("non-blank rejects " + relativeTo, "RangeError", () => instance.round({ smallestUnit: "minutes", relativeTo }));
        }
        for (const relativeTo of ["-271821-04-19", "-271821-04-19T01:00"]) {
          check("blank tolerates " + relativeTo, "PT0S", () => blank.round({ smallestUnit: "minutes", relativeTo }));
        }
        const invalid = [
          "-271821-04-19T23:59:59.999999999Z[UTC]", "-271821-04-19T23:00-00:59[-00:59]",
          "-271821-04-19T00:00:00-23:59[-23:59]", "+275760-09-13T00:00:00.000000001Z[UTC]",
          "+275760-09-13T01:00+00:59[+00:59]", "+275760-09-14T00:00+23:59[+23:59]",
          "-271821-04-18", "-271821-04-18T23:00", "+275760-09-14", "+275760-09-14T01:00",
        ];
        for (const relativeTo of invalid) {
          check("invalid " + relativeTo, "RangeError", () => instance.round({ smallestUnit: "minutes", relativeTo }));
          check("invalid blank " + relativeTo, "RangeError", () => blank.round({ smallestUnit: "minutes", relativeTo }));
        }

        // Durations far beyond the representable range fail whichever kind
        // of anchor they are applied to.
        const huge = [new D(4294967295), new D(0, 0, 4294967295),
                      new D(0, 0, 0, 100000000000), new D(0, 0, 0, 0, 2500000000000)];
        const anchors = ["2024-01-01", "2024-01-01T00:00[UTC]", "2024-01-01T00:00" + NY];
        for (const anchor of anchors) {
          for (const [index, duration] of huge.entries()) {
            const label = anchor + " #" + index;
            check("round month " + label, "RangeError", () => duration.round({ smallestUnit: "month", relativeTo: anchor }));
            check("round year " + label, "RangeError", () => duration.round({ largestUnit: "year", relativeTo: anchor }));
            check("round week " + label, "RangeError", () => duration.round({ smallestUnit: "week", relativeTo: anchor }));
            check("total month " + label, "RangeError", () => duration.total({ unit: "month", relativeTo: anchor }));
            if (anchor !== anchors[0] || index < 2) {
              check("compare " + label, "RangeError", () => D.compare(duration, new D(0, 0, 0, 1), { relativeTo: anchor }));
              check("compare reversed " + label, "RangeError", () => D.compare(new D(0, 0, 0, 1), duration, { relativeTo: anchor }));
            }
            if (anchor !== anchors[0] || index < 2) {
              check("round day " + label, "RangeError", () => duration.round({ smallestUnit: "day", relativeTo: anchor }));
              check("total day " + label, "RangeError", () => duration.total({ unit: "day", relativeTo: anchor }));
              check("total hour " + label, "RangeError", () => duration.total({ unit: "hour", relativeTo: anchor }));
            }
          }
        }
        "#,
    );
}
