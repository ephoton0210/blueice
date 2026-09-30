// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Extra coverage for `Temporal.PlainMonthDay` (`vm/temporal/year_month.rs`
//! and `vm/temporal/plain_month_day.rs`): constructor, property-bag and string
//! resolution for the ISO and non-ISO calendars, `with`, `equals`,
//! `toString`, `toPlainDate` and brand checks. Every expectation is what the
//! ECMAScript Temporal specification requires; each case runs inside one
//! JavaScript program that collects mismatches so a failure lists every
//! deviating case at once.

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
          const MD = Temporal.PlainMonthDay;
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
fn constructor_and_serialisation() {
    run_checks(
        r#"
        check("basic", "05-02", () => new MD(5, 2));
        check("no new", "TypeError", () => MD(5, 2));
        check("no args", "RangeError", () => new MD());
        check("month only", "RangeError", () => new MD(5));
        check("month 13", "RangeError", () => new MD(13, 1));
        check("month 0", "RangeError", () => new MD(0, 1));
        check("day 0", "RangeError", () => new MD(5, 0));
        check("day 32", "RangeError", () => new MD(5, 32));
        check("april 31", "RangeError", () => new MD(4, 31));
        check("leap day", "02-29", () => new MD(2, 29));
        check("feb 30", "RangeError", () => new MD(2, 30));
        check("string args", "05-02", () => new MD("5", "2"));
        check("infinite month", "RangeError", () => new MD(Infinity, 1));
        check("bad calendar", "RangeError", () => new MD(5, 2, "nonsense"));
        check("calendar wrong type", "TypeError", () => new MD(5, 2, 5));
        check("gregory", "1972-05-02[u-ca=gregory]", () => new MD(5, 2, "gregory"));
        check("gregory reference year", "2020-05-02[u-ca=gregory]", () => new MD(5, 2, "gregory", 2020));
        check("iso reference year", "2020-05-02[u-ca=iso8601]", () => new MD(5, 2, "iso8601", 2020).toString({ calendarName: "always" }));
        check("reference year makes non-leap feb 29 invalid", "RangeError", () => new MD(2, 29, "iso8601", 2019));
        check("length", "2", () => MD.length);

        const md = new MD(5, 2);
        check("toString auto", "05-02", () => md.toString({ calendarName: "auto" }));
        check("toString always", "1972-05-02[u-ca=iso8601]", () => md.toString({ calendarName: "always" }));
        check("toString critical", "1972-05-02[!u-ca=iso8601]", () => md.toString({ calendarName: "critical" }));
        check("toString never", "05-02", () => md.toString({ calendarName: "never" }));
        check("toString bad", "RangeError", () => md.toString({ calendarName: "nonsense" }));
        check("toString options primitive", "TypeError", () => md.toString(5));
        check("toString options null", "TypeError", () => md.toString(null));
        check("toString options undefined", "05-02", () => md.toString(undefined));
        const g = new MD(5, 2, "gregory");
        check("gregory never", "1972-05-02", () => g.toString({ calendarName: "never" }));
        check("gregory critical", "1972-05-02[!u-ca=gregory]", () => g.toString({ calendarName: "critical" }));
        check("toJSON", "05-02", () => md.toJSON());
        check("toJSON gregory", "1972-05-02[u-ca=gregory]", () => g.toJSON());
        check("String()", "05-02", () => String(md));
        check("valueOf", "TypeError", () => md.valueOf());
        check("less than", "TypeError", () => md < md);
        // a PlainMonthDay is only formatted in its own calendar -- even ISO is a
        // mismatch for a (Gregorian) English locale (`toLocaleString/calendar-mismatch.js`)
        check("toLocaleString iso mismatch", "RangeError", () => md.toLocaleString("en-US"));
        check("toLocaleString type", "string", () => typeof g.toLocaleString("en-US"));
        check("toLocaleString mentions month", "true", () => ["May", "5"].some((part) => g.toLocaleString("en-US", { timeZone: "UTC" }).includes(part)));
        check("toLocaleString timeStyle", "TypeError", () => md.toLocaleString("en-US", { timeStyle: "short" }));
        check("toLocaleString bad locale", "RangeError", () => md.toLocaleString("not a locale"));
        check("toStringTag", "Temporal.PlainMonthDay", () => MD.prototype[Symbol.toStringTag]);
        check("iso year getter absent", "undefined", () => md.year);
        check("monthCode", "M05", () => md.monthCode);
        check("day", "2", () => md.day);
        check("calendarId", "iso8601", () => md.calendarId);
        check("gregory calendarId", "gregory", () => g.calendarId);
        "#,
    );
}

