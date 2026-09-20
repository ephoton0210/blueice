// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Regression coverage for `Temporal.*.from()` argument, property-bag and options validation
//! (Phase 26 Stage 3 --
//! `development/browser_core/phase-26-ecma262-temporal/PLAN.md`).
//!
//! Each test states the spec rule it pins and the Test262 fixture(s) that observe it. Several of
//! these were invisible to the fixtures' own pass/fail because a fixture stops at its first
//! failing assertion; running each assertion independently shows every cause.
//!
//! - `ToTemporal{Date,DateTime,YearMonth,MonthDay,ZonedDateTime,Time}` reject every non-object,
//!   non-string argument with `TypeError` (no `ToString` coercion), and read internal slots --
//!   not the getters -- when given another Temporal object.
//! - `ToMonthCode` applies `ToPrimitive(string)`, requires a String, and validates its syntax
//!   at the moment the field is read (before a later field such as `year` is converted).
//! - `month` and `day` are `ToPositiveIntegerWithTruncation`: no upper bound, so `overflow`
//!   (not the reader) decides whether an out-of-range value constrains or throws, and a value
//!   that does not fit a byte must not wrap.
//! - `options` is read only after the item has been parsed/read, and a primitive `options`
//!   throws `TypeError` only after that.

use blueice_bluejs::{compile, parse, Value, Vm};

const PRELUDE: &str = r#"
const fails = [];
function nameOf(e) {
  if (e instanceof RangeError) return "RangeError";
  if (e instanceof TypeError) return "TypeError";
  if (e instanceof SyntaxError) return "SyntaxError";
  return "other:" + String(e);
}
// `expect(kind, label, fn)`: fn must throw exactly `kind` ("none" means it must not throw).
function expect(kind, label, f) {
  let outcome = "none";
  try { f(); } catch (e) { outcome = nameOf(e); }
  if (outcome !== kind) fails.push(label + " -> " + outcome + " (expected " + kind + ")");
}
function same(label, actual, expected) {
  if (!Object.is(actual, expected)) fails.push(label + " => " + String(actual) + " !== " + String(expected));
}
function sameArray(label, actual, expected) {
  if (actual.length !== expected.length || actual.some((v, i) => v !== expected[i])) {
    fails.push(label + " => [" + actual.join(", ") + "] !== [" + expected.join(", ") + "]");
  }
}
// Records every property read (and the coercions of a returned value) into `log`.
function observe(log, name, bag) {
  return new Proxy(bag, {
    get(target, key) {
      if (typeof key === "symbol") return undefined;
      log.push("get " + name + "." + key);
      const value = target[key];
      if (value === undefined) return value;
      return {
        toString() { log.push("call " + name + "." + key + ".toString"); return value === undefined ? value : String(value); },
        valueOf() { log.push("call " + name + "." + key + ".valueOf"); return value; },
      };
    },
    has(target, key) { return key in target; },
  });
}
const T = Temporal;
// One valid, fully-specified property bag per `from`-taking type that carries a calendar date.
const bags = {
  PlainDate: (extra) => Object.assign({ year: 2021, monthCode: "M05", day: 17 }, extra),
  PlainDateTime: (extra) => Object.assign({ year: 2021, monthCode: "M05", day: 17 }, extra),
  PlainYearMonth: (extra) => Object.assign({ year: 2021, monthCode: "M05" }, extra),
  PlainMonthDay: (extra) => Object.assign({ monthCode: "M05", day: 17 }, extra),
  ZonedDateTime: (extra) => Object.assign({ year: 2021, monthCode: "M05", day: 17, timeZone: "UTC" }, extra),
};
const bagKinds = Object.keys(bags);
"#;

fn run(body: &str) {
    let source = format!(
        "(function() {{\n{PRELUDE}\n{body}\nreturn fails.length === 0 ? true : fails.join(\"\\n\");\n}})()"
    );
    let program =
        compile(&parse(&source).expect("test script parses")).expect("test script compiles");
    match Vm::default().execute(&program) {
        Ok(Value::Bool(true)) => {}
        Ok(Value::String(text)) => panic!("failing cases:\n{}", text.to_utf8().unwrap()),
        other => panic!("unexpected result: {other:?}"),
    }
}

