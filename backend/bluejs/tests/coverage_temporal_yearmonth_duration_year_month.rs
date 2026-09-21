// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Extra coverage for `Temporal.PlainYearMonth` (`vm/temporal/year_month.rs`):
//! property-bag and string resolution, `with`, `add`/`subtract`,
//! `until`/`since` option validation and rounding-mode reflection,
//! `equals`/`compare`, `toString`/`toPlainDate`, brand checks and the
//! constructor. Every expectation is what the ECMAScript Temporal
//! specification requires; each case runs inside one JavaScript program that
//! collects mismatches so a failure lists every deviating case at once.

use blueice_bluejs::{compile, parse, Value, Vm};

/// Runs `body` after a small prelude defining `check(label, expected, thunk)`,
/// which compares `String(thunk())` (or the thrown error's class name) with
/// `expected`. The program's result is the newline-joined list of mismatches
/// and must be empty.
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
          // Passes when the thunk throws either error class; used where the
          // exact class is not pinned down by the cases below.
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
          const YM = Temporal.PlainYearMonth;
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
fn constructor_validates_its_arguments() {
    run_checks(
        r#"
        check("basic", "2020-05", () => new YM(2020, 5));
        check("no new", "TypeError", () => YM(2020, 5));
        check("no args", "RangeError", () => new YM());
        check("year only", "RangeError", () => new YM(2020));
        check("month 13", "RangeError", () => new YM(2020, 13));
        check("month 0", "RangeError", () => new YM(2020, 0));
        check("infinite year", "RangeError", () => new YM(Infinity, 1));
        check("string args", "2020-05", () => new YM("2020", "5"));
        check("fraction truncates", "2020-05", () => new YM(2020.9, 5.9));
        check("bad calendar", "RangeError", () => new YM(2020, 5, "nonsense"));
        check("calendar not string", "TypeError", () => new YM(2020, 5, 5));
        check("calendar case", "2020-05-01[u-ca=gregory]", () => new YM(2020, 5, "GREGORY"));
        check("iso calendar explicit", "2020-05", () => new YM(2020, 5, "iso8601"));
        check("reference day", "2020-05-31[u-ca=gregory]", () => new YM(2020, 5, "gregory", 31));
        check("reference day past month end", "RangeError", () => new YM(2020, 2, "iso8601", 30));
        check("reference day zero", "RangeError", () => new YM(2020, 2, "iso8601", 0));
        check("max limit", "+275760-09", () => new YM(275760, 9));
        check("past max", "RangeError", () => new YM(275760, 10));
        check("min limit", "-271821-04", () => new YM(-271821, 4));
        check("before min", "RangeError", () => new YM(-271821, 3));
        check("max with reference day", "+275760-09", () => new YM(275760, 9, undefined, 13));
        check("year zero", "0000-01", () => new YM(0, 1));
        check("large year", "+010000-01", () => new YM(10000, 1));
        check("negative year", "-000001-01", () => new YM(-1, 1));
        check("length", "2", () => YM.length);
        check("Symbol year", "TypeError", () => new YM(Symbol(), 1));
        "#,
    );
}

#[test]
fn from_resolves_property_bags() {
    run_checks(
        r#"
        check("month+year", "2020-05", () => YM.from({ year: 2020, month: 5 }));
        check("monthCode", "2020-05", () => YM.from({ year: 2020, monthCode: "M05" }));
        check("both agree", "2020-05", () => YM.from({ year: 2020, month: 5, monthCode: "M05" }));
        check("both disagree", "RangeError", () => YM.from({ year: 2020, month: 5, monthCode: "M06" }));
        check("no year", "TypeError", () => YM.from({ month: 5 }));
        check("no month", "TypeError", () => YM.from({ year: 2020 }));
        check("empty", "TypeError", () => YM.from({}));
        check("month 0", "RangeError", () => YM.from({ year: 2020, month: 0 }));
        check("month negative", "RangeError", () => YM.from({ year: 2020, month: -1 }));
        check("month 13 constrain", "2020-12", () => YM.from({ year: 2020, month: 13 }));
        check("month 13 explicit constrain", "2020-12",
              () => YM.from({ year: 2020, month: 13 }, { overflow: "constrain" }));
        check("month 13 reject", "RangeError", () => YM.from({ year: 2020, month: 13 }, { overflow: "reject" }));
        check("month code 13", "RangeError", () => YM.from({ year: 2020, monthCode: "M13" }));
        check("month code leap in iso", "RangeError", () => YM.from({ year: 2020, monthCode: "M05L" }));
        check("month code lowercase", "RangeError", () => YM.from({ year: 2020, monthCode: "m05" }));
        check("month code short", "RangeError", () => YM.from({ year: 2020, monthCode: "M5" }));
        check("month code M00", "RangeError", () => YM.from({ year: 2020, monthCode: "M00" }));
        check("year fraction", "2020-05", () => YM.from({ year: 2020.9, month: 5 }));
        check("year string", "2020-05", () => YM.from({ year: "2020", month: "5" }));
        check("year infinity", "RangeError", () => YM.from({ year: Infinity, month: 5 }));
        check("year NaN", "RangeError", () => YM.from({ year: NaN, month: 5 }));
        check("year symbol", "TypeError", () => YM.from({ year: Symbol(), month: 5 }));
        check("year zero", "0000-05", () => YM.from({ year: 0, month: 5 }));
        check("max ok", "+275760-09", () => YM.from({ year: 275760, month: 9 }));
        check("past max", "RangeError", () => YM.from({ year: 275760, month: 10 }));
        check("past max constrain", "RangeError", () => YM.from({ year: 275761, month: 1 }));
        check("min ok", "-271821-04", () => YM.from({ year: -271821, month: 4 }));
        check("before min", "RangeError", () => YM.from({ year: -271821, month: 3 }));
        check("iso ignores era", "2020-05", () => YM.from({ year: 2020, month: 5, era: "ce" }));
        check("options primitive", "TypeError", () => YM.from({ year: 2020, month: 5 }, 5));
        check("options null", "TypeError", () => YM.from({ year: 2020, month: 5 }, null));
        check("options bad overflow", "RangeError", () => YM.from({ year: 2020, month: 5 }, { overflow: "x" }));
        check("calendar bad", "RangeError", () => YM.from({ year: 2020, month: 5, calendar: "x" }));
        check("calendar wrong type", "TypeError", () => YM.from({ year: 2020, month: 5, calendar: 5 }));
        check("calendar in string form", "2020-05-01[u-ca=gregory]",
              () => YM.from({ year: 2020, month: 5, calendar: "gregory" }));
        check("calendar from ISO string", "2020-05-01[u-ca=gregory]",
              () => YM.from({ year: 2020, month: 5, calendar: "2020-01-01[u-ca=gregory]" }));
        check("from PlainDate", "2020-05", () => YM.from(new Temporal.PlainDate(2020, 5, 15)));
        check("from PlainDateTime", "2020-05", () => YM.from(new Temporal.PlainDateTime(2020, 5, 15, 1)));
        check("from instance", "2020-05", () => YM.from(new YM(2020, 5)));
        check("from instance bad options", "RangeError", () => YM.from(new YM(2020, 5), { overflow: "x" }));
        check("from instance options primitive", "TypeError", () => YM.from(new YM(2020, 5), 5));
        check("from undefined", "TypeError", () => YM.from(undefined));
        check("from null", "TypeError", () => YM.from(null));
        check("from number", "TypeError", () => YM.from(202005));
        check("from boolean", "TypeError", () => YM.from(true));
        check("from symbol", "TypeError", () => YM.from(Symbol()));
        check("from bigint", "TypeError", () => YM.from(5n));
        "#,
    );
}