#[test]
fn from_resolves_iso_property_bags() {
    run_checks(
        r#"
        check("month day", "05-02", () => MD.from({ month: 5, day: 2 }));
        check("monthCode day", "05-02", () => MD.from({ monthCode: "M05", day: 2 }));
        check("both agree", "05-02", () => MD.from({ month: 5, monthCode: "M05", day: 2 }));
        check("both disagree", "RangeError", () => MD.from({ month: 12, monthCode: "M11", day: 2 }));
        check("no day", "TypeError", () => MD.from({ month: 5 }));
        check("no month", "TypeError", () => MD.from({ day: 2 }));
        check("empty", "TypeError", () => MD.from({}));
        check("leap day", "02-29", () => MD.from({ month: 2, day: 29 }));
        check("feb 30 constrains", "02-29", () => MD.from({ month: 2, day: 30 }));
        check("feb 30 rejects", "RangeError", () => MD.from({ month: 2, day: 30 }, { overflow: "reject" }));
        check("year regulates leap day", "02-28", () => MD.from({ year: 2019, month: 2, day: 29 }));
        check("year regulates leap day reject", "RangeError",
              () => MD.from({ year: 2019, month: 2, day: 29 }, { overflow: "reject" }));
        check("leap year keeps leap day", "02-29", () => MD.from({ year: 2020, month: 2, day: 29 }, { overflow: "reject" }));
        check("huge year regulates", "02-29", () => MD.from({ year: 2000000, month: 2, day: 29 }));
        check("huge non-leap year", "02-28", () => MD.from({ year: 2000001, month: 2, day: 29 }));
        check("month 13 constrains", "12-01", () => MD.from({ month: 13, day: 1 }));
        check("month 13 rejects", "RangeError", () => MD.from({ month: 13, day: 1 }, { overflow: "reject" }));
        check("month 999999 constrains", "12-01", () => MD.from({ month: 999999, day: 1 }));
        check("month 0", "RangeError", () => MD.from({ month: 0, day: 1 }));
        check("month negative", "RangeError", () => MD.from({ month: -1, day: 1 }));
        check("monthCode 13", "RangeError", () => MD.from({ monthCode: "M13", day: 1 }));
        check("monthCode 00", "RangeError", () => MD.from({ monthCode: "M00", day: 1 }));
        check("monthCode leap", "RangeError", () => MD.from({ monthCode: "M05L", day: 1 }));
        check("monthCode malformed", "RangeError", () => MD.from({ monthCode: "L99M", day: 1 }));
        check("monthCode lowercase", "RangeError", () => MD.from({ monthCode: "m05", day: 1 }));
        check("day 0", "RangeError", () => MD.from({ month: 5, day: 0 }));
        check("day negative", "RangeError", () => MD.from({ month: 5, day: -1 }));
        check("day 32 constrains", "05-31", () => MD.from({ month: 5, day: 32 }));
        check("day 100 constrains", "05-31", () => MD.from({ month: 5, day: 100 }));
        check("day 32 rejects", "RangeError", () => MD.from({ month: 5, day: 32 }, { overflow: "reject" }));
        check("day 31 in april constrains", "04-30", () => MD.from({ month: 4, day: 31 }));
        check("day 31 in april rejects", "RangeError", () => MD.from({ month: 4, day: 31 }, { overflow: "reject" }));
        check("day fraction", "05-02", () => MD.from({ month: 5, day: 2.9 }));
        check("day string", "05-02", () => MD.from({ month: "5", day: "2" }));
        check("day infinity", "RangeError", () => MD.from({ month: 5, day: Infinity }));
        check("day symbol", "TypeError", () => MD.from({ month: 5, day: Symbol() }));
        check("year infinity", "RangeError", () => MD.from({ year: Infinity, month: 5, day: 2 }));
        check("iso ignores era", "05-02", () => MD.from({ month: 5, day: 2, era: "ce", eraYear: 2020 }));
        check("options primitive", "TypeError", () => MD.from({ month: 5, day: 2 }, 5));
        check("options bad overflow", "RangeError", () => MD.from({ month: 5, day: 2 }, { overflow: "x" }));
        check("calendar bad", "RangeError", () => MD.from({ month: 5, day: 2, calendar: "x" }));
        check("calendar wrong type", "TypeError", () => MD.from({ month: 5, day: 2, calendar: 5 }));
        check("from instance", "05-02", () => MD.from(new MD(5, 2)));
        check("from instance bad options", "RangeError", () => MD.from(new MD(5, 2), { overflow: "x" }));
        check("from PlainDate", "05-02", () => MD.from(new Temporal.PlainDate(2020, 5, 2)));
        check("from undefined", "TypeError", () => MD.from(undefined));
        check("from null", "TypeError", () => MD.from(null));
        check("from number", "TypeError", () => MD.from(502));
        check("from boolean", "TypeError", () => MD.from(false));
        check("from symbol", "TypeError", () => MD.from(Symbol()));
        "#,
    );
}

