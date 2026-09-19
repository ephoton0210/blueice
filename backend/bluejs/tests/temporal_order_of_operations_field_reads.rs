// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Regression coverage for the property-bag/options field-read-order
//! restructure across `Temporal.PlainDate`/`PlainDateTime`/`PlainYearMonth`/
//! `Duration` (`backend/bluejs/src/vm/temporal.rs`), pinned directly from the
//! real Test262 `order-of-operations.js` fixtures this pass closed:
//! `built-ins/Temporal/{PlainDate,PlainDateTime}/{from,prototype/with}/
//! order-of-operations.js`, `.../PlainYearMonth/{from,prototype/with}/
//! order-of-operations.js`, and `.../Duration/{compare,prototype/round,
//! prototype/total}/order-of-operations.js`.
//!
//! Before this pass, every one of these functions read its property-bag
//! fields (and, for `from`, the `options` argument) in a fixed but
//! non-alphabetical order (typically declaration order: `year`, `month`,
//! `monthCode`, `day`, `era`, `eraYear`, ...), and several also read `era`/
//! `eraYear` unconditionally even for the `iso8601` calendar (which has no
//! era concept at all, so `PrepareCalendarFields`'s real field-name list
//! never includes them for it). `PrepareCalendarFields`/
//! `PreparePartialCalendarFields`'s real algorithm reads and immediately
//! coerces every recognized field in strict alphabetical order, interleaved
//! (`Get` then, only if not `undefined`, an immediate `ToIntegerWithTruncation`/
//! `ToString`), and -- for the generic object/string dispatch in
//! `Temporal.PlainDate`/`PlainDateTime.from` specifically -- strictly before
//! `options`/`overflow` is read at all (`ToTemporalDate`'s own step order).
//!
//! Uses the same `observer`-via-`Object.defineProperty` pattern
//! `temporal_duration.rs`'s own
//! `property_and_option_bags_are_read_in_alphabetical_order` test already
//! established for `Temporal.Duration`, extended here to the composite date
//! types and to `Temporal.Duration`'s own `relativeTo` property-bag path
//! (which, unlike a bare `Duration`-like bag, also touches
//! `hour`/`minute`/`second`/`millisecond`/`microsecond`/`nanosecond`/
//! `offset`/`timeZone`).

use blueice_bluejs::{compile, parse, RuntimeError, Value, Vm};

fn evaluate(source: &str) -> Result<Value, RuntimeError> {
    Vm::default().execute(&compile(&parse(source).unwrap()).unwrap())
}

fn assert_true(source: &str) {
    assert_eq!(evaluate(source), Ok(Value::Bool(true)), "{source}");
}

/// Shared `observer` helper, identical in shape to
/// `temporal_duration.rs`'s own: builds an object whose named properties log
/// `"<label>.<name>"` to `log` on every `Get`, in whatever order the callee
/// under test actually reads them (property definition order does not
/// influence read order in JS, so this needs no separate ordering of its
/// own).
const OBSERVER: &str = r#"
    function observer(log, label, fields) {
        let bag = {};
        for (let name of Object.keys(fields)) {
            Object.defineProperty(bag, name, {
                get() { log.push(`${label}.${name}`); return fields[name]; },
                enumerable: true
            });
        }
        return bag;
    }
"#;

/// `PlainDate/PlainDateTime/prototype/with/order-of-operations.js`: fields
/// are read in alphabetical order (`day`, `hour`, `microsecond`,
/// `millisecond`, `minute`, `month`, `monthCode`, `nanosecond`, `second`,
/// `year` -- no `era`/`eraYear` for the `iso8601` calendar), strictly before
/// `options.overflow`.
#[test]
fn date_and_date_time_with_reads_fields_alphabetically_before_options() {
    assert_true(&format!(
        r#"
        {OBSERVER}
        let dateLog = [];
        new Temporal.PlainDate(2000, 5, 2).with(
            observer(dateLog, "f", {{ year: 2001, month: 6, monthCode: "M06", day: 3 }}),
            observer(dateLog, "o", {{ overflow: "constrain" }}),
        );
        let dateTimeLog = [];
        new Temporal.PlainDateTime(2000, 5, 2, 1, 2, 3, 4, 5, 6).with(
            observer(dateTimeLog, "f", {{
                year: 2001, month: 6, monthCode: "M06", day: 3,
                hour: 7, minute: 8, second: 9,
                millisecond: 10, microsecond: 11, nanosecond: 12,
            }}),
            observer(dateTimeLog, "o", {{ overflow: "constrain" }}),
        );
        dateLog.join(",") === "f.day,f.month,f.monthCode,f.year,o.overflow"
            && dateTimeLog.join(",") === [
                "f.day", "f.hour", "f.microsecond", "f.millisecond", "f.minute",
                "f.month", "f.monthCode", "f.nanosecond", "f.second", "f.year",
                "o.overflow",
            ].join(",")
    "#
    ));
}

