// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Regression coverage for a real, systematic Temporal gap: receiver (`this`)
//! brand checks (Phase 26 Stage 3 --
//! `development/browser_core/phase-26-ecma262-temporal/PLAN.md`).
//!
//! Every Temporal prototype getter and method begins with
//! `RequireInternalSlot(this, [[InitializedTemporal<Type>]])`, so a receiver
//! that is not *exactly* that type must throw `TypeError` -- before any
//! argument is read or coerced. That includes the *other* Temporal types: a
//! `PlainDateTime` is not a `PlainDate`, a `ZonedDateTime` is not an
//! `Instant`.
//!
//! BlueJS implements several prototype members as one native function shared
//! by more than one Temporal prototype (`TemporalGetter` serves all eight,
//! `TemporalDate*` and `TemporalWithCalendar`/`TemporalPlainToZonedDateTime`
//! serve `PlainDate` *and* `PlainDateTime`) and dispatches on the receiver's
//! own `TemporalKind` instead of on the prototype the function was reached
//! through. The shared native therefore accepted any receiver in a superset of
//! kinds: `Object.getOwnPropertyDescriptor(Temporal.PlainDate.prototype,
//! "year").get.call(zonedDateTime)` returned a year, `calendarId` accepted
//! every Temporal type including `Duration`, `Temporal.PlainTime.prototype`'s
//! `hour` accepted a `PlainDateTime`, and `PlainDate.prototype.add` on a
//! `PlainDateTime` read its argument's fields before (or instead of) throwing.
//!
//! Test262's own `*/prototype/*/branding.js` fixtures only pass `undefined`,
//! primitives, `{}`, the constructor and the prototype, none of which reach
//! that superset -- so the gap was invisible to the conformance run. The
//! audit below enumerates every own member of every Temporal prototype
//! instead of listing the known offenders, so a newly added member cannot
//! silently skip its brand check.

use blueice_bluejs::{compile, parse, Value, Vm};

const PRELUDE: &str = r#"
const fails = [];
const stats = { members: 0, calls: 0 };
function nameOf(e) {
  if (e instanceof RangeError) return "RangeError";
  if (e instanceof TypeError) return "TypeError";
  if (e instanceof SyntaxError) return "SyntaxError";
  return "other:" + String(e);
}
// One valid instance of every Temporal type. `Temporal.Now` has no receiver.
const factories = {
  Duration: () => new Temporal.Duration(1),
  Instant: () => new Temporal.Instant(0n),
  PlainDate: () => new Temporal.PlainDate(2000, 1, 1),
  PlainDateTime: () => new Temporal.PlainDateTime(2000, 1, 1),
  PlainMonthDay: () => new Temporal.PlainMonthDay(1, 1),
  PlainTime: () => new Temporal.PlainTime(),
  PlainYearMonth: () => new Temporal.PlainYearMonth(2000, 1),
  ZonedDateTime: () => new Temporal.ZonedDateTime(0n, "UTC"),
};
const constructorArguments = {
  Duration: [1], Instant: [0n], PlainDate: [2000, 1, 1], PlainDateTime: [2000, 1, 1],
  PlainMonthDay: [1, 1], PlainTime: [], PlainYearMonth: [2000, 1], ZonedDateTime: [0n, "UTC"],
};
const kinds = Object.keys(factories);
// An instance built through a *derived* constructor, the way `class extends`
// does. It carries the internal slot, so it must remain a valid receiver.
function subclassInstance(kind) {
  function Derived() {}
  Derived.prototype = Object.create(Temporal[kind].prototype);
  return Reflect.construct(Temporal[kind], constructorArguments[kind], Derived);
}
"#;