#[test]
fn from_resolves_strings() {
    run_checks(
        r#"
        check("year-month", "2020-05", () => YM.from("2020-05"));
        check("full date", "2020-05", () => YM.from("2020-05-15"));
        check("date time", "2020-05", () => YM.from("2020-05-15T10:30"));
        check("date time offset", "2020-05", () => YM.from("2020-05-15T10:30+01:00"));
        check("date time zone annotation", "2020-05", () => YM.from("2020-05-15T10:30[UTC]"));
        check("utc designator", "RangeError", () => YM.from("2020-05-15T10:30Z"));
        check("annotation iso", "2020-05", () => YM.from("2020-05-01[u-ca=iso8601]"));
        check("unknown annotation", "2020-05", () => YM.from("2020-05-01[foo=bar]"));
        check("critical unknown annotation", "RangeError", () => YM.from("2020-05-01[!foo=bar]"));
        check("calendar needs full date", "RangeError", () => YM.from("2020-05[u-ca=gregory]"));
        check("calendar full date", "2020-05-01[u-ca=gregory]", () => YM.from("2020-05-01[u-ca=gregory]"));
        check("calendar full date other day", "2020-05-01[u-ca=gregory]",
              () => YM.from("2020-05-15[u-ca=gregory]"));
        check("month 13", "RangeError", () => YM.from("2020-13"));
        check("day 32", "RangeError", () => YM.from("2020-05-32"));
        check("feb 30", "RangeError", () => YM.from("2020-02-30"));
        check("leap day", "2020-02", () => YM.from("2020-02-29"));
        check("non-leap day", "RangeError", () => YM.from("2019-02-29"));
        check("empty", "RangeError", () => YM.from(""));
        check("garbage", "RangeError", () => YM.from("nonsense"));
        check("negative zero year", "RangeError", () => YM.from("-000000-01"));
        check("extended year", "+010000-01", () => YM.from("+010000-01"));
        check("max", "+275760-09", () => YM.from("+275760-09"));
        check("past max", "RangeError", () => YM.from("+275760-10"));
        check("min", "-271821-04", () => YM.from("-271821-04"));
        check("before min", "RangeError", () => YM.from("-271821-03"));
        check("options bad overflow", "RangeError", () => YM.from("2020-05", { overflow: "x" }));
        check("options primitive", "TypeError", () => YM.from("2020-05", 5));
        check("hebrew string", "5784", () => YM.from("2024-03-15[u-ca=hebrew]").year);
        check("hebrew string monthCode", "M06", () => YM.from("2024-03-15[u-ca=hebrew]").monthCode);
        check("hebrew string calendarId", "hebrew", () => YM.from("2024-03-15[u-ca=hebrew]").calendarId);
        check("chinese string", "M02", () => YM.from("2024-03-15[u-ca=chinese]").monthCode);
        "#,
    );
}