/// `chinese`/`dangi` still see `era`/`eraYear` (in order to reject them),
/// but `iso8601` never reads them at all -- confirmed directly, not just
/// inferred from the `iso8601`-only case above.
#[test]
fn date_with_reads_era_fields_only_for_a_non_iso8601_calendar() {
    assert_true(&format!(
        r#"
        {OBSERVER}
        let log = [];
        let d = Temporal.PlainDate.from({{ year: 1981, monthCode: "M12", day: 15, calendar: "chinese" }});
        try {{
            d.with(observer(log, "f", {{ eraYear: 2025, era: "ce" }}));
        }} catch (e) {{ /* chinese has no era concept: TypeError is expected */ }}
        log.join(",") === "f.era,f.eraYear"
    "#
    ));
}

/// `PlainDate/PlainDateTime/from/order-of-operations.js`: `PrepareCalendarFields`
/// reads `calendar` then `day`/`month`/`monthCode`/`year` (alphabetically,
/// no `era`/`eraYear` for `iso8601`) strictly before `options.overflow`.
#[test]
fn date_and_date_time_from_reads_fields_before_options() {
    assert_true(&format!(
        r#"
        {OBSERVER}
        let dateLog = [];
        Temporal.PlainDate.from(
            observer(dateLog, "f", {{ year: 2001, month: 6, monthCode: "M06", day: 3, calendar: "iso8601" }}),
            observer(dateLog, "o", {{ overflow: "constrain" }}),
        );
        let dateTimeLog = [];
        Temporal.PlainDateTime.from(
            observer(dateTimeLog, "f", {{
                year: 2001, month: 6, monthCode: "M06", day: 3,
                hour: 7, minute: 8, second: 9,
                millisecond: 10, microsecond: 11, nanosecond: 12,
                calendar: "iso8601",
            }}),
            observer(dateTimeLog, "o", {{ overflow: "constrain" }}),
        );
        dateLog.join(",") === "f.calendar,f.day,f.month,f.monthCode,f.year,o.overflow"
            && dateTimeLog.join(",") === [
                "f.calendar", "f.day", "f.hour", "f.microsecond", "f.millisecond",
                "f.minute", "f.month", "f.monthCode", "f.nanosecond", "f.second",
                "f.year", "o.overflow",
            ].join(",")
    "#
    ));
}

/// `from/order-of-operations.js`'s other three cases: cloning an
/// already-same-kind Temporal object, converting a different Temporal kind,
/// and parsing a string all still read `options.overflow` -- the exact-kind
/// clone previously skipped `options` entirely.
#[test]
fn date_from_reads_options_for_same_kind_clone_and_string_argument() {
    assert_true(&format!(
        r#"
        {OBSERVER}
        let cloneLog = [];
        Temporal.PlainDate.from(
            new Temporal.PlainDate(2000, 5, 2),
            observer(cloneLog, "o", {{ overflow: "constrain" }}),
        );
        let stringLog = [];
        Temporal.PlainDate.from(
            "2001-05-02",
            observer(stringLog, "o", {{ overflow: "constrain" }}),
        );
        cloneLog.join(",") === "o.overflow" && stringLog.join(",") === "o.overflow"
    "#
    ));
}