#[test]
fn from_resolves_strings() {
    run_checks(
        r#"
        check("month day", "05-02", () => MD.from("05-02"));
        check("dashed", "05-02", () => MD.from("--05-02"));
        check("full date", "05-02", () => MD.from("2020-05-02"));
        check("date time", "05-02", () => MD.from("2020-05-02T10:30"));
        check("date time zone", "05-02", () => MD.from("2020-05-02T10:30[UTC]"));
        check("utc designator", "RangeError", () => MD.from("2020-05-02T10:30Z"));
        check("leap day", "02-29", () => MD.from("02-29"));
        check("leap day full date", "02-29", () => MD.from("2020-02-29"));
        check("non-leap full date", "RangeError", () => MD.from("2019-02-29"));
        check("feb 30", "RangeError", () => MD.from("02-30"));
        check("month 13", "RangeError", () => MD.from("13-01"));
        check("day 32", "RangeError", () => MD.from("05-32"));
        check("empty", "RangeError", () => MD.from(""));
        check("garbage", "RangeError", () => MD.from("nonsense"));
        check("annotation", "05-02", () => MD.from("2020-05-02[u-ca=iso8601]"));
        check("critical unknown annotation", "RangeError", () => MD.from("2020-05-02[!foo=bar]"));
        check("options bad overflow", "RangeError", () => MD.from("05-02", { overflow: "x" }));
        check("options primitive", "TypeError", () => MD.from("05-02", 5));
        check("invalid string beats bad options", "RangeError", () => MD.from("nonsense", 5));
        check("gregory string monthCode", "M05", () => MD.from("2020-05-02[u-ca=gregory]").monthCode);
        check("gregory string day", "2", () => MD.from("2020-05-02[u-ca=gregory]").day);
        check("gregory string calendar", "gregory", () => MD.from("2020-05-02[u-ca=gregory]").calendarId);
        check("hebrew string monthCode", "M06", () => MD.from("2024-03-15[u-ca=hebrew]").monthCode);
        check("hebrew string day", "5", () => MD.from("2024-03-15[u-ca=hebrew]").day);
        check("chinese string monthCode", "M02", () => MD.from("2024-03-15[u-ca=chinese]").monthCode);
        check("chinese string day", "6", () => MD.from("2024-03-15[u-ca=chinese]").day);
        "#,
    );
}