#[test]
fn calendar_aware_resolution_and_getters() {
    run_checks(
        r#"
        const gregory = YM.from({ year: 2020, month: 5, calendar: "gregory" });
        check("gregory era", "ce", () => gregory.era);
        check("gregory eraYear", "2020", () => gregory.eraYear);
        check("era+eraYear", "2020-05-01[u-ca=gregory]",
              () => YM.from({ era: "ce", eraYear: 2020, month: 5, calendar: "gregory" }));
        check("bce", "0000-05-01[u-ca=gregory]",
              () => YM.from({ era: "bce", eraYear: 1, month: 5, calendar: "gregory" }));
        check("era infinity", "RangeError",
              () => YM.from({ era: "ce", eraYear: Infinity, month: 5, calendar: "gregory" }));
        check("era symbol", "TypeError",
              () => YM.from({ era: Symbol(), eraYear: 2020, month: 5, calendar: "gregory" }));
        check("unknown era", "RangeError",
              () => YM.from({ era: "nonsense", eraYear: 2020, month: 5, calendar: "gregory" }));
        check("japanese reiwa", "2020", () => YM.from({ era: "reiwa", eraYear: 2, month: 5, calendar: "japanese" }).year);
        check("japanese era getter", "reiwa", () => YM.from({ era: "reiwa", eraYear: 2, month: 5, calendar: "japanese" }).era);
        check("japanese eraYear getter", "2", () => YM.from({ era: "reiwa", eraYear: 2, month: 5, calendar: "japanese" }).eraYear);
        check("iso era", "undefined", () => new YM(2020, 5).era);
        check("iso eraYear", "undefined", () => new YM(2020, 5).eraYear);
        check("chinese era", "undefined", () => YM.from({ year: 2023, month: 5, calendar: "chinese" }).era);

        const iso = new YM(2020, 2);
        check("daysInMonth", "29", () => iso.daysInMonth);
        check("daysInYear", "366", () => iso.daysInYear);
        check("monthsInYear", "12", () => iso.monthsInYear);
        check("inLeapYear", "true", () => iso.inLeapYear);
        check("monthCode", "M02", () => iso.monthCode);
        check("year", "2020", () => iso.year);
        check("month", "2", () => iso.month);
        check("calendarId", "iso8601", () => iso.calendarId);
        check("not leap", "false", () => new YM(2019, 2).inLeapYear);

        check("hebrew leap monthCode", "M05L",
              () => YM.from({ year: 5784, monthCode: "M05L", calendar: "hebrew" }).monthCode);
        check("hebrew leap ordinal", "6",
              () => YM.from({ year: 5784, monthCode: "M05L", calendar: "hebrew" }).month);
        check("hebrew leap monthsInYear", "13",
              () => YM.from({ year: 5784, monthCode: "M05L", calendar: "hebrew" }).monthsInYear);
        check("hebrew leap inLeapYear", "true",
              () => YM.from({ year: 5784, monthCode: "M05L", calendar: "hebrew" }).inLeapYear);
        check("hebrew common monthsInYear", "12",
              () => YM.from({ year: 5783, month: 1, calendar: "hebrew" }).monthsInYear);
        check("hebrew leap month constrained in common year", "M06",
              () => YM.from({ year: 5783, monthCode: "M05L", calendar: "hebrew" }).monthCode);
        check("hebrew leap month rejected in common year", "RangeError",
              () => YM.from({ year: 5783, monthCode: "M05L", calendar: "hebrew" }, { overflow: "reject" }));
        check("hebrew month 13 in common year constrains", "M12",
              () => YM.from({ year: 5783, month: 13, calendar: "hebrew" }).monthCode);
        check("hebrew month 13 in common year rejects", "RangeError",
              () => YM.from({ year: 5783, month: 13, calendar: "hebrew" }, { overflow: "reject" }));
        check("chinese leap", "M02L",
              () => YM.from({ year: 2023, monthCode: "M02L", calendar: "chinese" }).monthCode);
        check("chinese leap monthsInYear", "13",
              () => YM.from({ year: 2023, monthCode: "M02L", calendar: "chinese" }).monthsInYear);
        check("chinese leap month rejected elsewhere", "RangeError",
              () => YM.from({ year: 2022, monthCode: "M02L", calendar: "chinese" }, { overflow: "reject" }));
        check("dangi", "M03",
              () => YM.from({ year: 2023, monthCode: "M03", calendar: "dangi" }).monthCode);
        "#,
    );
}

#[test]
fn with_merges_recognised_fields() {
    run_checks(
        r#"
        const ym = new YM(2020, 5);
        check("month", "2020-06", () => ym.with({ month: 6 }));
        check("year", "2021-05", () => ym.with({ year: 2021 }));
        check("monthCode", "2020-07", () => ym.with({ monthCode: "M07" }));
        check("all", "2019-02", () => ym.with({ year: 2019, month: 2 }));
        check("year and monthCode", "2019-02", () => ym.with({ year: 2019, monthCode: "M02" }));
        check("month conflicts monthCode", "RangeError", () => ym.with({ month: 5, monthCode: "M06" }));
        check("month agrees monthCode", "2020-06", () => ym.with({ month: 6, monthCode: "M06" }));
        check("month 13 constrains", "2020-12", () => ym.with({ month: 13 }));
        check("month 13 rejects", "RangeError", () => ym.with({ month: 13 }, { overflow: "reject" }));
        check("month 0", "RangeError", () => ym.with({ month: 0 }));
        check("bad monthCode", "RangeError", () => ym.with({ monthCode: "M13" }));
        check("empty object", "TypeError", () => ym.with({}));
        check("unrecognised only", "TypeError", () => ym.with({ foo: 1 }));
        check("undefined", "TypeError", () => ym.with());
        check("string", "TypeError", () => ym.with("2020-06"));
        check("number", "TypeError", () => ym.with(5));
        check("null", "TypeError", () => ym.with(null));
        check("temporal object", "TypeError", () => ym.with(new YM(2021, 1)));
        check("temporal date object", "TypeError", () => ym.with(new Temporal.PlainDate(2021, 1, 1)));
        check("calendar property", "TypeError", () => ym.with({ month: 1, calendar: "iso8601" }));
        check("timeZone property", "TypeError", () => ym.with({ month: 1, timeZone: "UTC" }));
        check("calendar undefined is fine", "2020-01", () => ym.with({ month: 1, calendar: undefined }));
        check("options primitive", "TypeError", () => ym.with({ month: 1 }, 5));
        check("options bad overflow", "RangeError", () => ym.with({ month: 1 }, { overflow: "x" }));
        check("iso ignores era", "2000-05", () => ym.with({ year: 2000, era: "ce" }));
        check("iso era only is unrecognised", "TypeError", () => ym.with({ era: "ce", eraYear: 2000 }));
        check("out of range year", "RangeError", () => ym.with({ year: 275761 }));
        check("out of range month at max year", "RangeError", () => ym.with({ year: 275760, month: 10 }));
        check("in range at max year", "+275760-09", () => ym.with({ year: 275760, month: 9 }));
        check("min year in range", "-271821-04", () => ym.with({ year: -271821, month: 4 }));
        check("min year out of range", "RangeError", () => ym.with({ year: -271821, month: 3 }));
        check("year infinity", "RangeError", () => ym.with({ year: Infinity }));
        check("receiver unchanged", "2020-05", () => { ym.with({ month: 1 }); return ym; });

        const gregory = YM.from({ year: 2020, month: 5, calendar: "gregory" });
        check("gregory era", "0000-05-01[u-ca=gregory]", () => gregory.with({ era: "bce", eraYear: 1 }));
        check("gregory era only", "TypeError", () => gregory.with({ era: "ce" }));
        check("gregory eraYear only", "TypeError", () => gregory.with({ eraYear: 5 }));
        check("gregory month keeps year", "2020-08-01[u-ca=gregory]", () => gregory.with({ month: 8 }));
        check("gregory year keeps month", "1999-05-01[u-ca=gregory]", () => gregory.with({ year: 1999 }));
        check("gregory bad era", "RangeError", () => gregory.with({ era: "nonsense", eraYear: 1 }));

        const chinese = YM.from({ year: 2023, monthCode: "M02L", calendar: "chinese" });
        check("chinese era rejected", "TypeError", () => chinese.with({ era: "x", eraYear: 1 }));
        check("chinese eraYear rejected", "TypeError", () => chinese.with({ eraYear: 1, year: 2020 }));
        check("chinese leap keeps monthCode", "M02L", () => chinese.with({ year: 2023 }).monthCode);
        check("chinese leap into common year constrains", "M02", () => chinese.with({ year: 2022 }).monthCode);
        check("chinese leap into common year rejects", "RangeError",
              () => chinese.with({ year: 2022 }, { overflow: "reject" }));
        check("chinese month ordinal", "M01", () => chinese.with({ month: 1 }).monthCode);
        check("chinese order", "M05", () => chinese.with({ monthCode: "M05" }).monthCode);
        "#,
    );
}

