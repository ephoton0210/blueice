// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Regression coverage for `Temporal.PlainYearMonth` operations that resolve a month to a *date*
//! (Phase 26 Stage 3 -- `development/browser_core/phase-26-ecma262-temporal/PLAN.md`).
//!
//! A `PlainYearMonth` is valid from `-271821-04` to `+275760-09`, but the dates inside those two
//! months are not all valid: the earliest date is `-271821-04-19` and the latest `+275760-09-13`.
//! `toPlainDate`, `add`/`subtract` (which start from the month's first day) and `since`/`until`
//! (which difference the two months' first days) therefore throw `RangeError` at the edges,
//! after -- never instead of -- reading `options`. Two identical year-months still differ by a
//! blank duration, because that comparison happens before either is turned into a date.

use blueice_bluejs::{compile, parse, Value, Vm};

const PRELUDE: &str = r#"
const fails = [];
function nameOf(e) {
  if (e instanceof RangeError) return "RangeError";
  if (e instanceof TypeError) return "TypeError";
  return "other:" + String(e);
}
function expect(kind, label, f) {
  let outcome = "none";
  try { f(); } catch (e) { outcome = nameOf(e); }
  if (outcome !== kind) fails.push(label + " -> " + outcome + " (expected " + kind + ")");
}
function same(label, actual, expected) {
  if (!Object.is(actual, expected)) fails.push(label + " => " + String(actual) + " !== " + String(expected));
}
const T = Temporal;
const min = new T.PlainYearMonth(-271821, 4);
const max = new T.PlainYearMonth(275760, 9);
const epoch = new T.PlainYearMonth(1970, 1);
const blank = new T.Duration();
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

/// `PlainYearMonth/prototype/toPlainDate/limits.js`.
#[test]
fn to_plain_date_rejects_days_outside_the_representable_dates() {
    run(r#"
      expect("RangeError", "min day 18", () => min.toPlainDate({ day: 18 }));
      same("min day 19", min.toPlainDate({ day: 19 }).toString(), "-271821-04-19");
      same("min day 30", min.toPlainDate({ day: 30 }).toString(), "-271821-04-30");
      expect("RangeError", "max day 14", () => max.toPlainDate({ day: 14 }));
      expect("RangeError", "max day 30 (constrained to the month's end)", () => max.toPlainDate({ day: 30 }));
      same("max day 13", max.toPlainDate({ day: 13 }).toString(), "+275760-09-13");
      same("an ordinary month", epoch.toPlainDate({ day: 15 }).toString(), "1970-01-15");
    "#);
}

/// `add`/`subtract` `throws-if-year-outside-valid-iso-range.js` and
/// `options-read-before-algorithmic-validation.js`.
#[test]
fn add_and_subtract_need_a_valid_first_day_and_read_options_first() {
    run(r#"
      for (const method of ["add", "subtract"]) {
        expect("RangeError", "min." + method + "(blank)", () => min[method](blank));
        expect("RangeError", "min." + method + "({ months: 1 })", () => min[method]({ months: 1 }));
        expect("none", "max." + method + "(blank)", () => max[method](blank));
        expect("none", "epoch." + method + "(blank)", () => epoch[method](blank));
        // `options` is read in full before the first day is validated.
        const log = [];
        const options = new Proxy({ overflow: "constrain" }, {
          get(target, key) { if (typeof key !== "symbol") log.push("get " + key); return target[key]; },
        });
        expect("RangeError", "options first (" + method + ")", () => min[method](blank, options));
        if (log.join() !== "get overflow") fails.push(method + " read " + log.join() + " instead of overflow");
      }
      same("min + 1 month", new T.PlainYearMonth(-271821, 5).subtract({ months: 0 }).toString(), "-271821-05");
      same("epoch + 13 months", epoch.add({ months: 13 }).toString(), "1971-02");
    "#);
}

/// `add/options-read-before-algorithmic-validation.js`: a duration with a week, day or time
/// part is a `RangeError` too, but `options` is read in full first -- the unit check is
/// "algorithmic validation" like the range check above.
#[test]
fn add_and_subtract_read_options_before_rejecting_a_too_small_unit() {
    run(r#"
      const instance = new T.PlainYearMonth(1999, 12);
      for (const method of ["add", "subtract"]) {
        for (const duration of [new T.Duration(0, 0, 1), new T.Duration(0, 0, 0, 1), new T.Duration(0, 0, 0, 0, 1),
                                new T.Duration(0, 0, 0, 0, 0, 1), new T.Duration(0, 0, 0, 0, 0, 0, 1),
                                new T.Duration(0, 0, 0, 0, 0, 0, 0, 1), new T.Duration(0, 0, 0, 0, 0, 0, 0, 0, 1),
                                new T.Duration(0, 0, 0, 0, 0, 0, 0, 0, 0, 1)]) {
          const log = [];
          const options = new Proxy({ overflow: "constrain" }, {
            get(target, key) { if (typeof key !== "symbol") log.push("get " + key); return target[key]; },
          });
          expect("RangeError", method + " " + duration.toString(), () => instance[method](duration, options));
          if (log.join() !== "get overflow") fails.push(method + " " + duration.toString() + " read [" + log.join() + "]");
        }
        // A years/months duration is accepted; an invalid `overflow` value still throws.
        same(method + " months", instance[method]({ months: 1 }).toString(), method === "add" ? "2000-01" : "1999-11");
        expect("RangeError", method + " bad overflow", () => instance[method]({ months: 1 }, { overflow: "sometimes" }));
        expect("TypeError", method + " primitive options", () => instance[method]({ months: 1 }, 5));
      }
    "#);
}

/// `since`/`until` `throws-if-year-outside-valid-iso-range.js` and `argument-string-limits.js`.
#[test]
fn since_and_until_difference_valid_first_days_only() {
    run(r#"
      for (const method of ["since", "until"]) {
        // Identical year-months differ by nothing, even at the invalid edge.
        same(method + " min/min", min[method](min).toString(), "PT0S");
        same(method + " max/max", max[method](max).toString(), "PT0S");
        expect("RangeError", "min." + method + "(max)", () => min[method](max));
        expect("RangeError", "min." + method + "(epoch)", () => min[method](epoch));
        expect("RangeError", "epoch." + method + "(min)", () => epoch[method](min));
        expect("none", "epoch." + method + "(max)", () => epoch[method](max));
        // A difference between two valid first days is unaffected.
        same(method + " ordinary", epoch[method]("1972-03").years, method === "since" ? -2 : 2);
        // The string form of the same rule: the argument's first day must be a valid date.
        for (const arg of ["-271821-05", "-271821-05-01", "-271821-05-01T00:00", "+275760-09",
                           "+275760-09-30", "+275760-09-30T23:59:59.999999999"]) {
          expect("none", method + " " + arg, () => epoch[method](arg));
        }
        for (const arg of ["-271821-04", "-271821-04-30", "-271821-04-30T23:59:59.999999999",
                           "+275760-10", "+275760-10-01", "+275760-10-01T00:00"]) {
          expect("RangeError", method + " " + arg, () => epoch[method](arg));
        }
      }
    "#);
}