fn run(body: &str) {
    let source = format!(
        "(function() {{\n{PRELUDE}\n{body}\nreturn fails.length === 0 ? true : fails.join(\"\\n\");\n}})()"
    );
    let program =
        compile(&parse(&source).expect("test script parses")).expect("test script compiles");
    match Vm::default().execute(&program) {
        Ok(Value::Bool(true)) => {}
        Ok(Value::String(text)) => {
            panic!("failing cases:\n{}", text.to_utf8().unwrap());
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

/// The audit itself: every own string-keyed accessor or method of one Temporal
/// prototype, called with every wrong receiver and several argument shapes,
/// must throw `TypeError` and must not touch its arguments first. `audited`
/// and `minimumMembers` are prepended per type; one script per type keeps each
/// inside the VM's instruction budget.
const AUDIT_BODY: &str = r#"
      let touched = "";
      // Every property name a Temporal method might read from an argument
      // (property bags, options bags, duration-likes) is a recording trap.
      const probe = {};
      for (const name of ["year", "month", "monthCode", "day", "hour", "minute", "second",
          "millisecond", "microsecond", "nanosecond", "years", "months", "weeks", "days", "hours",
          "minutes", "seconds", "milliseconds", "microseconds", "nanoseconds", "era", "eraYear",
          "calendar", "timeZone", "offset", "smallestUnit", "largestUnit", "roundingMode",
          "roundingIncrement", "relativeTo", "overflow", "disambiguation", "offsetOption",
          "fractionalSecondDigits", "calendarName", "timeZoneName", "unit", "direction",
          "toString", "valueOf", "toJSON"]) {
        Object.defineProperty(probe, name,
          { get() { touched = name; return undefined; }, enumerable: true, configurable: true });
      }
      Object.defineProperty(probe, Symbol.toPrimitive,
        { get() { touched = "@@toPrimitive"; return undefined; }, enumerable: true, configurable: true });
      // Arguments that are *valid* for some other receiver matter too: a
      // member that throws `TypeError` only because the argument is
      // unusable would otherwise mask a missing brand check.
      const argumentShapes = [
        [], [probe, probe], ["iso8601"], ["UTC"], ["2000-01-01"], [{ years: 1 }],
        [new Temporal.PlainTime()], [new Temporal.Duration(1)],
      ];
      const primitives = [
        ["undefined", () => undefined], ["null", () => null], ["true", () => true],
        ["1", () => 1], ["empty string", () => ""], ["symbol", () => Symbol()],
        ["1n", () => 1n], ["plain object", () => ({})], ["array", () => []],
        ["function", () => function () {}],
      ];
      for (const kind of audited) {
        const constructor = Temporal[kind];
        const prototype = constructor.prototype;
        const wrong = primitives.slice();
        wrong.push(["Object.create(" + kind + ".prototype)", () => Object.create(prototype)]);
        wrong.push([kind + ".prototype", () => prototype]);
        wrong.push([kind + " constructor", () => constructor]);
        // Neither a proxy for a valid instance nor an object that inherits
        // from one has the internal slot itself.
        wrong.push(["a Proxy for a " + kind, () => new Proxy(factories[kind](), {})]);
        wrong.push(["an object inheriting from a " + kind, () => Object.create(factories[kind]())]);
        for (const other of kinds) {
          if (other !== kind) wrong.push(["a " + other, factories[other]]);
        }
        for (const key of Reflect.ownKeys(prototype)) {
          if (key === "constructor" || typeof key === "symbol") continue;
          const descriptor = Object.getOwnPropertyDescriptor(prototype, key);
          const member = descriptor.get !== undefined ? descriptor.get
            : typeof descriptor.value === "function" ? descriptor.value : undefined;
          if (member === undefined) continue;
          stats.members++;
          for (const [label, make] of wrong) {
            const receiver = make();
            for (const args of argumentShapes) {
              touched = "";
              let outcome = "returned";
              try { member.apply(receiver, args); } catch (e) { outcome = nameOf(e); }
              stats.calls++;
              const where = kind + ".prototype." + key + " on " + label;
              if (outcome !== "TypeError") fails.push(where + " -> " + outcome);
              else if (touched !== "") fails.push(where + " read argument " + touched + " first");
            }
          }
        }
      }
      // The enumeration must not silently shrink to nothing.
      if (stats.members < minimumMembers) fails.push("only audited " + stats.members + " members");
"#;

fn audit_prototype(kind: &str, minimum_members: usize) {
    run(&format!(
        "const audited = [\"{kind}\"];\nconst minimumMembers = {minimum_members};\n{AUDIT_BODY}"
    ));
}

#[test]
fn duration_members_reject_every_wrong_receiver_before_reading_arguments() {
    audit_prototype("Duration", 23);
}

#[test]
fn instant_members_reject_every_wrong_receiver_before_reading_arguments() {
    audit_prototype("Instant", 13);
}

#[test]
fn plain_date_members_reject_every_wrong_receiver_before_reading_arguments() {
    audit_prototype("PlainDate", 31);
}

#[test]
fn plain_date_time_members_reject_every_wrong_receiver_before_reading_arguments() {
    audit_prototype("PlainDateTime", 38);
}

#[test]
fn plain_month_day_members_reject_every_wrong_receiver_before_reading_arguments() {
    audit_prototype("PlainMonthDay", 11);
}

#[test]
fn plain_time_members_reject_every_wrong_receiver_before_reading_arguments() {
    audit_prototype("PlainTime", 17);
}

#[test]
fn plain_year_month_members_reject_every_wrong_receiver_before_reading_arguments() {
    audit_prototype("PlainYearMonth", 21);
}

#[test]
fn zoned_date_time_members_reject_every_wrong_receiver_before_reading_arguments() {
    audit_prototype("ZonedDateTime", 51);
}

/// The concrete cross-type acceptances the audit found, kept as named cases
/// so each documents one observable symptom.
#[test]
fn getters_shared_across_prototypes_reject_receivers_of_a_sibling_type() {
    run(r#"
      function getter(kind, name) { return Object.getOwnPropertyDescriptor(Temporal[kind].prototype, name).get; }
      function rejects(kind, name, receiver, label) {
        let outcome = "returned";
        try { getter(kind, name).call(receiver); } catch (e) { outcome = nameOf(e); }
        if (outcome !== "TypeError") fails.push(kind + "." + name + " on " + label + " -> " + outcome);
      }
      const zoned = factories.ZonedDateTime();
      const dateTime = factories.PlainDateTime();
      const yearMonth = factories.PlainYearMonth();
      // A `ZonedDateTime` and a `PlainDateTime` both carry a calendar date,
      // yet neither is a `PlainDate`.
      for (const name of ["year", "month", "monthCode", "day", "era", "eraYear", "monthsInYear",
                          "dayOfWeek", "dayOfYear", "weekOfYear", "yearOfWeek", "daysInWeek",
                          "daysInMonth", "daysInYear", "inLeapYear"]) {
        rejects("PlainDate", name, zoned, "a ZonedDateTime");
        rejects("PlainDate", name, dateTime, "a PlainDateTime");
      }
      rejects("PlainDate", "year", yearMonth, "a PlainYearMonth");
      rejects("PlainYearMonth", "year", factories.PlainDate(), "a PlainDate");
      rejects("PlainMonthDay", "day", factories.PlainDate(), "a PlainDate");
      rejects("PlainMonthDay", "monthCode", yearMonth, "a PlainYearMonth");
      // `calendarId` is installed on five prototypes; no Temporal type
      // outside its own five may reach it.
      for (const kind of ["PlainDate", "PlainDateTime", "PlainMonthDay", "PlainYearMonth", "ZonedDateTime"]) {
        for (const other of ["Duration", "Instant", "PlainTime"]) {
          rejects(kind, "calendarId", factories[other](), "a " + other);
        }
      }
      // Time-of-day getters.
      rejects("PlainTime", "hour", dateTime, "a PlainDateTime");
      rejects("PlainTime", "nanosecond", zoned, "a ZonedDateTime");
      rejects("PlainDateTime", "minute", factories.PlainTime(), "a PlainTime");
      rejects("ZonedDateTime", "second", dateTime, "a PlainDateTime");
      // `Instant` and `ZonedDateTime` both carry epoch nanoseconds.
      rejects("Instant", "epochNanoseconds", zoned, "a ZonedDateTime");
      rejects("Instant", "epochMilliseconds", zoned, "a ZonedDateTime");
      rejects("ZonedDateTime", "epochNanoseconds", factories.Instant(), "an Instant");
      rejects("ZonedDateTime", "epochMilliseconds", factories.Instant(), "an Instant");
    "#);
}

/// `PlainDate` and `PlainDateTime` share the native functions behind
/// `with`/`add`/`subtract`/`until`/`since`/`equals`/`toString`/`toJSON`/
/// `toLocaleString`/`withCalendar`/`toZonedDateTime`; each must still reject
/// the other type, and must do so before it reads any argument.
#[test]
fn methods_shared_by_plain_date_and_plain_date_time_reject_the_sibling_type() {
    run(r#"
      let read = false;
      const bag = { get year() { read = true; return 2000; }, get years() { read = true; return 1; },
                    get month() { read = true; return 1; }, get day() { read = true; return 1; },
                    get timeZone() { read = true; return "UTC"; }, get smallestUnit() { read = true; return "day"; } };
      function method(kind, name) { return Temporal[kind].prototype[name]; }
      const pairs = [["PlainDate", factories.PlainDateTime(), "a PlainDateTime"],
                     ["PlainDateTime", factories.PlainDate(), "a PlainDate"]];
      for (const [kind, receiver, label] of pairs) {
        for (const name of ["with", "add", "subtract", "until", "since", "equals", "toString", "toJSON",
                            "toLocaleString", "withCalendar", "toZonedDateTime"]) {
          read = false;
          let outcome = "returned";
          try { method(kind, name).call(receiver, bag, bag); } catch (e) { outcome = nameOf(e); }
          const where = kind + ".prototype." + name + " on " + label;
          if (outcome !== "TypeError") fails.push(where + " -> " + outcome);
          else if (read) fails.push(where + " read an argument before the brand check");
        }
      }
      // Methods only one of the two has.
      for (const [kind, name, receiver, label] of [
        ["PlainDate", "toPlainDateTime", factories.PlainDateTime(), "a PlainDateTime"],
        ["PlainDate", "toPlainYearMonth", factories.PlainDateTime(), "a PlainDateTime"],
        ["PlainDate", "toPlainMonthDay", factories.PlainDateTime(), "a PlainDateTime"],
        ["PlainDateTime", "toPlainDate", factories.PlainDate(), "a PlainDate"],
        ["PlainDateTime", "toPlainTime", factories.PlainDate(), "a PlainDate"],
        ["PlainDateTime", "withPlainTime", factories.PlainDate(), "a PlainDate"],
        ["PlainDateTime", "round", factories.PlainDate(), "a PlainDate"],
      ]) {
        read = false;
        let outcome = "returned";
        try { method(kind, name).call(receiver, bag); } catch (e) { outcome = nameOf(e); }
        const where = kind + ".prototype." + name + " on " + label;
        if (outcome !== "TypeError") fails.push(where + " -> " + outcome);
        else if (read) fails.push(where + " read an argument before the brand check");
      }
    "#);
}

/// `withCalendar` is installed on `PlainDate`, `PlainDateTime` and
/// `ZonedDateTime` through one native function that used to accept any
/// Temporal value and re-stamp its calendar.
#[test]
fn with_calendar_rejects_every_temporal_type_it_is_not_installed_on() {
    run(r#"
      for (const kind of ["PlainDate", "PlainDateTime", "ZonedDateTime"]) {
        for (const other of kinds) {
          if (other === kind) continue;
          let outcome = "returned";
          try { Temporal[kind].prototype.withCalendar.call(factories[other](), "iso8601"); }
          catch (e) { outcome = nameOf(e); }
          if (outcome !== "TypeError") fails.push(kind + ".prototype.withCalendar on a " + other + " -> " + outcome);
        }
      }
    "#);
}

/// The brand check must not reject anything it should accept: every getter
/// on a direct instance and on an instance built through a derived
/// constructor, plus a representative method on each type.
#[test]
fn valid_receivers_including_derived_instances_still_work() {
    run(r#"
      for (const kind of kinds) {
        const prototype = Temporal[kind].prototype;
        for (const [label, receiver] of [["direct", factories[kind]()], ["derived", subclassInstance(kind)]]) {
          for (const key of Reflect.ownKeys(prototype)) {
            if (key === "constructor" || typeof key === "symbol") continue;
            const descriptor = Object.getOwnPropertyDescriptor(prototype, key);
            if (descriptor.get === undefined) continue;
            let outcome = "returned";
            try { descriptor.get.call(receiver); } catch (e) { outcome = nameOf(e); }
            if (outcome !== "returned") fails.push(kind + "." + key + " getter on a " + label + " instance -> " + outcome);
          }
        }
      }
      function value(f) { try { return f(); } catch (e) { return "threw " + nameOf(e); } }
      function same(what, f, expected) {
        const actual = value(f);
        if (actual !== expected) fails.push(what + " => " + String(actual) + " !== " + String(expected));
      }
      for (const [label, make] of [
        ["direct", (kind) => factories[kind]()], ["derived", (kind) => subclassInstance(kind)]]) {
        const date = make("PlainDate");
        const dateTime = make("PlainDateTime");
        const time = make("PlainTime");
        const zoned = make("ZonedDateTime");
        const instant = make("Instant");
        same(label + " PlainDate.year", () => date.year, 2000);
        same(label + " PlainDate.calendarId", () => date.calendarId, "iso8601");
        same(label + " PlainDate.dayOfWeek", () => date.dayOfWeek, 6);
        same(label + " PlainDate.add", () => date.add({ days: 1 }).toString(), "2000-01-02");
        same(label + " PlainDate.with", () => date.with({ day: 5 }).toString(), "2000-01-05");
        same(label + " PlainDate.until", () => date.until("2000-01-04").days, 3);
        same(label + " PlainDate.equals", () => date.equals("2000-01-01"), true);
        same(label + " PlainDate.toPlainDateTime", () => date.toPlainDateTime().toString(), "2000-01-01T00:00:00");
        same(label + " PlainDate.toZonedDateTime", () => date.toZonedDateTime("UTC").epochNanoseconds, 946684800000000000n);
        same(label + " PlainDate.withCalendar", () => date.withCalendar("gregory").calendarId, "gregory");
        same(label + " PlainDateTime.hour", () => dateTime.hour, 0);
        same(label + " PlainDateTime.month", () => dateTime.month, 1);
        same(label + " PlainDateTime.add", () => dateTime.add({ hours: 25 }).toString(), "2000-01-02T01:00:00");
        same(label + " PlainDateTime.round", () => dateTime.round("day").toString(), "2000-01-01T00:00:00");
        same(label + " PlainDateTime.toPlainDate", () => dateTime.toPlainDate().toString(), "2000-01-01");
        same(label + " PlainDateTime.withPlainTime", () => dateTime.withPlainTime("12:00").hour, 12);
        same(label + " PlainTime.minute", () => time.minute, 0);
        same(label + " PlainTime.add", () => time.add({ hours: 3 }).hour, 3);
        same(label + " ZonedDateTime.year", () => zoned.year, 1970);
        same(label + " ZonedDateTime.timeZoneId", () => zoned.timeZoneId, "UTC");
        same(label + " ZonedDateTime.epochMilliseconds", () => zoned.epochMilliseconds, 0);
        same(label + " ZonedDateTime.toInstant", () => zoned.toInstant().epochNanoseconds, 0n);
        same(label + " ZonedDateTime.withCalendar", () => zoned.withCalendar("gregory").calendarId, "gregory");
        same(label + " Instant.epochNanoseconds", () => instant.epochNanoseconds, 0n);
        same(label + " Instant.epochMilliseconds", () => instant.epochMilliseconds, 0);
        same(label + " Instant.toString", () => instant.toString(), "1970-01-01T00:00:00Z");
        same(label + " Duration.sign", () => make("Duration").sign, 1);
        same(label + " Duration.with", () => make("Duration").with({ years: 5 }).years, 5);
        same(label + " Duration.negated", () => make("Duration").negated().sign, -1);
        same(label + " PlainYearMonth.year", () => make("PlainYearMonth").year, 2000);
        same(label + " PlainYearMonth.calendarId", () => make("PlainYearMonth").calendarId, "iso8601");
        same(label + " PlainMonthDay.day", () => make("PlainMonthDay").day, 1);
        same(label + " PlainMonthDay.monthCode", () => make("PlainMonthDay").monthCode, "M01");
      }
    "#);
}

/// Wiring each member to its owning type must not disturb the observable
/// shape of the accessors and methods (`prop-desc.js`, `name.js`).
#[test]
fn accessor_and_method_property_descriptors_are_unchanged() {
    run(r#"
      let accessors = 0;
      for (const kind of kinds) {
        const prototype = Temporal[kind].prototype;
        for (const key of Reflect.ownKeys(prototype)) {
          if (key === "constructor") continue;
          const d = Object.getOwnPropertyDescriptor(prototype, key);
          const where = kind + ".prototype." + String(key);
          if (typeof key === "symbol") {
            if (d.writable !== false || d.enumerable !== false || d.configurable !== true) {
              fails.push(where + " has an unexpected descriptor");
            }
            continue;
          }
          if (d.get !== undefined) {
            accessors++;
            if (typeof d.get !== "function" || d.set !== undefined || d.enumerable !== false || d.configurable !== true) {
              fails.push(where + " accessor descriptor changed");
            }
            if (d.get.name !== "get " + key || d.get.length !== 0) fails.push(where + " getter name/length changed");
          } else {
            if (typeof d.value !== "function" || d.writable !== true || d.enumerable !== false || d.configurable !== true) {
              fails.push(where + " method descriptor changed");
            }
            if (d.value.name !== key) fails.push(where + " method name is " + d.value.name);
          }
        }
      }
      if (accessors < 100) fails.push("only " + accessors + " accessors seen");
    "#);
}

/// The counterpart of the brand check: the static functions (`from`,
/// `compare`, `Instant.fromEpoch*`) and every `Temporal.Now` function have no
/// internal slot to require, so they must work for *any* `this` value --
/// `Temporal.PlainDate.from.call(undefined, ...)` is legal. A receiver check
/// added to the wrong table would break exactly this.
#[test]
fn static_and_now_functions_accept_any_receiver() {
    run(r#"
      const statics = {
        Duration: { from: ["PT1H"], compare: [{ hours: 1 }, { hours: 2 }] },
        Instant: { from: ["1970-01-01T00:00:00Z"], compare: [new Temporal.Instant(0n), new Temporal.Instant(1n)],
                   fromEpochMilliseconds: [0], fromEpochNanoseconds: [0n] },
        PlainDate: { from: ["2000-01-01"], compare: ["2000-01-01", "2000-01-02"] },
        PlainDateTime: { from: ["2000-01-01T00:00"], compare: ["2000-01-01T00:00", "2000-01-02T00:00"] },
        PlainMonthDay: { from: ["01-01"] },
        PlainTime: { from: ["12:00"], compare: ["12:00", "13:00"] },
        PlainYearMonth: { from: ["2000-01"], compare: ["2000-01", "2000-02"] },
        ZonedDateTime: { from: ["2000-01-01T00:00[UTC]"],
                         compare: ["2000-01-01T00:00[UTC]", "2000-01-02T00:00[UTC]"] },
      };
      const receivers = [undefined, null, true, 1, "", Symbol(), 1n, {}, [], function () {}];
      let called = 0;
      for (const kind of kinds) {
        const constructor = Temporal[kind];
        // Every static function must be in the table above, so a new one is
        // not silently left unexercised.
        for (const key of Reflect.ownKeys(constructor)) {
          const descriptor = Object.getOwnPropertyDescriptor(constructor, key);
          if (typeof descriptor.value === "function" && !(key in statics[kind])) {
            fails.push(kind + "." + String(key) + " is a static function missing from the table");
          }
        }
        for (const name of Object.keys(statics[kind])) {
          for (const receiver of receivers) {
            called++;
            let outcome = "returned";
            try { constructor[name].call(receiver, ...statics[kind][name]); } catch (e) { outcome = nameOf(e); }
            if (outcome !== "returned") fails.push(kind + "." + name + " with this = " + String(typeof receiver) + " -> " + outcome);
          }
        }
      }
      for (const key of Reflect.ownKeys(Temporal.Now)) {
        const descriptor = Object.getOwnPropertyDescriptor(Temporal.Now, key);
        if (typeof descriptor.value !== "function") continue;
        for (const receiver of receivers) {
          called++;
          let outcome = "returned";
          try { descriptor.value.call(receiver); } catch (e) { outcome = nameOf(e); }
          if (outcome !== "returned") fails.push("Temporal.Now." + key + " with this = " + String(typeof receiver) + " -> " + outcome);
        }
      }
      if (called < 150) fails.push("only " + called + " calls made");
    "#);
}

/// `Date.prototype.toTemporalInstant` (the one Temporal member on a non-
/// Temporal prototype) starts with `RequireInternalSlot(this, [[DateValue]])`:
/// Test262's `built-ins/Date/prototype/toTemporalInstant/this-value-*.js`,
/// `prop-desc.js`, `name.js`, `length.js` and `not-a-constructor.js`.
#[test]
fn date_prototype_to_temporal_instant_brand_checks_its_receiver() {
    run(r#"
      const toTemporalInstant = Date.prototype.toTemporalInstant;
      if (typeof toTemporalInstant !== "function") {
        fails.push("Date.prototype.toTemporalInstant is " + typeof toTemporalInstant);
      } else {
        const descriptor = Object.getOwnPropertyDescriptor(Date.prototype, "toTemporalInstant");
        if (descriptor.writable !== true || descriptor.enumerable !== false || descriptor.configurable !== true) {
          fails.push("descriptor is not { writable, !enumerable, configurable }");
        }
        if (toTemporalInstant.name !== "toTemporalInstant") fails.push("name is " + toTemporalInstant.name);
        if (toTemporalInstant.length !== 0) fails.push("length is " + toTemporalInstant.length);
        let constructed = "returned";
        try { new toTemporalInstant(); } catch (e) { constructed = nameOf(e); }
        if (constructed !== "TypeError") fails.push("[[Construct]] -> " + constructed);
        const args = (function () { return arguments; })();
        const wrong = [
          ["undefined", undefined], ["null", null], ["true", true], ["0", 0], ["empty string", ""],
          ["symbol", Symbol()], ["0n", 0n], ["{}", {}], ["[]", []], ["arguments", args],
          ["function", function () {}], ["Date.prototype", Date.prototype],
          // A Temporal object is not a Date either.
          ["a Temporal.Instant", new Temporal.Instant(0n)],
          // Nor is an object that merely inherits from `Date.prototype`.
          ["Object.create(Date.prototype)", Object.create(Date.prototype)],
        ];
        for (const [label, receiver] of wrong) {
          let outcome = "returned";
          try { toTemporalInstant.call(receiver); } catch (e) { outcome = nameOf(e); }
          if (outcome !== "TypeError") fails.push("this = " + label + " -> " + outcome);
        }
        // An invalid date has no instant.
        let invalid = "returned";
        try { new Date(NaN).toTemporalInstant(); } catch (e) { invalid = nameOf(e); }
        if (invalid !== "RangeError") fails.push("invalid date -> " + invalid);
        // Valid dates: milliseconds become nanoseconds exactly, at both ends
        // of the Date range.
        for (const [time, expected] of [
          [0, 0n], [123456789, 123456789000000n], [-123456789, -123456789000000n],
          [-8.64e15, -8640000000000000000000n], [8.64e15, 8640000000000000000000n],
        ]) {
          const instant = new Date(time).toTemporalInstant();
          if (!(instant instanceof Temporal.Instant)) fails.push(time + " did not produce a Temporal.Instant");
          else if (instant.epochNanoseconds !== expected) fails.push(time + " -> " + instant.epochNanoseconds);
        }
        // A `Date` subclass instance carries the slot.
        function DerivedDate() {}
        DerivedDate.prototype = Object.create(Date.prototype);
        const derived = Reflect.construct(Date, [5], DerivedDate);
        if (toTemporalInstant.call(derived).epochNanoseconds !== 5000000n) fails.push("derived Date instance rejected");
      }
    "#);
}