/// `argument-number.js` / `argument-wrong-type.js`: a non-String primitive is a `TypeError`
/// rather than being stringified and parsed (`from(19761118)` looks like a date once
/// stringified); an empty String is an ISO-syntax `RangeError`.
#[test]
fn from_rejects_every_non_string_primitive_with_type_error() {
    run(r#"
      const wrong = [["undefined", undefined], ["null", null], ["true", true], ["number 1", 1],
                     ["number 19761118", 19761118], ["number -19761118", -19761118],
                     ["bigint", 1n], ["symbol", Symbol()]];
      for (const kind of ["PlainDate", "PlainDateTime", "PlainYearMonth", "PlainMonthDay",
                          "ZonedDateTime", "PlainTime"]) {
        expect("TypeError", kind + ".from()", () => T[kind].from());
        for (const [label, arg] of wrong) {
          expect("TypeError", kind + ".from(" + label + ")", () => T[kind].from(arg));
          for (const options of [undefined, { overflow: "constrain" }, { overflow: "reject" }]) {
            expect("TypeError", kind + ".from(" + label + ", options)", () => T[kind].from(arg, options));
          }
        }
        expect("RangeError", kind + ".from('')", () => T[kind].from(""));
      }
      // A plain object with no usable fields is a TypeError (missing required field), not a
      // RangeError, and so are constructors/prototypes passed by mistake.
      for (const kind of ["PlainDate", "PlainDateTime", "PlainYearMonth", "PlainMonthDay", "ZonedDateTime"]) {
        expect("TypeError", kind + ".from({})", () => T[kind].from({}));
        expect("TypeError", kind + ".from(constructor)", () => T[kind].from(T[kind]));
        expect("TypeError", kind + ".from(prototype)", () => T[kind].from(T[kind].prototype));
      }
    "#);
}

/// `month-code-wrong-type.js`: `ToMonthCode` requires the `ToPrimitive(string)` result to be a
/// String, so a number/boolean/null/symbol -- or an object whose `toString` returns a number --
/// is a `TypeError` in every calendar-date property bag.
#[test]
fn month_code_must_be_a_string_in_every_property_bag() {
    run(r#"
      const values = [["5", 5], ["5n", 5n], ["false", false], ["symbol", Symbol()], ["null", null],
                      ["object with numeric toString", { toString: () => 5 }]];
      for (const kind of bagKinds) {
        for (const [label, monthCode] of values) {
          expect("TypeError", kind + " monthCode " + label, () => T[kind].from(bags[kind]({ monthCode })));
        }
        // An object that *does* convert to a string is fine.
        expect("none", kind + " monthCode object with string toString",
          () => T[kind].from(bags[kind]({ monthCode: { toString: () => "M05" } })));
      }
      // `with()` shares the field reader.
      for (const [label, monthCode] of values) {
        expect("TypeError", "PlainDate.with monthCode " + label, () => new T.PlainDate(2021, 5, 17).with({ monthCode }));
        expect("TypeError", "PlainDateTime.with monthCode " + label, () => new T.PlainDateTime(2021, 5, 17).with({ monthCode }));
        expect("TypeError", "PlainYearMonth.with monthCode " + label, () => new T.PlainYearMonth(2021, 5).with({ monthCode }));
        expect("TypeError", "PlainMonthDay.with monthCode " + label, () => new T.PlainMonthDay(5, 17).with({ monthCode }));
        expect("TypeError", "ZonedDateTime.with monthCode " + label, () => new T.ZonedDateTime(0n, "UTC").with({ monthCode }));
      }
    "#);
}

/// `monthcode-invalid.js`: a month code's *syntax* is validated the moment it is read -- before
/// a later field (`year`) is converted -- while its *suitability* for the calendar (`M99L`
/// is well-formed but no ISO month) is only judged once every field has been read.
#[test]
fn month_code_syntax_is_checked_when_read_and_suitability_after_every_field() {
    run(r#"
      for (const kind of bagKinds) {
        for (const monthCode of ["m1", "M1", "m01", "M001", "M", "", "L99M", "M0A", "M+1", "Mab"]) {
          expect("RangeError", kind + " malformed monthCode " + JSON.stringify(monthCode),
            () => T[kind].from(bags[kind]({ monthCode })));
        }
        if (kind !== "PlainMonthDay") {
          // Syntax is validated before `year` is converted (RangeError beats the Symbol's TypeError) ...
          expect("RangeError", kind + " syntax before year", () => T[kind].from(bags[kind]({ monthCode: "L99M", year: Symbol() })));
          // ... but suitability comes after it.
          expect("TypeError", kind + " suitability after year", () => T[kind].from(bags[kind]({ monthCode: "M99L", year: Symbol() })));
        }
        for (const monthCode of ["M00", "M13", "M19", "M99", "M00L", "M05L", "M13L"]) {
          expect("RangeError", kind + " unsuitable monthCode " + monthCode, () => T[kind].from(bags[kind]({ monthCode })));
        }
      }
      expect("RangeError", "month/monthCode conflict", () => T.PlainDate.from({ year: 2021, month: 12, monthCode: "M11", day: 17 }));
      expect("RangeError", "month/monthCode conflict (PlainYearMonth)", () => T.PlainYearMonth.from({ year: 2021, month: 12, monthCode: "M11" }));
      // `with` applies the same syntax rule.
      expect("RangeError", "PlainDate.with syntax", () => new T.PlainDate(2021, 5, 17).with({ monthCode: "m05" }));
      expect("RangeError", "ZonedDateTime.with syntax", () => new T.ZonedDateTime(0n, "UTC").with({ monthCode: "m05" }));
    "#);
}

/// `with-year-month-day-need-constrain.js` / `overflow-constrain.js`: `month` and `day` are
/// positive integers with no upper bound. `overflow: "constrain"` clamps a too-large value;
/// `"reject"` throws; zero and negatives are always a `RangeError`. A `day` over 255 must not
/// wrap around a byte (`day: 256` would become `0`).
#[test]
fn month_and_day_are_constrained_or_rejected_by_overflow_not_by_the_field_reader() {
    run(r#"
      const cases = [[1, 133, 1, 31], [2, 133, 2, 28], [13, 500, 12, 31], [999999, 500, 12, 31],
                     [3, 9033, 3, 31], [4, 256, 4, 30], [5, 257, 5, 31], [6, 511, 6, 30], [12, 2147483647, 12, 31]];
      for (const [month, day, expectedMonth, expectedDay] of cases) {
        for (const kind of ["PlainDate", "PlainDateTime"]) {
          const result = T[kind].from({ year: 2021, month, day });
          same(kind + " " + month + "/" + day + " month", result.month, expectedMonth);
          same(kind + " " + month + "/" + day + " day", result.day, expectedDay);
        }
        const zoned = T.ZonedDateTime.from({ year: 2021, month, day, timeZone: "UTC" });
        same("ZonedDateTime " + month + "/" + day + " month", zoned.month, expectedMonth);
        same("ZonedDateTime " + month + "/" + day + " day", zoned.day, expectedDay);
        const yearMonth = T.PlainYearMonth.from({ year: 2021, month });
        same("PlainYearMonth month " + month, yearMonth.month, expectedMonth);
        const monthDay = T.PlainMonthDay.from({ month, day, year: 2021 });
        same("PlainMonthDay " + month + "/" + day + " month", monthDay.monthCode, "M" + String(expectedMonth).padStart(2, "0"));
        same("PlainMonthDay " + month + "/" + day + " day", monthDay.day, expectedDay);
      }
      for (const kind of bagKinds) {
        for (const [month, day] of [[13, 1], [999999, 1], [1, 32], [1, 256], [2, 30]]) {
          if (kind === "PlainYearMonth" && day !== 1) continue;
          const bag = kind === "PlainYearMonth" ? { year: 2021, month }
            : kind === "PlainMonthDay" ? { year: 2021, month, day }
            : bags[kind]({ monthCode: undefined, month, day });
          expect("RangeError", kind + " reject " + month + "/" + day, () => T[kind].from(bag, { overflow: "reject" }));
        }
        for (const month of [0, -1, -99999]) {
          const bag = kind === "PlainMonthDay" ? { year: 2021, month, day: 1 }
            : kind === "PlainYearMonth" ? { year: 2021, month }
            : bags[kind]({ monthCode: undefined, month });
          expect("RangeError", kind + " month " + month + " constrain", () => T[kind].from(bag, { overflow: "constrain" }));
        }
        for (const day of [0, -1]) {
          if (kind === "PlainYearMonth") continue;
          const bag = kind === "PlainMonthDay" ? { year: 2021, month: 1, day } : bags[kind]({ day });
          expect("RangeError", kind + " day " + day + " constrain", () => T[kind].from(bag, { overflow: "constrain" }));
        }
      }
    "#);
}

/// `PlainDateTime/from/leap-second.js`: `second: 60` in a property bag is constrained to 59 by
/// default and is a `RangeError` with `overflow: "reject"`. A leap second in an ISO *string* is
/// always accepted (clamped), as is `second: 60` where the time is discarded (`PlainDate`).
#[test]
fn property_bag_second_60_follows_overflow() {
    run(r#"
      const bag = { year: 2016, month: 12, day: 31, hour: 23, minute: 59, second: 60 };
      same("PlainDateTime constrain", T.PlainDateTime.from(bag).second, 59);
      same("PlainDateTime explicit constrain", T.PlainDateTime.from(bag, { overflow: "constrain" }).second, 59);
      expect("RangeError", "PlainDateTime reject", () => T.PlainDateTime.from(bag, { overflow: "reject" }));
      const zoned = Object.assign({ timeZone: "UTC" }, bag);
      same("ZonedDateTime constrain", T.ZonedDateTime.from(zoned).second, 59);
      expect("RangeError", "ZonedDateTime reject", () => T.ZonedDateTime.from(zoned, { overflow: "reject" }));
      // `second: 61` is out of range either way once `overflow` is "reject"; constrain clamps it.
      same("second 61 constrain", T.PlainDateTime.from(Object.assign({}, bag, { second: 61 })).second, 59);
      expect("RangeError", "second 61 reject", () => T.PlainDateTime.from(Object.assign({}, bag, { second: 61 }), { overflow: "reject" }));
      same("string leap second", T.PlainDateTime.from("2016-12-31T23:59:60").second, 59);
      same("string leap second reject", T.PlainDateTime.from("2016-12-31T23:59:60", { overflow: "reject" }).second, 59);
      // `PlainDate` ignores the time-of-day fields entirely.
      same("PlainDate bag", T.PlainDate.from(bag, { overflow: "reject" }).day, 31);
      same("PlainDate string", T.PlainDate.from("2016-12-31T23:59:60").day, 31);
    "#);
}

/// `PlainDate/compare/leap-second.js`: a `PlainDate` parsed from a date-time string must carry
/// no time of day, or two equal dates compare unequal.
#[test]
fn plain_date_from_a_date_time_string_discards_the_time() {
    run(r#"
      const fromString = T.PlainDate.from("2016-12-31T23:59:60");
      same("compare leap second", T.PlainDate.compare("2016-12-31T23:59:60", new T.PlainDate(2016, 12, 31)), 0);
      same("compare leap second (second)", T.PlainDate.compare(new T.PlainDate(2016, 12, 31), "2016-12-31T23:59:60"), 0);
      same("compare bag", T.PlainDate.compare({ year: 2016, month: 12, day: 31, hour: 23, minute: 59, second: 60 }, new T.PlainDate(2016, 12, 31)), 0);
      same("equals", fromString.equals(new T.PlainDate(2016, 12, 31)), true);
      same("toPlainDateTime", fromString.toPlainDateTime().toString(), "2016-12-31T00:00:00");
      same("toZonedDateTime", fromString.toZonedDateTime("UTC").toString(), "2016-12-31T00:00:00+00:00[UTC]");
      const yearMonth = T.PlainYearMonth.from("2016-12-31T23:59:59");
      same("PlainYearMonth equals", yearMonth.equals(new T.PlainYearMonth(2016, 12)), true);
      const monthDay = T.PlainMonthDay.from("2016-12-31T23:59:59");
      same("PlainMonthDay equals", monthDay.equals(T.PlainMonthDay.from("12-31")), true);
    "#);
}

/// `argument-plaindatetime.js` / `argument-plaindate.js` / `argument-zoneddatetime-slots.js`:
/// converting one Temporal object to another reads internal slots only. No getter
/// (`year`, `calendar`, ...) is called, and it must work for a named time zone too.
#[test]
fn converting_between_temporal_types_reads_slots_not_getters() {
    run(r#"
            const trapped = ["year", "month", "monthCode", "day", "hour", "minute", "second", "millisecond",
                       "microsecond", "nanosecond", "calendar", "calendarId", "era", "eraYear", "timeZone"];
      const log = [];
      const saved = [];
      for (const type of [T.PlainDate, T.PlainDateTime, T.ZonedDateTime, T.PlainYearMonth, T.PlainMonthDay]) {
        for (const name of trapped) {
          const d = Object.getOwnPropertyDescriptor(type.prototype, name);
          saved.push([type.prototype, name, d]);
          Object.defineProperty(type.prototype, name, { get() { log.push(type.prototype.constructor.name + "." + name); return undefined; }, configurable: true });
        }
      }
      const date = new T.PlainDate(2000, 5, 2);
      const dateTime = new T.PlainDateTime(2000, 5, 2, 12, 30);
      const zoned = new T.ZonedDateTime(957270896987654321n, "UTC");
      const named = new T.ZonedDateTime(957270896987654321n, "America/New_York");
      const results = {
        dateFromDateTime: T.PlainDate.from(dateTime),
        dateFromZoned: T.PlainDate.from(zoned),
        dateFromNamed: T.PlainDate.from(named),
        dateFromDate: T.PlainDate.from(date),
        dateTimeFromDate: T.PlainDateTime.from(date),
        dateTimeFromZoned: T.PlainDateTime.from(zoned),
        dateTimeFromNamed: T.PlainDateTime.from(named),
        dateTimeFromDateTime: T.PlainDateTime.from(dateTime),
      };
      for (const [proto, name, d] of saved) { if (d) Object.defineProperty(proto, name, d); else delete proto[name]; }
      sameArray("getters called", log, []);
      same("dateFromDateTime", results.dateFromDateTime.toString(), "2000-05-02");
      same("dateFromZoned", results.dateFromZoned.toString(), "2000-05-02");
      same("dateFromNamed", results.dateFromNamed.toString(), "2000-05-02");
      same("dateFromDate", results.dateFromDate.toString(), "2000-05-02");
      same("dateTimeFromDate", results.dateTimeFromDate.toString(), "2000-05-02T00:00:00");
      same("dateTimeFromZoned", results.dateTimeFromZoned.toString(), "2000-05-02T12:34:56.987654321");
      same("dateTimeFromNamed", results.dateTimeFromNamed.toString(), "2000-05-02T08:34:56.987654321");
      same("dateTimeFromDateTime", results.dateTimeFromDateTime.toString(), "2000-05-02T12:30:00");
      // The conversion keeps the source calendar.
      same("calendar kept", T.PlainDate.from(new T.PlainDateTime(2000, 5, 2, 0, 0, 0, 0, 0, 0, "gregory")).calendarId, "gregory");
      // `PlainYearMonth`/`PlainMonthDay` have no slot fast path: they read the argument's fields.
      same("year month from date", T.PlainYearMonth.from(date).toString(), "2000-05");
      same("month day from date", T.PlainMonthDay.from(date).toString(), "05-02");
    "#);
}

/// `observable-get-overflow-argument-primitive.js`, `observable-get-overflow-argument-string-
/// invalid.js`, `options-wrong-type.js`: `options` is read after the argument has been parsed,
/// so an invalid string throws its `RangeError` with `options` untouched, and a non-object
/// `options` throws `TypeError` only for an argument that would otherwise have been accepted.
#[test]
fn options_are_read_after_the_argument_is_parsed() {
    run(r#"
      const valid = { PlainDate: "2021-05-17", PlainDateTime: "2021-05-17T12:00", PlainYearMonth: "2021-05",
                      PlainMonthDay: "05-17", ZonedDateTime: "2021-05-17T12:00+00:00[UTC]" };
      const invalid = { PlainDate: "2021-05-32", PlainDateTime: "2021-05-17T25:00", PlainYearMonth: "2020-13",
                        PlainMonthDay: "13-01", ZonedDateTime: "2021-05-17T12:00" };
      const badOptions = [null, true, "some string", Symbol(), 1, 2n];
      for (const kind of bagKinds) {
        const log = [];
        const options = observe(log, "options", { overflow: "constrain", disambiguation: "compatible", offset: "prefer" });
        expect("RangeError", kind + " invalid string", () => T[kind].from(invalid[kind], options));
        sameArray(kind + " options untouched for an invalid string", log, []);
        log.length = 0;
        expect("TypeError", kind + " primitive argument", () => T[kind].from(7, options));
        sameArray(kind + " options untouched for a primitive argument", log, []);
        log.length = 0;
        T[kind].from(valid[kind], options);
        if (log.length === 0) fails.push(kind + " valid string never read options");
        for (const value of badOptions) {
          expect("RangeError", kind + " invalid string beats " + typeof value + " options", () => T[kind].from(invalid[kind], value));
          expect("TypeError", kind + " valid string + " + typeof value + " options", () => T[kind].from(valid[kind], value));
          expect("TypeError", kind + " property bag + " + typeof value + " options", () => T[kind].from(bags[kind](), value));
          expect("TypeError", kind + " clone + " + typeof value + " options", () => T[kind].from(T[kind].from(valid[kind]), value));
        }
      }
    "#);
}

/// `ZonedDateTime/argument-convert.js`: the constructor's `epochNanoseconds` is `ToBigInt`, so a
/// Boolean is `0n`/`1n`, a numeric string parses, and a Number, `undefined` or Symbol is a
/// `TypeError` (a non-integer String is a `SyntaxError`).
#[test]
fn zoned_date_time_constructor_converts_epoch_nanoseconds_with_to_bigint() {
    run(r#"
      same("false", new T.ZonedDateTime(false, "UTC").epochNanoseconds, 0n);
      same("true", new T.ZonedDateTime(true, "UTC").epochNanoseconds, 1n);
      same("string", new T.ZonedDateTime("5", "UTC").epochNanoseconds, 5n);
      same("object with toString", new T.ZonedDateTime({ valueOf() { return 7n; } }, "UTC").epochNanoseconds, 7n);
      expect("TypeError", "undefined", () => new T.ZonedDateTime(undefined, "UTC"));
      expect("TypeError", "null", () => new T.ZonedDateTime(null, "UTC"));
      expect("TypeError", "symbol", () => new T.ZonedDateTime(Symbol(), "UTC"));
      expect("TypeError", "number", () => new T.ZonedDateTime(1, "UTC"));
      expect("SyntaxError", "non-integer string", () => new T.ZonedDateTime("1.5", "UTC"));
      expect("RangeError", "out of range", () => new T.ZonedDateTime(8640000000000000000001n, "UTC"));
    "#);
}

/// `ZonedDateTime/timezone-iso-string.js`: the constructor's `timeZone` is a bare identifier,
/// so an ISO string that merely *contains* one is a `RangeError` (`from` and friends do accept
/// such strings, through `ToTemporalTimeZoneIdentifier`).
#[test]
fn zoned_date_time_constructor_time_zone_is_a_bare_identifier() {
    run(r#"
      for (const timeZone of ["1997-12-04T12:34[+01:00]", "2000-01-01T00:00Z", "1997-12-04T12:34+01:00[UTC]", "UTC[u-ca=iso8601]"]) {
        expect("RangeError", "constructor " + timeZone, () => new T.ZonedDateTime(0n, timeZone, "iso8601"));
      }
      expect("TypeError", "non-string time zone", () => new T.ZonedDateTime(0n, 5));
      expect("TypeError", "undefined time zone", () => new T.ZonedDateTime(0n, undefined));
      same("bare identifier", new T.ZonedDateTime(0n, "UTC").timeZoneId, "UTC");
      same("offset identifier", new T.ZonedDateTime(0n, "+01:00").timeZoneId, "+01:00");
      same("iana identifier", new T.ZonedDateTime(0n, "America/New_York").timeZoneId, "America/New_York");
      // `from` accepts an ISO string with a bracketed zone as the *property-bag* `timeZone`.
      same("from bag with an ISO string time zone", T.ZonedDateTime.from({ year: 2000, month: 1, day: 1, timeZone: "2000-01-01T00:00[+01:00]" }).timeZoneId, "+01:00");
    "#);
}

/// `PlainDateTime/prototype/with/overflow-undefined.js`,
/// `ZonedDateTime/prototype/with/overflow-{options,undefined}.js`: `with` reads its time fields
/// exactly as `from` does -- no range while reading, `RegulateTime` afterwards -- so the default
/// `constrain` clamps (a `minute` of 67 is 59, an `hour` of -1 is 0) and `reject` throws. A `day`
/// past a byte must be constrained to the month's last day, not wrapped (`256` is `0` as a `u8`).
#[test]
fn with_regulates_time_fields_and_days_by_overflow() {
    run(r#"
      const dateTime = new T.PlainDateTime(2000, 5, 2, 12);
      for (const options of [undefined, {}, { overflow: undefined }, () => {}, { overflow: "constrain" }]) {
        same("PlainDateTime minute 67", dateTime.with({ minute: 67 }, options).minute, 59);
      }
      same("PlainDateTime hour 24", dateTime.with({ hour: 24 }).hour, 23);
      same("PlainDateTime hour -1", dateTime.with({ hour: -1 }).hour, 0);
      same("PlainDateTime second 60", dateTime.with({ second: 60 }).second, 59);
      same("PlainDateTime nanosecond 9000", dateTime.with({ nanosecond: 9000 }).nanosecond, 999);
      expect("RangeError", "PlainDateTime hour 24 reject", () => dateTime.with({ hour: 24 }, { overflow: "reject" }));
      expect("RangeError", "PlainDateTime second 60 reject", () => dateTime.with({ second: 60 }, { overflow: "reject" }));
      expect("RangeError", "PlainDateTime hour Infinity", () => dateTime.with({ hour: Infinity }));
      const zoned = new T.PlainDateTime(1976, 11, 18, 15, 23, 30, 123, 456, 789).toZonedDateTime("UTC");
      same("ZonedDateTime hour 29", zoned.with({ hour: 29 }, { overflow: "constrain" }).hour, 23);
      same("ZonedDateTime hour 29 by default", zoned.with({ hour: 29 }).hour, 23);
      same("ZonedDateTime nanosecond 9000", zoned.with({ nanosecond: 9000 }).nanosecond, 999);
      same("ZonedDateTime month 29", zoned.with({ month: 29 }).month, 12);
      same("ZonedDateTime day 31", zoned.with({ day: 31 }).day, 30);
      expect("RangeError", "ZonedDateTime hour 29 reject", () => zoned.with({ hour: 29 }, { overflow: "reject" }));
      // A day past a byte constrains instead of wrapping.
      for (const day of [256, 257, 300, 2147483647]) {
        same("PlainDate day " + day, new T.PlainDate(2021, 2, 1).with({ day }).day, 28);
        same("PlainDateTime day " + day, dateTime.with({ day }).day, 31);
        same("ZonedDateTime day " + day, zoned.with({ day }).day, 30);
        expect("RangeError", "PlainDate day " + day + " reject", () => new T.PlainDate(2021, 2, 1).with({ day }, { overflow: "reject" }));
      }
      // A time-field-only `with` on a `PlainDate` has nothing to change.
      expect("TypeError", "PlainDate.with time field only", () => new T.PlainDate(2021, 2, 1).with({ hour: 5 }));
    "#);
}

/// `PlainDate|PlainDateTime|ZonedDateTime/prototype/withCalendar/missing-argument.js`: the
/// argument is required -- `undefined` is a `TypeError`, not the ISO default that an absent
/// property-bag `calendar` field means.
#[test]
fn with_calendar_requires_its_argument() {
    run(r#"
      for (const value of [new T.PlainDate(2000, 5, 2), new T.PlainDateTime(2000, 5, 2), new T.ZonedDateTime(0n, "UTC")]) {
        const name = value.constructor.name;
        expect("TypeError", name + ".withCalendar()", () => value.withCalendar());
        expect("TypeError", name + ".withCalendar(undefined)", () => value.withCalendar(undefined));
        expect("TypeError", name + ".withCalendar(null)", () => value.withCalendar(null));
        expect("TypeError", name + ".withCalendar(1)", () => value.withCalendar(1));
        expect("RangeError", name + ".withCalendar('nonsense')", () => value.withCalendar("nonsense"));
        same(name + ".withCalendar('gregory')", value.withCalendar("gregory").calendarId, "gregory");
        same(name + ".withCalendar('iso8601')", value.withCalendar("iso8601").calendarId, "iso8601");
      }
    "#);
}