#[test]
fn add_and_subtract_years_and_months() {
    run_checks(
        r#"
        const ym = new YM(2020, 1);
        check("add months", "2020-02", () => ym.add({ months: 1 }));
        check("add many months", "2021-02", () => ym.add({ months: 13 }));
        check("add years", "2023-01", () => ym.add({ years: 3 }));
        check("add years and months", "2022-04", () => ym.add({ years: 2, months: 3 }));
        check("add string", "2021-01", () => ym.add("P1Y"));
        check("add duration object", "2020-04", () => ym.add(new Temporal.Duration(0, 3)));
        check("add negative", "2019-12", () => ym.add({ months: -1 }));
        check("subtract months", "2019-12", () => ym.subtract({ months: 1 }));
        check("subtract years", "2010-01", () => ym.subtract({ years: 10 }));
        check("subtract negative is add", "2020-03", () => ym.subtract({ months: -2 }));
        check("subtract string", "2019-01", () => ym.subtract("P1Y"));
        check("subtract wraps year", "2018-11", () => ym.subtract({ years: 1, months: 2 }));
        check("add zero", "2020-01", () => ym.add({ months: 0 }));
        // Units below a month are rejected (Test262 `add/argument-lower-units.js`).
        for (const unit of ["weeks", "days", "hours", "minutes", "seconds", "milliseconds", "microseconds", "nanoseconds"]) {
          check("add " + unit, "RangeError", () => ym.add({ [unit]: 1 }));
          check("subtract " + unit, "RangeError", () => ym.subtract({ [unit]: 1 }));
          check("add " + unit + " reject", "RangeError", () => ym.add({ [unit]: 1 }, { overflow: "reject" }));
        }
        check("add lower units in string", "RangeError", () => ym.add("P1M1D"));
        check("add time units in string", "RangeError", () => ym.add("PT1H"));
        check("add empty", "TypeError", () => ym.add({}));
        check("add undefined", "TypeError", () => ym.add());
        check("add null", "TypeError", () => ym.add(null));
        check("add number", "TypeError", () => ym.add(5));
        check("add bad string", "RangeError", () => ym.add("nonsense"));
        check("add mixed sign", "RangeError", () => ym.add({ years: 1, months: -1 }));
        check("add options primitive", "TypeError", () => ym.add({ months: 1 }, 5));
        check("add options bad overflow", "RangeError", () => ym.add({ months: 1 }, { overflow: "x" }));
        check("add options ok", "2020-02", () => ym.add({ months: 1 }, { overflow: "reject" }));
        check("add past max", "RangeError", () => new YM(275760, 9).add({ months: 1 }));
        check("add reaching max", "+275760-09", () => new YM(275760, 8).add({ months: 1 }));
        check("subtract past min", "RangeError", () => new YM(-271821, 4).subtract({ months: 1 }));
        check("subtract reaching min", "-271821-04", () => new YM(-271821, 5).subtract({ months: 1 }));
        check("add huge", "RangeError", () => ym.add({ years: 400000 }));
        check("subtract huge", "RangeError", () => ym.subtract({ years: 400000 }));
        check("subtract from max", "+275759-09", () => new YM(275760, 9).subtract({ years: 1 }));

        const hebrew = YM.from({ year: 5784, monthCode: "M05L", calendar: "hebrew" });
        check("hebrew leap plus year constrains", "M06", () => hebrew.add({ years: 1 }).monthCode);
        check("hebrew leap plus year rejects", "RangeError", () => hebrew.add({ years: 1 }, { overflow: "reject" }));
        check("hebrew leap plus month", "M06", () => hebrew.add({ months: 1 }).monthCode);
        check("hebrew leap minus month", "M05", () => hebrew.subtract({ months: 1 }).monthCode);
        check("hebrew year change", "5785", () => hebrew.add({ years: 1 }).year);
        check("hebrew subtract calendar preserved", "hebrew", () => hebrew.subtract({ months: 1 }).calendarId);
        const chinese = YM.from({ year: 2023, monthCode: "M02L", calendar: "chinese" });
        check("chinese leap plus month", "M03", () => chinese.add({ months: 1 }).monthCode);
        check("chinese leap minus month", "M02", () => chinese.subtract({ months: 1 }).monthCode);
        check("chinese plus 13 months", "2024", () => chinese.add({ months: 13 }).year);
        "#,
    );
}