#[test]
fn from_resolves_non_iso_property_bags() {
    run_checks(
        r#"
        const g = (fields, options) => MD.from({ calendar: "gregory", ...fields }, options);
        check("monthCode day", "1972-05-02[u-ca=gregory]", () => g({ monthCode: "M05", day: 2 }));
        check("ordinal month alone", "TypeError", () => g({ month: 5, day: 2 }));
        check("year and ordinal month", "M05", () => g({ year: 2021, month: 5, day: 2 }).monthCode);
        check("year and ordinal month day", "2", () => g({ year: 2021, month: 5, day: 2 }).day);
        check("no day", "TypeError", () => g({ monthCode: "M05" }));
        check("no month", "TypeError", () => g({ day: 2, year: 2020 }));
        check("feb 30 constrains", "1972-02-29[u-ca=gregory]", () => g({ monthCode: "M02", day: 30 }));
        check("feb 30 rejects", "RangeError", () => g({ monthCode: "M02", day: 30 }, { overflow: "reject" }));
        check("year regulates feb 29", "28", () => g({ year: 2021, month: 2, day: 29 }).day);
        check("year regulates feb 29 rejects", "RangeError",
              () => g({ year: 2021, month: 2, day: 29 }, { overflow: "reject" }));
        check("monthCode 13", "RangeError", () => g({ monthCode: "M13", day: 1 }));
        check("monthCode leap", "RangeError", () => g({ monthCode: "M05L", day: 1 }));
        check("month 13 constrains", "M12", () => g({ year: 2020, month: 13, day: 1 }).monthCode);
        check("month 13 rejects", "RangeError", () => g({ year: 2020, month: 13, day: 1 }, { overflow: "reject" }));
        check("era", "M05", () => g({ era: "ce", eraYear: 2020, month: 5, day: 2 }).monthCode);
        check("era without eraYear", "TypeError", () => g({ era: "ce", month: 5, monthCode: "M05", day: 2 }));
        check("eraYear without era", "TypeError", () => g({ eraYear: 2020, monthCode: "M05", day: 2 }));
        check("era infinity", "RangeError", () => g({ era: "ce", eraYear: Infinity, monthCode: "M05", day: 2 }));
        check("era unknown", "RangeError", () => g({ era: "nonsense", eraYear: 2020, monthCode: "M05", day: 2 }));
        check("era symbol", "TypeError", () => g({ era: Symbol(), eraYear: 2020, monthCode: "M05", day: 2 }));
        check("year infinity", "RangeError", () => g({ year: Infinity, monthCode: "M05", day: 2 }));
        check("hebrew leap month", "M05L",
              () => MD.from({ calendar: "hebrew", monthCode: "M05L", day: 15 }).monthCode);
        check("hebrew leap month day", "15",
              () => MD.from({ calendar: "hebrew", monthCode: "M05L", day: 15 }).day);
        check("hebrew adar", "M06", () => MD.from({ calendar: "hebrew", monthCode: "M06", day: 29 }).monthCode);
        check("hebrew invalid leap code", "RangeError", () => MD.from({ calendar: "hebrew", monthCode: "M02L", day: 1 }));
        // 5784 is a deficient leap year, so Kislev (M03) has 29 days.
        check("hebrew year day", "29",
              () => MD.from({ calendar: "hebrew", year: 5784, monthCode: "M03", day: 30 }).day);
        check("hebrew year day rejects", "RangeError",
              () => MD.from({ calendar: "hebrew", year: 5784, monthCode: "M03", day: 30 }, { overflow: "reject" }));
        check("chinese leap month", "M02L", () => MD.from({ calendar: "chinese", monthCode: "M02L", day: 1 }).monthCode);
        check("chinese month 13", "RangeError", () => MD.from({ calendar: "chinese", monthCode: "M13", day: 1 }));
        check("chinese era ignored", "M03", () => MD.from({ calendar: "chinese", monthCode: "M03", day: 1, era: "ignored" }).monthCode);
        check("dangi", "M04", () => MD.from({ calendar: "dangi", monthCode: "M04", day: 8 }).monthCode);
        check("japanese era", "M05",
              () => MD.from({ calendar: "japanese", era: "reiwa", eraYear: 2, month: 5, day: 2 }).monthCode);
        check("ordinal month in chinese needs year", "TypeError", () => MD.from({ calendar: "chinese", month: 3, day: 1 }));
        "#,
    );
}