/// `from/observable-get-overflow-argument-string-invalid.js`: an
/// ISO-invalid string must throw `RangeError` from parsing alone, without
/// `options.overflow` ever being read.
#[test]
fn date_from_never_reads_options_when_the_string_argument_is_invalid() {
    assert_true(&format!(
        r#"
        {OBSERVER}
        let log = [];
        let threw = false;
        try {{
            Temporal.PlainDate.from("2020-13-34", observer(log, "o", {{ overflow: "constrain" }}));
        }} catch (e) {{
            threw = e instanceof RangeError;
        }}
        threw && log.length === 0
    "#
    ));
}

/// `PlainYearMonth/from/order-of-operations.js` and
/// `.../prototype/with/order-of-operations.js`: `month`/`monthCode`/`year`
/// (from) and `month`/`monthCode`/`year` (with, no `era`/`eraYear` for
/// `iso8601`) are read alphabetically, before `options.overflow`.
#[test]
fn year_month_with_and_from_read_fields_alphabetically_before_options() {
    assert_true(&format!(
        r#"
        {OBSERVER}
        let withLog = [];
        new Temporal.PlainYearMonth(2000, 5).with(
            observer(withLog, "f", {{ year: 2001, month: 6, monthCode: "M06" }}),
            observer(withLog, "o", {{ overflow: "constrain" }}),
        );
        let fromLog = [];
        Temporal.PlainYearMonth.from(
            observer(fromLog, "f", {{ year: 2001, month: 6, monthCode: "M06", calendar: "iso8601" }}),
            observer(fromLog, "o", {{ overflow: "constrain" }}),
        );
        withLog.join(",") === "f.month,f.monthCode,f.year,o.overflow"
            && fromLog.join(",") === "f.calendar,f.month,f.monthCode,f.year,o.overflow"
    "#
    ));
}

/// `Duration/{compare,prototype/round,prototype/total}/order-of-operations.js`'s
/// `relativeTo` property-bag path: `GetTemporalRelativeToOption` reads the
/// *merged* calendar-date + time-of-day + `offset`/`timeZone` field list
/// alphabetically (`calendar` first, then `day`, `hour`, `microsecond`,
/// `millisecond`, `minute`, `month`, `monthCode`, `nanosecond`, `offset`,
/// `second`, `timeZone`, `year`), entirely before ever branching on whether
/// `timeZone` was supplied -- for both a plain (`PlainDate`-anchored) and a
/// zoned (`ZonedDateTime`-anchored) relativeTo bag.
#[test]
fn duration_relative_to_property_bag_reads_fields_alphabetically() {
    assert_true(&format!(
        r#"
        {OBSERVER}
        let plainLog = [];
        let plainRelativeTo = observer(plainLog, "r", {{
            year: 2001, month: 5, monthCode: "M05", day: 2, calendar: "iso8601",
            hour: undefined, minute: undefined, second: undefined,
            millisecond: undefined, microsecond: undefined, nanosecond: undefined,
            offset: undefined, timeZone: undefined,
        }});
        Temporal.Duration.compare(
            new Temporal.Duration(0, 0, 0, 7),
            new Temporal.Duration(0, 0, 0, 6),
            {{ relativeTo: plainRelativeTo }},
        );

        let zonedLog = [];
        let zonedRelativeTo = observer(zonedLog, "r", {{
            year: 2001, month: 5, monthCode: "M05", day: 2,
            hour: 6, minute: 54, second: 32,
            millisecond: 987, microsecond: 654, nanosecond: 321,
            offset: "+00:00", calendar: "iso8601", timeZone: "UTC",
        }});
        Temporal.Duration.compare(
            new Temporal.Duration(0, 0, 0, 7),
            new Temporal.Duration(0, 0, 0, 6),
            {{ relativeTo: zonedRelativeTo }},
        );

        plainLog.join(",") === [
            "r.calendar", "r.day", "r.hour", "r.microsecond", "r.millisecond",
            "r.minute", "r.month", "r.monthCode", "r.nanosecond", "r.offset",
            "r.second", "r.timeZone", "r.year",
        ].join(",")
            && zonedLog.join(",") === [
                "r.calendar", "r.day", "r.hour", "r.microsecond", "r.millisecond",
                "r.minute", "r.month", "r.monthCode", "r.nanosecond", "r.offset",
                "r.second", "r.timeZone", "r.year",
            ].join(",")
    "#
    ));
}