#[test]
fn until_and_since_validate_options() {
    run_checks(
        r#"
        const a = new YM(2020, 1);
        const b = new YM(2021, 3);
        check("until", "P1Y2M", () => a.until(b));
        check("since", "-P1Y2M", () => a.since(b));
        check("until backwards", "-P1Y2M", () => b.until(a));
        check("since backwards", "P1Y2M", () => b.since(a));
        check("until string", "P1Y2M", () => a.until("2021-03"));
        check("until bag", "P1Y2M", () => a.until({ year: 2021, month: 3 }));
        check("until equal", "PT0S", () => a.until(a));
        check("largestUnit month", "P14M", () => a.until(b, { largestUnit: "month" }));
        check("largestUnit months", "P14M", () => a.until(b, { largestUnit: "months" }));
        check("largestUnit year", "P1Y2M", () => a.until(b, { largestUnit: "year" }));
        check("largestUnit auto", "P1Y2M", () => a.until(b, { largestUnit: "auto" }));
        check("largestUnit undefined", "P1Y2M", () => a.until(b, { largestUnit: undefined }));
        check("largestUnit day", "RangeError", () => a.until(b, { largestUnit: "day" }));
        check("largestUnit week", "RangeError", () => a.until(b, { largestUnit: "week" }));
        check("largestUnit hour", "RangeError", () => a.until(b, { largestUnit: "hour" }));
        check("largestUnit nanosecond", "RangeError", () => a.until(b, { largestUnit: "nanosecond" }));
        check("largestUnit bad", "RangeError", () => a.until(b, { largestUnit: "nonsense" }));
        check("largestUnit symbol", "TypeError", () => a.until(b, { largestUnit: Symbol() }));
        check("smallestUnit year", "P1Y", () => a.until(b, { smallestUnit: "year" }));
        check("smallestUnit month", "P1Y2M", () => a.until(b, { smallestUnit: "month" }));
        check("smallestUnit day", "RangeError", () => a.until(b, { smallestUnit: "day" }));
        check("smallestUnit week", "RangeError", () => a.until(b, { smallestUnit: "week" }));
        check("smallestUnit second", "RangeError", () => a.until(b, { smallestUnit: "second" }));
        check("smallestUnit bad", "RangeError", () => a.until(b, { smallestUnit: "nonsense" }));
        check("smallestUnit larger than largestUnit", "RangeError",
              () => a.until(b, { largestUnit: "month", smallestUnit: "year" }));
        check("year year", "P1Y", () => a.until(b, { largestUnit: "year", smallestUnit: "year" }));
        check("month month", "P14M", () => a.until(b, { largestUnit: "month", smallestUnit: "month" }));
        check("increment 0", "RangeError", () => a.until(b, { roundingIncrement: 0 }));
        check("increment negative", "RangeError", () => a.until(b, { roundingIncrement: -1 }));
        check("increment NaN", "RangeError", () => a.until(b, { roundingIncrement: NaN }));
        check("increment too large", "RangeError", () => a.until(b, { roundingIncrement: 1e9 + 1 }));
        check("increment infinity", "RangeError", () => a.until(b, { roundingIncrement: Infinity }));
        check("increment symbol", "TypeError", () => a.until(b, { roundingIncrement: Symbol() }));
        check("increment 1", "P1Y2M", () => a.until(b, { roundingIncrement: 1 }));
        check("increment fraction truncates", "P1Y2M", () => a.until(b, { roundingIncrement: 1.9 }));
        check("mode bad", "RangeError", () => a.until(b, { roundingMode: "nonsense" }));
        check("mode symbol", "TypeError", () => a.until(b, { roundingMode: Symbol() }));
        check("options primitive", "TypeError", () => a.until(b, 5));
        check("options null", "TypeError", () => a.until(b, null));
        check("options string", "TypeError", () => a.until(b, "year"));
        check("since options primitive", "TypeError", () => a.since(b, true));
        check("other undefined", "TypeError", () => a.until());
        check("other null", "TypeError", () => a.until(null));
        check("other number", "TypeError", () => a.until(5));
        check("other bad string", "RangeError", () => a.until("nonsense"));
        check("other incomplete bag", "TypeError", () => a.until({ year: 2020 }));
        check("different calendar", "RangeError",
              () => a.until(YM.from({ year: 2021, month: 1, calendar: "gregory" })));
        check("different calendar since", "RangeError",
              () => a.since(YM.from({ year: 2021, month: 1, calendar: "gregory" })));
        check("gregory same calendar", "P1Y2M",
              () => YM.from({ year: 2020, month: 1, calendar: "gregory" })
                      .until(YM.from({ year: 2021, month: 3, calendar: "gregory" })));
        check("increment months", "P4M", () => new YM(2020, 1).until(new YM(2020, 6), { smallestUnit: "month", roundingIncrement: 2 }));
        check("increment months expand", "P6M",
              () => new YM(2020, 1).until(new YM(2020, 6), { smallestUnit: "month", roundingIncrement: 2, roundingMode: "expand" }));
        check("increment months ceil", "P6M",
              () => new YM(2020, 1).until(new YM(2020, 6), { smallestUnit: "month", roundingIncrement: 2, roundingMode: "ceil" }));
        check("increment months floor", "P4M",
              () => new YM(2020, 1).until(new YM(2020, 6), { smallestUnit: "month", roundingIncrement: 2, roundingMode: "floor" }));
        check("increment years", "P4Y", () => new YM(2020, 1).until(new YM(2025, 1), { smallestUnit: "year", roundingIncrement: 4 }));
        check("increment years expand", "P8Y",
              () => new YM(2020, 1).until(new YM(2025, 1), { smallestUnit: "year", roundingIncrement: 4, roundingMode: "expand" }));
        "#,
    );
}