#[test]
fn with_merges_recognised_fields() {
    run_checks(
        r#"
        const md = new MD(5, 2);
        check("day", "05-10", () => md.with({ day: 10 }));
        check("month", "06-02", () => md.with({ month: 6 }));
        check("monthCode", "07-02", () => md.with({ monthCode: "M07" }));
        check("month and day", "08-20", () => md.with({ month: 8, day: 20 }));
        check("month conflicts monthCode", "RangeError", () => md.with({ month: 5, monthCode: "M06" }));
        check("month agrees monthCode", "06-02", () => md.with({ month: 6, monthCode: "M06" }));
        check("month 13 constrains", "12-02", () => md.with({ month: 13 }));
        check("month 13 rejects", "RangeError", () => md.with({ month: 13 }, { overflow: "reject" }));
        check("month 0", "RangeError", () => md.with({ month: 0 }));
        check("day 0", "RangeError", () => md.with({ day: 0 }));
        check("day 100 constrains", "05-31", () => md.with({ day: 100 }));
        check("day 32 rejects", "RangeError", () => md.with({ day: 32 }, { overflow: "reject" }));
        check("feb 30 constrains", "02-29", () => md.with({ month: 2, day: 30 }));
        check("feb 30 rejects", "RangeError", () => md.with({ month: 2, day: 30 }, { overflow: "reject" }));
        check("year regulates", "02-28", () => md.with({ year: 2019, month: 2, day: 29 }));
        check("year regulates rejects", "RangeError",
              () => md.with({ year: 2019, month: 2, day: 29 }, { overflow: "reject" }));
        check("year alone does nothing", "05-02", () => md.with({ year: 2019 }));
        check("year huge", "02-28", () => new MD(2, 29).with({ year: 2000001 }));
        check("leap day keeps", "02-29", () => new MD(2, 29).with({ day: 29 }));
        check("leap day to march", "03-29", () => new MD(2, 29).with({ month: 3 }));
        check("leap monthCode", "RangeError", () => md.with({ monthCode: "M05L" }));
        check("empty", "TypeError", () => md.with({}));
        check("unrecognised", "TypeError", () => md.with({ foo: 1 }));
        check("undefined", "TypeError", () => md.with());
        check("string", "TypeError", () => md.with("05-03"));
        check("number", "TypeError", () => md.with(5));
        check("temporal object", "TypeError", () => md.with(new MD(1, 1)));
        check("plain date object", "TypeError", () => md.with(new Temporal.PlainDate(2020, 1, 1)));
        check("calendar property", "TypeError", () => md.with({ day: 1, calendar: "iso8601" }));
        check("timeZone property", "TypeError", () => md.with({ day: 1, timeZone: "UTC" }));
        check("options primitive", "TypeError", () => md.with({ day: 1 }, 5));
        check("options bad overflow", "RangeError", () => md.with({ day: 1 }, { overflow: "x" }));
        check("invalid field before bad options", "RangeError", () => md.with({ day: -1 }, 5));
        check("receiver unchanged", "05-02", () => { md.with({ day: 9 }); return md; });

        const g = new MD(5, 2, "gregory");
        check("gregory day", "10", () => g.with({ day: 10 }).day);
        check("gregory monthCode", "M07", () => g.with({ monthCode: "M07" }).monthCode);
        check("gregory ordinal month alone", "TypeError", () => g.with({ month: 6 }));
        check("gregory ordinal month with year", "M06", () => g.with({ month: 6, year: 2021 }).monthCode);
        check("gregory year regulates", "28", () => g.with({ month: 2, day: 29, year: 2021 }).day);
        check("gregory year regulates rejects", "RangeError",
              () => g.with({ month: 2, day: 29, year: 2021 }, { overflow: "reject" }));
        check("gregory feb 30 via monthCode", "29", () => g.with({ monthCode: "M02", day: 30 }).day);
        check("gregory feb 30 via monthCode rejects", "RangeError",
              () => g.with({ monthCode: "M02", day: 30 }, { overflow: "reject" }));
        check("gregory keeps calendar", "gregory", () => g.with({ day: 3 }).calendarId);
        check("gregory bad monthCode", "RangeError", () => g.with({ monthCode: "M13" }));
        check("gregory empty", "TypeError", () => g.with({}));
        const hebrew = MD.from({ calendar: "hebrew", monthCode: "M05L", day: 15 });
        check("hebrew keeps leap month", "M05L", () => hebrew.with({ day: 20 }).monthCode);
        check("hebrew day changes", "20", () => hebrew.with({ day: 20 }).day);
        check("hebrew monthCode changes", "M06", () => hebrew.with({ monthCode: "M06" }).monthCode);
        "#,
    );
}