#[test]
fn until_and_since_round_symmetrically_and_reflect_direction_sensitive_modes() {
    run_checks(
        r#"
        const early = new YM(2020, 1);
        const late = new YM(2020, 5);
        function u(from, to, mode) {
          return from.until(to, { smallestUnit: "year", roundingMode: mode }).toString();
        }
        function s(from, to, mode) {
          return from.since(to, { smallestUnit: "year", roundingMode: mode }).toString();
        }
        // four months is a third of a year, never near a rounding tie.
        for (const [mode, forward, backward] of [
          ["trunc", "PT0S", "PT0S"],
          ["floor", "PT0S", "-P1Y"],
          ["ceil", "P1Y", "PT0S"],
          ["expand", "P1Y", "-P1Y"],
          ["halfExpand", "PT0S", "PT0S"],
          ["halfTrunc", "PT0S", "PT0S"],
          ["halfCeil", "PT0S", "PT0S"],
          ["halfFloor", "PT0S", "PT0S"],
          ["halfEven", "PT0S", "PT0S"],
        ]) {
          check("until forward " + mode, forward, () => u(early, late, mode));
          check("until backward " + mode, backward, () => u(late, early, mode));
          // `since` is `until` with the receiver and argument swapped in sign:
          // early.since(late) is the negative of early.until(late).
          const negate = (text) => text === "PT0S" ? text : text.startsWith("-") ? text.slice(1) : "-" + text;
          check("since " + mode, negate(u(early, late, mode === "floor" ? "ceil" : mode === "ceil" ? "floor" : mode)),
                () => s(early, late, mode));
        }
        // two thirds of a year rounds up under half modes.
        check("eight months halfExpand", "P1Y", () => early.until(new YM(2020, 9), { smallestUnit: "year", roundingMode: "halfExpand" }));
        check("eight months halfEven", "P1Y", () => early.until(new YM(2020, 9), { smallestUnit: "year", roundingMode: "halfEven" }));
        check("eight months halfCeil", "P1Y", () => early.until(new YM(2020, 9), { smallestUnit: "year", roundingMode: "halfCeil" }));
        check("eight months halfFloor", "P1Y", () => early.until(new YM(2020, 9), { smallestUnit: "year", roundingMode: "halfFloor" }));
        check("eight months halfTrunc", "P1Y", () => early.until(new YM(2020, 9), { smallestUnit: "year", roundingMode: "halfTrunc" }));
        check("eight months since halfCeil", "-P1Y", () => early.since(new YM(2020, 9), { smallestUnit: "year", roundingMode: "halfCeil" }));
        check("eight months since halfFloor", "-P1Y", () => early.since(new YM(2020, 9), { smallestUnit: "year", roundingMode: "halfFloor" }));
        check("eight months backward since halfCeil", "P1Y", () => new YM(2020, 9).since(early, { smallestUnit: "year", roundingMode: "halfCeil" }));
        check("1y8m rounds to 2y", "P2Y", () => early.until(new YM(2021, 9), { smallestUnit: "year", roundingMode: "halfExpand" }));
        check("1y8m largestUnit month", "P20M", () => early.until(new YM(2021, 9), { largestUnit: "month" }));
        check("since largestUnit month", "-P20M", () => early.since(new YM(2021, 9), { largestUnit: "month" }));
        check("since default mode is trunc", "-P1Y", () => early.since(new YM(2021, 9), { smallestUnit: "year" }));
        check("until default mode is trunc", "-P1Y", () => new YM(2021, 9).until(early, { smallestUnit: "year" }));
        check("across year boundary", "P1M", () => new YM(2019, 12).until(new YM(2020, 1)));
        check("negative years", "P3Y", () => new YM(-5, 1).until(new YM(-2, 1)));
        check("since equal", "PT0S", () => early.since(early));
        check("wide span", "P1000Y", () => new YM(1000, 1).until(new YM(2000, 1)));
        // The difference is taken between the months' first days, and -271821-04's is
        // before the earliest representable date (-271821-04-19): the widest valid span
        // starts a month later, and starting at the minimum month is a RangeError.
        check("extremes", "P547581Y4M", () => new YM(-271821, 5).until(new YM(275760, 9)));
        check("extremes months", "P6570976M", () => new YM(-271821, 5).until(new YM(275760, 9), { largestUnit: "month" }));
        check("minimum month cannot be differenced", "RangeError", () => new YM(-271821, 4).until(new YM(275760, 9)));
        check("minimum month still equals itself", "PT0S", () => new YM(-271821, 4).until(new YM(-271821, 4)));
        "#,
    );
}