#[test]
fn equals_and_to_plain_date() {
    run_checks(
        r#"
        const md = new MD(5, 2);
        check("equals same", "true", () => md.equals(new MD(5, 2)));
        check("equals day differs", "false", () => md.equals(new MD(5, 3)));
        check("equals month differs", "false", () => md.equals(new MD(6, 2)));
        check("equals string", "true", () => md.equals("05-02"));
        check("equals bag", "true", () => md.equals({ month: 5, day: 2 }));
        check("equals bad string", "RangeError", () => md.equals("nonsense"));
        check("equals undefined", "TypeError", () => md.equals());
        check("equals incomplete bag", "TypeError", () => md.equals({ month: 5 }));
        check("equals number", "TypeError", () => md.equals(5));
        check("equals calendar differs", "false", () => md.equals(new MD(5, 2, "gregory")));
        check("equals reference year differs", "false", () => md.equals(new MD(5, 2, "iso8601", 2020)));
        check("equals gregory", "true", () => new MD(5, 2, "gregory").equals(new MD(5, 2, "gregory")));
        check("equals hebrew leap", "false",
              () => MD.from({ calendar: "hebrew", monthCode: "M05L", day: 1 })
                      .equals(MD.from({ calendar: "hebrew", monthCode: "M06", day: 1 })));

        check("toPlainDate", "2020-05-02", () => md.toPlainDate({ year: 2020 }));
        check("toPlainDate leap day in leap year", "2020-02-29", () => new MD(2, 29).toPlainDate({ year: 2020 }));
        check("toPlainDate leap day constrained", "2019-02-28", () => new MD(2, 29).toPlainDate({ year: 2019 }));
        check("toPlainDate year zero", "0000-05-02", () => md.toPlainDate({ year: 0 }));
        check("toPlainDate year string", "2020-05-02", () => md.toPlainDate({ year: "2020" }));
        check("toPlainDate year fraction", "2020-05-02", () => md.toPlainDate({ year: 2020.9 }));
        check("toPlainDate year infinity", "RangeError", () => md.toPlainDate({ year: Infinity }));
        check("toPlainDate year symbol", "TypeError", () => md.toPlainDate({ year: Symbol() }));
        check("toPlainDate no year", "TypeError", () => md.toPlainDate({}));
        check("toPlainDate undefined", "TypeError", () => md.toPlainDate());
        check("toPlainDate string", "TypeError", () => md.toPlainDate("2020"));
        check("toPlainDate number", "TypeError", () => md.toPlainDate(2020));
        check("toPlainDate era ignored in iso", "TypeError", () => md.toPlainDate({ era: "ce", eraYear: 2020 }));
        check("toPlainDate max year", "+275760-09-13", () => new MD(9, 13).toPlainDate({ year: 275760 }));
        check("toPlainDate past max", "RangeError", () => new MD(9, 14).toPlainDate({ year: 275760 }));
        check("toPlainDate min year", "-271821-04-19", () => new MD(4, 19).toPlainDate({ year: -271821 }));
        check("toPlainDate before min", "RangeError", () => new MD(4, 18).toPlainDate({ year: -271821 }));
        check("toPlainDate year out of range", "RangeError", () => md.toPlainDate({ year: 300000 }));

        const g = new MD(5, 2, "gregory");
        check("gregory toPlainDate", "2020-05-02[u-ca=gregory]", () => g.toPlainDate({ year: 2020 }));
        check("gregory toPlainDate era", "2020-05-02[u-ca=gregory]", () => g.toPlainDate({ era: "ce", eraYear: 2020 }));
        check("gregory toPlainDate era only", "TypeError", () => g.toPlainDate({ era: "ce" }));
        check("gregory toPlainDate eraYear only", "TypeError", () => g.toPlainDate({ eraYear: 2020 }));
        check("gregory toPlainDate era infinity", "RangeError", () => g.toPlainDate({ era: "ce", eraYear: Infinity }));
        check("gregory toPlainDate unknown era", "RangeError", () => g.toPlainDate({ era: "nonsense", eraYear: 5 }));
        check("gregory toPlainDate no year", "TypeError", () => g.toPlainDate({}));
        check("gregory leap day", "2019-02-28[u-ca=gregory]",
              () => new MD(2, 29, "gregory").toPlainDate({ year: 2019 }));
        const hebrew = MD.from({ calendar: "hebrew", monthCode: "M05L", day: 15 });
        check("hebrew leap month in leap year", "M05L", () => hebrew.toPlainDate({ year: 5784 }).monthCode);
        check("hebrew leap month in common year constrains", "M06", () => hebrew.toPlainDate({ year: 5783 }).monthCode);
        check("hebrew calendar kept", "hebrew", () => hebrew.toPlainDate({ year: 5784 }).calendarId);
        "#,
    );
}