#[test]
fn until_and_since_handle_leap_month_calendars() {
    run_checks(
        r#"
        const mk = (year, monthCode, calendar) => YM.from({ year, monthCode, calendar });
        check("hebrew leap to next month", "P1M",
              () => mk(5784, "M05L", "hebrew").until(mk(5784, "M06", "hebrew")));
        check("hebrew adar to nisan", "P1M",
              () => mk(5784, "M06", "hebrew").until(mk(5784, "M07", "hebrew")));
        check("hebrew across years", "P1Y",
              () => mk(5783, "M06", "hebrew").until(mk(5784, "M06", "hebrew")));
        check("hebrew leap year months", "P13M",
              () => mk(5784, "M01", "hebrew").until(mk(5785, "M01", "hebrew"), { largestUnit: "month" }));
        check("hebrew common year months", "P12M",
              () => mk(5783, "M01", "hebrew").until(mk(5784, "M01", "hebrew"), { largestUnit: "month" }));
        check("hebrew since", "-P1M",
              () => mk(5784, "M05L", "hebrew").since(mk(5784, "M06", "hebrew")));
        check("chinese leap to common", "P1M",
              () => mk(2023, "M02L", "chinese").until(mk(2023, "M03", "chinese")));
        check("chinese common to leap", "P1M",
              () => mk(2023, "M02", "chinese").until(mk(2023, "M02L", "chinese")));
        check("chinese years", "P1Y",
              () => mk(2022, "M03", "chinese").until(mk(2023, "M03", "chinese")));
        check("chinese since", "-P1Y",
              () => mk(2022, "M03", "chinese").since(mk(2023, "M03", "chinese")));
        check("chinese rounding year", "P1Y",
              () => mk(2022, "M03", "chinese").until(mk(2023, "M06", "chinese"), { smallestUnit: "year" }));
        check("dangi years", "P2Y",
              () => mk(2021, "M03", "dangi").until(mk(2023, "M03", "dangi")));
        check("gregory years", "P2Y5M",
              () => YM.from({ year: 2019, month: 1, calendar: "gregory" })
                      .until(YM.from({ year: 2021, month: 6, calendar: "gregory" })));
        check("gregory since rounded ceil", "-P3Y",
              () => YM.from({ year: 2019, month: 1, calendar: "gregory" })
                      .since(YM.from({ year: 2021, month: 6, calendar: "gregory" }), { smallestUnit: "year", roundingMode: "floor" }));
        "#,
    );
}

#[test]
fn equals_compare_and_conversions() {
    run_checks(
        r#"
        const a = new YM(2020, 5);
        check("equals same", "true", () => a.equals(new YM(2020, 5)));
        check("equals month differs", "false", () => a.equals(new YM(2020, 6)));
        check("equals year differs", "false", () => a.equals(new YM(2021, 5)));
        check("equals string", "true", () => a.equals("2020-05"));
        check("equals bag", "true", () => a.equals({ year: 2020, month: 5 }));
        check("equals calendar differs", "false",
              () => a.equals(YM.from({ year: 2020, month: 5, calendar: "gregory" })));
        check("equals bad string", "RangeError", () => a.equals("nonsense"));
        check("equals undefined", "TypeError", () => a.equals());
        check("equals incomplete", "TypeError", () => a.equals({ year: 2020 }));
        check("equals number", "TypeError", () => a.equals(5));
        check("equals hebrew leap distinct", "false",
              () => YM.from({ year: 5784, monthCode: "M05L", calendar: "hebrew" })
                      .equals(YM.from({ year: 5784, monthCode: "M06", calendar: "hebrew" })));
        check("compare less", "-1", () => YM.compare(new YM(2020, 5), new YM(2020, 6)));
        check("compare greater", "1", () => YM.compare(new YM(2021, 1), new YM(2020, 12)));
        check("compare equal", "0", () => YM.compare(new YM(2020, 5), "2020-05"));
        check("compare bags", "-1", () => YM.compare({ year: 2020, month: 1 }, { year: 2020, month: 2 }));
        check("compare across calendars", "-1",
              () => YM.compare(new YM(2020, 5), YM.from({ year: 2020, month: 6, calendar: "gregory" })));
        check("compare same iso date across calendars", "0",
              () => YM.compare(new YM(2020, 5), YM.from({ year: 2020, month: 5, calendar: "gregory" })));
        check("compare bad first", "RangeError", () => YM.compare("nonsense", a));
        check("compare bad second", "RangeError", () => YM.compare(a, "nonsense"));
        check("compare undefined", "TypeError", () => YM.compare(undefined, a));
        check("compare second undefined", "TypeError", () => YM.compare(a, undefined));
        check("compare length", "2", () => YM.compare.length);

        check("toString default", "2020-05", () => a.toString());
        check("toString auto", "2020-05", () => a.toString({ calendarName: "auto" }));
        check("toString always", "2020-05-01[u-ca=iso8601]", () => a.toString({ calendarName: "always" }));
        check("toString critical", "2020-05-01[!u-ca=iso8601]", () => a.toString({ calendarName: "critical" }));
        check("toString never", "2020-05", () => a.toString({ calendarName: "never" }));
        check("toString bad", "RangeError", () => a.toString({ calendarName: "nonsense" }));
        check("toString options primitive", "TypeError", () => a.toString(5));
        check("toString options null", "TypeError", () => a.toString(null));
        check("toString undefined options", "2020-05", () => a.toString(undefined));
        const g = YM.from({ year: 2020, month: 5, calendar: "gregory" });
        check("gregory toString", "2020-05-01[u-ca=gregory]", () => g.toString());
        check("gregory never", "2020-05-01", () => g.toString({ calendarName: "never" }));
        check("gregory critical", "2020-05-01[!u-ca=gregory]", () => g.toString({ calendarName: "critical" }));
        check("toJSON", "2020-05", () => a.toJSON());
        check("toJSON gregory", "2020-05-01[u-ca=gregory]", () => g.toJSON());
        check("String()", "2020-05", () => String(a));
        check("valueOf", "TypeError", () => a.valueOf());
        check("less than", "TypeError", () => a < a);
        check("plus", "TypeError", () => a + 1);
        // a PlainYearMonth is only formatted in its own calendar -- even ISO is a
        // mismatch for a (Gregorian) English locale (`toLocaleString/calendar-mismatch.js`)
        check("toLocaleString iso mismatch", "RangeError", () => a.toLocaleString("en-US"));
        check("toLocaleString type", "string", () => typeof g.toLocaleString("en-US"));
        check("toLocaleString mentions year", "true", () => g.toLocaleString("en-US", { timeZone: "UTC" }).includes("2020"));
        check("toLocaleString timeStyle", "TypeError", () => a.toLocaleString("en-US", { timeStyle: "short" }));
        check("toLocaleString bad locale", "RangeError", () => a.toLocaleString("not a locale"));

        check("toPlainDate", "2020-05-15", () => a.toPlainDate({ day: 15 }));
        check("toPlainDate last day", "2020-05-31", () => a.toPlainDate({ day: 31 }));
        check("toPlainDate constrains", "2020-04-30", () => new YM(2020, 4).toPlainDate({ day: 31 }));
        check("toPlainDate feb leap", "2020-02-29", () => new YM(2020, 2).toPlainDate({ day: 30 }));
        check("toPlainDate feb common", "2019-02-28", () => new YM(2019, 2).toPlainDate({ day: 29 }));
        check("toPlainDate day string", "2020-05-07", () => a.toPlainDate({ day: "7" }));
        check("toPlainDate day fraction", "2020-05-07", () => a.toPlainDate({ day: 7.9 }));
        check("toPlainDate day zero", "RangeError", () => a.toPlainDate({ day: 0 }));
        check("toPlainDate day negative", "RangeError", () => a.toPlainDate({ day: -1 }));
        check("toPlainDate day infinity", "RangeError", () => a.toPlainDate({ day: Infinity }));
        check("toPlainDate no day", "TypeError", () => a.toPlainDate({}));
        check("toPlainDate undefined", "TypeError", () => a.toPlainDate());
        check("toPlainDate string", "TypeError", () => a.toPlainDate("2020-05-15"));
        check("toPlainDate number", "TypeError", () => a.toPlainDate(15));
        check("toPlainDate day symbol", "TypeError", () => a.toPlainDate({ day: Symbol() }));
        check("toPlainDate gregory calendar", "gregory", () => g.toPlainDate({ day: 3 }).calendarId);
        check("toPlainDate gregory value", "2020-05-03[u-ca=gregory]", () => g.toPlainDate({ day: 3 }));
        check("toPlainDate hebrew leap month", "M05L",
              () => YM.from({ year: 5784, monthCode: "M05L", calendar: "hebrew" }).toPlainDate({ day: 10 }).monthCode);
        check("toPlainDate max month", "+275760-09-13", () => new YM(275760, 9).toPlainDate({ day: 13 }));
        "#,
    );
}

#[test]
fn methods_reject_foreign_receivers() {
    run_checks(
        r#"
        const proto = YM.prototype;
        const foreign = [undefined, null, 5, "2020-05", {}, [], new Temporal.PlainDate(2020, 5, 1),
                         new Temporal.PlainMonthDay(5, 1), Temporal.PlainYearMonth.prototype];
        for (const name of ["with", "add", "subtract", "until", "since", "equals", "toString", "toJSON",
                            "toLocaleString", "valueOf", "toPlainDate"]) {
          for (const [index, receiver] of foreign.entries()) {
            check(name + " receiver " + index, "TypeError",
                  () => proto[name].call(receiver, { year: 2020, month: 5, day: 1 }));
          }
        }
        for (const name of ["year", "month", "monthCode", "calendarId", "era", "eraYear", "daysInMonth",
                            "daysInYear", "monthsInYear", "inLeapYear"]) {
          const getter = Object.getOwnPropertyDescriptor(proto, name).get;
          const getterForeign = foreign.filter((r) => !(r instanceof Temporal.PlainDate
            || r instanceof Temporal.PlainMonthDay || r instanceof Temporal.PlainYearMonth));
          for (const [index, receiver] of getterForeign.entries()) {
            check(name + " getter receiver " + index, "TypeError", () => getter.call(receiver));
          }
        }
        check("compare is static", "function", () => typeof YM.compare);
        check("from is static", "function", () => typeof YM.from);
        check("toStringTag", "Temporal.PlainYearMonth", () => proto[Symbol.toStringTag]);
        check("method lengths", "1,1,1,1,1,1,0,0,0,0,1", () => [
          proto.with.length, proto.add.length, proto.subtract.length, proto.until.length,
          proto.since.length, proto.equals.length, proto.toString.length, proto.toJSON.length,
          proto.toLocaleString.length, proto.valueOf.length, proto.toPlainDate.length,
        ].join(","));
        "#,
    );
}

#[test]
fn malformed_and_inconsistent_era_input_is_rejected() {
    run_checks(
        r#"
        check("lone surrogate string", "RangeError", () => YM.from("\uD800"));
        check("lone surrogate string with options", "RangeError", () => YM.from("\uDC00-05", { overflow: "reject" }));
        throwsSome("eraYear without era", () => YM.from({ eraYear: 2020, month: 5, calendar: "gregory" }));
        throwsSome("era without eraYear", () => YM.from({ era: "ce", month: 5, calendar: "gregory" }));
        throwsSome("era and year without eraYear", () => YM.from({ era: "ce", year: 2020, month: 5, calendar: "gregory" }));
        throwsSome("eraYear and year without era", () => YM.from({ eraYear: 2020, year: 2020, month: 5, calendar: "gregory" }));
        throwsSome("with eraYear without era", () => YM.from({ year: 2020, month: 5, calendar: "gregory" }).with({ eraYear: 2019 }));
        throwsSome("with era without eraYear", () => YM.from({ year: 2020, month: 5, calendar: "gregory" }).with({ era: "ce" }));
        check("era and eraYear together", "2019-05-01[u-ca=gregory]",
              () => YM.from({ year: 2020, month: 5, calendar: "gregory" }).with({ era: "ce", eraYear: 2019 }));
        "#,
    );
}