#[test]
fn methods_reject_foreign_receivers() {
    run_checks(
        r#"
        const proto = MD.prototype;
        const foreign = [undefined, null, 5, "05-02", {}, [], new Temporal.PlainDate(2020, 5, 1),
                         new Temporal.PlainYearMonth(2020, 5), Temporal.PlainMonthDay.prototype];
        for (const name of ["with", "equals", "toString", "toJSON", "toLocaleString", "valueOf", "toPlainDate"]) {
          for (const [index, receiver] of foreign.entries()) {
            check(name + " receiver " + index, "TypeError",
                  () => proto[name].call(receiver, { month: 5, day: 1, year: 2020 }));
          }
        }
        for (const name of ["monthCode", "day", "calendarId"]) {
          const getter = Object.getOwnPropertyDescriptor(proto, name).get;
          const getterForeign = foreign.filter((r) => !(r instanceof Temporal.PlainDate
            || r instanceof Temporal.PlainMonthDay || r instanceof Temporal.PlainYearMonth));
          for (const [index, receiver] of getterForeign.entries()) {
            check(name + " getter receiver " + index, "TypeError", () => getter.call(receiver));
          }
        }
        check("method lengths", "1,1,0,0,0,0,1", () => [
          proto.with.length, proto.equals.length, proto.toString.length, proto.toJSON.length,
          proto.toLocaleString.length, proto.valueOf.length, proto.toPlainDate.length,
        ].join(","));
        "#,
    );
}

#[test]
fn malformed_string_input_is_rejected() {
    run_checks(
        r#"
        check("lone surrogate string", "RangeError", () => MD.from("\uD800"));
        check("lone surrogate string with options", "RangeError", () => MD.from("--05-\uDC00", { overflow: "reject" }));
        throwsSome("eraYear without era", () => MD.from({ eraYear: 2020, monthCode: "M05", day: 2, calendar: "gregory" }));
        throwsSome("with eraYear without era", () => MD.from({ monthCode: "M05", day: 2, calendar: "gregory" }).with({ eraYear: 2019 }));
        "#,
    );
}
